//! gRPC service: one vehicle session at a time; all stepping happens on blocking threads.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ofs_config::FcKind;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status};

use crate::pb::{self, sim_server::Sim};
use crate::vehicle::{self, BuildOptions, Sticks, Vehicle, VehicleState};

pub const PROTOCOL_VERSION: u32 = 1;

struct Session {
    vehicle: Vehicle,
    /// Set after a firmware or numerical failure; the session stays paused until the next Load.
    failure: Option<SimError>,
}

type Slot = Option<Session>;

#[derive(Clone)]
pub struct SimService {
    session: Arc<Mutex<Slot>>,
    data_dir: PathBuf,
}

/// A status carrying the machine-readable `ofs-error-kind` metadata that clients map to typed errors.
pub fn error(kind: &'static str, code: Code, message: impl Into<String>) -> Status {
    let mut md = MetadataMap::new();
    md.insert("ofs-error-kind", kind.parse().expect("kind is ASCII"));
    Status::with_metadata(code, message.into(), md)
}

fn sim_error(e: &SimError) -> Status {
    match e {
        SimError::Firmware(m) => error("firmware", Code::Aborted, m.clone()),
        SimError::NonFinite(_) => error("numerical", Code::Aborted, e.to_string()),
        SimError::InvalidArgument(m) => error("invalid_argument", Code::InvalidArgument, m.clone()),
        SimError::Other(m) => error("internal", Code::Internal, m.clone()),
    }
}

fn loaded(slot: &mut Slot) -> Result<&mut Session, Status> {
    slot.as_mut().ok_or_else(|| error("not_loaded", Code::FailedPrecondition, "no quad loaded; call Load first"))
}

fn vec3(v: glam::DVec3) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: v.x, y: v.y, z: v.z })
}

fn state_msg(s: &VehicleState) -> pb::State {
    pb::State {
        time_s: s.time_s,
        position_ned_m: vec3(s.pos_ned_m),
        velocity_ned_mps: vec3(s.vel_ned_mps),
        attitude: Some(pb::Quat { w: s.att.w, x: s.att.x, y: s.att.y, z: s.att.z }),
        rate_frd_radps: vec3(s.rate_frd_radps),
        battery_voltage_v: s.battery_voltage_v,
        battery_current_a: s.battery_current_a,
        motor_rpm: s.motor_rpm.clone(),
        motor_cmd: s.motor_cmd.clone(),
    }
}

impl SimService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { session: Arc::new(Mutex::new(None)), data_dir }
    }

    async fn blocking<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&mut Slot) -> Result<T, Status> + Send + 'static,
    {
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().map_err(|_| error("internal", Code::Internal, "session lock poisoned"))?;
            f(&mut guard)
        })
        .await
        .map_err(|e| error("internal", Code::Internal, e.to_string()))?
        .map(Response::new)
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
        match pb::Mode::try_from(req.mode) {
            Ok(pb::Mode::Unspecified) | Ok(pb::Mode::Lockstep) => {}
            _ => return Err(error("invalid_argument", Code::InvalidArgument, "only lockstep mode is available in protocol version 1")),
        }
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop) };
        self.blocking(move |slot| {
            *slot = None; // stop the previous vehicle (and its SITL) before the new one binds the ports
            let vehicle = vehicle::build(&cfg, &opts).map_err(|e| sim_error(&e))?;
            *slot = Some(Session { vehicle, failure: None });
            Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz })
        })
        .await
    }

    async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
        let s = req.into_inner();
        if s.aux.len() > RC_AUX_COUNT {
            return Err(error("invalid_argument", Code::InvalidArgument, format!("at most {RC_AUX_COUNT} aux channels (got {})", s.aux.len())));
        }
        let mut aux = [-1.0; RC_AUX_COUNT];
        aux[..s.aux.len()].copy_from_slice(&s.aux);
        let sticks = Sticks { roll: s.roll, pitch: s.pitch, yaw: s.yaw, throttle: s.throttle, aux };
        let all_finite = [sticks.roll, sticks.pitch, sticks.yaw, sticks.throttle].iter().chain(aux.iter()).all(|v| v.is_finite());
        if !all_finite {
            return Err(error("invalid_argument", Code::InvalidArgument, "stick values must be finite"));
        }
        self.blocking(move |slot| {
            loaded(slot)?.vehicle.set_sticks(&sticks);
            Ok(pb::Empty {})
        })
        .await
    }

    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::State>, Status> {
        let seconds = req.into_inner().seconds;
        self.blocking(move |slot| {
            let s = loaded(slot)?;
            if let Some(f) = &s.failure {
                return Err(sim_error(f));
            }
            if let Err(e) = s.vehicle.run_for(seconds) {
                let status = sim_error(&e);
                if matches!(e, SimError::Firmware(_) | SimError::NonFinite(_)) {
                    s.failure = Some(e);
                }
                return Err(status);
            }
            Ok(state_msg(&s.vehicle.state()))
        })
        .await
    }

    async fn get_state(&self, _req: Request<pb::Empty>) -> Result<Response<pb::State>, Status> {
        self.blocking(|slot| Ok(state_msg(&loaded(slot)?.vehicle.state()))).await
    }

    async fn unload(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|slot| {
            *slot = None;
            Ok(pb::Empty {})
        })
        .await
    }
}
