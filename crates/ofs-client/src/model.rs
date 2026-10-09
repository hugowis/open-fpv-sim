//! Plain data the client exchanges with its users: sticks in, telemetry/events/phases out.
use std::path::PathBuf;
use std::time::Duration;

use glam::DVec3;
use ofs_proto::pb;

use crate::error::{ClientError, ErrorKind};
use crate::frames::vec_to_godot;

pub const AUX_COUNT: usize = 4;

/// The transmitter's sticks: roll, pitch, yaw in [-1, 1], throttle in [0, 1], aux switches in [-1, 1]
/// (up to 4; aux 1 arms and aux 2 selects Angle mode in the reference quad's Betaflight config).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; AUX_COUNT] }
    }
}

impl Sticks {
    /// Clamps every value to its range. A non-finite value (which the server would refuse) becomes its neutral
    /// position: 0 for the sticks and throttle, -1 for an aux switch.
    pub fn sanitized(self) -> Sticks {
        fn fix(v: f64, lo: f64, hi: f64, neutral: f64) -> f64 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                neutral
            }
        }
        Sticks {
            roll: fix(self.roll, -1.0, 1.0, 0.0),
            pitch: fix(self.pitch, -1.0, 1.0, 0.0),
            yaw: fix(self.yaw, -1.0, 1.0, 0.0),
            throttle: fix(self.throttle, 0.0, 1.0, 0.0),
            aux: self.aux.map(|a| fix(a, -1.0, 1.0, -1.0)),
        }
    }

    pub(crate) fn to_pb(self) -> pb::Sticks {
        pb::Sticks { roll: self.roll, pitch: self.pitch, yaw: self.yaw, throttle: self.throttle, aux: self.aux.to_vec() }
    }
}

/// The video transmitter as of the last state message (all zero without a VTX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VtxInfo {
    pub present: bool,
    /// 1..=6 (A, B, E, F, R, L), 0 in user-frequency mode.
    pub band: u8,
    /// 1..=8, 0 in user-frequency mode.
    pub channel: u8,
    pub freq_mhz: u32,
    pub power_mw: u32,
    pub pit_mode: bool,
}

impl VtxInfo {
    pub fn from_pb(v: &pb::Vtx) -> VtxInfo {
        VtxInfo {
            present: v.present,
            band: v.band as u8,
            channel: v.channel as u8,
            freq_mhz: v.freq_mhz,
            power_mw: v.power_mw,
            pit_mode: v.pit_mode,
        }
    }

    /// "R3"; empty in user-frequency mode or without a VTX.
    pub fn channel_name(&self) -> String {
        const BANDS: [char; 6] = ['A', 'B', 'E', 'F', 'R', 'L'];
        match self.band {
            1..=6 if self.present => format!("{}{}", BANDS[usize::from(self.band) - 1], self.channel),
            _ => String::new(),
        }
    }
}

/// The goggles' hold on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoSync {
    /// No video link (the quad has no VTX).
    #[default]
    None,
    Locked,
    /// Tearing, line jitter.
    Unstable,
    /// Rolling, static.
    Lost,
}

impl VideoSync {
    pub fn as_str(self) -> &'static str {
        match self {
            VideoSync::None => "",
            VideoSync::Locked => "locked",
            VideoSync::Unstable => "unstable",
            VideoSync::Lost => "lost",
        }
    }
}

/// The analog video link at the goggles as of the last state message (`present = false` without a VTX).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub present: bool,
    pub snr_db: f64,
    pub interference_dbm: f64,
    /// Received power by goggle antenna, in the world file's order.
    pub rssi_dbm: Vec<(String, f64)>,
    pub active_antenna: String,
    /// Grain, 0 (clean) to 1 (static).
    pub noise: f64,
    pub sparkles: f64,
    /// Colour, 1 (full) to 0 (black and white).
    pub chroma: f64,
    pub sync: VideoSync,
}

impl Default for VideoInfo {
    fn default() -> Self {
        VideoInfo {
            present: false,
            snr_db: 0.0,
            interference_dbm: 0.0,
            rssi_dbm: Vec::new(),
            active_antenna: String::new(),
            noise: 0.0,
            sparkles: 0.0,
            chroma: 1.0,
            sync: VideoSync::None,
        }
    }
}

impl VideoInfo {
    pub fn from_pb(v: &pb::VideoLink) -> VideoInfo {
        let sync = match pb::VideoSync::try_from(v.sync) {
            Ok(pb::VideoSync::Locked) => VideoSync::Locked,
            Ok(pb::VideoSync::Unstable) => VideoSync::Unstable,
            Ok(pb::VideoSync::Lost) => VideoSync::Lost,
            Ok(pb::VideoSync::Unspecified) | Err(_) => VideoSync::None,
        };
        VideoInfo {
            present: v.present,
            snr_db: v.snr_db,
            interference_dbm: v.interference_dbm,
            rssi_dbm: v.rssi.iter().map(|r| (r.name.clone(), r.rssi_dbm)).collect(),
            active_antenna: v.active_antenna.clone(),
            noise: v.noise,
            sparkles: v.sparkles,
            chroma: v.chroma,
            sync,
        }
    }
}

/// A goggle antenna; `aim` is where it points (an omni's axis), a unit vector in Godot's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldAntenna {
    pub name: String,
    /// "omni" or "patch".
    pub kind: String,
    pub aim: DVec3,
}

/// An object on the field, in Godot's frame: a box's `size` is (east, height, north).
#[derive(Debug, Clone, PartialEq)]
pub struct WorldObject {
    pub name: String,
    /// "box" or "cylinder".
    pub shape: String,
    pub center: DVec3,
    pub size: DVec3,
    pub radius_m: f64,
    pub height_m: f64,
    /// r, g, b in 0..1.
    pub color: [f64; 3],
    pub rf_loss_db: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorldEmitter {
    pub name: String,
    pub position: DVec3,
    pub freq_mhz: f64,
    pub power_mw: f64,
}

/// The world the session flies in, converted to Godot's frame (+Y up, north is -Z).
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub name: String,
    pub pilot_position: DVec3,
    pub pilot_facing_deg: f64,
    pub antennas: Vec<WorldAntenna>,
    pub objects: Vec<WorldObject>,
    pub emitters: Vec<WorldEmitter>,
}

fn ned(v: Option<pb::Vec3>) -> DVec3 {
    let v = v.unwrap_or_default();
    DVec3::new(v.x, v.y, v.z)
}

/// A direction in NED from a heading (degrees clockwise from north) and an elevation (degrees up).
fn aim_ned(heading_deg: f64, elevation_deg: f64) -> DVec3 {
    let (h, e) = (heading_deg.to_radians(), elevation_deg.to_radians());
    DVec3::new(e.cos() * h.cos(), e.cos() * h.sin(), -e.sin())
}

impl World {
    pub fn from_pb(w: pb::World) -> World {
        let facing = w.pilot_facing_deg;
        World {
            name: w.name,
            pilot_position: vec_to_godot(ned(w.pilot_position_ned_m)),
            pilot_facing_deg: facing,
            antennas: w
                .antennas
                .into_iter()
                .map(|a| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind })
                .collect(),
            objects: w
                .objects
                .into_iter()
                .map(|o| {
                    let s = ned(o.size_m); // north, east, height
                    let c = o.color.unwrap_or_default();
                    WorldObject {
                        name: o.name,
                        shape: o.shape,
                        center: vec_to_godot(ned(o.center_ned_m)),
                        size: DVec3::new(s.y, s.z, s.x),
                        radius_m: o.radius_m,
                        height_m: o.height_m,
                        color: [c.x, c.y, c.z],
                        rf_loss_db: o.rf_loss_db,
                    }
                })
                .collect(),
            emitters: w
                .emitters
                .into_iter()
                .map(|e| WorldEmitter { name: e.name, position: vec_to_godot(ned(e.position_ned_m)), freq_mhz: e.freq_mhz, power_mw: e.power_mw })
                .collect(),
        }
    }
}

/// Betaflight's OSD as a character grid. `cells` are row-major, each `char | font_page << 8 | blink << 10`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OsdFrame {
    pub seq: u64,
    pub time_s: f64,
    pub present: bool,
    pub cols: u32,
    pub rows: u32,
    pub cells: Vec<u32>,
}

impl OsdFrame {
    pub fn from_pb(f: pb::OsdFrame) -> OsdFrame {
        OsdFrame { seq: f.seq, time_s: f.time_s, present: f.present, cols: f.cols, rows: f.rows, cells: f.cells }
    }
}

/// Everything the HUD shows, from one state message.
#[derive(Debug, Clone, PartialEq)]
pub struct Telemetry {
    pub time_s: f64,
    pub altitude_m: f64,
    pub speed_mps: f64,
    /// Positive when climbing.
    pub climb_mps: f64,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    /// Motor commands in [0, 1], in Betaflight's motor order.
    pub motor_cmd: Vec<f64>,
    /// True when any motor is commanded above 2 %: armed Betaflight idles its motors at about 5 %.
    pub motors_spinning: bool,
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
    /// The session is paced to the wall clock right now (false while paused).
    pub running: bool,
    pub overruns: u64,
    pub fc_restarts: u32,
    pub vtx: VtxInfo,
    pub video: VideoInfo,
    /// Seconds since this state message arrived (filled in when the telemetry is read).
    pub age_s: f64,
}

impl Telemetry {
    pub fn from_pb(s: &pb::State) -> Telemetry {
        let pos = s.position_ned_m.unwrap_or_default();
        let vel = s.velocity_ned_mps.unwrap_or_default();
        let radio = s.radio.unwrap_or_default();
        Telemetry {
            time_s: s.time_s,
            altitude_m: -pos.z,
            speed_mps: (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt(),
            climb_mps: -vel.z,
            battery_voltage_v: s.battery_voltage_v,
            battery_current_a: s.battery_current_a,
            motors_spinning: s.motor_cmd.iter().any(|c| *c > 0.02),
            motor_cmd: s.motor_cmd.clone(),
            tx_enabled: radio.tx_enabled,
            link_up: radio.link_up,
            lq_pct: radio.lq_pct,
            rssi_dbm: radio.rssi_dbm,
            running: s.running,
            overruns: s.overruns,
            fc_restarts: s.fc_restarts,
            vtx: s.vtx.as_ref().map(VtxInfo::from_pb).unwrap_or_default(),
            video: s.video.as_ref().map(VideoInfo::from_pb).unwrap_or_default(),
            age_s: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Overrun,
    FirmwareRestarted,
    SimError,
    LinkDown,
    LinkUp,
    PilotConnected,
    PilotDisconnected,
    SessionEnded,
    VtxChanged,
    SerialOverflow,
    VideoLost,
    VideoRestored,
    /// A kind this client does not know (a newer server).
    Unknown,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Overrun => "overrun",
            EventKind::FirmwareRestarted => "firmware_restarted",
            EventKind::SimError => "sim_error",
            EventKind::LinkDown => "link_down",
            EventKind::LinkUp => "link_up",
            EventKind::PilotConnected => "pilot_connected",
            EventKind::PilotDisconnected => "pilot_disconnected",
            EventKind::SessionEnded => "session_ended",
            EventKind::VtxChanged => "vtx_changed",
            EventKind::SerialOverflow => "serial_overflow",
            EventKind::VideoLost => "video_lost",
            EventKind::VideoRestored => "video_restored",
            EventKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Simulated time of the event.
    pub time_s: f64,
    pub kind: EventKind,
    pub message: String,
}

impl Event {
    pub fn from_pb(e: pb::Event) -> Event {
        let kind = match pb::EventKind::try_from(e.kind) {
            Ok(pb::EventKind::Overrun) => EventKind::Overrun,
            Ok(pb::EventKind::FirmwareRestarted) => EventKind::FirmwareRestarted,
            Ok(pb::EventKind::SimError) => EventKind::SimError,
            Ok(pb::EventKind::LinkDown) => EventKind::LinkDown,
            Ok(pb::EventKind::LinkUp) => EventKind::LinkUp,
            Ok(pb::EventKind::PilotConnected) => EventKind::PilotConnected,
            Ok(pb::EventKind::PilotDisconnected) => EventKind::PilotDisconnected,
            Ok(pb::EventKind::SessionEnded) => EventKind::SessionEnded,
            Ok(pb::EventKind::VtxChanged) => EventKind::VtxChanged,
            Ok(pb::EventKind::SerialOverflow) => EventKind::SerialOverflow,
            Ok(pb::EventKind::VideoLost) => EventKind::VideoLost,
            Ok(pb::EventKind::VideoRestored) => EventKind::VideoRestored,
            Ok(pb::EventKind::Unspecified) | Err(_) => EventKind::Unknown,
        };
        Event { time_s: e.time_s, kind, message: e.message }
    }
}

/// Where the client is in its connect, load and fly cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Looking for the server (and starting it when allowed).
    Connecting,
    /// The quad is loading; Betaflight takes a few seconds to boot.
    Loading,
    /// The pilot link is open and the simulation runs in real time.
    Flying,
    /// The user paused the simulation.
    Paused,
    /// Something went wrong (see the detail and kind); `Command::Reload` tries again.
    Failed,
    /// The client was shut down.
    Stopped,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Connecting => "connecting",
            Phase::Loading => "loading",
            Phase::Flying => "flying",
            Phase::Paused => "paused",
            Phase::Failed => "failed",
            Phase::Stopped => "stopped",
        }
    }
}

/// What `Client::poll` returns, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Phase { phase: Phase, detail: String, kind: Option<ErrorKind> },
    /// The quad is loaded; `configurator_address` is empty without Betaflight.
    Session { quad_name: String, configurator_address: String },
    Event(Event),
    /// A request failed without ending the session (for example a Pause the server refused).
    Error(ClientError),
}

/// Requests to the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    /// Loads the quad again (resetting the drone) or, after a failure, reconnects and retries.
    Reload,
    /// Cuts (true) or restores (false) the radio link: the failsafe test.
    SetRadioLoss(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverrunPolicy {
    /// Catch up in bursts; drop a backlog over 100 ms and count an overrun.
    #[default]
    Warn,
    /// Never burst: simulated time stretches.
    Slow,
}

/// How to start `ofs-sim` when nothing answers on the server address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    /// Passed as `--data-dir`: per-quad Betaflight working directories (EEPROM, SITL log) live here.
    pub data_dir: PathBuf,
    /// Extra environment for the server (for example `OFS_SITL_LAUNCH`); the rest is inherited.
    pub env: Vec<(String, String)>,
    /// The server's stderr is appended here.
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// gRPC address of the server, like `127.0.0.1:50051`.
    pub server_addr: String,
    /// Started on `server_addr` when nothing answers there; `None` fails instead.
    pub launch: Option<LaunchSpec>,
    pub quad_path: String,
    /// A world file (`worlds/flat.toml`); empty flies in the server's built-in open field.
    pub world_path: String,
    pub seed: u64,
    /// Fly without Betaflight: motor commands follow the throttle stick (a testing aid).
    pub open_loop_fc: bool,
    pub overrun_policy: OverrunPolicy,
    /// Vehicle state messages per second, 1..=240.
    pub state_rate_hz: u32,
    /// Stick messages per second.
    pub stick_rate_hz: u32,
    /// How long a launched server may take to start answering.
    pub launch_timeout: Duration,
}

impl Settings {
    pub fn new(quad_path: impl Into<String>) -> Settings {
        Settings {
            server_addr: "127.0.0.1:50051".into(),
            launch: None,
            quad_path: quad_path.into(),
            world_path: String::new(),
            seed: 1,
            open_loop_fc: false,
            overrun_policy: OverrunPolicy::Warn,
            state_rate_hz: 240,
            stick_rate_hz: 250,
            launch_timeout: Duration::from_secs(15),
        }
    }
}
