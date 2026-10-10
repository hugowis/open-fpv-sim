//! gRPC service: one session at a time. Unary calls lock the session on blocking threads, the runner
//! thread paces real-time sessions, and the streaming calls live in `streams`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ofs_config::{FcKind, WorldConfig};
use ofs_core::scheduler::MAX_RUN_FOR_S;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::pacer::OverrunPolicy;
use crate::pb::{self, sim_server::Sim};
use crate::runner;
use crate::session::{event, panic_message, world_msg, RunMode, Session, Shared, Slot};
use crate::streams;
use crate::vehicle::{self, BuildOptions, Fault, Sticks};

pub use ofs_proto::PROTOCOL_VERSION;

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

fn invalid_state(message: &str) -> Status {
    error("invalid_state", Code::FailedPrecondition, message)
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
                    if let Some(s) = shared.lock().as_mut() {
                        s.poison(SimError::Other(poisoned));
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
        Ok(Response::new(pb::HandshakeReply { protocol_version: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() }))
    }

    async fn load(&self, req: Request<pb::LoadRequest>) -> Result<Response<pb::LoadReply>, Status> {
        let req = req.into_inner();
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
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let world = if req.world_path.is_empty() {
            WorldConfig::open_field()
        } else {
            ofs_config::world::load(Path::new(&req.world_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
        };
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop), world };
        let keep_alive = req.keep_alive;
        self.blocking(move |shared, slot| {
            *slot = None; // stop the previous vehicle (and its SITL) before the new one binds the ports
            let vehicle = vehicle::build(&cfg, &opts).map_err(|e| sim_error(&e))?;
            let configurator_address = vehicle.configurator_address().unwrap_or_default();
            let mut session = Session::new(vehicle, mode, policy, keep_alive);
            session.watched = shared.watchers.load(Ordering::Acquire) > 0;
            *slot = Some(session);
            Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz, configurator_address })
        })
        .await
    }

    async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
        let sticks = parse_sticks(req.into_inner())?;
        self.blocking(move |_, slot| {
            loaded(slot)?.vehicle.set_sticks(&sticks);
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
                            shared.publish([event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string())]);
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
        self.blocking_always(|_, slot| {
            *slot = None;
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
        let fault = match req.into_inner().kind {
            Some(pb::fault::Kind::RadioLinkLoss(_)) => Fault::RadioLinkLoss,
            None => return Err(error("invalid_argument", Code::InvalidArgument, "the fault has no kind")),
        };
        self.blocking(move |_, slot| {
            loaded(slot)?.vehicle.set_fault(fault, true);
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
            let err = svc.stream_osd(Request::new(pb::StreamRequest { rate_hz: hz })).await.err().expect("rate rejected");
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
        session.vehicle.set_sticks(&crate::vehicle::Sticks { throttle: f64::NAN, ..Default::default() });
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
