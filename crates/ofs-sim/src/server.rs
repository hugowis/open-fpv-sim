//! gRPC service: one session at a time. Unary calls lock the session on blocking threads, the runner
//! thread paces real-time sessions, and the streaming calls live in `streams`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ofs_config::FcKind;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::pacer::OverrunPolicy;
use crate::pb::{self, sim_server::Sim};
use crate::runner;
use crate::session::{event, panic_message, RunMode, Session, Shared, Slot};
use crate::streams;
use crate::vehicle::{self, BuildOptions, Fault, Sticks};

pub use ofs_proto::PROTOCOL_VERSION;

#[derive(Clone)]
pub struct SimService {
    shared: Arc<Shared>,
    data_dir: PathBuf,
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

/// Sets its flag when dropped: a Run whose client went away (tonic drops its future) stops at the next chunk.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl SimService {
    pub fn new(data_dir: PathBuf) -> Self {
        let shared = Shared::new();
        runner::spawn(shared.clone());
        Self { shared, data_dir }
    }

    /// Every event published from now on (the Watch RPC reads the same channel).
    pub fn subscribe(&self) -> broadcast::Receiver<pb::Event> {
        self.shared.events.subscribe()
    }

    /// Stops the real-time runner and unloads the session, stopping Betaflight SITL. For server shutdown.
    pub fn shutdown(&self) {
        self.shared.stop();
        *self.shared.lock() = None;
    }

    async fn blocking<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&Shared, &mut Slot) -> Result<T, Status> + Send + 'static,
    {
        let shared = self.shared.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let mut slot = shared.lock();
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
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop) };
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

    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::State>, Status> {
        let seconds = req.into_inner().seconds;
        if !seconds.is_finite() || seconds < 0.0 {
            let message = format!("seconds must be finite and >= 0 (got {seconds})");
            return Err(error("invalid_argument", Code::InvalidArgument, message));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(cancelled.clone());
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if let Some(f) = &s.failure {
                return Err(sim_error(f)); // also for a duration of zero ticks, which never reaches `step`
            }
            if s.mode == RunMode::Realtime && s.running {
                return Err(invalid_state("pause the real-time session before calling Run"));
            }
            let hz = u64::from(s.vehicle.base_hz());
            let total = (seconds * hz as f64).round() as u64;
            let chunk = (hz / 20).max(1); // 50 ms of simulated time between cancellation checks
            let healthy = s.failure.is_none();
            let mut done = 0;
            while done < total {
                if cancelled.load(Ordering::Acquire) {
                    return Err(Status::cancelled("the client went away"));
                }
                if shared.stopping() {
                    return Err(Status::unavailable("server shutting down")); // `shutdown` waits for this lock
                }
                let n = chunk.min(total - done);
                let result = s.step(n);
                shared.publish(s.changes());
                if let Err(e) = result {
                    if healthy && s.failure.is_some() {
                        shared.publish([event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string())]);
                    }
                    return Err(sim_error(&e));
                }
                done += n;
            }
            Ok(s.state_msg())
        })
        .await
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

    async fn unload(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
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
        let state = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner();
        let vtx = state.vtx.expect("State.vtx is always set");
        assert!(!vtx.present);
        assert_eq!(state.serial_dropped_bytes, 0);
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
}
