//! The loaded session and the state every part of the server shares: gRPC handlers, the real-time runner
//! and the streams.
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ofs_config::world::{self as world_cfg, WorldConfig};
use ofs_config::{AntennaKind, Polarization};
use ofs_core::SimError;
use ofs_video::link::VideoSync;
use ofs_video::osd::OsdFrame;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{VideoInfo, Vehicle, VehicleState, VtxInfo};

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
    last_video_lost: Option<bool>,
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

/// "SNR 2.4 dB on patch".
pub(crate) fn video_message(v: &VideoInfo) -> String {
    format!("SNR {:.1} dB on {}", v.snr_db, v.active_antenna)
}

pub(crate) fn video_msg(v: &VideoInfo) -> pb::VideoLink {
    let sync = match (v.present, v.sync) {
        (false, _) => pb::VideoSync::Unspecified,
        (true, VideoSync::Locked) => pb::VideoSync::Locked,
        (true, VideoSync::Unstable) => pb::VideoSync::Unstable,
        (true, VideoSync::Lost) => pb::VideoSync::Lost,
    };
    pb::VideoLink {
        present: v.present,
        snr_db: v.snr_db,
        interference_dbm: v.interference_dbm,
        rssi: v.rssi_dbm.iter().map(|(name, dbm)| pb::AntennaRssi { name: name.clone(), rssi_dbm: *dbm }).collect(),
        active_antenna: v.active_antenna.clone(),
        noise: v.noise,
        sparkles: v.sparkles,
        chroma: v.chroma,
        sync: sync as i32,
    }
}

fn vec3_of(a: [f64; 3]) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: a[0], y: a[1], z: a[2] })
}

fn polarization_name(p: Polarization) -> &'static str {
    match p {
        Polarization::Rhcp => "rhcp",
        Polarization::Lhcp => "lhcp",
        Polarization::Linear => "linear",
    }
}

/// The world as the protocol carries it. Emitters given by band and channel carry their frequency.
pub(crate) fn world_msg(w: &WorldConfig) -> pb::World {
    pb::World {
        name: w.name.clone(),
        pilot_position_ned_m: vec3_of(w.pilot.position_ned_m),
        pilot_facing_deg: w.pilot.facing_deg,
        antennas: w
            .receiver
            .antennas
            .iter()
            .map(|a| pb::ReceiverAntenna {
                name: a.name.clone(),
                kind: match a.kind {
                    AntennaKind::Omni => "omni",
                    AntennaKind::Patch => "patch",
                }
                .into(),
                gain_dbi: a.gain_dbi,
                beamwidth_deg: a.beamwidth_deg.unwrap_or(0.0),
                polarization: polarization_name(a.polarization).into(),
                aim_az_deg: a.aim_az_deg,
                aim_el_deg: a.aim_el(),
            })
            .collect(),
        objects: w
            .objects
            .iter()
            .map(|o| pb::WorldObject {
                name: o.name.clone(),
                shape: match o.shape {
                    world_cfg::Shape::Box => "box",
                    world_cfg::Shape::Cylinder => "cylinder",
                }
                .into(),
                center_ned_m: vec3_of(o.center_ned_m),
                size_m: vec3_of(o.size_m.unwrap_or_default()),
                radius_m: o.radius_m.unwrap_or(0.0),
                height_m: o.height_m.unwrap_or(0.0),
                color: vec3_of(o.color),
                rf_loss_db: o.rf_loss_db,
            })
            .collect(),
        emitters: w
            .emitters
            .iter()
            .map(|e| pb::Emitter {
                name: e.name.clone(),
                position_ned_m: vec3_of(e.position_ned_m),
                freq_mhz: crate::vehicle::emitter_freq_mhz(e).unwrap_or(0.0),
                power_mw: e.power_mw,
            })
            .collect(),
    }
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
            last_video_lost: None,
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
    /// VTX changes, dropped UART bytes, video sync lost or regained.
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
        if s.video.present {
            let lost = s.video.sync == VideoSync::Lost;
            if self.last_video_lost.is_some_and(|was| was != lost) {
                let (kind, what) = if lost { (pb::EventKind::VideoLost, "video lost") } else { (pb::EventKind::VideoRestored, "video restored") };
                out.push(event(s.time_s, kind, format!("{what}: {}", video_message(&s.video))));
            }
            self.last_video_lost = Some(lost);
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
            video: Some(video_msg(&s.video)),
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
    fn losing_and_regaining_video_sync_become_events() {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        let mut s = session.vehicle.state();
        s.video = VideoInfo {
            present: true,
            snr_db: 20.0,
            interference_dbm: -150.0,
            rssi_dbm: vec![("omni".into(), -70.0)],
            active_antenna: "omni".into(),
            noise: 0.1,
            sparkles: 0.0,
            chroma: 1.0,
            sync: VideoSync::Locked,
        };
        let video_events = |events: Vec<pb::Event>| {
            events
                .into_iter()
                .filter(|e| e.kind == pb::EventKind::VideoLost as i32 || e.kind == pb::EventKind::VideoRestored as i32)
                .collect::<Vec<_>>()
        };
        assert!(video_events(session.events_for(&s)).is_empty(), "the first sample is not a change");
        s.video.sync = VideoSync::Unstable;
        assert!(video_events(session.events_for(&s)).is_empty(), "tearing is not a loss");
        s.video.sync = VideoSync::Lost;
        s.video.snr_db = -2.04;
        let lost = video_events(session.events_for(&s));
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].kind, pb::EventKind::VideoLost as i32);
        assert_eq!(lost[0].message, "video lost: SNR -2.0 dB on omni");
        assert!(video_events(session.events_for(&s)).is_empty(), "still lost: no new event");
        s.video.sync = VideoSync::Locked;
        s.video.snr_db = 9.0;
        let back = video_events(session.events_for(&s));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].kind, pb::EventKind::VideoRestored as i32);
        assert_eq!(back[0].message, "video restored: SNR 9.0 dB on omni");
    }

    #[test]
    fn the_video_message_says_absent_without_a_vtx() {
        let msg = video_msg(&VideoInfo::default());
        assert!(!msg.present);
        assert_eq!(msg.sync(), pb::VideoSync::Unspecified);
        let lost = video_msg(&VideoInfo { present: true, sync: VideoSync::Lost, ..VideoInfo::default() });
        assert_eq!(lost.sync(), pb::VideoSync::Lost);
    }

    #[test]
    fn the_world_message_carries_the_world_file() {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"));
        let msg = world_msg(&ofs_config::world::load(path).unwrap());
        assert_eq!(msg.name, "Flat field");
        assert_eq!(msg.pilot_position_ned_m, Some(pb::Vec3 { x: -3.0, y: 2.0, z: -1.7 }));
        let patch = &msg.antennas[1];
        assert_eq!((patch.name.as_str(), patch.kind.as_str(), patch.beamwidth_deg, patch.polarization.as_str()), ("patch", "patch", 60.0, "rhcp"));
        assert_eq!(msg.objects.len(), 30);
        let pylon = msg.objects.iter().find(|o| o.name == "Pylon1L").unwrap();
        assert_eq!((pylon.shape.as_str(), pylon.radius_m, pylon.height_m), ("cylinder", 0.15, 3.0));
        let b = msg.objects.iter().find(|o| o.name == "BuildingB").unwrap();
        assert_eq!((b.shape.as_str(), b.size_m, b.rf_loss_db), ("box", Some(pb::Vec3 { x: 10.0, y: 10.0, z: 22.0 }), 30.0));
        assert_eq!((msg.emitters[0].name.as_str(), msg.emitters[0].freq_mhz), ("parked-quad", 5695.0), "R2, from band and channel");
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
