//! gRPC service: one session at a time. Unary calls lock the session on blocking threads, the runner
//! thread paces real-time sessions, and the streaming calls live in `streams`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use glam::DVec3;
use ofs_config::{FcKind, WorldConfig};
use ofs_core::scheduler::MAX_RUN_FOR_S;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::blackbox::BlackboxCopy;
use crate::inputs;
use crate::pacer::OverrunPolicy;
use crate::pb::{self, sim_server::Sim, sim_server::SimServer};
use crate::repro::{snapshot_firmware, BundleSource};
use crate::runner;
use crate::session::{event, panic_message, world_msg, RunMode, Session, Shared, Slot};
use crate::streams;
use crate::vehicle::{self, BuildOptions, Fault, Sticks};
use ofs_log::recorder::create_numbered;
use ofs_log::{RecordError, SignalRate};

pub use ofs_proto::PROTOCOL_VERSION;

/// The largest request the server accepts (tonic's default is 4 MiB): a scripted-inputs CSV of a long flight is
/// bigger than that.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;

/// The gRPC service with this server's limits: what `main` and the tests serve.
pub fn sim_server(service: SimService) -> SimServer<SimService> {
    SimServer::new(service).max_decoding_message_size(MAX_REQUEST_BYTES)
}

#[derive(Clone)]
pub struct SimService {
    shared: Arc<Shared>,
    data_dir: PathBuf,
    /// The real-time runner thread, joined by `shutdown`.
    runner: Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>,
}

/// A status carrying the machine-readable `ofs-error-kind` metadata that clients map to typed errors.
pub fn error(kind: &'static str, code: Code, message: impl Into<String>) -> Status {
    let mut md = MetadataMap::new();
    md.insert("ofs-error-kind", kind.parse().expect("kind is ASCII"));
    Status::with_metadata(code, message.into(), md)
}

pub(crate) fn sim_error(e: &SimError) -> Status {
    match e {
        SimError::Firmware(m) => error("firmware", Code::Aborted, m.clone()),
        SimError::NonFinite(_) => error("numerical", Code::Aborted, e.to_string()),
        SimError::InvalidArgument(m) => error("invalid_argument", Code::InvalidArgument, m.clone()),
        SimError::Other(m) => error("internal", Code::Internal, m.clone()),
    }
}

pub(crate) fn not_loaded() -> Status {
    error("not_loaded", Code::FailedPrecondition, "no quad loaded; call Load first")
}

pub(crate) fn loaded(slot: &mut Slot) -> Result<&mut Session, Status> {
    slot.as_mut().ok_or_else(not_loaded)
}

fn part_info(p: &ofs_config::part::PartProvenance) -> pb::PartInfo {
    let provenance = p
        .parameters
        .iter()
        .map(|(path, v)| pb::ParamProvenance {
            path: path.clone(),
            source: v.source.as_str().to_string(),
            link: v.link.clone(),
            note: v.note.clone(),
        })
        .collect();
    pb::PartInfo { kind: p.kind.to_string(), id: p.id.clone(), name: p.name.clone(), provenance }
}

fn invalid_state(message: &str) -> Status {
    error("invalid_state", Code::FailedPrecondition, message)
}

fn record_error(e: RecordError) -> Status {
    match e {
        RecordError::AlreadyRecording | RecordError::NotRecording => invalid_state(&e.to_string()),
        RecordError::Spec(m) => error("invalid_argument", Code::InvalidArgument, m),
        RecordError::Io(_) | RecordError::Taken(_) => error("internal", Code::Internal, e.to_string()),
    }
}

fn inputs_record_error(e: RecordError) -> Status {
    match e {
        RecordError::AlreadyRecording => invalid_state("an inputs recording is already running; stop it first"),
        RecordError::NotRecording => invalid_state("no inputs recording is running"),
        RecordError::Spec(m) => error("invalid_argument", Code::InvalidArgument, m),
        RecordError::Io(_) | RecordError::Taken(_) => error("internal", Code::Internal, e.to_string()),
    }
}

/// NED metres from the proto's optional Vec3 (absent = the origin).
fn dvec3(v: Option<pb::Vec3>) -> DVec3 {
    let v = v.unwrap_or_default();
    DVec3::new(v.x, v.y, v.z)
}

/// Validates sticks from a request (finite values, at most four aux channels; missing aux read -1).
pub(crate) fn parse_sticks(s: pb::Sticks) -> Result<Sticks, Status> {
    if s.aux.len() > RC_AUX_COUNT {
        let message = format!("at most {RC_AUX_COUNT} aux channels (got {})", s.aux.len());
        return Err(error("invalid_argument", Code::InvalidArgument, message));
    }
    let mut aux = [-1.0; RC_AUX_COUNT];
    aux[..s.aux.len()].copy_from_slice(&s.aux);
    let sticks = Sticks { roll: s.roll, pitch: s.pitch, yaw: s.yaw, throttle: s.throttle, aux };
    let all_finite = [sticks.roll, sticks.pitch, sticks.yaw, sticks.throttle].iter().chain(aux.iter()).all(|v| v.is_finite());
    if !all_finite {
        return Err(error("invalid_argument", Code::InvalidArgument, "stick values must be finite"));
    }
    Ok(sticks)
}

/// The run mode and overrun policy a `Load` asks for (the unspecified values are lockstep and warn).
fn load_mode_and_policy(req: &pb::LoadRequest) -> Result<(RunMode, OverrunPolicy), Status> {
    let mode = match pb::Mode::try_from(req.mode) {
        Ok(pb::Mode::Unspecified | pb::Mode::Lockstep) => RunMode::Lockstep,
        Ok(pb::Mode::Realtime) => RunMode::Realtime,
        Err(_) => return Err(error("invalid_argument", Code::InvalidArgument, format!("unknown mode {}", req.mode))),
    };
    let policy = match pb::OverrunPolicy::try_from(req.overrun_policy) {
        Ok(pb::OverrunPolicy::Unspecified | pb::OverrunPolicy::Warn) => OverrunPolicy::Warn,
        Ok(pb::OverrunPolicy::Slow) => OverrunPolicy::Slow,
        Err(_) => {
            let message = format!("unknown overrun policy {}", req.overrun_policy);
            return Err(error("invalid_argument", Code::InvalidArgument, message));
        }
    };
    Ok((mode, policy))
}

/// Ends the loaded session, if any. A running recording (MCAP or inputs) is stopped first, so its file is complete and the
/// clients that track the recording's state hear that it ended (as they do for `StopInputsRecording`).
fn end_session(shared: &Shared, slot: &mut Slot) {
    if let Some(s) = slot.as_mut() {
        // A running MCAP recording ends with the session, exactly as a running inputs recording does: say so, naming
        // the file, so clients tracking the recording's state hear that it ended.
        if s.vehicle.is_recording() {
            let time = s.vehicle.time_s();
            let message = match s.vehicle.stop_recording() {
                Ok(summary) => format!("{}: {} messages", summary.path.display(), summary.messages),
                Err(e) => format!("recording failed: {e}"),
            };
            shared.publish([event(time, pb::EventKind::RecordingStopped, message)]);
        }
        if s.vehicle.is_recording_inputs() {
            let time = s.vehicle.time_s();
            let message = match s.vehicle.stop_inputs_recording() {
                Ok(summary) => format!("{}: {} rows over {} s", summary.path.display(), summary.rows, summary.duration_s),
                // The same text `StopInputsRecording` publishes for a failed stop.
                Err(RecordError::Io(m)) => format!("inputs recording failed: {m}"),
                Err(e) => format!("inputs recording failed: {e}"),
            };
            shared.publish([event(time, pb::EventKind::InputsRecordingStopped, message)]);
        }
    }
    *slot = None;
}

/// Sets its flag when dropped: a call whose client went away (tonic drops its future) does not start its work.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl SimService {
    pub fn new(data_dir: PathBuf) -> Self {
        let shared = Shared::new();
        let runner = Arc::new(std::sync::Mutex::new(Some(runner::spawn(shared.clone()))));
        Self { shared, data_dir, runner }
    }

    /// Every event published from now on (the Watch RPC reads the same channel).
    pub fn subscribe(&self) -> broadcast::Receiver<pb::Event> {
        self.shared.events.subscribe()
    }

    /// Stops the real-time runner (and waits for it) and unloads the session, stopping Betaflight SITL. For server
    /// shutdown.
    pub fn shutdown(&self) {
        self.shared.stop();
        let runner = self.runner.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = runner {
            let _ = handle.join();
        }
        *self.shared.lock() = None;
    }

    /// Where recordings and copied Blackbox logs land: `<data_dir>/logs`.
    pub fn logs_dir(&self) -> PathBuf {
        let logs = self.data_dir.join("logs");
        std::path::absolute(&logs).unwrap_or(logs)
    }

    #[cfg(test)]
    fn runner_finished(&self) -> bool {
        self.runner.lock().unwrap().as_ref().is_none_or(|h| h.is_finished())
    }

    /// Runs `f` with the session locked, on a blocking thread. If the caller goes away (its client cancelled the
    /// call) before `f` gets the lock, `f` does not run.
    async fn blocking<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&Shared, &mut Slot) -> Result<T, Status> + Send + 'static,
    {
        self.locked(f, true).await
    }

    /// Like `blocking`, but `f` runs even when the caller went away (Unload: Betaflight must stop either way).
    async fn blocking_always<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&Shared, &mut Slot) -> Result<T, Status> + Send + 'static,
    {
        self.locked(f, false).await
    }

    async fn locked<T, F>(&self, f: F, skip_if_cancelled: bool) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&Shared, &mut Slot) -> Result<T, Status> + Send + 'static,
    {
        let shared = self.shared.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(cancelled.clone());
        let joined = tokio::task::spawn_blocking(move || {
            let mut slot = shared.lock();
            if skip_if_cancelled && cancelled.load(Ordering::Acquire) {
                return Err(Status::cancelled("the client went away"));
            }
            f(&shared, &mut slot)
        })
        .await;
        match joined {
            Ok(result) => result.map(Response::new),
            Err(e) if e.is_panic() => {
                // The handler panicked while holding the slot (the poisoned mutex is recovered by `Shared::lock`):
                // poison the loaded session, which may be half-stepped, so it cannot be used further.
                let message = format!("internal panic: {}", panic_message(e.into_panic().as_ref()));
                let shared = self.shared.clone();
                let poisoned = message.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let mut slot = shared.lock();
                    if let Some(s) = slot.as_mut() {
                        let e = SimError::Other(poisoned);
                        s.poison(e.clone());
                        // The clients are told, with the repro bundle the poison wrote.
                        shared.publish([event(s.vehicle.time_s(), pb::EventKind::SimError, s.failure_text(&e))]);
                    }
                })
                .await;
                Err(error("internal", Code::Internal, message))
            }
            Err(e) => Err(error("internal", Code::Internal, e.to_string())),
        }
    }
}

#[tonic::async_trait]
impl Sim for SimService {
    async fn handshake(&self, req: Request<pb::HandshakeRequest>) -> Result<Response<pb::HandshakeReply>, Status> {
        let v = req.into_inner().protocol_version;
        if v != PROTOCOL_VERSION {
            return Err(error("protocol", Code::FailedPrecondition, format!("client speaks protocol {v}, server speaks {PROTOCOL_VERSION}")));
        }
        Ok(Response::new(pb::HandshakeReply {
            protocol_version: PROTOCOL_VERSION,
            server_version: env!("CARGO_PKG_VERSION").into(),
            logs_dir: self.logs_dir().display().to_string(),
        }))
    }

    async fn load(&self, req: Request<pb::LoadRequest>) -> Result<Response<pb::LoadReply>, Status> {
        let req = req.into_inner();
        let (mode, policy) = load_mode_and_policy(&req)?;
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let world = if req.world_path.is_empty() {
            WorldConfig::open_field()
        } else {
            let path = ofs_config::world::resolve(&req.world_path, Path::new(&req.quad_path));
            ofs_config::world::load(&path).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
        };
        if req.stabilize && !req.open_loop_fc {
            return Err(error("invalid_argument", Code::InvalidArgument, "stabilize needs open_loop_fc"));
        }
        let fc_override = req.open_loop_fc.then_some(if req.stabilize { FcKind::OpenLoopStabilized } else { FcKind::OpenLoop });
        // An override that downgrades a quad file's own stabilizer would fly it throttle-only and only the digest
        // would ever say so: refuse, and say what to ask for instead.
        if fc_override == Some(FcKind::OpenLoop) && cfg.fc.kind == FcKind::OpenLoopStabilized {
            return Err(error(
                "invalid_argument",
                Code::InvalidArgument,
                "this quad file is open_loop_stabilized; load with stabilize = true, or without the open_loop_fc override",
            ));
        }
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override, world };
        let keep_alive = req.keep_alive;
        let quad_stem = cfg.source_path.file_stem().map_or_else(|| "quad".into(), |s| s.to_string_lossy().into_owned());
        let logs_dir = self.logs_dir();
        // What a repro bundle needs to run this session again. A SITL session's firmware directory depends on the
        // slot it takes, which the build decides: it is filled in below.
        let world_file = (!req.world_path.is_empty()).then(|| ofs_config::world::resolve(&req.world_path, Path::new(&req.quad_path)));
        let library_env = std::env::var("OFS_LIBRARY").ok();
        let library_roots = ofs_config::library_roots(library_env.as_deref(), cfg.source_dir());
        let mut repro = BundleSource::new(
            &cfg,
            world_file.as_deref(),
            &library_roots,
            req.seed,
            req.open_loop_fc,
            mode == RunMode::Realtime,
            logs_dir.clone(),
            None,
        )
        .map_err(|why| tracing::warn!("this session will write no repro bundle: {why}"))
        .ok();
        if let Some(r) = repro.as_mut() {
            r.stabilize = req.stabilize;
        }
        self.blocking(move |shared, slot| {
            // The rebuild keeps the old session's slot (its claim never leaves this process), so a reload cannot
            // land in a slot — and firmware directory — that another simulator's acquire took in between (#90).
            let keep_slot = slot.as_mut().and_then(|s| s.vehicle.take_slot_claim());
            end_session(shared, slot); // stop the previous vehicle (and its SITL) before the new one binds the ports
            let vehicle = vehicle::build(&cfg, &opts, keep_slot).map_err(|e| sim_error(&e))?;
            // The firmware's directory is the slot's (the first simulator on the machine has the stock one).
            let blackbox = vehicle.firmware_dir().map(|dir| BlackboxCopy::new(dir, logs_dir));
            // The firmware's state as the session starts: after the first boot's diff apply, before any flight.
            if let Some(r) = repro.as_mut() {
                r.firmware_dir = vehicle.firmware_dir();
                r.firmware_initial = r.firmware_dir.as_deref().map(snapshot_firmware).unwrap_or_default();
                // The bundle records what the session flies (the build's FC), not what the request asked for: a
                // quad file that is `open_loop_stabilized` flies stabilized even with no load-request flags.
                let (open_loop_fc, stabilize) = crate::repro::effective_fc_flags(cfg.fc.kind, fc_override);
                r.open_loop_fc = open_loop_fc;
                r.stabilize = stabilize;
            }
            let configurator_address = vehicle.configurator_address().unwrap_or_default();
            let mut session = Session::new(vehicle, mode, policy, keep_alive);
            session.watched = shared.watchers.load(Ordering::Acquire) > 0;
            session.quad_stem = quad_stem;
            session.repro = repro;
            session.blackbox = blackbox;
            *slot = Some(session);
            let parts = cfg.part_provenance.iter().map(part_info).collect();
            Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz, configurator_address, parts })
        })
        .await
    }

    async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
        let sticks = parse_sticks(req.into_inner())?;
        self.blocking(move |_, slot| {
            let s = loaded(slot)?;
            if let Some(playing) = s.vehicle.inputs_playing() {
                let message = format!("{playing} is playing: SetSticks would be overwritten (call StopInputs first)");
                return Err(error("invalid_argument", Code::InvalidArgument, message));
            }
            s.vehicle.set_sticks(&sticks).map_err(|e| sim_error(&e))?;
            Ok(pb::Empty {})
        })
        .await
    }

    /// Steps a lockstep (or paused real-time) session in 50 ms chunks of simulated time. The session is locked per
    /// chunk, so other calls (GetState, SetSticks, streams) get in between; a Run whose session is unloaded or
    /// replaced meanwhile stops with not_loaded.
    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::State>, Status> {
        let seconds = req.into_inner().seconds;
        if !seconds.is_finite() || !(0.0..=MAX_RUN_FOR_S).contains(&seconds) {
            let message = format!("seconds must be finite and in [0, {MAX_RUN_FOR_S}] (got {seconds})");
            return Err(error("invalid_argument", Code::InvalidArgument, message));
        }
        let (id, total, chunk) = self
            .blocking(move |_, slot| {
                let s = loaded(slot)?;
                if let Some(f) = &s.failure {
                    return Err(sim_error(f)); // also for a duration of zero ticks, which never reaches `step`
                }
                let hz = u64::from(s.vehicle.base_hz());
                let total = (seconds * hz as f64).round() as u64;
                Ok((s.id, total, (hz / 20).max(1))) // 50 ms of simulated time per chunk
            })
            .await?
            .into_inner();
        let mut done = 0;
        loop {
            let n = chunk.min(total - done);
            let last = done + n == total;
            let reply = self
                .blocking(move |shared, slot| {
                    if shared.stopping() {
                        return Err(Status::unavailable("server shutting down"));
                    }
                    let s = slot.as_mut().filter(|s| s.id == id).ok_or_else(not_loaded)?;
                    if s.mode == RunMode::Realtime && s.running {
                        return Err(invalid_state("pause the real-time session before calling Run"));
                    }
                    if let Some(f) = &s.failure {
                        return Err(sim_error(f));
                    }
                    let result = s.step(n);
                    shared.publish(s.changes());
                    if let Err(e) = result {
                        if s.failure.is_some() {
                            shared.publish([event(s.vehicle.time_s(), pb::EventKind::SimError, s.failure_text(&e))]);
                        }
                        return Err(sim_error(&e));
                    }
                    Ok(last.then(|| s.state_msg()))
                })
                .await?
                .into_inner();
            done += n;
            if let Some(state) = reply {
                return Ok(Response::new(state));
            }
        }
    }

    async fn start(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            if s.mode != RunMode::Realtime {
                return Err(invalid_state("Start and Pause need a session loaded with mode = realtime"));
            }
            if let Some(f) = &s.failure {
                return Err(sim_error(f));
            }
            if !s.running {
                s.pacer.restart(shared.wall_s(), s.vehicle.time_s());
                s.running = true;
            }
            Ok(pb::Empty {})
        })
        .await
    }

    async fn pause(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
            let s = loaded(slot)?;
            if s.mode != RunMode::Realtime {
                return Err(invalid_state("Start and Pause need a session loaded with mode = realtime"));
            }
            s.running = false;
            Ok(pb::Empty {})
        })
        .await
    }

    async fn get_state(&self, _req: Request<pb::Empty>) -> Result<Response<pb::State>, Status> {
        self.blocking(|_, slot| Ok(loaded(slot)?.state_msg())).await
    }

    async fn get_osd(&self, _req: Request<pb::Empty>) -> Result<Response<pb::OsdFrame>, Status> {
        self.blocking(|_, slot| Ok(loaded(slot)?.osd_msg())).await
    }

    async fn get_world(&self, _req: Request<pb::Empty>) -> Result<Response<pb::World>, Status> {
        self.blocking(|_, slot| Ok(world_msg(loaded(slot)?.vehicle.world()))).await
    }

    async fn unload(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking_always(|shared, slot| {
            end_session(shared, slot);
            Ok(pb::Empty {})
        })
        .await
    }

    type StreamStateStream = ReceiverStream<Result<pb::State, Status>>;

    async fn stream_state(&self, req: Request<pb::StreamRequest>) -> Result<Response<Self::StreamStateStream>, Status> {
        let rate_hz = streams::state_rate(req.into_inner().rate_hz)?;
        Ok(Response::new(streams::state_feed(self.shared.clone(), rate_hz)))
    }

    type StreamOsdStream = ReceiverStream<Result<pb::OsdFrame, Status>>;

    async fn stream_osd(&self, req: Request<pb::StreamRequest>) -> Result<Response<Self::StreamOsdStream>, Status> {
        let rate_hz = streams::osd_rate(req.into_inner().rate_hz)?;
        Ok(Response::new(streams::osd_feed(self.shared.clone(), rate_hz)))
    }

    type PilotStream = ReceiverStream<Result<pb::State, Status>>;

    async fn pilot(&self, req: Request<Streaming<pb::PilotInput>>) -> Result<Response<Self::PilotStream>, Status> {
        streams::pilot(self.shared.clone(), req.into_inner()).await.map(Response::new)
    }

    type WatchStream = ReceiverStream<Result<pb::Event, Status>>;

    async fn watch(&self, _req: Request<pb::Empty>) -> Result<Response<Self::WatchStream>, Status> {
        Ok(Response::new(streams::watch(self.shared.clone()).await))
    }

    async fn inject_fault(&self, req: Request<pb::Fault>) -> Result<Response<pb::Empty>, Status> {
        let req = req.into_inner();
        let at_s = req.at_s;
        let fault = match req.kind {
            Some(pb::fault::Kind::RadioLinkLoss(_)) => Fault::RadioLinkLoss,
            Some(pb::fault::Kind::RadioLoss(m)) => Fault::RadioLoss { fraction: m.fraction },
            Some(pb::fault::Kind::DownlinkLoss(m)) => Fault::DownlinkLoss { fraction: m.fraction },
            Some(pb::fault::Kind::BurstLoss(m)) => {
                Fault::BurstLoss { on_s: f64::from(m.on_ms) / 1000.0, off_s: f64::from(m.off_ms) / 1000.0 }
            }
            Some(pb::fault::Kind::MotorFailure(m)) => Fault::MotorFailure { motor: m.motor as usize, scale: m.scale },
            Some(pb::fault::Kind::PropDamage(m)) => {
                Fault::PropDamage { prop: m.prop as usize, thrust_scale: m.thrust_scale, imbalance_kgm: m.imbalance_kgm }
            }
            Some(pb::fault::Kind::WeakCell(m)) => {
                Fault::WeakCell { cell: m.cell as usize, r_offset_ohm: m.r_offset_ohm, v_offset_v: m.v_offset_v }
            }
            Some(pb::fault::Kind::GyroFault(m)) => Fault::GyroFault { bias_radps: dvec3(m.bias_radps), noise_scale: m.noise_scale },
            Some(pb::fault::Kind::VtxInterference(m)) => {
                Fault::VtxInterference { freq_mhz: m.freq_mhz, power_mw: m.power_mw, position_ned_m: dvec3(m.position_ned_m) }
            }
            None => return Err(error("invalid_argument", Code::InvalidArgument, "the fault has no kind")),
        };
        self.blocking(move |_, slot| {
            let vehicle = &mut loaded(slot)?.vehicle;
            match at_s {
                Some(at_s) => vehicle.schedule_fault(fault, at_s),
                None => vehicle.set_fault(fault, true),
            }
            .map_err(|e| sim_error(&e))?;
            Ok(pb::Empty {})
        })
        .await
    }

    async fn clear_faults(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
            loaded(slot)?.vehicle.clear_faults();
            Ok(pb::Empty {})
        })
        .await
    }

    async fn start_recording(&self, req: Request<pb::StartRecordingRequest>) -> Result<Response<pb::RecordingStarted>, Status> {
        let requests: Vec<SignalRate> = req
            .into_inner()
            .signals
            .into_iter()
            .map(|s| SignalRate { pattern: s.pattern, rate_hz: (s.rate_hz != 0.0).then_some(s.rate_hz) })
            .collect();
        let logs = self.logs_dir();
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if s.vehicle.is_recording() {
                return Err(record_error(RecordError::AlreadyRecording));
            }
            let (path, signals) =
                create_numbered(&logs, &s.quad_stem, "mcap", |p| s.vehicle.start_recording(&requests, p)).map_err(record_error)?;
            let path = path.display().to_string();
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::RecordingStarted, path.clone())]);
            Ok(pb::RecordingStarted { path, signals })
        })
        .await
    }

    async fn stop_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::RecordingSummary>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            let summary = s.vehicle.stop_recording().map_err(record_error)?;
            let path = summary.path.display().to_string();
            let message = format!("{path}: {} messages", summary.messages);
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::RecordingStopped, message)]);
            Ok(pb::RecordingSummary { path, messages: summary.messages, first_s: summary.first_s, last_s: summary.last_s })
        })
        .await
    }

    async fn play_inputs(&self, req: Request<pb::InputsFile>) -> Result<Response<pb::Empty>, Status> {
        let file = req.into_inner();
        let name = if file.name.is_empty() { "inputs".to_string() } else { file.name };
        let inputs = inputs::parse(&name, &file.csv).map_err(|m| error("config", Code::InvalidArgument, m))?;
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if shared.pilot.load(Ordering::Acquire) {
                return Err(invalid_state("a pilot is connected: scripted inputs would fight the pilot's sticks"));
            }
            if s.vehicle.is_recording_inputs() {
                return Err(invalid_state(
                    "an inputs recording is running: it records live sticks only; call StopInputsRecording first",
                ));
            }
            if let Some(playing) = s.vehicle.inputs_playing() {
                return Err(invalid_state(&format!("{playing} is still playing; call StopInputs first")));
            }
            let message = format!("{name}: {} rows over {} s", inputs.rows.len(), inputs.duration_s());
            s.vehicle.play_inputs(inputs).map_err(|e| sim_error(&e))?;
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsStarted, message)]);
            shared.publish(s.changes()); // a file whose rows are all at t_s = 0 has already finished
            Ok(pb::Empty {})
        })
        .await
    }

    async fn stop_inputs(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            if let Some(name) = s.vehicle.stop_inputs() {
                shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsEnded, format!("{name}: stopped"))]);
            }
            Ok(pb::Empty {})
        })
        .await
    }

    async fn reset_race(&self, req: Request<pb::ResetRaceRequest>) -> Result<Response<pb::Empty>, Status> {
        let course = req.into_inner().course;
        self.blocking(move |_shared, slot| {
            let s = loaded(slot)?;
            if s.vehicle.courses().is_empty() {
                return Err(invalid_state("this world has no gates, so there is no race to reset"));
            }
            let index = match course.as_str() {
                "" => None,
                name => match s.vehicle.courses().iter().position(|c| c.name == name) {
                    Some(i) => Some(i),
                    None => {
                        let known: Vec<&str> = s.vehicle.courses().iter().map(|c| c.name.as_str()).collect();
                        return Err(error("invalid_argument", Code::InvalidArgument, format!("this world has no course {name:?} (courses: {known:?})")));
                    }
                },
            };
            s.vehicle.reset_race(index);
            Ok(pb::Empty {})
        })
        .await
    }

    async fn start_inputs_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::InputsRecordingStarted>, Status> {
        let logs = self.logs_dir();
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if s.vehicle.is_recording_inputs() {
                return Err(inputs_record_error(RecordError::AlreadyRecording));
            }
            if let Some(playing) = s.vehicle.inputs_playing() {
                return Err(invalid_state(&format!("{playing} is playing scripted inputs: there are no live sticks to record")));
            }
            let stem = format!("{}-inputs", s.quad_stem);
            let (path, ()) =
                create_numbered(&logs, &stem, "csv", |p| s.vehicle.start_inputs_recording(p)).map_err(inputs_record_error)?;
            let path = path.display().to_string();
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsRecordingStarted, path.clone())]);
            Ok(pb::InputsRecordingStarted { path })
        })
        .await
    }

    async fn stop_inputs_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::InputsRecordingSummary>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            let summary = match s.vehicle.stop_inputs_recording() {
                Ok(summary) => summary,
                Err(RecordError::Io(m)) => {
                    // The recording is over either way: tell the clients that track its state, as `Session::changes` does.
                    shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsRecordingStopped, format!("inputs recording failed: {m}"))]);
                    return Err(inputs_record_error(RecordError::Io(m)));
                }
                Err(e) => return Err(inputs_record_error(e)),
            };
            let path = summary.path.display().to_string();
            let message = format!("{path}: {} rows over {} s", summary.rows, summary.duration_s);
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsRecordingStopped, message)]);
            Ok(pb::InputsRecordingSummary { path, rows: summary.rows, duration_s: summary.duration_s })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::testing::open_loop_session;

    fn kind_of(s: &Status) -> String {
        s.metadata().get("ofs-error-kind").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
    }

    fn service_with(session: Session) -> SimService {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        *svc.shared.lock() = Some(session);
        svc
    }

    /// Making a session fail needs live Betaflight (a firmware failure) or a numerical blow-up that open-loop
    /// sticks cannot cause, so the failure is set directly.
    #[tokio::test]
    async fn run_refuses_a_failed_session_even_for_zero_seconds() {
        let mut session = open_loop_session(RunMode::Lockstep);
        session.poison(SimError::Firmware("simulated failure".into()));
        let svc = service_with(session);
        for seconds in [0.0, 1e-9, 0.1] {
            let err = svc.run(Request::new(pb::RunRequest { seconds })).await.unwrap_err();
            assert_eq!(kind_of(&err), "firmware", "seconds = {seconds}");
            assert_eq!(err.message(), "simulated failure");
        }
        svc.shutdown();
    }

    /// A session loaded from a quad file whose kind is `open_loop_stabilized`, with no load-request flags (Godot's
    /// only way to fly one): its bundle says the stabilizer flew, because the bundle records the effective FC, not
    /// the request.
    #[tokio::test]
    async fn a_quad_file_stabilized_sessions_bundle_records_the_stabilizer() {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        let dir = tempfile::tempdir().unwrap();
        // The library resolves beside the quads directory (`<quad_dir>/../library`): the quad goes in its own
        // subdirectory, next to a copy of the repo's library.
        std::fs::create_dir(dir.path().join("quads")).unwrap();
        let quad = dir.path().join("quads").join("stabilized.toml");
        std::fs::write(
            &quad,
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))
                .unwrap()
                .replace("kind = \"sitl\"", "kind = \"open_loop_stabilized\""),
        )
        .unwrap();
        let lib = dir.path().join("library");
        for entry in walk(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library")) {
            let to = lib.join(&entry);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            let from = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library")).join(&entry);
            std::fs::copy(from, to).unwrap();
        }
        svc.load(Request::new(pb::LoadRequest { quad_path: quad.display().to_string(), seed: 1, ..Default::default() }))
            .await
            .unwrap();
        // The poison's capture_failure writes the bundle at once; take the folder from the session.
        svc.shared.lock().as_mut().unwrap().poison(SimError::Firmware("simulated failure".into()));
        let mut binding = svc.shared.lock();
        let folder = binding.as_mut().unwrap().bundle.as_ref().unwrap().as_ref().unwrap().clone();
        drop(binding);
        let t: toml::Table = std::fs::read_to_string(folder.join("bundle.toml")).unwrap().parse().unwrap();
        assert_eq!(t["open_loop_fc"].as_bool(), Some(true), "the session flew an open-loop FC");
        assert_eq!(t["stabilize"].as_bool(), Some(true), "and its stabilizer was on");
        svc.shutdown();
    }
    /// Paths of every file under `root`, relative to it.
    fn walk(root: &str) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            let name = entry.file_name();
            let rel = std::path::PathBuf::from(&name);
            if entry.file_type().unwrap().is_dir() {
                for sub in walk(&format!("{root}/{}", name.to_string_lossy())) {
                    out.push(rel.join(sub));
                }
            } else {
                out.push(rel);
            }
        }
        out
    }


    #[tokio::test]
    async fn a_failure_at_the_end_of_an_inputs_recording_still_publishes_its_stop_event() {
        use crate::inputs_recorder::{tests::Faulty, InputsRecorder, InputsState};
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let mut events = svc.subscribe();
        {
            let mut slot = svc.shared.lock();
            let s = slot.as_mut().unwrap();
            // Writes reach the buffer; the flush at the end fails.
            let out = Faulty { budget: usize::MAX, flush_fails: true };
            let state = InputsState { sticks: Sticks::default(), transmitter: true, link_loss: false };
            let recorder = InputsRecorder::over(Box::new(out), 8192, 500, 0, &state).unwrap();
            s.vehicle.set_inputs_recorder(recorder);
        }
        let err = svc.stop_inputs_recording(Request::new(pb::Empty {})).await.unwrap_err();
        assert_eq!(kind_of(&err), "internal");
        let event = events.try_recv().expect("the failed stop is announced");
        assert_eq!(event.kind(), pb::EventKind::InputsRecordingStopped);
        assert!(event.message.starts_with("inputs recording failed: "), "{}", event.message);
        assert!(!svc.shared.lock().as_ref().unwrap().vehicle.is_recording_inputs(), "the recording is over");
        svc.shutdown();
    }

    #[tokio::test]
    async fn a_panicking_handler_poisons_the_loaded_session() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let err = svc.blocking::<(), _>(|_, _| panic!("boom")).await.unwrap_err();
        assert_eq!(err.code(), Code::Internal);
        assert_eq!(kind_of(&err), "internal");
        assert!(err.message().contains("boom"), "{}", err.message());
        // The slot lock still works and the session is poisoned: Run reports the failure.
        let err = svc.run(Request::new(pb::RunRequest { seconds: 0.0 })).await.unwrap_err();
        assert_eq!(kind_of(&err), "internal");
        assert!(err.message().contains("internal panic: boom"), "{}", err.message());
        let state = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner();
        assert!(!state.running);
        svc.shutdown();
    }

    #[tokio::test]
    async fn a_panicking_handler_tells_the_clients_and_names_the_bundle() {
        let logs = tempfile::tempdir().unwrap();
        let mut session = open_loop_session(RunMode::Lockstep);
        session.repro = Some(crate::repro::test_source(logs.path()));
        let svc = service_with(session);
        let mut events = svc.subscribe();
        svc.blocking::<(), _>(|_, _| panic!("boom")).await.unwrap_err();
        let mut errors = Vec::new();
        while let Ok(e) = events.try_recv() {
            if e.kind() == pb::EventKind::SimError {
                errors.push(e.message);
            }
        }
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("internal panic: boom"), "{}", errors[0]);
        assert!(errors[0].contains("(repro bundle: "), "{}", errors[0]);
        svc.shutdown();
    }

    #[tokio::test]
    async fn a_session_without_an_osd_answers_get_osd_with_an_absent_frame() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let frame = svc.get_osd(Request::new(pb::Empty {})).await.unwrap().into_inner();
        assert!(!frame.present);
        assert_eq!((frame.cols, frame.rows, frame.seq), (0, 0, 0));
        assert!(frame.cells.is_empty());
        svc.run(Request::new(pb::RunRequest { seconds: 0.05 })).await.unwrap();
        let state = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner();
        let vtx = state.vtx.expect("State.vtx is always set");
        assert!(vtx.present && vtx.freq_mhz == 5658, "open loop: the VTX transmits its power-up channel: {vtx:?}");
        assert_eq!(state.serial_dropped_bytes, 0);
        let video = state.video.expect("State.video is always set");
        assert!(video.present);
        assert_eq!(video.sync(), pb::VideoSync::Locked, "{video:?}");
        assert_eq!(video.rssi.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["omni"]);
        svc.shutdown();
    }

    #[tokio::test]
    async fn get_world_returns_the_sessions_world() {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        assert_eq!(kind_of(&svc.get_world(Request::new(pb::Empty {})).await.unwrap_err()), "not_loaded");
        *svc.shared.lock() = Some(open_loop_session(RunMode::Lockstep));
        let world = svc.get_world(Request::new(pb::Empty {})).await.unwrap().into_inner();
        assert_eq!(world.name, "open field");
        assert_eq!(world.antennas.len(), 1);
        assert_eq!((world.antennas[0].kind.as_str(), world.antennas[0].aim_el_deg), ("omni", 90.0));
        assert!(world.objects.is_empty() && world.emitters.is_empty());
        svc.shutdown();
    }

    #[tokio::test]
    async fn get_osd_needs_a_loaded_session_and_the_osd_stream_rate_is_validated() {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        assert_eq!(kind_of(&svc.get_osd(Request::new(pb::Empty {})).await.unwrap_err()), "not_loaded");
        for hz in [61, 1000] {
            let err = svc.stream_osd(Request::new(pb::StreamRequest { rate_hz: hz })).await.expect_err("rate rejected");
            assert_eq!(kind_of(&err), "invalid_argument", "rate {hz}");
        }
        assert!(svc.stream_osd(Request::new(pb::StreamRequest { rate_hz: 0 })).await.is_ok(), "0 means the default rate");
        svc.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_long_run_lets_other_calls_in_between_its_chunks() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let runner = svc.clone();
        let run = tokio::spawn(async move { runner.run(Request::new(pb::RunRequest { seconds: 5.0 })).await });
        let mut seen_mid_run = None;
        while !run.is_finished() {
            let t = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner().time_s;
            if t > 0.0 && t < 5.0 {
                seen_mid_run = Some(t);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(seen_mid_run.is_some(), "GetState waited for the whole Run");
        let state = run.await.unwrap().unwrap().into_inner();
        assert!((state.time_s - 5.0).abs() < 1e-9, "{}", state.time_s);
        svc.shutdown();
    }

    #[tokio::test]
    async fn a_run_stops_when_its_session_is_replaced() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let runner = svc.clone();
        let run = tokio::spawn(async move { runner.run(Request::new(pb::RunRequest { seconds: 5.0 })).await });
        while svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner().time_s == 0.0 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        svc.unload(Request::new(pb::Empty {})).await.unwrap();
        *svc.shared.lock() = Some(open_loop_session(RunMode::Lockstep));
        let err = run.await.unwrap().unwrap_err();
        assert_eq!(kind_of(&err), "not_loaded", "{err:?}");
        let t = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner().time_s;
        assert_eq!(t, 0.0, "the old Run stepped the new session");
        svc.shutdown();
    }

    #[allow(clippy::await_holding_lock)] // the test holds the session on purpose, as a long call would
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_call_whose_client_left_before_it_got_the_session_does_nothing() {
        let wait = std::time::Duration::from_millis(100);
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let held = svc.shared.lock(); // a long call holds the session
        let sticks = svc.set_sticks(Request::new(pb::Sticks { throttle: 1.0, ..Default::default() }));
        assert!(tokio::time::timeout(wait, sticks).await.is_err(), "the client gives up while it waits");
        drop(held);
        tokio::time::sleep(wait * 2).await;
        let state = svc.run(Request::new(pb::RunRequest { seconds: 0.2 })).await.unwrap().into_inner();
        assert!(state.motor_cmd.iter().all(|m| *m == 0.0), "the abandoned SetSticks ran later: {:?}", state.motor_cmd);

        // Unload is the exception: it still runs, so a dropped Unload never leaves Betaflight running.
        let held = svc.shared.lock();
        let unload = svc.unload(Request::new(pb::Empty {}));
        assert!(tokio::time::timeout(wait, unload).await.is_err());
        drop(held);
        tokio::time::sleep(wait * 2).await;
        assert!(svc.shared.lock().is_none(), "a cancelled Unload still unloads");
        svc.shutdown();
    }

    #[tokio::test]
    async fn run_refuses_more_than_a_simulated_day() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let err = svc.run(Request::new(pb::RunRequest { seconds: 86_401.0 })).await.unwrap_err();
        assert_eq!(kind_of(&err), "invalid_argument");
        svc.shutdown();
    }

    #[tokio::test]
    async fn a_numerical_failure_is_published_as_a_sim_error_event() {
        let mut session = open_loop_session(RunMode::Lockstep);
        session.vehicle.put_nan_on_the_throttle();
        let svc = service_with(session);
        let mut events = svc.subscribe();
        let err = svc.run(Request::new(pb::RunRequest { seconds: 0.1 })).await.unwrap_err();
        assert_eq!(kind_of(&err), "numerical", "{err:?}");
        let mut kinds = Vec::new();
        while let Ok(e) = events.try_recv() {
            kinds.push(e.kind());
        }
        assert!(kinds.contains(&pb::EventKind::SimError), "{kinds:?}");
        svc.shutdown();
    }

    #[tokio::test]
    async fn shutdown_waits_for_the_real_time_runner() {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        svc.shutdown();
        assert!(svc.runner_finished(), "the runner thread is still going after shutdown");
    }

    /// A Watch whose client gives up while it waits for the session (e.g. behind a long call) must not register a
    /// watcher later: the session would then never end with its last real watcher.
    #[allow(clippy::await_holding_lock)] // the test holds the session on purpose, as a long call would
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_watch_cancelled_while_it_waits_for_the_session_leaves_no_watcher() {
        let svc = service_with(open_loop_session(RunMode::Lockstep));
        let held = svc.shared.lock();
        let watch = svc.watch(Request::new(pb::Empty {}));
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), watch).await.is_err());
        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(svc.shared.watchers.load(Ordering::Acquire), 0);
        svc.shutdown();
    }
}
