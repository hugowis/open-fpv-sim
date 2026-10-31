//! The loaded session and the state every part of the server shares: gRPC handlers, the real-time runner
//! and the streams.
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ofs_config::world::{self as world_cfg, Course, RaceMode, WorldConfig};
use ofs_config::{AntennaKind, Polarization};
use ofs_core::SimError;
use ofs_video::link::VideoSync;
use ofs_video::osd::OsdFrame;
use tokio::sync::broadcast;

use crate::blackbox::BlackboxCopy;
use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::race::{RaceEvent, RaceInfo};
use crate::repro::BundleSource;
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
    last_collision_count: u64,
    last_restarts: u32,
    last_vtx: Option<VtxInfo>,
    last_dropped: u64,
    last_video_lost: Option<bool>,
    /// The faults that were active at the last `events_for`; its edge raises the fault events.
    last_faults: Vec<String>,
    /// Unit tests only: the next `step` panics (exercises panic containment).
    #[cfg(test)]
    pub panic_on_step: bool,
    /// The quad file's stem: recordings are `<logs dir>/<stem>-<n>.mcap`.
    pub quad_stem: String,
    /// How the session was loaded: what a repro bundle needs. `None` (unit tests) writes no bundle.
    pub repro: Option<BundleSource>,
    /// The repro bundle of this session's failure: written once, when the failure is first set.
    pub bundle: Option<Result<PathBuf, String>>,
    /// Betaflight sessions: copies SITL's Blackbox logs when the session ends. Declared last on purpose: fields
    /// drop in order, so the vehicle (and SITL) is gone before the copy runs.
    pub blackbox: Option<BlackboxCopy>,
}

impl Drop for Session {
    /// A session on Betaflight that ends armed disarms it first, so its Blackbox log is finished before SITL is
    /// stopped (and the log copied, by the field declared last). A failed session is not stepped again.
    fn drop(&mut self) {
        if self.failure.is_none() {
            self.vehicle.disarm_for_shutdown();
        }
    }
}

pub type Slot = Option<Session>;

pub fn event(time_s: f64, kind: pb::EventKind, message: impl Into<String>) -> pb::Event {
    pb::Event { time_s, kind: kind as i32, message: message.into() }
}

/// A race event as the protocol carries it; the text formats are documented in `docs/race-timing.md`:
/// "Gate0 (gate 0) start", "Gate1 (gate 1) split 8.412 s", "lap 1: 31.204 s (best)", "Gate1 (gate 1) hit".
/// One `changes()` batch in time order: race events keep their crossing time, which can sit between two
/// state-derived events of the same batch. Stable, so events at the same time keep the order they were raised.
pub(crate) fn events_in_time_order(mut out: Vec<pb::Event>) -> Vec<pb::Event> {
    out.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
    out
}

/// What race events are written with: for each course its gates' names by place, and the course names (empty for the
/// implicit course of a world without `[[courses]]`).
pub(crate) struct RaceNames {
    gates: Vec<Vec<String>>,
    courses: Vec<String>,
}

impl RaceNames {
    pub(crate) fn new(world: &WorldConfig, courses: &[Course]) -> Self {
        RaceNames {
            gates: courses.iter().map(|c| c.gates.iter().map(|&i| world.gates[i].name.clone()).collect()).collect(),
            courses: courses.iter().map(|c| c.name.clone()).collect(),
        }
    }

    fn gate(&self, course: usize, place: usize) -> &str {
        self.gates.get(course).and_then(|g| g.get(place)).map_or("unknown", String::as_str)
    }
}

/// The protocol events a race event makes: the last lap also ends the race.
pub(crate) fn race_events(e: &RaceEvent, names: &RaceNames) -> Vec<pb::Event> {
    let mut out = vec![race_event(e, names)];
    if let RaceEvent::LapCompleted { lap, time_s, finished: true, .. } = *e {
        out.push(event(time_s, pb::EventKind::RaceFinished, format!("race finished: {lap} {}", if lap == 1 { "lap" } else { "laps" })));
    }
    out
}

pub(crate) fn race_event(e: &RaceEvent, names: &RaceNames) -> pb::Event {
    match *e {
        RaceEvent::GatePassed { gate, course, time_s, split_s } => {
            let name = names.gate(course, gate);
            let message =
                if gate == 0 && split_s == 0.0 { format!("{name} (gate {gate}) start") } else { format!("{name} (gate {gate}) split {split_s:.3} s") };
            event(time_s, pb::EventKind::GatePassed, message)
        }
        RaceEvent::LapCompleted { lap, time_s, lap_s, best, .. } => {
            event(time_s, pb::EventKind::LapCompleted, format!("lap {lap}: {lap_s:.3} s{}", if best { " (best)" } else { "" }))
        }
        RaceEvent::GateHit { gate, course, time_s } => {
            let name = names.gate(course, gate);
            event(time_s, pb::EventKind::GateHit, format!("{name} (gate {gate}) hit"))
        }
        RaceEvent::TimeUp { time_s, laps } => {
            event(time_s, pb::EventKind::RaceFinished, format!("race finished: {laps} {}", if laps == 1 { "lap" } else { "laps" }))
        }
        RaceEvent::Reset { time_s, course } => match names.courses.get(course).filter(|n| !n.is_empty()) {
            Some(name) => event(time_s, pb::EventKind::RaceReset, format!("race reset: {name}")),
            None => event(time_s, pb::EventKind::RaceReset, "race reset"),
        },
    }
}

fn race_msg(r: &RaceInfo, courses: &[Course]) -> pb::RaceState {
    pb::RaceState {
        course: courses.get(r.course as usize).map(|c| c.name.clone()).unwrap_or_default(),
        course_index: r.course,
        mode: race_mode_msg(r.mode) as i32,
        time_limit_s: r.time_limit_s,
        time_left_s: r.time_left_s,
        present: r.present,
        running: r.running,
        finished: r.finished,
        lap_limit: r.lap_limit,
        next_gate: r.next_gate,
        gate_count: r.gate_count,
        laps: r.laps,
        current_lap_s: r.current_lap_s,
        last_lap_s: r.last_lap_s,
        best_lap_s: r.best_lap_s,
    }
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
        field_valid: v.field_valid,
        latency_ms: v.latency_ms,
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

fn antenna_msg(a: &world_cfg::AntennaSection) -> pb::ReceiverAntenna {
    pb::ReceiverAntenna {
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
    }
}

/// The world as the protocol carries it. Emitters given by band and channel carry their frequency.
pub(crate) fn world_msg(w: &WorldConfig) -> pb::World {
    pb::World {
        name: w.name.clone(),
        pilot_position_ned_m: vec3_of(w.pilot.position_ned_m),
        pilot_facing_deg: w.pilot.facing_deg,
        antennas: w.receiver.antennas.iter().map(antenna_msg).collect(),
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
        handset: Some(pb::Handset {
            position_ned_m: vec3_of(w.handset.position(&w.pilot)),
            antennas: w.handset.antennas.iter().map(antenna_msg).collect(),
        }),
        gates: w
            .gates
            .iter()
            .map(|g| pb::Gate {
                name: g.name.clone(),
                center_ned_m: vec3_of(g.center_ned_m),
                heading_deg: g.heading_deg,
                width_m: g.width_m,
                height_m: g.height_m,
                pitch_deg: g.pitch_deg,
                roll_deg: g.roll_deg,
            })
            .collect(),
        courses: if w.courses.is_empty() {
            Vec::new()
        } else {
            w.courses
                .iter()
                .zip(w.resolved_courses())
                .map(|(c, r)| pb::Course {
                    name: c.name.clone(),
                    gates: c.gates.clone(),
                    mode: race_mode_msg(c.mode) as i32,
                    laps: r.laps.unwrap_or(0),
                    time_limit_s: c.time_limit_s.unwrap_or(0.0),
                })
                .collect()
        },
        default_course: w.default_course() as u32,
        // The same deterministic bake the vehicle samples.
        terrain: w.terrain.as_ref().map(|section| {
            let t = section.build();
            let (nodes_n, nodes_e) = t.nodes();
            let [origin_north_m, origin_east_m] = t.origin_ne();
            pb::Terrain {
                origin_north_m,
                origin_east_m,
                cell_m: t.cell_m(),
                nodes_n: nodes_n as u32,
                nodes_e: nodes_e as u32,
                heights: t.heights().to_vec(),
                digest: t.digest(),
            }
        }),
    }
}

fn race_mode_msg(m: RaceMode) -> pb::RaceMode {
    match m {
        RaceMode::Laps => pb::RaceMode::Laps,
        RaceMode::TimeAttack => pb::RaceMode::TimeAttack,
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
            last_collision_count: 0,
            last_restarts: 0,
            last_vtx: None,
            last_dropped: 0,
            last_video_lost: None,
            last_faults: Vec::new(),
            #[cfg(test)]
            panic_on_step: false,
            quad_stem: "quad".into(),
            repro: None,
            bundle: None,
            blackbox: None,
        }
    }

    /// Poisons the session after an internal failure (a panic): it stays paused until the next Load.
    pub fn poison(&mut self, e: SimError) {
        let first = self.failure.is_none();
        self.failure = Some(e.clone());
        self.running = false;
        if first {
            self.capture_failure(&e);
        }
    }

    /// Writes the repro bundle for the failure `e`, once. It never fails the caller: a bundle that cannot be
    /// written is logged and kept as the reason, and the original error stands.
    fn capture_failure(&mut self, e: &SimError) {
        if self.bundle.is_some() {
            return;
        }
        let Some(source) = &self.repro else { return };
        let written = catch_unwind(AssertUnwindSafe(|| crate::repro::write_bundle(source, &self.vehicle, e)))
            .unwrap_or_else(|p| Err(format!("writing the bundle panicked: {}", panic_message(p.as_ref()))));
        match &written {
            Ok(path) => tracing::error!("session failed ({e}); repro bundle: {}", path.display()),
            Err(why) => tracing::error!("session failed ({e}); the repro bundle could not be written: {why}"),
        }
        self.bundle = Some(written);
    }

    /// `e` as the clients are told: with the repro bundle's folder (or why there is none) when the session wrote one.
    pub fn failure_text(&self, e: &SimError) -> String {
        match &self.bundle {
            Some(Ok(path)) => format!("{e} (repro bundle: {})", path.display()),
            Some(Err(why)) => format!("{e} (repro bundle failed: {why})"),
            None => e.to_string(),
        }
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
                self.capture_failure(&e);
            }
            return Err(e);
        }
        Ok(())
    }

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts,
    /// VTX changes, dropped UART bytes, video sync lost or regained, faults activating or clearing, an inputs file
    /// finishing, a recording failing.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let faults = self.vehicle.active_fault_names();
        let s = self.vehicle.state();
        let mut out = self.events_for(&s, faults);
        let raised = self.vehicle.take_race_events();
        if !raised.is_empty() {
            let names = RaceNames::new(self.vehicle.world(), self.vehicle.courses());
            out.extend(raised.iter().flat_map(|e| race_events(e, &names)));
        }
        if let Some(name) = self.vehicle.take_finished_inputs() {
            out.push(event(s.time_s, pb::EventKind::InputsEnded, format!("{name}: finished")));
        }
        if let Some(message) = self.vehicle.take_recording_failure() {
            out.push(event(s.time_s, pb::EventKind::RecordingStopped, format!("recording failed: {message}")));
        }
        if let Some(message) = self.vehicle.take_inputs_recording_failure() {
            out.push(event(s.time_s, pb::EventKind::InputsRecordingStopped, format!("inputs recording failed: {message}")));
        }
        events_in_time_order(out)
    }

    /// The events a vehicle state implies, compared against the remembered previous state; `faults` is the
    /// active faults' names. Split from `changes` so the rules are testable with a hand-built state.
    pub(crate) fn events_for(&mut self, s: &VehicleState, faults: Vec<String>) -> Vec<pb::Event> {
        let mut out = Vec::new();
        if s.radio.link_up != self.last_link_up {
            self.last_link_up = s.radio.link_up;
            let (kind, message) =
                if s.radio.link_up { (pb::EventKind::LinkUp, "radio link up") } else { (pb::EventKind::LinkDown, "radio link lost") };
            out.push(event(s.time_s, kind, message));
        }
        if s.collision_event_count != self.last_collision_count {
            self.last_collision_count = s.collision_event_count;
            let message = if s.collision_object < 0 {
                format!("HARD LANDING {:.1} m/s", s.collision_speed_mps)
            } else {
                let name = self
                    .vehicle
                    .world()
                    .objects
                    .get(s.collision_object.max(0) as usize)
                    .map_or("unknown", |o| o.name.as_str());
                format!("HIT {name} {:.1} m/s", s.collision_speed_mps)
            };
            out.push(event(s.time_s, pb::EventKind::Collision, message));
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
        for name in &faults {
            if !self.last_faults.contains(name) {
                out.push(event(s.time_s, pb::EventKind::FaultInjected, name.clone()));
            }
        }
        for name in &self.last_faults {
            if !faults.contains(name) {
                out.push(event(s.time_s, pb::EventKind::FaultCleared, name.clone()));
            }
        }
        self.last_faults = faults;
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
            battery_cell_voltages: s.battery_cell_voltages,
            motor_rpm: s.motor_rpm,
            motor_cmd: s.motor_cmd,
            radio: Some(pb::RadioLink {
                tx_enabled: s.radio.tx_enabled,
                link_up: s.radio.link_up,
                lq_pct: s.radio.lq_pct,
                rssi_dbm: s.radio.rssi_dbm,
                snr_db: s.radio.snr_db,
                active_antenna: s.radio.active_antenna.clone(),
                downlink_lq_pct: s.radio.downlink_lq_pct,
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
            collision_speed_mps: s.collision_speed_mps,
            fc_telemetry: Some(pb::FcTelemetry {
                battery_v: s.fc_telemetry.battery_v,
                battery_a: s.fc_telemetry.battery_a,
                battery_mah: s.fc_telemetry.battery_mah,
                roll_deg: s.fc_telemetry.roll_deg,
                pitch_deg: s.fc_telemetry.pitch_deg,
                yaw_deg: s.fc_telemetry.yaw_deg,
                flight_mode: s.fc_telemetry.flight_mode.clone(),
                frame_count: s.fc_telemetry.frame_count,
                telemetry_age_s: s.fc_telemetry.age_s,
            }),
            race: Some(race_msg(&s.race, self.vehicle.courses())),
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
        Session::new(vehicle::build(&cfg, &opts, None).unwrap(), mode, OverrunPolicy::Warn, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::VtxInfo;
    use std::path::Path;

    #[test]
    fn a_batch_of_events_is_time_ordered_with_ties_kept_in_insertion_order() {
        let mut out = vec![event(1.0, pb::EventKind::LinkDown, "early state event"), event(3.0, pb::EventKind::LinkUp, "later state event")];
        let names = names(&["Gate0", "Gate1"]);
        let race = [RaceEvent::GatePassed { gate: 1, course: 0, time_s: 2.0, split_s: 2.0 }];
        out.extend(race.iter().map(|e| race_event(e, &names)));
        let times: Vec<f64> = events_in_time_order(out).iter().map(|e| e.time_s).collect();
        assert_eq!(times, vec![1.0, 2.0, 3.0], "the race crossing sits between the two state events");
        let tied =
            events_in_time_order(vec![event(1.0, pb::EventKind::LinkDown, "first"), event(1.0, pb::EventKind::LinkUp, "second")]);
        assert_eq!((tied[0].message.as_str(), tied[1].message.as_str()), ("first", "second"), "a stable sort keeps ties");
    }

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
        let first = session.events_for(&s, session.vehicle.active_fault_names());
        assert!(first.iter().all(|e| e.kind != pb::EventKind::VtxChanged as i32), "the first sample is not a change");
        s.vtx.channel = 3;
        s.vtx.freq_mhz = 5732;
        let events = session.events_for(&s, session.vehicle.active_fault_names());
        let change = events.iter().find(|e| e.kind == pb::EventKind::VtxChanged as i32).expect("a vtx_changed event");
        assert_eq!(change.message, "R3 5732 MHz 200 mW");
        assert!(session.events_for(&s, session.vehicle.active_fault_names()).is_empty(), "no change, no event");
        s.serial_dropped_bytes = 12;
        let events = session.events_for(&s, session.vehicle.active_fault_names());
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
            field_valid: false,
            latency_ms: 0,
        };
        let video_events = |events: Vec<pb::Event>| {
            events
                .into_iter()
                .filter(|e| e.kind == pb::EventKind::VideoLost as i32 || e.kind == pb::EventKind::VideoRestored as i32)
                .collect::<Vec<_>>()
        };
        assert!(video_events(session.events_for(&s, session.vehicle.active_fault_names())).is_empty(), "the first sample is not a change");
        s.video.sync = VideoSync::Unstable;
        assert!(video_events(session.events_for(&s, session.vehicle.active_fault_names())).is_empty(), "tearing is not a loss");
        s.video.sync = VideoSync::Lost;
        s.video.snr_db = -2.04;
        let lost = video_events(session.events_for(&s, session.vehicle.active_fault_names()));
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].kind, pb::EventKind::VideoLost as i32);
        assert_eq!(lost[0].message, "video lost: SNR -2.0 dB on omni");
        assert!(video_events(session.events_for(&s, session.vehicle.active_fault_names())).is_empty(), "still lost: no new event");
        s.video.sync = VideoSync::Locked;
        s.video.snr_db = 9.0;
        let back = video_events(session.events_for(&s, session.vehicle.active_fault_names()));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].kind, pb::EventKind::VideoRestored as i32);
        assert_eq!(back[0].message, "video restored: SNR 9.0 dB on omni");
    }

    fn flat_session() -> Session {
        let world = ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap();
        let cfg = ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap();
        let opts = crate::vehicle::BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-session-race-test"),
            fc_override: Some(ofs_config::FcKind::OpenLoop),
            world,
        };
        Session::new(crate::vehicle::build(&cfg, &opts, None).unwrap(), RunMode::Lockstep, OverrunPolicy::Warn, true)
    }

    /// One unnamed course over these gates.
    fn names(gates: &[&str]) -> RaceNames {
        RaceNames { gates: vec![gates.iter().map(|g| g.to_string()).collect()], courses: vec![String::new()] }
    }

    #[test]
    fn a_named_course_is_in_the_reset_message_and_names_its_gates_by_place() {
        // Course 1 flies the file's gate 2 first.
        let names = RaceNames { gates: vec![vec!["A".into(), "B".into(), "C".into()], vec!["C".into(), "A".into()]], courses: vec!["Line".into(), "Hunt".into()] };
        assert_eq!(race_event(&RaceEvent::Reset { time_s: 1.0, course: 1 }, &names).message, "race reset: Hunt");
        assert_eq!(race_event(&RaceEvent::GatePassed { gate: 1, course: 1, time_s: 2.0, split_s: 1.0 }, &names).message, "A (gate 1) split 1.000 s");
        let up = race_event(&RaceEvent::TimeUp { time_s: 61.0, laps: 4 }, &names);
        assert_eq!((up.kind, up.time_s, up.message.as_str()), (pb::EventKind::RaceFinished as i32, 61.0, "race finished: 4 laps"));
        assert_eq!(race_event(&RaceEvent::TimeUp { time_s: 61.0, laps: 1 }, &names).message, "race finished: 1 lap");
    }

    #[test]
    fn race_events_become_gate_passed_and_lap_completed_events_with_the_documented_text() {
        use crate::race::RaceEvent;
        let names = names(&["Gate0", "Gate1", "Gate2"]);
        let start = race_event(&RaceEvent::GatePassed { gate: 0, course: 0, time_s: 2.5, split_s: 0.0 }, &names);
        assert_eq!((start.kind, start.time_s, start.message.as_str()), (pb::EventKind::GatePassed as i32, 2.5, "Gate0 (gate 0) start"));
        let split = race_event(&RaceEvent::GatePassed { gate: 1, course: 0, time_s: 10.9124, split_s: 8.4123 }, &names);
        assert_eq!((split.kind, split.message.as_str()), (pb::EventKind::GatePassed as i32, "Gate1 (gate 1) split 8.412 s"));
        let first = race_event(&RaceEvent::LapCompleted { lap: 1, time_s: 33.0, lap_s: 31.2041, best: true, finished: false }, &names);
        assert_eq!((first.kind, first.time_s, first.message.as_str()), (pb::EventKind::LapCompleted as i32, 33.0, "lap 1: 31.204 s (best)"));
        let slower = race_event(&RaceEvent::LapCompleted { lap: 2, time_s: 66.0, lap_s: 33.0, best: false, finished: false }, &names);
        assert_eq!(slower.message, "lap 2: 33.000 s");
        let hit = race_event(&RaceEvent::GateHit { gate: 2, course: 0, time_s: 40.25 }, &names);
        assert_eq!((hit.kind, hit.time_s, hit.message.as_str()), (pb::EventKind::GateHit as i32, 40.25, "Gate2 (gate 2) hit"));
        let reset = race_event(&RaceEvent::Reset { time_s: 41.0, course: 0 }, &names);
        assert_eq!((reset.kind, reset.time_s, reset.message.as_str()), (pb::EventKind::RaceReset as i32, 41.0, "race reset"));
        let last = race_events(&RaceEvent::LapCompleted { lap: 3, time_s: 99.0, lap_s: 30.0, best: false, finished: true }, &names);
        let shown: Vec<(i32, &str)> = last.iter().map(|e| (e.kind, e.message.as_str())).collect();
        assert_eq!(
            shown,
            [(pb::EventKind::LapCompleted as i32, "lap 3: 30.000 s"), (pb::EventKind::RaceFinished as i32, "race finished: 3 laps")],
            "the last lap also ends the race"
        );
        assert_eq!(race_events(&RaceEvent::LapCompleted { lap: 1, time_s: 9.0, lap_s: 9.0, best: true, finished: true }, &names)[1].message, "race finished: 1 lap");
    }

    #[test]
    fn world_msg_carries_the_gates_and_state_msg_the_race() {
        let session = flat_session();
        let world = world_msg(session.vehicle.world());
        assert_eq!(world.gates.len(), 3);
        let g = &world.gates[0];
        assert_eq!((g.name.as_str(), g.heading_deg, g.width_m, g.height_m), ("Gate0", 0.0, 3.08, 2.44));
        assert_eq!((g.pitch_deg, g.roll_deg), (0.0, 0.0));
        assert_eq!(g.center_ned_m, Some(pb::Vec3 { x: 30.0, y: 0.0, z: -1.22 }));
        let race = session.state_msg().race.expect("a race message");
        assert_eq!((race.present, race.running, race.gate_count, race.laps, race.next_gate), (true, false, 3, 0, 0));
        assert!(world.courses.is_empty() && race.course.is_empty(), "a world without [[courses]] names no course");
        assert_eq!((race.course_index, race.mode, race.time_limit_s), (0, pb::RaceMode::Laps as i32, 0.0));
        let bare = testing::open_loop_session(RunMode::Lockstep);
        assert!(!bare.state_msg().race.unwrap().present);
    }

    #[test]
    fn a_new_collision_count_raises_one_collision_event_naming_the_object() {
        let world = ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap();
        let building_a = world.objects.iter().position(|o| o.name == "BuildingA").expect("the flat world has BuildingA") as i32;
        let cfg = ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap();
        let opts = crate::vehicle::BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-session-collision-test"),
            fc_override: Some(ofs_config::FcKind::OpenLoop),
            world,
        };
        let mut session = Session::new(crate::vehicle::build(&cfg, &opts, None).unwrap(), RunMode::Lockstep, OverrunPolicy::Warn, true);
        session.last_link_up = true; // the state's link edge is already consumed: only collisions raise here
        let state = crate::vehicle::VehicleState {
            time_s: 1.0,
            pos_ned_m: glam::DVec3::ZERO,
            vel_ned_mps: glam::DVec3::ZERO,
            att: glam::DQuat::IDENTITY,
            rate_frd_radps: glam::DVec3::ZERO,
            battery_voltage_v: 25.0,
            battery_current_a: 0.0,
            battery_cell_voltages: vec![25.0 / 6.0; 6],
            motor_rpm: vec![0.0; 4],
            motor_cmd: vec![0.0; 4],
            radio: crate::vehicle::RadioState {
                tx_enabled: true,
                link_up: true,
                lq_pct: 100.0,
                rssi_dbm: -60.0,
                snr_db: 49.0,
                active_antenna: "antenna".into(),
                downlink_lq_pct: 100.0,
            },
            fc_telemetry: crate::vehicle::FcTelemetrySnapshot {
                battery_v: 0.0,
                battery_a: 0.0,
                battery_mah: 0.0,
                roll_deg: 0.0,
                pitch_deg: 0.0,
                yaw_deg: 0.0,
                flight_mode: String::new(),
                frame_count: 0,
                age_s: f64::INFINITY,
            },
            fc_restarts: 0,
            vtx: crate::vehicle::VtxInfo { present: false, band: 0, channel: 0, freq_mhz: 0, power_mw: 0, pit_mode: false },
            serial_dropped_bytes: 0,
            video: crate::vehicle::VideoInfo::default(),
            collision_speed_mps: 7.2,
            collision_object: building_a,
            collision_event_count: 1,
            race: Default::default(),
        };
        let events = session.events_for(&state, session.vehicle.active_fault_names());
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].kind, pb::EventKind::Collision as i32);
        assert_eq!(events[0].message, "HIT BuildingA 7.2 m/s");
        // The same count again raises nothing: the edge is the counter.
        assert!(session.events_for(&state, session.vehicle.active_fault_names()).is_empty());
        // A negative object index is the ground.
        let mut ground = state;
        ground.collision_object = -1;
        ground.collision_event_count = 2;
        let events = session.events_for(&ground, session.vehicle.active_fault_names());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "HARD LANDING 7.2 m/s");
    }

    #[test]
    fn faults_activating_and_clearing_become_one_event_each() {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        let s = session.vehicle.state();
        assert!(session.events_for(&s, Vec::new()).is_empty(), "no faults, no events");
        let set = || vec!["radio_loss".to_string(), "weak_cell".to_string()];
        let events = session.events_for(&s, set());
        assert_eq!(
            events.iter().map(|e| (e.kind(), e.message.as_str())).collect::<Vec<_>>(),
            [(pb::EventKind::FaultInjected, "radio_loss"), (pb::EventKind::FaultInjected, "weak_cell")],
            "each newly active fault raises one fault_injected naming it"
        );
        assert!(session.events_for(&s, set()).is_empty(), "an unchanged set raises nothing");
        let events = session.events_for(&s, vec!["weak_cell".to_string()]);
        assert_eq!(
            events.iter().map(|e| (e.kind(), e.message.as_str())).collect::<Vec<_>>(),
            [(pb::EventKind::FaultCleared, "radio_loss")],
            "only the fault that went away is cleared"
        );
        let events = session.events_for(&s, Vec::new());
        assert_eq!(events.iter().map(|e| (e.kind(), e.message.as_str())).collect::<Vec<_>>(), [(pb::EventKind::FaultCleared, "weak_cell")]);
    }

    #[test]
    fn the_video_message_says_absent_without_a_vtx() {
        let msg = video_msg(&VideoInfo::default());
        assert!(!msg.present);
        assert!(!msg.field_valid);
        assert_eq!(msg.latency_ms, 0);
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

    /// An open-loop session that writes its repro bundles under `logs` and fails on its first step.
    fn failing_session(logs: &Path) -> Session {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        session.repro = Some(crate::repro::test_source(logs));
        session.vehicle.put_nan_on_the_throttle();
        session
    }

    fn bundles_in(logs: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(logs)
            .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".repro")).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    #[test]
    fn a_failed_step_writes_one_bundle() {
        let logs = tempfile::tempdir().unwrap();
        let mut session = failing_session(logs.path());
        let e = session.step(100).unwrap_err();
        assert!(matches!(e, SimError::NonFinite(_)), "{e:?}");
        assert_eq!(bundles_in(logs.path()), ["opendrone-5f-freestyle-1.repro"]);
        let text = session.failure_text(&e);
        assert!(text.starts_with(&e.to_string()) && text.contains("(repro bundle: "), "{text}");
        assert!(text.contains("opendrone-5f-freestyle-1.repro"), "{text}");
        // The session stays failed; asking again, or poisoning it, writes nothing more.
        assert!(session.step(10).is_err());
        session.poison(SimError::Other("internal panic: later".into()));
        assert_eq!(bundles_in(logs.path()), ["opendrone-5f-freestyle-1.repro"]);
    }

    #[test]
    fn an_unwritable_logs_dir_does_not_hide_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_directory = dir.path().join("logs");
        std::fs::write(&not_a_directory, "a file where the logs directory should be").unwrap();
        let mut session = failing_session(&not_a_directory);
        let e = session.step(100).unwrap_err();
        assert!(matches!(e, SimError::NonFinite(_)), "the original error stands: {e:?}");
        assert!(session.failure.is_some());
        assert!(session.failure_text(&e).contains("(repro bundle failed: "), "{}", session.failure_text(&e));
    }

    #[test]
    fn a_panic_poison_writes_a_bundle_too() {
        let logs = tempfile::tempdir().unwrap();
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        session.repro = Some(crate::repro::test_source(logs.path()));
        session.poison(SimError::Other("internal panic: boom".into()));
        assert_eq!(bundles_in(logs.path()).len(), 1);
        assert!(session.failure_text(&SimError::Other("x".into())).contains("repro bundle: "));
    }

    #[test]
    fn a_session_without_load_data_writes_nothing() {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        session.poison(SimError::Other("boom".into()));
        assert!(session.bundle.is_none());
        assert_eq!(session.failure_text(&SimError::Other("boom".into())), "boom");
    }
}
