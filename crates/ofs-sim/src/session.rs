//! The loaded session and the state every part of the server shares: gRPC handlers, the real-time runner
//! and the streams.
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ofs_core::SimError;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{Vehicle, VehicleState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Lockstep,
    Realtime,
}

/// Identities of loaded sessions, so a stream can tell its session from a later one.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub struct Session {
    /// Unique per loaded session (never 0).
    pub id: u64,
    pub vehicle: Vehicle,
    pub mode: RunMode,
    /// Real-time sessions only: paced to the wall clock right now.
    pub running: bool,
    /// Set after a firmware or numerical failure; the session stays paused until the next Load.
    pub failure: Option<SimError>,
    pub pacer: Pacer,
    /// Keep the session when its last watcher disconnects.
    pub keep_alive: bool,
    /// A watcher has seen this session: only watched sessions end with their watchers.
    pub watched: bool,
    last_link_up: bool,
    last_restarts: u32,
}

pub type Slot = Option<Session>;

pub fn event(time_s: f64, kind: pb::EventKind, message: impl Into<String>) -> pb::Event {
    pb::Event { time_s, kind: kind as i32, message: message.into() }
}

fn vec3(v: glam::DVec3) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: v.x, y: v.y, z: v.z })
}

impl Session {
    pub fn new(vehicle: Vehicle, mode: RunMode, policy: OverrunPolicy, keep_alive: bool) -> Self {
        let pacer = Pacer::new(policy, vehicle.base_hz());
        Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            vehicle,
            mode,
            running: false,
            failure: None,
            pacer,
            keep_alive,
            watched: false,
            last_link_up: false,
            last_restarts: 0,
        }
    }

    /// Steps `ticks` base ticks. A firmware or numerical failure poisons the session and pauses it.
    pub fn step(&mut self, ticks: u64) -> Result<(), SimError> {
        if let Some(f) = &self.failure {
            return Err(f.clone());
        }
        if let Err(e) = self.vehicle.step_ticks(ticks) {
            if matches!(e, SimError::Firmware(_) | SimError::NonFinite(_)) {
                self.failure = Some(e.clone());
                self.running = false;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let s = self.vehicle.state();
        let mut out = Vec::new();
        if s.radio.link_up != self.last_link_up {
            self.last_link_up = s.radio.link_up;
            let (kind, message) =
                if s.radio.link_up { (pb::EventKind::LinkUp, "radio link up") } else { (pb::EventKind::LinkDown, "radio link lost") };
            out.push(event(s.time_s, kind, message));
        }
        if s.fc_restarts != self.last_restarts {
            self.last_restarts = s.fc_restarts;
            out.push(event(s.time_s, pb::EventKind::FirmwareRestarted, "Betaflight rebooted; SITL relaunched"));
        }
        out
    }

    pub fn state_msg(&self) -> pb::State {
        let s: VehicleState = self.vehicle.state();
        pb::State {
            time_s: s.time_s,
            position_ned_m: vec3(s.pos_ned_m),
            velocity_ned_mps: vec3(s.vel_ned_mps),
            attitude: Some(pb::Quat { w: s.att.w, x: s.att.x, y: s.att.y, z: s.att.z }),
            rate_frd_radps: vec3(s.rate_frd_radps),
            battery_voltage_v: s.battery_voltage_v,
            battery_current_a: s.battery_current_a,
            motor_rpm: s.motor_rpm,
            motor_cmd: s.motor_cmd,
            radio: Some(pb::RadioLink {
                tx_enabled: s.radio.tx_enabled,
                link_up: s.radio.link_up,
                lq_pct: s.radio.lq_pct,
                rssi_dbm: s.radio.rssi_dbm,
            }),
            running: self.running,
            overruns: self.pacer.overruns(),
            fc_restarts: s.fc_restarts,
        }
    }
}

/// State shared by the gRPC handlers, the real-time runner thread and the streams.
pub struct Shared {
    slot: Mutex<Slot>,
    epoch: Instant,
    stopping: AtomicBool,
    pub events: broadcast::Sender<pb::Event>,
    /// Open Watch streams.
    pub watchers: AtomicUsize,
    /// A Pilot stream is connected.
    pub pilot: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            slot: Mutex::new(None),
            epoch: Instant::now(),
            stopping: AtomicBool::new(false),
            events,
            watchers: AtomicUsize::new(0),
            pilot: AtomicBool::new(false),
        })
    }

    /// The session slot. A panic while it was held does not lock the server out.
    pub fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Seconds since the server started (the real-time pacer's wall clock).
    pub fn wall_s(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    pub fn publish(&self, events: impl IntoIterator<Item = pb::Event>) {
        for e in events {
            let _ = self.events.send(e); // no subscribers is fine
        }
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }
}
