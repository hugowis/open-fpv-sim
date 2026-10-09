//! The loaded session and the state every part of the server shares: gRPC handlers, the real-time runner
//! and the streams.
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ofs_core::SimError;
use ofs_video::osd::OsdFrame;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{Vehicle, VehicleState, VtxInfo};

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
    last_vtx: Option<VtxInfo>,
    last_dropped: u64,
    /// Unit tests only: the next `step` panics (exercises panic containment).
    #[cfg(test)]
    pub panic_on_step: bool,
}

pub type Slot = Option<Session>;

pub fn event(time_s: f64, kind: pb::EventKind, message: impl Into<String>) -> pb::Event {
    pb::Event { time_s, kind: kind as i32, message: message.into() }
}

fn vec3(v: glam::DVec3) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: v.x, y: v.y, z: v.z })
}

/// "R3 5732 MHz 600 mW", or "user frequency 5800 MHz 600 mW (pit mode)".
pub(crate) fn vtx_message(v: &VtxInfo) -> String {
    const BANDS: [char; 6] = ['A', 'B', 'E', 'F', 'R', 'L'];
    let channel = match v.band {
        1..=6 => format!("{}{}", BANDS[usize::from(v.band) - 1], v.channel),
        _ => "user frequency".to_string(),
    };
    format!("{channel} {} MHz {} mW{}", v.freq_mhz, v.power_mw, if v.pit_mode { " (pit mode)" } else { "" })
}

pub(crate) fn osd_frame_msg(f: &OsdFrame) -> pb::OsdFrame {
    pb::OsdFrame {
        seq: f.seq,
        time_s: f.time_s,
        present: f.present,
        cols: f.grid.cols as u32,
        rows: f.grid.rows as u32,
        cells: f.grid.cells.iter().map(|c| c.packed()).collect(),
    }
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
            last_vtx: None,
            last_dropped: 0,
            #[cfg(test)]
            panic_on_step: false,
        }
    }

    /// Poisons the session after an internal failure (a panic): it stays paused until the next Load.
    pub fn poison(&mut self, e: SimError) {
        self.failure = Some(e);
        self.running = false;
    }

    /// Steps `ticks` base ticks. A firmware or numerical failure poisons the session and pauses it.
    pub fn step(&mut self, ticks: u64) -> Result<(), SimError> {
        #[cfg(test)]
        if self.panic_on_step {
            panic!("test panic in step");
        }
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

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts,
    /// VTX changes, dropped UART bytes.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let s = self.vehicle.state();
        self.events_for(&s)
    }

    /// The events a vehicle state implies, compared against the remembered previous state. Split from
    /// `changes` so the rules are testable with a hand-built state.
    pub(crate) fn events_for(&mut self, s: &VehicleState) -> Vec<pb::Event> {
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
        if s.vtx.present {
            if let Some(previous) = self.last_vtx {
                if previous != s.vtx {
                    out.push(event(s.time_s, pb::EventKind::VtxChanged, vtx_message(&s.vtx)));
                }
            }
            self.last_vtx = Some(s.vtx);
        }
        if s.serial_dropped_bytes != self.last_dropped {
            let dropped = s.serial_dropped_bytes.saturating_sub(self.last_dropped);
            self.last_dropped = s.serial_dropped_bytes;
            out.push(event(s.time_s, pb::EventKind::SerialOverflow, format!("{dropped} byte(s) of Betaflight UART output were dropped")));
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
            vtx: Some(pb::Vtx {
                present: s.vtx.present,
                band: u32::from(s.vtx.band),
                channel: u32::from(s.vtx.channel),
                freq_mhz: s.vtx.freq_mhz,
                power_mw: s.vtx.power_mw,
                pit_mode: s.vtx.pit_mode,
            }),
            serial_dropped_bytes: s.serial_dropped_bytes,
        }
    }

    /// The current OSD frame; absent (`present = false`, no cells) for a quad without an OSD.
    pub fn osd_msg(&self) -> pb::OsdFrame {
        match self.vehicle.osd_frame() {
            Some(frame) => osd_frame_msg(&frame),
            None => pb::OsdFrame { seq: 0, time_s: self.vehicle.time_s(), present: false, cols: 0, rows: 0, cells: Vec::new() },
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

/// A readable message from a panic payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::pacer::OverrunPolicy;
    use crate::vehicle::{self, BuildOptions};
    use ofs_config::FcKind;

    /// An open-loop session (no Betaflight needed) for unit tests.
    pub fn open_loop_session(mode: RunMode) -> Session {
        let quad = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
        let cfg = ofs_config::load(std::path::Path::new(quad)).unwrap();
        let opts = BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-unit-test-data"),
            fc_override: Some(FcKind::OpenLoop),
            world: ofs_config::WorldConfig::open_field(),
        };
        Session::new(vehicle::build(&cfg, &opts).unwrap(), mode, OverrunPolicy::Warn, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::VtxInfo;

    #[test]
    fn a_vtx_change_is_described_by_band_channel_frequency_and_power() {
        let raceband3 = VtxInfo { present: true, band: 5, channel: 3, freq_mhz: 5732, power_mw: 600, pit_mode: false };
        assert_eq!(vtx_message(&raceband3), "R3 5732 MHz 600 mW");
        let user = VtxInfo { band: 0, channel: 0, freq_mhz: 5800, pit_mode: true, ..raceband3 };
        assert_eq!(vtx_message(&user), "user frequency 5800 MHz 600 mW (pit mode)");
    }

    #[test]
    fn vtx_changes_and_dropped_bytes_become_events() {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        let mut s = session.vehicle.state();
        s.vtx = VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false };
        let first = session.events_for(&s);
        assert!(first.iter().all(|e| e.kind != pb::EventKind::VtxChanged as i32), "the first sample is not a change");
        s.vtx.channel = 3;
        s.vtx.freq_mhz = 5732;
        let events = session.events_for(&s);
        let change = events.iter().find(|e| e.kind == pb::EventKind::VtxChanged as i32).expect("a vtx_changed event");
        assert_eq!(change.message, "R3 5732 MHz 200 mW");
        assert!(session.events_for(&s).is_empty(), "no change, no event");
        s.serial_dropped_bytes = 12;
        let events = session.events_for(&s);
        let overflow = events.iter().find(|e| e.kind == pb::EventKind::SerialOverflow as i32).expect("a serial_overflow event");
        assert!(overflow.message.contains("12 byte"), "{}", overflow.message);
    }

    #[test]
    fn the_osd_message_packs_cells_row_major() {
        use ofs_video::osd::{Cell, OsdFrame, OsdGrid};
        let mut grid = OsdGrid::blank(2, 2);
        grid.cells[1] = Cell { ch: b'A', page: 1, blink: true };
        let msg = osd_frame_msg(&OsdFrame { seq: 7, time_s: 1.5, present: true, grid });
        assert_eq!((msg.seq, msg.cols, msg.rows, msg.present), (7, 2, 2, true));
        assert_eq!(msg.cells, vec![0x20, u32::from(b'A') | 1 << 8 | 1 << 10, 0x20, 0x20]);
    }
}
