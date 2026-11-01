//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_config::world::{self as world_cfg, WorldConfig};
use ofs_config::WorldMeshData;
use ofs_config::{BandKind, FcKind, FixedSection, HandsetKind, QuadConfig, RadioKind, ReceiverKind, VtxSection};
use ofs_core::rng::fnv1a64;
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
use ofs_electrical::battery::{Battery, BatteryParams};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_electrical::thermal::{EscThermalParams, MotorThermalParams, NodeParams, Thermal, ThermalParams, VtxThermalParams};
use ofs_fc::open_loop::OpenLoopFc;
use ofs_fc::stabilizer::Stabilizer;
use ofs_log::tail::{TailHandle, TailRecorder, DEFAULT_WINDOW_S};
use ofs_log::{RecordError, Recorder, RecorderHandle, RecordingSummary, SignalRate};
use crate::inputs::{tick_at, Column, Inputs, Stick};
use crate::inputs_recorder::{InputsRecorder, InputsRecordingSummary, InputsState};
use crate::journal::{Action, Journal};
use ofs_config::world::Course;
use crate::race::{GateTimer, RaceEvent, RaceHandle, RaceInfo, RaceSignals};
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
use ofs_fc::sitl::codec::PortMap;
use ofs_fc::sitl::slot::{SitlSlot, SlotClaim};
use ofs_fc::sitl::esc_telemetry::EscTelemetry;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_physics::collision::CollisionParams;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody, WorldMesh, WorldObject};
use ofs_radio::band::Band;
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{ElrsLink, LinkParams};
use ofs_radio::fixed::{FixedLink, FixedParams};
use ofs_radio::ibus::IbusReceiver;
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::{Handset, Link, Receiver, RfFrontEnd};
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};
use ofs_radio::telemetry::{FcTelemetry, FcTelemetryHandle};
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::mag::{earth_field_ned, soft_iron_from_rows, Mag, MagParams};
use ofs_sensors::imu::{Imu, ImuParams, VibrationParams};
use ofs_sensors::mount::MountResponse;
use ofs_video::link::{
    Emitter, LinkParams as VideoLinkParams, LinkWorld, ReceiverAntenna, ReceiverThresholds, VideoLink, VideoSync,
    VtxInterferenceHandle, FIELD_RATE_HZ,
};
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_core::shape::Shape;
use ofs_core::terrain::Terrain;
use ofs_rf::propagation::{Antenna, AntennaKind, MeshObstacle, Obstacle, Polarization};
use ofs_video::vtx::{band_index, VtxModel, VtxParams, FREQUENCIES_MHZ};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub seed: u64,
    /// Per-quad firmware working directories live under here.
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
    /// The field the quad flies in: the pilot's goggles, objects and other transmitters (the video link's world).
    pub world: WorldConfig,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; names::RC_AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; names::RC_AUX_COUNT] }
    }
}

impl Sticks {
    /// Every stick limited to the range a transmitter has: [-1, 1], and [0, 1] for the throttle.
    pub fn clamped(&self) -> Sticks {
        Sticks {
            roll: self.roll.clamp(-1.0, 1.0),
            pitch: self.pitch.clamp(-1.0, 1.0),
            yaw: self.yaw.clamp(-1.0, 1.0),
            throttle: self.throttle.clamp(0.0, 1.0),
            aux: self.aux.map(|a| a.clamp(-1.0, 1.0)),
        }
    }

    /// The name of the first non-finite field, if any (no allocation: pilots call this at their input rate).
    fn first_non_finite(&self) -> Option<&'static str> {
        const AUX: [&str; names::RC_AUX_COUNT] = ["aux1", "aux2", "aux3", "aux4"];
        let named = [("roll", self.roll), ("pitch", self.pitch), ("yaw", self.yaw), ("throttle", self.throttle)];
        named
            .into_iter()
            .chain(AUX.into_iter().zip(self.aux))
            .find(|(_, v)| !v.is_finite())
            .map(|(n, _)| n)
    }
}

/// What the radio receiver reports.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioState {
    /// The transmitter is on (a pilot or script is connected).
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    /// The active antenna's RSSI.
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// The quad file's name of the antenna in use.
    pub active_antenna: String,
    /// What the handset receives.
    pub downlink_lq_pct: f64,
}

/// Betaflight's telemetry as the handset received it over the ELRS downlink (`age_s` = INFINITY until
/// the first frame arrives; always INFINITY without SITL).
#[derive(Debug, Clone, PartialEq)]
pub struct FcTelemetrySnapshot {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    pub frame_count: u64,
    pub age_s: f64,
}

impl FcTelemetrySnapshot {
    fn new(t: FcTelemetry, now_s: f64) -> Self {
        let age_s = if t.frame_count == 0 { f64::INFINITY } else { now_s - t.last_frame_s };
        Self {
            battery_v: t.battery_v,
            battery_a: t.battery_a,
            battery_mah: t.battery_mah,
            roll_deg: t.roll_deg,
            pitch_deg: t.pitch_deg,
            yaw_deg: t.yaw_deg,
            flight_mode: t.flight_mode,
            frame_count: t.frame_count,
            age_s,
        }
    }
}

/// What the VTX model publishes (all zero without a `[vtx]` section or without Betaflight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The glass-to-glass latency the pipeline can actually deliver: whole fields (20 ms at 50 Hz), rounded down.
pub fn quantize_latency_ms(ms: u32) -> u32 {
    ms / 20 * 20
}

/// The analog video link as the goggles see it (`present = false` and a clean picture without a VTX).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub present: bool,
    pub snr_db: f64,
    pub interference_dbm: f64,
    /// Received power at each goggle antenna, in the world file's order.
    pub rssi_dbm: Vec<(String, f64)>,
    /// The antenna the receiver uses.
    pub active_antenna: String,
    pub noise: f64,
    pub sparkles: f64,
    pub chroma: f64,
    pub sync: VideoSync,
    /// Whether the receiver completed the last field.
    pub field_valid: bool,
    /// The world file's glass-to-glass latency, quantized down to whole fields.
    pub latency_ms: u32,
}

impl Default for VideoInfo {
    fn default() -> Self {
        Self {
            present: false,
            snr_db: 0.0,
            interference_dbm: 0.0,
            rssi_dbm: Vec::new(),
            active_antenna: String::new(),
            noise: 0.0,
            sparkles: 0.0,
            chroma: 1.0,
            sync: VideoSync::Locked,
            field_valid: false,
            latency_ms: 0,
        }
    }
}

/// Faults a script can inject (spec §6.2). Names here are the `active_fault_names` strings.
#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    /// Every radio uplink packet is lost: the receiver goes silent and Betaflight fails safe.
    RadioLinkLoss,
    /// Extra per-packet uplink loss on top of the link model's own, `fraction` in [0, 1].
    RadioLoss {
        fraction: f64,
    },
    /// The handset loses `fraction` of the downlink packets, `fraction` in [0, 1]; uplink control is untouched.
    DownlinkLoss {
        fraction: f64,
    },
    /// Every uplink packet is lost for `on_s`, then the link delivers normally for `off_s`, repeating.
    BurstLoss {
        on_s: f64,
        off_s: f64,
    },
    /// Motor `motor` (0-based) produces `scale` of its torque and current; 0 = dead motor.
    MotorFailure {
        motor: usize,
        scale: f64,
    },
    /// Prop `prop` (0-based) produces `thrust_scale` of its thrust and torque, with `imbalance_kgm` of vibration.
    PropDamage {
        prop: usize,
        thrust_scale: f64,
        imbalance_kgm: f64,
    },
    /// Cell `cell` (0-based) carries `r_offset_ohm` of extra series resistance and `v_offset_v` of extra droop.
    WeakCell {
        cell: usize,
        r_offset_ohm: f64,
        v_offset_v: f64,
    },
    /// The IMU's gyro reads `bias_radps` of extra bias and `noise_scale` times its normal noise.
    GyroFault {
        bias_radps: DVec3,
        noise_scale: f64,
    },
    /// A transmitter at `position_ned_m` on `freq_mhz` at `power_mw` interferes with the video link while active.
    VtxInterference {
        freq_mhz: f64,
        power_mw: f64,
        position_ned_m: DVec3,
    },
}

impl Fault {
    /// The fault's name, as the events and `active_fault_names` spell it.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Fault::RadioLinkLoss => "radio_link_loss",
            Fault::RadioLoss { .. } => "radio_loss",
            Fault::DownlinkLoss { .. } => "downlink_loss",
            Fault::BurstLoss { .. } => "burst_loss",
            Fault::MotorFailure { .. } => "motor_failure",
            Fault::PropDamage { .. } => "prop_damage",
            Fault::WeakCell { .. } => "weak_cell",
            Fault::GyroFault { .. } => "gyro_fault",
            Fault::VtxInterference { .. } => "vtx_interference",
        }
    }
}

/// The receiver's UART buffer. With no flight controller draining it (open-loop FC), the oldest bytes are
/// dropped like a UART overrun.
const RECEIVER_UART_CAPACITY: usize = 4096;

/// Buffers between Betaflight's UARTs and the video models. Large enough for a few ticks of bursts; when a consumer
/// falls behind, the oldest bytes are dropped like a UART overrun.
const ESC_UART_CAPACITY: usize = 1024;
const VIDEO_TAP_CAPACITY: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    pub time_s: f64,
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    /// Per-cell voltage: each cell's own state in the per-cell battery model, the weak cell carrying the fault's droop.
    pub battery_cell_voltages: Vec<f64>,
    pub motor_rpm: Vec<f64>,
    pub motor_cmd: Vec<f64>,
    pub radio: RadioState,
    pub fc_telemetry: FcTelemetrySnapshot,
    /// Betaflight reboots so far (e.g. after a Configurator save); always 0 without SITL.
    pub fc_restarts: u32,
    pub vtx: VtxInfo,
    /// TX bytes Betaflight's UART capture dropped because a consumer fell behind.
    pub serial_dropped_bytes: u64,
    pub video: VideoInfo,
    /// The inward speed of the last collision event (0 until one happens).
    pub collision_speed_mps: f64,
    /// The object the last collision event hit (-1 = the ground).
    pub collision_object: i32,
    /// Collision events raised so far (the server detects the event's edge on this).
    pub collision_event_count: u64,
    /// The lap clock (all zero when the world has no gates).
    pub race: RaceInfo,
}

/// A world with gates: the `race.*` signals and the model's event queue.
struct RaceLink {
    signals: RaceSignals,
    handle: RaceHandle,
    courses: Vec<Course>,
}

struct VideoHandles {
    present: Signal<f64>,
    snr: Signal<f64>,
    interference: Signal<f64>,
    rssi: Vec<(String, Signal<f64>)>,
    antenna: Signal<f64>,
    noise: Signal<f64>,
    sparkles: Signal<f64>,
    chroma: Signal<f64>,
    sync: Signal<f64>,
    field_valid: Signal<f64>,
    latency_ms: u32,
}

impl VideoHandles {
    fn register(bus: &mut Bus, world: &WorldConfig) -> Self {
        Self {
            present: bus.signal(names::VIDEO_PRESENT),
            snr: bus.signal(names::VIDEO_SNR),
            interference: bus.signal(names::VIDEO_INTERFERENCE),
            rssi: world.receiver.antennas.iter().map(|a| (a.name.clone(), bus.signal(&names::video_rssi(&a.name)))).collect(),
            antenna: bus.signal(names::VIDEO_ANTENNA),
            noise: bus.signal(names::VIDEO_NOISE),
            sparkles: bus.signal(names::VIDEO_SPARKLES),
            chroma: bus.signal(names::VIDEO_CHROMA),
            sync: bus.signal(names::VIDEO_SYNC),
            field_valid: bus.signal(names::VIDEO_FIELD_VALID),
            latency_ms: quantize_latency_ms(world.receiver.latency_ms),
        }
    }

    fn read(&self, b: &Bus) -> VideoInfo {
        if b.get(self.present) < 0.5 {
            // Nothing transmits, but the goggles' glass-to-glass latency is still theirs.
            return VideoInfo { latency_ms: self.latency_ms, ..VideoInfo::default() };
        }
        let active = b.get(self.antenna) as usize;
        VideoInfo {
            present: true,
            snr_db: b.get(self.snr),
            interference_dbm: b.get(self.interference),
            rssi_dbm: self.rssi.iter().map(|(name, s)| (name.clone(), b.get(*s))).collect(),
            active_antenna: self.rssi.get(active).map(|(name, _)| name.clone()).unwrap_or_default(),
            noise: b.get(self.noise),
            sparkles: b.get(self.sparkles),
            chroma: b.get(self.chroma),
            sync: VideoSync::from_signal(b.get(self.sync)),
            field_valid: b.get(self.field_valid) > 0.5,
            latency_ms: self.latency_ms,
        }
    }
}

struct Handles {
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    vbat: Signal<f64>,
    ibat: Signal<f64>,
    cell_voltage: Vec<Signal<f64>>,
    omega: Vec<Signal<f64>>,
    cmd: Vec<Signal<f64>>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    fault_radio_loss: Signal<f64>,
    /// Uplink loss fraction 0..1 — written by set_fault's RadioLoss arm; the ELRS model reads it per packet.
    fault_radio_loss_fraction: Signal<f64>,
    /// Downlink loss fraction 0..1 — written by set_fault's DownlinkLoss arm; the radio models read it per packet.
    fault_radio_downlink_loss_fraction: Signal<f64>,
    /// Burst window edges, s — written by set_fault's BurstLoss arm (Task 4 consumes).
    fault_burst_on: Signal<f64>,
    fault_burst_off: Signal<f64>,
    /// Battery Thevenin offsets — written by set_fault's WeakCell arm (Task 5 consumes).
    fault_battery_r_offset: Signal<f64>,
    fault_battery_v_offset: Signal<f64>,
    /// 1-based weak-cell index, 0 = none — written by set_fault's WeakCell arm (Task 5 consumes).
    fault_battery_weak_cell: Signal<f64>,
    /// Gyro bias offset, FRD rad/s — written by set_fault's GyroFault arm.
    fault_imu_gyro_bias: Signal<DVec3>,
    fault_imu_noise_scale: Signal<f64>,
    fault_motor_scale: Vec<Signal<f64>>,
    fault_prop_thrust_scale: Vec<Signal<f64>>,
    prop_imbalance: Vec<Signal<f64>>,
    /// Integrated rotor phase — the M4b vibration model's phase input (Task 2).
    #[allow(dead_code)]
    motor_theta: Vec<Signal<f64>>,
    fc_restarts: Signal<f64>,
    vtx_present: Signal<f64>,
    vtx_band: Signal<f64>,
    vtx_channel: Signal<f64>,
    vtx_freq: Signal<f64>,
    vtx_power: Signal<f64>,
    vtx_pit: Signal<f64>,
    serial_dropped: Signal<f64>,
    radio_snr: Signal<f64>,
    radio_antenna: Signal<f64>,
    radio_downlink_lq: Signal<f64>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
    collision_count: Signal<f64>,
    video: VideoHandles,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize, cells: usize, world: &WorldConfig) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            vbat: bus.signal(names::BATTERY_VOLTAGE),
            ibat: bus.signal(names::BATTERY_CURRENT),
            cell_voltage: (0..cells).map(|k| bus.signal(&names::battery_cell_voltage(k))).collect(),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            cmd: (0..motors).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            fault_radio_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            fault_radio_loss_fraction: bus.signal(names::FAULT_RADIO_LOSS_FRACTION),
            fault_radio_downlink_loss_fraction: bus.signal(names::FAULT_RADIO_DOWNLINK_LOSS_FRACTION),
            fault_burst_on: bus.signal(names::FAULT_RADIO_BURST_ON_S),
            fault_burst_off: bus.signal(names::FAULT_RADIO_BURST_OFF_S),
            fault_battery_r_offset: bus.signal(names::FAULT_BATTERY_R_OFFSET_OHM),
            fault_battery_v_offset: bus.signal(names::FAULT_BATTERY_V_OFFSET_V),
            fault_battery_weak_cell: bus.signal(names::FAULT_BATTERY_WEAK_CELL),
            fault_imu_gyro_bias: bus.signal(names::FAULT_IMU_GYRO_BIAS),
            fault_imu_noise_scale: bus.signal(names::FAULT_IMU_NOISE_SCALE),
            fault_motor_scale: (0..motors).map(|i| bus.signal(&names::fault_motor_scale(i))).collect(),
            fault_prop_thrust_scale: (0..motors).map(|i| bus.signal(&names::fault_prop_thrust_scale(i))).collect(),
            prop_imbalance: (0..motors).map(|i| bus.signal(&names::prop_imbalance(i))).collect(),
            motor_theta: (0..motors).map(|i| bus.signal(&names::motor_theta(i))).collect(),
            fc_restarts: bus.signal(names::FC_RESTARTS),
            vtx_present: bus.signal(names::VTX_PRESENT),
            vtx_band: bus.signal(names::VTX_BAND),
            vtx_channel: bus.signal(names::VTX_CHANNEL),
            vtx_freq: bus.signal(names::VTX_FREQ_MHZ),
            vtx_power: bus.signal(names::VTX_POWER_MW),
            vtx_pit: bus.signal(names::VTX_PIT),
            serial_dropped: bus.signal(names::FC_SERIAL_DROPPED),
            radio_snr: bus.signal(names::RADIO_SNR),
            radio_antenna: bus.signal(names::RADIO_ANTENNA),
            radio_downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
            collision_count: bus.signal(names::BODY_COLLISION_COUNT),
            video: VideoHandles::register(bus, world),
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
    /// The SITL's ports (the stock ones unless the session took another slot).
    ports: PortMap,
    /// The SITL's working directory (its EEPROM, logs and Blackbox files); `None` without Betaflight.
    firmware_dir: Option<PathBuf>,
    /// The SITL slot this session holds; `None` without Betaflight. Dropped with the vehicle, and taken out by the
    /// server for a rebuild (a reload), which retakes the same slot through [`SitlSlot::from_claim`] so the reload
    /// cannot land in another slot's firmware directory (issue #90).
    slot_claim: Option<SlotClaim>,
    osd: Option<OsdHandle>,
    fc_telemetry: FcTelemetryHandle,
    world: WorldConfig,
    /// The terrain baked once at build, shared with the rigid body; `GetWorld` sends this grid instead of baking again.
    terrain: Option<std::sync::Arc<ofs_core::terrain::Terrain>>,
    /// The world's meshes, parsed once at build and shared with the rigid body and both links.
    meshes: Vec<WorldMeshData>,
    /// The quad file's radio antenna names, by index (the state's `active_antenna`).
    radio_antenna_names: Vec<String>,
    /// The video link's runtime-emitter handle, written by set_fault's VtxInterference arm.
    vtx_interference: VtxInterferenceHandle,
    /// Per-motor part imbalance at build time; `clear_faults` restores it.
    prop_imbalance_baseline: Vec<f64>,
    /// Pack cell count, for the weak-cell fault.
    battery_cells: u32,
    /// Whether the quad has a VTX, for the VtxInterference fault.
    has_vtx: bool,
    /// The active faults' names in injection order; `active_fault_names` sorts them for the events.
    active_faults: Vec<String>,
    /// The recorder model's handle: StartRecording/StopRecording go through it.
    recorder: RecorderHandle,
    /// The rolling tail of the default channel table: a failed session's bundle takes its snapshot.
    tail: TailHandle,
    /// Faults scheduled with `at`, by due tick (ties in scheduling order).
    pending_faults: Vec<(u64, Fault)>,
    /// The inputs file playing now.
    playback: Option<Playback>,
    /// An inputs file that finished on its own, until the session raises its event.
    finished_inputs: Option<String>,
    /// The sticks recording running now (a row whenever `set_sticks` changes a value).
    inputs_recorder: Option<InputsRecorder>,
    /// Everything that steers the run, by tick: what a repro bundle replays.
    journal: Journal,
    /// The sticks last journaled, so a repeated value adds no entry.
    journaled_sticks: [u64; 8],
    /// The lap clock, in a world with gates.
    race: Option<RaceLink>,
}

/// An inputs file being played: its rows' due ticks and the next row to apply.
struct Playback {
    inputs: Inputs,
    ticks: Vec<u64>,
    next: usize,
}

/// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
/// the same file name in different directories never share an EEPROM.
pub fn firmware_dir(data_dir: &Path, quad_path: &Path) -> PathBuf {
    let full = std::fs::canonicalize(quad_path)
        .or_else(|_| std::path::absolute(quad_path))
        .unwrap_or_else(|_| quad_path.to_path_buf());
    let stem = quad_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
    let hash = fnv1a64(full.to_string_lossy().as_bytes()) as u32;
    data_dir.join(format!("{stem}-{hash:08x}"))
}

/// The firmware directory of slot `slot` (see `ofs_fc::sitl::slot`): slot 0, the first simulator on the machine, uses
/// `base` as always; slot n > 0 uses the sibling `<name>-s<n>`, so two live SITLs never share an EEPROM, log or
/// Blackbox file. The sibling starts empty: its first boot applies the quad's diff like any new directory.
pub fn slot_firmware_dir(base: &Path, slot: u16) -> PathBuf {
    if slot == 0 {
        return base.to_path_buf();
    }
    let name = base.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
    base.with_file_name(format!("{name}-s{slot}"))
}

/// The eight stick values of the journal, as bit patterns (so NaN compares equal to itself).
fn sticks_bits(s: &Sticks) -> [u64; 8] {
    sticks_values(s).map(f64::to_bits)
}

fn sticks_values(s: &Sticks) -> [f64; 8] {
    [s.roll, s.pitch, s.yaw, s.throttle, s.aux[0], s.aux[1], s.aux[2], s.aux[3]]
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn pairs(t: &[[f64; 2]]) -> Vec<(f64, f64)> {
    t.iter().map(|r| (r[0], r[1])).collect()
}

/// The video devices a quad file asks for, wired to Betaflight's UARTs.
struct VideoDevices {
    /// Run before the flight controller each tick (they write bytes it reads in this tick's exchange).
    before_fc: Vec<Box<dyn Model>>,
    /// Run after it (they read what it wrote in this tick's exchange).
    after_fc: Vec<Box<dyn Model>>,
    /// Bytes into Betaflight's UARTs (0-based UART indexes), besides the receiver's CRSF.
    serial: Vec<SerialLink>,
    /// Bytes out of Betaflight's UARTs.
    taps: Vec<SerialTap>,
    osd: Option<OsdHandle>,
}

fn video_devices(cfg: &QuadConfig, motors: usize, fc_divisor: u32, bus: &mut Bus) -> Result<VideoDevices, SimError> {
    let mut v = VideoDevices { before_fc: Vec::new(), after_fc: Vec::new(), serial: Vec::new(), taps: Vec::new(), osd: None };
    if let Some(e) = &cfg.esc_telemetry {
        let uart = Wire::new(ESC_UART_CAPACITY);
        let pp = cfg.motors[0].pole_pairs as f64;
        v.before_fc.push(Box::new(EscTelemetry::new(uart.clone(), motors, cfg.sim.base_hz / e.rate_hz, pp, bus)));
        v.serial.push(SerialLink { uart_index: e.uart - 1, rx: uart });
    }
    if let Some(o) = &cfg.osd {
        let tap = Wire::new(VIDEO_TAP_CAPACITY);
        let model = OsdModel::new(o.cols as usize, o.rows as usize, tap.clone(), fc_divisor);
        v.osd = Some(model.handle());
        v.taps.push(SerialTap { uart_index: o.uart - 1, tx: tap });
        v.after_fc.push(Box::new(model));
    }
    if let Some(x) = &cfg.vtx {
        let requests = Wire::new(VIDEO_TAP_CAPACITY);
        let replies = Wire::new(VIDEO_TAP_CAPACITY);
        v.taps.push(SerialTap { uart_index: x.uart - 1, tx: requests.clone() });
        v.serial.push(SerialLink { uart_index: x.uart - 1, rx: replies.clone() });
        v.after_fc.push(Box::new(vtx_model(x, requests, replies, fc_divisor, bus)?));
    }
    Ok(v)
}

/// The VTX: it answers SmartAudio on `requests`/`replies` and transmits its power-up channel and power from the start.
fn vtx_model(x: &VtxSection, requests: Wire, replies: Wire, divisor: u32, bus: &mut Bus) -> Result<VtxModel, SimError> {
    let params = VtxParams {
        power_levels_mw: x.power_levels_mw.clone(),
        power_levels_dbm: x.power_levels_dbm.clone(),
        default_band: band_index(&x.default_band)
            .ok_or_else(|| SimError::InvalidArgument(format!("vtx.default_band {:?} is not a band letter", x.default_band)))?,
        default_channel: x.default_channel,
        default_power_index: x.default_power_index,
        reply_latency_s: x.reply_latency_ms / 1000.0,
    };
    Ok(VtxModel::new(x.kind, params, requests, replies, divisor, bus))
}

fn polarization(p: world_cfg::Polarization) -> Polarization {
    match p {
        world_cfg::Polarization::Rhcp => Polarization::Rhcp,
        world_cfg::Polarization::Lhcp => Polarization::Lhcp,
        world_cfg::Polarization::Linear => Polarization::Linear,
    }
}

fn antenna_kind(kind: world_cfg::AntennaKind, beamwidth_deg: Option<f64>) -> AntennaKind {
    match kind {
        world_cfg::AntennaKind::Omni => AntennaKind::Omni,
        world_cfg::AntennaKind::Patch => AntennaKind::Patch { beamwidth_deg: beamwidth_deg.unwrap_or(60.0) },
    }
}

/// A direction in NED from a heading (degrees clockwise from north) and an elevation (degrees up).
pub fn aim_ned(heading_deg: f64, elevation_deg: f64) -> DVec3 {
    DVec3::from_array(ofs_proto::aim_ned(heading_deg, elevation_deg))
}

/// The world's objects as collidable shapes, exactly as the file gives them (not rooted).
pub fn world_objects(world: &WorldConfig) -> Vec<WorldObject> {
    world.objects.iter().map(|o| WorldObject { name: o.name.clone(), shape: object_shape(o) }).collect()
}

/// The quad file's radio, wired to the flight controller's UART: the joystick handset (at the world's `[handset]`),
/// the link the quad file names (ELRS or fixed) and the CRSF receiver. `uart` is the receiver's output (the FC's UART RX); `telemetry_in` carries
/// the FC's own CRSF telemetry back.
#[allow(clippy::too_many_arguments)]
fn radio_chain(
    cfg: &QuadConfig,
    opts: &BuildOptions,
    terrain: Option<Arc<Terrain>>,
    meshes: &[WorldMeshData],
    base_hz: u32,
    uart: Wire,
    telemetry_in: Wire,
    bus: &mut Bus,
) -> RadioChain {
    let r = &cfg.radio;
    let facing = opts.world.pilot.facing_deg;
    let handset = &opts.world.handset;
    let front_end = RfFrontEnd {
        position: v3(handset.position(&opts.world.pilot)),
        antennas: handset
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, a.beamwidth_deg),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el()),
            })
            .collect(),
        tx_power_mw: f64::from(r.tx_power_mw),
    };
    let link_params = LinkParams {
        band: match r.band_or_default() {
            BandKind::G2_4 => Band::G2_4,
            BandKind::Mhz900 => Band::Mhz900,
        },
        packet_rate_hz: r.packet_rate_hz,
        telemetry_slot_divisor: r.telemetry_slot_divisor,
        latency_packets: r.latency_packets,
        tx_power_mw: front_end.tx_power_mw,
        handset_position: front_end.position,
        handset_antennas: front_end.antennas.clone(),
        quad_antennas: r
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, None),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: v3(a.mount_frd).normalize(),
            })
            .collect(),
        obstacles: world_obstacles(&opts.world, terrain.as_deref()),
        terrain,
        meshes: mesh_obstacles(meshes),
        fading: true,
        ground_bounce: true,
    };
    let handset: Box<dyn Handset> = match r.handset {
        HandsetKind::Joystick => Box::new(JoystickHandset::new(front_end, bus)),
    };
    let link: Box<dyn Link> = match r.kind {
        RadioKind::Elrs => Box::new(ElrsLink::new(link_params, opts.seed, bus)),
        RadioKind::Fixed => {
            let f = r.fixed.unwrap_or(FixedSection { loss_fraction: 0.0, burst_on_s: 0.0, burst_off_s: 0.0 });
            Box::new(FixedLink::new(
                FixedParams {
                    packet_rate_hz: r.packet_rate_hz,
                    latency_packets: r.latency_packets,
                    telemetry_slot_divisor: r.telemetry_slot_divisor,
                    loss_fraction: f.loss_fraction,
                    burst_on_s: f.burst_on_s,
                    burst_off_s: f.burst_off_s,
                },
                opts.seed,
                bus,
            ))
        }
    };
    let receiver: Box<dyn Receiver> = match r.receiver {
        ReceiverKind::Crsf => Box::new(CrsfReceiver::new(ReceiverParams { link_stats_interval_packets: r.link_stats_interval_packets }, uart, telemetry_in)),
        ReceiverKind::Ibus => Box::new(IbusReceiver::new(r.packet_rate_hz, uart, telemetry_in)),
    };
    RadioChain::new(handset, link, receiver, base_hz, bus)
}

/// The loaded meshes as RF obstacles, sharing the triangles.
fn mesh_obstacles(meshes: &[WorldMeshData]) -> Vec<MeshObstacle> {
    meshes.iter().map(|m| MeshObstacle { mesh: m.mesh.clone(), loss_db: m.loss_db.clone() }).collect()
}

/// The world's objects as RF obstacles: those that take signal, rooted on the ground (the terrain's, if any) so nothing
/// diffracts underneath them.
pub fn world_obstacles(world: &WorldConfig, terrain: Option<&Terrain>) -> Vec<Obstacle> {
    world
        .objects
        .iter()
        .filter(|o| o.rf_loss_db > 0.0)
        .map(|o| Obstacle { shape: object_shape(o).rooted_on(terrain), rf_loss_db: o.rf_loss_db })
        .collect()
}

fn object_shape(o: &world_cfg::ObjectSection) -> Shape {
    let center = v3(o.center_ned_m);
    match o.shape {
        world_cfg::Shape::Box => {
            let s = o.size_m.unwrap_or_default();
            let yaw = o.yaw_deg.map_or(0.0, |y| y.rem_euclid(360.0).to_radians());
            Shape::Box { center, half: DVec3::new(s[0], s[1], s[2]) * 0.5, yaw_rad: yaw }
        }
        world_cfg::Shape::Cylinder => {
            Shape::Cylinder { center, radius: o.radius_m.unwrap_or_default(), half_height: o.height_m.unwrap_or_default() * 0.5 }
        }
    }
}

/// An emitter's frequency: its `freq_mhz`, or its band and channel in the factory table.
pub(crate) fn emitter_freq_mhz(e: &world_cfg::EmitterSection) -> Result<f64, SimError> {
    if let Some(f) = e.freq_mhz {
        return Ok(f);
    }
    let band = e.band.as_deref().and_then(band_index);
    match (band, e.channel) {
        (Some(b), Some(c @ 1..=8)) => Ok(f64::from(FREQUENCIES_MHZ[b][usize::from(c) - 1])),
        _ => Err(SimError::InvalidArgument(format!("emitter {:?} has no valid frequency", e.name))),
    }
}

/// The video link's view of the world file and the quad's VTX antenna.
pub fn link_params(vtx: &VtxSection, world: &WorldConfig, terrain: Option<Arc<Terrain>>, meshes: &[WorldMeshData]) -> Result<VideoLinkParams, SimError> {
    let facing = world.pilot.facing_deg;
    let antennas = world
        .receiver
        .antennas
        .iter()
        .map(|a| ReceiverAntenna {
            name: a.name.clone(),
            antenna: Antenna {
                kind: antenna_kind(a.kind, a.beamwidth_deg),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el()),
            },
        })
        .collect();
    let obstacles = world_obstacles(world, terrain.as_deref());
    let emitters = world
        .emitters
        .iter()
        .map(|e| {
            Ok(Emitter {
                position: v3(e.position_ned_m),
                freq_mhz: emitter_freq_mhz(e)?,
                power_mw: e.power_mw,
                antenna: Antenna { kind: AntennaKind::Omni, gain_dbi: e.gain_dbi, polarization: polarization(e.polarization), axis: DVec3::NEG_Z },
            })
        })
        .collect::<Result<Vec<_>, SimError>>()?;
    let a = &vtx.antenna;
    Ok(VideoLinkParams {
        world: LinkWorld {
            pilot_position: v3(world.pilot.position_ned_m),
            antennas,
            noise_floor_dbm: world.receiver.noise_floor_dbm,
            diversity: world.receiver.diversity,
            obstacles,
            terrain,
            meshes: mesh_obstacles(meshes),
            emitters,
        },
        vtx_antenna: Antenna {
            kind: antenna_kind(a.kind, a.beamwidth_deg),
            gain_dbi: a.gain_dbi,
            polarization: polarization(a.polarization),
            axis: v3(a.mount_frd).normalize(),
        },
        pit_power_mw: vtx.pit_power_mw,
        fading: true,
        ground_bounce: true,
        thresholds: receiver_thresholds(&world.video),
    })
}

/// The parts' thermal sections and the world's ambient as the thermal model's parameters.
pub fn thermal_params(cfg: &QuadConfig, world: &WorldConfig) -> ThermalParams {
    let node = |c: f64, r: f64| NodeParams { heat_capacity_jpk: c, resistance_kpw: r };
    ThermalParams {
        ambient_c: world.environment.ambient_c,
        esc: cfg
            .escs
            .iter()
            .map(|e| {
                e.thermal.map(|t| EscThermalParams {
                    node: node(t.heat_capacity_jpk, t.resistance_kpw),
                    r_on_ohm: t.r_on_ohm,
                    switching_loss_coeff: t.switching_loss_coeff,
                })
            })
            .collect(),
        motor: cfg
            .motors
            .iter()
            .map(|m| {
                m.thermal.map(|t| MotorThermalParams {
                    node: node(t.heat_capacity_jpk, t.resistance_kpw),
                    winding_resistance_ohm: m.resistance_ohm,
                    // The torque EscMotor takes off the rotor as friction: Ke · I0.
                    friction_torque_nm: 60.0 / (2.0 * PI * m.kv_rpm_per_v) * m.no_load_current_a,
                })
            })
            .collect(),
        vtx: cfg.vtx.as_ref().and_then(|v| {
            v.thermal.map(|t| VtxThermalParams { node: node(t.heat_capacity_jpk, t.resistance_kpw), efficiency: t.efficiency, idle_w: t.idle_w })
        }),
    }
}

/// The world file's `[video]` table as the link model's thresholds.
pub fn receiver_thresholds(v: &world_cfg::VideoSection) -> ReceiverThresholds {
    ReceiverThresholds {
        clean_snr_db: v.clean_snr_db,
        sparkle_snr_db: v.sparkle_snr_db,
        sparkle_full_snr_db: v.sparkle_full_snr_db,
        color_fade_snr_db: v.color_fade_snr_db,
        color_lost_snr_db: v.color_lost_snr_db,
        unstable_snr_db: v.unstable_snr_db,
        lost_snr_db: v.lost_snr_db,
        lost_after_fields: v.lost_after_fields,
        relock_after_fields: v.relock_after_fields,
        field_drop_p_max: v.field_drop_p_max,
    }
}

/// One motor's drivetrain as the models take it: the propeller, the motor, its ESC and the propeller's inertia.
/// `build` and the validation harness's thrust stand both read it from here, so they cannot drift apart.
#[derive(Debug, Clone)]
pub struct Drivetrain {
    pub prop: PropParams,
    pub motor: MotorParams,
    pub esc: EscParams,
    pub prop_inertia_kgm2: f64,
}

/// Motor `i`'s drivetrain parameters from the quad config.
pub fn drivetrain_params(cfg: &QuadConfig, i: usize) -> Drivetrain {
    Drivetrain {
        prop: PropParams {
            diameter_m: cfg.props[i].diameter_m,
            ct_table: pairs(&cfg.props[i].ct_table),
            cp_table: pairs(&cfg.props[i].cp_table),
            windmill_j0: cfg.props[i].windmill_j0,
        },
        motor: MotorParams {
            kv_rpm_per_v: cfg.motors[i].kv_rpm_per_v,
            resistance_ohm: cfg.motors[i].resistance_ohm,
            no_load_current_a: cfg.motors[i].no_load_current_a,
            rotor_inertia_kgm2: cfg.motors[i].rotor_inertia_kgm2,
        },
        esc: EscParams { response_tau_s: cfg.escs[i].response_tau_s, current_limit_a: cfg.escs[i].current_limit_a },
        prop_inertia_kgm2: cfg.props[i].inertia_kgm2,
    }
}

/// Builds a vehicle from a quad file. `keep_slot` retakes a slot kept across a session rebuild (a reload): the
/// claim never left this process, so the rebuild keeps its slot — and its firmware directory — instead of racing
/// other simulators' acquires for the first free one (issue #90). `None` for every fresh session.
pub fn build(cfg: &QuadConfig, opts: &BuildOptions, keep_slot: Option<SlotClaim>) -> Result<Vehicle, SimError> {
    build_seeded(cfg, opts, None, keep_slot)
}

/// As [`build`]; with `seed_firmware` (a repro bundle's `firmware` folder), a SITL quad's firmware state files
/// ([`crate::repro::FIRMWARE_STATE`]) are copied into the directory of the slot the session takes before SITL
/// boots, so a replay starts from the bundle's EEPROM whichever slot is free.
pub fn build_seeded(
    cfg: &QuadConfig,
    opts: &BuildOptions,
    seed_firmware: Option<&Path>,
    keep_slot: Option<SlotClaim>,
) -> Result<Vehicle, SimError> {
    let mut bus = Bus::new();
    let n = cfg.frame.motor_positions_frd_m.len();
    let base_hz = cfg.sim.base_hz;
    let mut models: Vec<Box<dyn Model>> = Vec::new();

    for i in 0..n {
        models.push(Box::new(Propeller::new(i, drivetrain_params(cfg, i).prop, &mut bus)));
    }
    for i in 0..n {
        let d = drivetrain_params(cfg, i);
        models.push(Box::new(EscMotor::new(i, d.esc, d.motor, d.prop_inertia_kgm2, &mut bus)));
    }

    let battery = BatteryParams {
        cells: cfg.battery.cells,
        capacity_mah: cfg.battery.capacity_mah,
        r0_ohm: cfg.battery.r0_ohm,
        r1_ohm: cfg.battery.r1_ohm,
        c1_f: cfg.battery.c1_f,
        ocv_table: pairs(&cfg.battery.ocv_table),
        initial_soc: cfg.battery.initial_soc,
        cell_r0_scale: cfg.battery.cell_r0_scale.clone(),
        cell_capacity_scale: cfg.battery.cell_capacity_scale.clone(),
        capacity_factor: 1.0 - cfg.battery.capacity_fade_per_cycle * f64::from(cfg.battery.cycles),
        resistance_factor: 1.0 + cfg.battery.resistance_growth_per_cycle * f64::from(cfg.battery.cycles),
    };
    models.push(Box::new(Battery::new(battery, n, base_hz / cfg.battery.rate_hz, &mut bus)));
    // After the motors and the battery, so it heats from this tick's currents; 100 Hz is plenty for time constants of
    // seconds, and the exact integrator does not care about the step.
    models.push(Box::new(Thermal::new(thermal_params(cfg, &opts.world), (base_hz / 100).max(1), &mut bus)));

    // One grid for the physics and both links: they must agree on where the ground is.
    let terrain = opts.world.terrain.as_ref().map(|t| Arc::new(t.build()));
    let meshes = opts.world.load_meshes().map_err(SimError::InvalidArgument)?;
    let f = &cfg.frame;
    let (restitution, friction_coeff) = cfg.collision_contact();
    let airframe = AirframeParams {
        mass_kg: f.mass_kg,
        inertia_kgm2: v3(f.inertia_kgm2),
        cda_m2: v3(f.cda_m2),
        angular_damping_nms: f.angular_damping_nms,
        rotor_inertia_kgm2: (0..n).map(|i| cfg.motors[i].rotor_inertia_kgm2 + cfg.props[i].inertia_kgm2).collect(),
        mounts: f
            .motor_positions_frd_m
            .iter()
            .zip(&f.motor_spin)
            .map(|(p, s)| MotorMount { position_frd_m: v3(*p), spin: f64::from(*s) })
            .collect(),
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
        objects: world_objects(&opts.world),
        collision: CollisionParams {
            restitution,
            friction_coeff,
            spheres: cfg.collision_spheres().into_iter().map(|s| (DVec3::new(s[0], s[1], s[2]), s[3])).collect(),
        },
        terrain: terrain.clone(),
        meshes: meshes.iter().map(|m| WorldMesh::new(m.name.clone(), m.mesh.clone())).collect(),
    };
    let terrain = airframe.terrain.clone();
    let initial = BodyState {
        pos_ned_m: v3(cfg.initial.position_ned_m),
        vel_ned_mps: DVec3::ZERO,
        att: DQuat::from_rotation_z(cfg.initial.yaw_deg.to_radians()),
        rate_frd_radps: DVec3::ZERO,
    };
    models.push(Box::new(RigidBody::new(airframe, initial, &mut bus)));

    let imu = ImuParams {
        gyro_noise_std_radps: cfg.imu.gyro_noise_std_radps,
        gyro_bias_radps: v3(cfg.imu.gyro_bias_radps),
        accel_noise_std_mps2: cfg.imu.accel_noise_std_mps2,
        accel_bias_mps2: v3(cfg.imu.accel_bias_mps2),
        // The frame's motors shake the IMU (Ruling 8); the empty-motor edge carries no vibration params.
        vibration: (n > 0).then(|| VibrationParams {
            positions_frd: f.motor_positions_frd_m.iter().map(|p| v3(*p)).collect(),
            mass_kg: f.mass_kg,
            inertia_kgm2: v3(f.inertia_kgm2),
            mount: f.fc_mount.as_ref().map(|m| MountResponse { natural_hz: m.natural_hz, damping_ratio: m.damping_ratio }),
        }),
    };
    models.push(Box::new(Imu::new(imu, opts.seed, &mut bus)));
    models.push(Box::new(Baro::new(BaroParams { noise_std_pa: cfg.baro.noise_std_pa, home_alt_m: cfg.home.alt_m }, opts.seed, &mut bus)));
    if let Some(m) = &cfg.mag {
        let e = &opts.world.environment;
        models.push(Box::new(Mag::new(
            MagParams {
                noise_std_ut: m.noise_std_ut,
                hard_iron_ut: v3(m.hard_iron_ut),
                soft_iron: soft_iron_from_rows(m.soft_iron),
                earth_ned_ut: earth_field_ned(e.magnetic_field_ut, e.magnetic_inclination_deg, e.magnetic_declination_deg),
                rate_divisor: base_hz / m.rate_hz,
            },
            opts.seed,
            &mut bus,
        )));
    }

    let r = &cfg.radio;
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    let radio_tap = Wire::new(VIDEO_TAP_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    let chain = radio_chain(cfg, opts, terrain.clone(), &meshes, base_hz, receiver_uart.clone(), radio_tap.clone(), &mut bus);
    let fc_telemetry = chain.fc_telemetry_handle();
    models.push(Box::new(chain));

    let fc_divisor = base_hz / cfg.fc.exchange_hz;
    let fc_kind = opts.fc_override.unwrap_or(cfg.fc.kind);
    let mut osd: Option<OsdHandle> = None;
    let mut ports = PortMap::default();
    let mut sitl_firmware_dir: Option<PathBuf> = None;
    let mut slot_claim: Option<SlotClaim> = None;
    match fc_kind {
        FcKind::OpenLoop | FcKind::OpenLoopStabilized => {
            if fc_kind == FcKind::OpenLoopStabilized {
                models.push(Box::new(Stabilizer::new(&cfg.frame.motor_positions_frd_m, &cfg.frame.motor_spin, fc_divisor, &mut bus)));
            } else {
                models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus)));
            }
            // No Betaflight to talk to, but the VTX is on the quad all the same: it transmits its power-up channel.
            if let Some(x) = &cfg.vtx {
                models.push(Box::new(vtx_model(x, Wire::new(VIDEO_TAP_CAPACITY), Wire::new(VIDEO_TAP_CAPACITY), fc_divisor, &mut bus)?));
            }
        }
        FcKind::Sitl => {
            let video = video_devices(cfg, n, fc_divisor, &mut bus)?;
            let mut launch_argv = net::env_argv("OFS_SITL_LAUNCH").unwrap_or_else(|| cfg.fc.launch.clone());
            let env_ip = |var| net::env_ip(var).map_err(SimError::InvalidArgument);
            let sitl_net = net::resolve(&launch_argv, env_ip("OFS_SITL_HOST")?, env_ip("OFS_SITL_REPLY_IP")?)
                .map_err(|e| SimError::Firmware(e.to_string()))?;
            // The slot of this session: the first free one, or — for a rebuild whose claim never left this process —
            // the same one as before, whose firmware directory (and EEPROM) is therefore also the same (issue #90).
            let slot = match keep_slot {
                Some(claim) => SitlSlot::from_claim(claim, sitl_net.bind_ip),
                None => SitlSlot::acquire(sitl_net.bind_ip),
            }
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            ports = slot.ports;
            launch_argv = net::with_port_offset(&launch_argv, ports.offset); // whatever the user wrote is the slot's to set
            let mut cleanup = net::env_argv("OFS_SITL_CLEANUP").unwrap_or_else(|| cfg.fc.cleanup.clone());
            if cleanup.is_empty() {
                cleanup = net::default_cleanup(&launch_argv); // required under WSL (M0 §6); only this slot's SITL
            }
            let workdir = slot_firmware_dir(&firmware_dir(&opts.data_dir, &cfg.source_path), slot.index);
            sitl_firmware_dir = Some(workdir.clone());
            if let Some(from) = seed_firmware {
                std::fs::create_dir_all(&workdir).map_err(|e| SimError::Firmware(format!("{}: {e}", workdir.display())))?;
                for name in crate::repro::FIRMWARE_STATE {
                    let file = from.join(name);
                    if file.is_file() {
                        std::fs::copy(&file, workdir.join(name)).map_err(|e| SimError::Firmware(format!("copying {}: {e}", file.display())))?;
                    }
                }
            }
            let launch = LaunchConfig {
                launch: launch_argv,
                cleanup,
                workdir,
                diff_file: cfg.resolve(&cfg.fc.betaflight_diff),
                startup_timeout: Duration::from_millis(cfg.fc.startup_timeout_ms),
            };
            let mut serial = vec![SerialLink { uart_index: r.uart - 1, rx: receiver_uart }];
            serial.extend(video.serial);
            let (bridge, claim) = SitlBridge::start_in_slot(
                BridgeConfig {
                    launch,
                    net: sitl_net,
                    ports,
                    rate_divisor: fc_divisor,
                    first_reply_timeout: Duration::from_millis(cfg.fc.first_reply_timeout_ms),
                    reply_timeout: Duration::from_millis(cfg.fc.reply_timeout_ms),
                    home: Home { lat_deg: cfg.home.lat_deg, lon_deg: cfg.home.lon_deg, alt_m: cfg.home.alt_m },
                    motor_count: n,
                    serial,
                    erpm: (0..n).map(|i| bus.signal(&names::motor_omega(i))).collect(),
                    pole_pairs: cfg.motors.iter().map(|m| f64::from(m.pole_pairs)).collect(),
                    taps: {
                        let mut taps = vec![SerialTap { uart_index: r.uart - 1, tx: radio_tap.clone() }];
                        taps.extend(video.taps);
                        taps
                    },
                },
                slot,
                &mut bus,
            )
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            models.extend(video.before_fc);
            models.push(Box::new(bridge));
            slot_claim = Some(claim);
            models.extend(video.after_fc);
            osd = video.osd;
        }
    }
    let vtx_interference = VtxInterferenceHandle::default();
    if let Some(x) = &cfg.vtx {
        // Last in the tick: it reads the pose the physics wrote and the channel the VTX published.
        let params = link_params(x, &opts.world, terrain.clone(), &meshes)?;
        models.push(Box::new(VideoLink::new(params, base_hz / FIELD_RATE_HZ, opts.seed, vtx_interference.clone(), &mut bus)));
    }

    // After the physics, which wrote this tick's pose; a world without gates adds neither the model nor its signals.
    let mut race = None;
    if !opts.world.gates.is_empty() {
        let courses = opts.world.resolved_courses();
        let (timer, handle) = GateTimer::with_courses(&opts.world.gates, courses.clone(), opts.world.default_course(), cfg.bounding_radius_m(), &mut bus);
        race = Some(RaceLink { signals: RaceSignals::register(&mut bus), handle, courses });
        models.push(Box::new(timer));
    }

    // Last of all: the recorder reads the tick's final values and writes nothing to the bus.
    let recorder = RecorderHandle::default();
    models.push(Box::new(Recorder::new(recorder.clone(), base_hz)));

    let h = Handles::register(&mut bus, n, cfg.battery.cells as usize, &opts.world);
    // After the recorder, once every signal exists: it keeps the last seconds of all of them in memory.
    let tail = TailHandle::default();
    models.push(Box::new(
        TailRecorder::new(tail.clone(), &bus, base_hz, DEFAULT_WINDOW_S).map_err(|e| SimError::Other(format!("tail recorder: {e}")))?,
    ));
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let radio_antenna_names = cfg.radio.antennas.iter().map(|a| a.name.clone()).collect();
    let mut vehicle = Vehicle {
        scheduler,
        h,
        sitl: fc_kind == FcKind::Sitl,
        ports,
        firmware_dir: sitl_firmware_dir,
        slot_claim,
        osd,
        fc_telemetry,
        world: opts.world.clone(),
        terrain,
        meshes,
        radio_antenna_names,
        vtx_interference,
        prop_imbalance_baseline: cfg.props.iter().map(|p| p.imbalance_kgm).collect(),
        battery_cells: cfg.battery.cells,
        has_vtx: cfg.vtx.is_some(),
        active_faults: Vec::new(),
        recorder,
        tail,
        pending_faults: Vec::new(),
        playback: None,
        finished_inputs: None,
        inputs_recorder: None,
        journal: Journal::new(),
        journaled_sticks: [0; 8],
        race,
    };
    // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default())?;
    vehicle.set_transmitter(true);
    vehicle.init_fault_identities(&cfg.props.iter().map(|p| p.imbalance_kgm).collect::<Vec<_>>());
    // The set-up above is what a replay's own `build` does: the journal starts from here.
    vehicle.journal = Journal::new();
    vehicle.journaled_sticks = sticks_bits(&Sticks::default());
    Ok(vehicle)
}

impl Vehicle {
    /// Puts the sticks on the bus. A non-finite value is refused (nothing is applied, recorded or journaled);
    /// what is accepted is clamped to the transmitter's range first, so the bus, a recording and a replayed journal
    /// all see the same numbers.
    pub fn set_sticks(&mut self, s: &Sticks) -> Result<(), SimError> {
        if let Some(field) = s.first_non_finite() {
            return Err(SimError::InvalidArgument(format!("stick {field} must be finite")));
        }
        let s = &s.clamped();
        let tick = self.scheduler.tick();
        {
            let h = &self.h;
            let bus = self.scheduler.bus_mut();
            bus.set(h.roll, s.roll);
            bus.set(h.pitch, s.pitch);
            bus.set(h.yaw, s.yaw);
            bus.set(h.throttle, s.throttle);
            for (sig, v) in h.aux.iter().zip(s.aux) {
                bus.set(*sig, v);
            }
        }
        let state = self.inputs_state();
        if let Some(recorder) = &mut self.inputs_recorder {
            recorder.record(tick, &state);
        }
        let bits = sticks_bits(s);
        if bits != self.journaled_sticks {
            self.journaled_sticks = bits;
            self.journal.push(tick, Action::Sticks(sticks_values(s)));
        }
        Ok(())
    }

    /// Betaflight is armed: every motor is held above zero (an armed quad idles; a disarmed one outputs nothing).
    fn firmware_armed(&self) -> bool {
        let b = self.scheduler.bus();
        !self.h.cmd.is_empty() && self.h.cmd.iter().all(|s| b.get(*s) > 0.0)
    }

    /// For a session that is about to end on Betaflight while the quad is armed: disarms it, so it closes its
    /// Blackbox log (a log of a session that ended armed is cut short when SITL is killed). Everything goes straight
    /// on the bus, not through `set_sticks` or `set_transmitter`, so nothing is journaled or written to an inputs
    /// recording: the aux switches go low and the throttle to zero, the transmitter comes back on and the radio
    /// faults are cleared (a pilot who left first, or a cut link, would otherwise leave Betaflight in failsafe stage
    /// 1, holding its last aux values), and a script that is still playing is dropped. It steps in 0.1 s chunks until
    /// the motors read disarmed, at most 3 simulated seconds. A dead SITL makes the steps fail at once and that is
    /// ignored. A no-op without SITL, and for a quad that is not armed.
    pub fn disarm_for_shutdown(&mut self) {
        if !self.sitl || !self.firmware_armed() {
            return;
        }
        self.playback = None;
        let h = &self.h;
        let bus = self.scheduler.bus_mut();
        bus.set(h.throttle, 0.0);
        for sig in &h.aux {
            bus.set(*sig, -1.0);
        }
        bus.set(h.tx_enabled, 1.0);
        bus.set(h.fault_radio_loss, 0.0);
        bus.set(h.fault_radio_loss_fraction, 0.0);
        bus.set(h.fault_radio_downlink_loss_fraction, 0.0);
        bus.set(h.fault_burst_on, 0.0);
        for _ in 0..30 {
            if self.run_for(0.1).is_err() || !self.firmware_armed() {
                break;
            }
        }
    }

    /// Tests only: a NaN straight on the throttle signal, which `set_sticks` now refuses, to make the next step fail.
    #[cfg(test)]
    pub(crate) fn put_nan_on_the_throttle(&mut self) {
        let throttle = self.h.throttle;
        self.scheduler.bus_mut().set(throttle, f64::NAN);
    }

    /// Fault seams start at their identities: multiplicative faults at 1.0, the prop imbalance at its
    /// part data, additive faults at the bus's zero.
    fn init_fault_identities(&mut self, imbalance: &[f64]) {
        let h = &self.h;
        let bus = self.scheduler.bus_mut();
        for (i, imbalance) in imbalance.iter().enumerate() {
            bus.set(h.fault_motor_scale[i], 1.0);
            bus.set(h.fault_prop_thrust_scale[i], 1.0);
            bus.set(h.prop_imbalance[i], *imbalance);
        }
        bus.set(h.fault_imu_noise_scale, 1.0);
    }

    /// Transmitter on (pilot or script connected) or off (the receiver hears nothing).
    pub fn set_transmitter(&mut self, on: bool) {
        let tick = self.scheduler.tick();
        let s = self.h.tx_enabled;
        self.scheduler.bus_mut().set(s, if on { 1.0 } else { 0.0 });
        let state = self.inputs_state();
        if let Some(recorder) = &mut self.inputs_recorder {
            recorder.record(tick, &state);
        }
        self.journal.push(tick, Action::Transmitter(on));
    }

    /// Validates a fault against this quad without applying it: what `set_fault` and `schedule_fault` refuse.
    fn check_fault(&self, fault: &Fault) -> Result<(), SimError> {
        match fault {
            Fault::RadioLinkLoss => {}
            Fault::RadioLoss { fraction } => {
                if !(0.0..=1.0).contains(fraction) {
                    return Err(SimError::InvalidArgument(format!("radio loss fraction must be in [0, 1], got {fraction}")));
                }
            }
            Fault::DownlinkLoss { fraction } => {
                if !(0.0..=1.0).contains(fraction) {
                    return Err(SimError::InvalidArgument(format!(
                        "downlink loss fraction must be in [0, 1], got {fraction}"
                    )));
                }
            }
            Fault::BurstLoss { on_s, off_s } => {
                if !on_s.is_finite() || !off_s.is_finite() || *on_s <= 0.0 || *off_s <= 0.0 {
                    return Err(SimError::InvalidArgument(format!(
                        "burst on and off windows must both be finite and > 0 s, got on {on_s} s / off {off_s} s"
                    )));
                }
            }
            Fault::MotorFailure { motor, scale } => {
                if *motor >= self.h.fault_motor_scale.len() {
                    return Err(SimError::InvalidArgument(format!(
                        "motor {motor} does not exist (the quad has {})",
                        self.h.fault_motor_scale.len()
                    )));
                }
                if !(0.0..=1.0).contains(scale) {
                    return Err(SimError::InvalidArgument(format!("motor failure scale must be in [0, 1], got {scale}")));
                }
            }
            Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
                if *prop >= self.h.fault_prop_thrust_scale.len() {
                    return Err(SimError::InvalidArgument(format!(
                        "prop {prop} does not exist (the quad has {})",
                        self.h.fault_prop_thrust_scale.len()
                    )));
                }
                if !thrust_scale.is_finite() || !imbalance_kgm.is_finite() || *thrust_scale < 0.0 || *imbalance_kgm < 0.0 {
                    return Err(SimError::InvalidArgument(format!(
                        "prop damage thrust_scale and imbalance_kgm must be finite and >= 0, got {thrust_scale} / {imbalance_kgm}"
                    )));
                }
            }
            Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
                if *cell as u32 >= self.battery_cells {
                    return Err(SimError::InvalidArgument(format!(
                        "cell {cell} does not exist (the pack has {} cells)",
                        self.battery_cells
                    )));
                }
                if !r_offset_ohm.is_finite() || !v_offset_v.is_finite() || *r_offset_ohm < 0.0 || *v_offset_v < 0.0 {
                    return Err(SimError::InvalidArgument("weak-cell offsets must be finite and >= 0".into()));
                }
            }
            Fault::GyroFault { bias_radps, noise_scale } => {
                if !noise_scale.is_finite() || *noise_scale < 0.0 || !bias_radps.is_finite() {
                    return Err(SimError::InvalidArgument("gyro fault noise_scale must be >= 0 and bias finite".into()));
                }
            }
            Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
                if !self.has_vtx {
                    return Err(SimError::InvalidArgument("vtx interference needs a quad with a [vtx] section".into()));
                }
                if !freq_mhz.is_finite() || !power_mw.is_finite() || *freq_mhz <= 0.0 || *power_mw <= 0.0 || !position_ned_m.is_finite() {
                    return Err(SimError::InvalidArgument(format!(
                        "vtx interference needs a positive frequency and power and a finite position, got {freq_mhz} MHz / {power_mw} mW / {position_ned_m:?}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Activates (`active = true`) or deactivates a fault. A fault that fails validation leaves nothing
    /// behind: no signal moves and it is not recorded as active.
    pub fn set_fault(&mut self, fault: Fault, active: bool) -> Result<(), SimError> {
        self.apply_fault(fault.clone(), active)?;
        self.journal.push(self.scheduler.tick(), Action::Fault { fault, active });
        Ok(())
    }

    /// `set_fault` without the journal: what a scheduled fault does when its tick comes.
    fn apply_fault(&mut self, fault: Fault, active: bool) -> Result<(), SimError> {
        self.check_fault(&fault)?;
        {
            let b = self.scheduler.bus_mut();
            match &fault {
                Fault::RadioLinkLoss => b.set(self.h.fault_radio_loss, if active { 1.0 } else { 0.0 }),
                Fault::RadioLoss { fraction } => {
                    b.set(self.h.fault_radio_loss_fraction, if active { *fraction } else { 0.0 })
                }
                Fault::DownlinkLoss { fraction } => {
                    b.set(self.h.fault_radio_downlink_loss_fraction, if active { *fraction } else { 0.0 })
                }
                Fault::BurstLoss { on_s, off_s } => {
                    let (on, off) = if active { (*on_s, *off_s) } else { (0.0, 0.0) };
                    b.set(self.h.fault_burst_on, on);
                    b.set(self.h.fault_burst_off, off);
                }
                Fault::MotorFailure { motor, scale } => {
                    b.set(self.h.fault_motor_scale[*motor], if active { *scale } else { 1.0 })
                }
                Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
                    b.set(self.h.fault_prop_thrust_scale[*prop], if active { *thrust_scale } else { 1.0 });
                    b.set(self.h.prop_imbalance[*prop], if active { *imbalance_kgm } else { self.prop_imbalance_baseline[*prop] });
                }
                Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
                    b.set(self.h.fault_battery_weak_cell, if active { *cell as f64 + 1.0 } else { 0.0 });
                    b.set(self.h.fault_battery_r_offset, if active { *r_offset_ohm } else { 0.0 });
                    b.set(self.h.fault_battery_v_offset, if active { *v_offset_v } else { 0.0 });
                }
                Fault::GyroFault { bias_radps, noise_scale } => {
                    b.set(self.h.fault_imu_gyro_bias, if active { *bias_radps } else { DVec3::ZERO });
                    b.set(self.h.fault_imu_noise_scale, if active { *noise_scale } else { 1.0 });
                }
                Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
                    self.vtx_interference.set(active.then_some(Emitter {
                        position: *position_ned_m,
                        freq_mhz: *freq_mhz,
                        power_mw: *power_mw,
                        antenna: Antenna {
                            kind: AntennaKind::Omni,
                            gain_dbi: 2.0,
                            polarization: Polarization::Rhcp,
                            axis: DVec3::NEG_Z,
                        },
                    }));
                }
            }
        }
        self.record_fault_active(&fault, active); // maintains self.active_faults
        if matches!(&fault, Fault::RadioLinkLoss) {
            let tick = self.scheduler.tick();
            let state = self.inputs_state();
            if let Some(recorder) = &mut self.inputs_recorder {
                recorder.record(tick, &state);
            }
        }
        Ok(())
    }

    /// Activates `fault` at simulated time `at_s`, on the first tick at or after it (spec §6.2's `at=`), so a
    /// lockstep run is the same however its `Run` calls are cut. Validated now; a time already passed is refused;
    /// the current time applies at once. `clear_faults` also drops the scheduled faults.
    pub fn schedule_fault(&mut self, fault: Fault, at_s: f64) -> Result<(), SimError> {
        if !at_s.is_finite() || at_s < 0.0 {
            return Err(SimError::InvalidArgument(format!("fault time must be finite and >= 0 s, got {at_s}")));
        }
        self.check_fault(&fault)?;
        let at = tick_at(at_s, self.base_hz());
        let now = self.scheduler.tick();
        if at < now {
            return Err(SimError::InvalidArgument(format!("fault time {at_s} s has passed (the session is at {} s)", self.time_s())));
        }
        self.journal.push(now, Action::ScheduleFault { fault: fault.clone(), at_s });
        if at == now {
            return self.apply_fault(fault, true);
        }
        // Stable: faults due on one tick apply in the order they were scheduled.
        let index = self.pending_faults.partition_point(|(t, _)| *t <= at);
        self.pending_faults.insert(index, (at, fault));
        Ok(())
    }

    /// Starts playing `inputs` from now: row `t_s` applies on the first tick at or after now + `t_s`, columns the
    /// file leaves out keep their current value, and the last row's values hold after playback ends. Refused while
    /// another file plays.
    pub fn play_inputs(&mut self, inputs: Inputs) -> Result<(), SimError> {
        if let Some(playing) = &self.playback {
            return Err(SimError::InvalidArgument(format!("{} is still playing; stop it first", playing.inputs.name)));
        }
        let now = self.scheduler.tick();
        let base_hz = self.base_hz();
        let ticks = inputs.rows.iter().map(|(t, _)| now + tick_at(*t, base_hz)).collect();
        self.journal.push(now, Action::PlayInputs(inputs.clone()));
        self.playback = Some(Playback { inputs, ticks, next: 0 });
        self.apply_due(); // rows at t_s = 0 apply now
        Ok(())
    }

    /// Stops the playing inputs file, if any (the sticks keep their last values). Returns its name.
    pub fn stop_inputs(&mut self) -> Option<String> {
        self.journal.push(self.scheduler.tick(), Action::StopInputs);
        self.playback.take().map(|p| p.inputs.name)
    }

    /// The name of the inputs file playing now.
    pub fn inputs_playing(&self) -> Option<&str> {
        self.playback.as_ref().map(|p| p.inputs.name.as_str())
    }

    /// The inputs file that finished on its own since the last call (its last row applied), once.
    pub fn take_finished_inputs(&mut self) -> Option<String> {
        self.finished_inputs.take()
    }

    /// Applies what is due at the current tick: scheduled faults, then inputs rows.
    fn apply_due(&mut self) {
        let now = self.scheduler.tick();
        while self.pending_faults.first().is_some_and(|(t, _)| *t <= now) {
            let (_, fault) = self.pending_faults.remove(0);
            self.apply_fault(fault, true).expect("validated when scheduled");
        }
        while let Some((columns, values)) = self.playback.as_ref().and_then(|p| {
            (p.next < p.ticks.len() && p.ticks[p.next] <= now)
                .then(|| (p.inputs.columns.clone(), p.inputs.rows[p.next].1.clone()))
        }) {
            let mut transmitter = None;
            let mut link_loss = None;
            {
                let h = &self.h;
                let bus = self.scheduler.bus_mut();
                for (column, v) in columns.iter().zip(&values) {
                    match column {
                        Column::Stick(stick) => {
                            let signal = match stick {
                                Stick::Roll => h.roll,
                                Stick::Pitch => h.pitch,
                                Stick::Yaw => h.yaw,
                                Stick::Throttle => h.throttle,
                                Stick::Aux(i) => h.aux[*i],
                            };
                            bus.set(signal, *v);
                        }
                        Column::Transmitter => transmitter = Some(*v > 0.5),
                        Column::LinkLoss => link_loss = Some(*v > 0.5),
                    }
                }
                // The row moved the sticks without a `set_sticks`: the journal's idea of "the last sticks" is the
                // bus's now, or the next `set_sticks` to the values from before the file would look like no change.
                let values = [h.roll, h.pitch, h.yaw, h.throttle, h.aux[0], h.aux[1], h.aux[2], h.aux[3]];
                for (slot, signal) in self.journaled_sticks.iter_mut().zip(values) {
                    *slot = bus.get(signal).to_bits();
                }
            }
            if let Some(p) = self.playback.as_mut() {
                p.next += 1;
            }
            // The actions go through their own setters after the bus and playback borrows are gone. A row that
            // repeats the bus state adds nothing (and, for `tx`, no journal entry).
            if let Some(on) = transmitter {
                if (self.scheduler.bus().get(self.h.tx_enabled) > 0.5) != on {
                    self.set_transmitter(on);
                }
            }
            if let Some(loss) = link_loss {
                if (self.scheduler.bus().get(self.h.fault_radio_loss) > 0.5) != loss {
                    self.apply_fault(Fault::RadioLinkLoss, loss).expect("RadioLinkLoss is always valid");
                }
            }
        }
        if self.playback.as_ref().is_some_and(|p| p.next == p.ticks.len()) {
            self.finished_inputs = self.playback.take().map(|p| p.inputs.name);
        }
    }

    /// Remembers the fault as active (replacing an existing same-name entry, so a re-inject updates the
    /// parameters without duplicating) or forgets it.
    fn record_fault_active(&mut self, fault: &Fault, active: bool) {
        let name = fault.name();
        self.active_faults.retain(|n| n != name);
        if active {
            self.active_faults.push(name.to_string());
        }
    }

    /// The active faults' names, in the spec table's order: the event stream is deterministic regardless of
    /// injection order.
    pub fn active_fault_names(&self) -> Vec<String> {
        const ORDER: [&str; 9] = [
            "radio_link_loss",
            "radio_loss",
            "downlink_loss",
            "burst_loss",
            "motor_failure",
            "prop_damage",
            "weak_cell",
            "gyro_fault",
            "vtx_interference",
        ];
        let mut names = self.active_faults.clone();
        names.sort_by_key(|n| ORDER.iter().position(|o| o == n).unwrap_or(ORDER.len()));
        names
    }

    /// Clears every fault: the multiplicative signals back to their identities, the additive ones to zero,
    /// the radio_link_loss signal and the runtime emitter off. Never fails.
    pub fn clear_faults(&mut self) {
        self.journal.push(self.scheduler.tick(), Action::ClearFaults);
        let imbalance = self.prop_imbalance_baseline.clone();
        self.init_fault_identities(&imbalance);
        {
            let h = &self.h;
            let bus = self.scheduler.bus_mut();
            bus.set(h.fault_radio_loss, 0.0);
            bus.set(h.fault_radio_loss_fraction, 0.0);
            bus.set(h.fault_radio_downlink_loss_fraction, 0.0);
            bus.set(h.fault_burst_on, 0.0);
            bus.set(h.fault_burst_off, 0.0);
            bus.set(h.fault_battery_r_offset, 0.0);
            bus.set(h.fault_battery_v_offset, 0.0);
            bus.set(h.fault_battery_weak_cell, 0.0);
            bus.set(h.fault_imu_gyro_bias, DVec3::ZERO);
        }
        self.vtx_interference.set(None);
        self.active_faults.clear();
        self.pending_faults.clear();
        let tick = self.scheduler.tick();
        let state = self.inputs_state();
        if let Some(recorder) = &mut self.inputs_recorder {
            recorder.record(tick, &state);
        }
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        let ticks = self.scheduler.ticks_for(seconds)?;
        self.step_ticks(ticks)
    }

    /// Steps `ticks` base ticks. Scheduled faults and inputs rows apply as soon as the simulated time reaches
    /// theirs, so the state at that time already shows them. On error the failing tick is not counted.
    pub fn step_ticks(&mut self, ticks: u64) -> Result<(), SimError> {
        for _ in 0..ticks {
            self.scheduler.step()?;
            if !self.pending_faults.is_empty() || self.playback.is_some() {
                self.apply_due();
            }
        }
        Ok(())
    }

    /// The base tick the next step runs.
    pub fn tick(&self) -> u64 {
        self.scheduler.tick()
    }

    /// What has steered this run since `build`.
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Applies one journal entry through the same method the server called, so a replay is the same call sequence.
    pub fn replay_action(&mut self, action: &Action) -> Result<(), SimError> {
        match action {
            Action::Sticks(v) => self.set_sticks(&Sticks { roll: v[0], pitch: v[1], yaw: v[2], throttle: v[3], aux: [v[4], v[5], v[6], v[7]] })?,
            Action::Transmitter(on) => self.set_transmitter(*on),
            Action::Fault { fault, active } => return self.set_fault(fault.clone(), *active),
            Action::ScheduleFault { fault, at_s } => return self.schedule_fault(fault.clone(), *at_s),
            Action::ClearFaults => self.clear_faults(),
            Action::PlayInputs(inputs) => return self.play_inputs(inputs.clone()),
            Action::StopInputs => {
                self.stop_inputs();
            }
            Action::ResetRace(course) => {
                self.reset_race(*course);
            }
        }
        Ok(())
    }

    pub fn time_s(&self) -> f64 {
        self.scheduler.time_s()
    }

    pub fn base_hz(&self) -> u32 {
        self.scheduler.base_hz()
    }

    /// Where Betaflight Configurator can connect (SITL's MSP UART), when this vehicle runs Betaflight SITL.
    pub fn configurator_address(&self) -> Option<String> {
        self.sitl.then(|| self.ports.configurator_address())
    }

    /// The directory Betaflight SITL runs in (EEPROM, `sitl.log`, Blackbox logs) when this vehicle runs Betaflight.
    pub fn firmware_dir(&self) -> Option<PathBuf> {
        self.firmware_dir.clone()
    }

    /// Hands the SITL slot's claim to a rebuild of this session (a reload), which retakes the same slot through
    /// [`SitlSlot::from_claim`]: without this, the vehicle's drop releases the slot, and the first acquire after it
    /// can be another simulator's, landing the reload in a different slot's firmware directory (issue #90).
    pub fn take_slot_claim(&mut self) -> Option<SlotClaim> {
        self.slot_claim.take()
    }

    /// The latest OSD frame, when the quad has an `[osd]` section and runs Betaflight.
    pub fn osd_frame(&self) -> Option<OsdFrame> {
        self.osd.as_ref().map(OsdHandle::latest)
    }

    /// The current value of a scalar bus signal by name (`None` if no model registered it), for tests and tools.
    pub fn signal(&self, name: &str) -> Option<f64> {
        let bus = self.scheduler.bus();
        bus.lookup::<f64>(name).map(|s| bus.get(s))
    }

    /// The world the vehicle flies in (the open field when the session named none).
    pub fn world(&self) -> &WorldConfig {
        &self.world
    }

    /// The terrain the vehicle was built with (`None` for a world without `[terrain]`).
    pub fn terrain(&self) -> Option<&std::sync::Arc<ofs_core::terrain::Terrain>> {
        self.terrain.as_ref()
    }

    /// The meshes the vehicle was built with (empty for a world without `[[meshes]]`).
    pub fn meshes(&self) -> &[WorldMeshData] {
        &self.meshes
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
        let h = &self.h;
        let battery_voltage_v = b.get(h.vbat);
        let battery_current_a = b.get(h.ibat);
        // The battery model's own cell voltages: the weak-cell fault sits on the cell it names.
        let battery_cell_voltages = h.cell_voltage.iter().map(|s| b.get(*s)).collect();
        VehicleState {
            time_s: self.scheduler.time_s(),
            pos_ned_m: b.get(h.pos),
            vel_ned_mps: b.get(h.vel),
            att: b.get(h.att),
            rate_frd_radps: b.get(h.rate),
            battery_voltage_v,
            battery_current_a,
            battery_cell_voltages,
            motor_rpm: h.omega.iter().map(|s| b.get(*s) * 60.0 / (2.0 * PI)).collect(),
            motor_cmd: h.cmd.iter().map(|s| b.get(*s)).collect(),
            radio: {
                let active = b.get(h.radio_antenna);
                RadioState {
                    tx_enabled: b.get(h.tx_enabled) > 0.5,
                    link_up: b.get(h.link_up) > 0.5,
                    lq_pct: b.get(h.lq),
                    rssi_dbm: b.get(h.rssi),
                    snr_db: b.get(h.radio_snr),
                    active_antenna: self
                        .radio_antenna_names
                        .get(active.max(0.0) as usize)
                        .cloned()
                        .unwrap_or_default(),
                    downlink_lq_pct: b.get(h.radio_downlink_lq),
                }
            },
            fc_telemetry: FcTelemetrySnapshot::new(self.fc_telemetry.latest(), self.scheduler.time_s()),
            fc_restarts: b.get(h.fc_restarts) as u32,
            vtx: VtxInfo {
                present: b.get(h.vtx_present) > 0.5,
                band: b.get(h.vtx_band) as u8,
                channel: b.get(h.vtx_channel) as u8,
                freq_mhz: b.get(h.vtx_freq) as u32,
                power_mw: b.get(h.vtx_power) as u32,
                pit_mode: b.get(h.vtx_pit) > 0.5,
            },
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
            video: h.video.read(b),
            collision_speed_mps: b.get(h.collision_speed),
            collision_object: b.get(h.collision_object) as i32,
            collision_event_count: b.get(h.collision_count) as u64,
            race: self
                .race
                .as_ref()
                .map_or_else(RaceInfo::default, |r| r.signals.info(b, self.scheduler.time_s(), &r.courses)),
        }
    }

    /// Starts a fresh race at the next tick (no clock, no laps, no best time, gate 0 next) on the course with index
    /// `course`, or the current one. `false` when the world has no gates, so there is no race to reset, or `course`
    /// is not one of the world's.
    pub fn reset_race(&mut self, course: Option<usize>) -> bool {
        let Some(race) = &self.race else { return false };
        if course.is_some_and(|c| c >= race.courses.len()) {
            return false;
        }
        self.journal.push(self.scheduler.tick(), Action::ResetRace(course));
        race.handle.request_reset(course);
        true
    }

    /// The courses a race can run (empty when the world has no gates).
    pub fn courses(&self) -> &[Course] {
        self.race.as_ref().map_or(&[], |r| &r.courses)
    }

    /// The gate and lap events raised since the last call.
    pub fn take_race_events(&mut self) -> Vec<RaceEvent> {
        self.race.as_ref().map(|r| r.handle.take_events()).unwrap_or_default()
    }

    pub fn digest(&self) -> u64 {
        self.scheduler.bus().digest()
    }

    /// Starts recording the bus to a new MCAP file at `path` (the server names it). No requests: every signal at
    /// the default table's rate. Returns the recorded signals' names.
    pub fn start_recording(&mut self, requests: &[SignalRate], path: &Path) -> Result<Vec<String>, RecordError> {
        self.recorder.start(self.scheduler.bus(), self.scheduler.base_hz(), requests, path)
    }

    /// Finishes the running recording's file.
    pub fn stop_recording(&mut self) -> Result<RecordingSummary, RecordError> {
        self.recorder.stop()
    }

    /// Writes the last ten seconds of the default channel table to a new MCAP file at `path`.
    pub fn snapshot_tail(&self, path: &Path) -> Result<RecordingSummary, RecordError> {
        self.tail.snapshot(path)
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.is_recording()
    }

    /// The sticks as the bus holds them now.
    fn sticks_now(&self) -> Sticks {
        let (h, bus) = (&self.h, self.scheduler.bus());
        let mut aux = [0.0; names::RC_AUX_COUNT];
        for (a, s) in aux.iter_mut().zip(&h.aux) {
            *a = bus.get(*s);
        }
        Sticks { roll: bus.get(h.roll), pitch: bus.get(h.pitch), yaw: bus.get(h.yaw), throttle: bus.get(h.throttle), aux }
    }

    /// Everything an inputs recording row holds: the sticks and the two link booleans, as the bus holds them now.
    fn inputs_state(&self) -> InputsState {
        let bus = self.scheduler.bus();
        InputsState {
            sticks: self.sticks_now(),
            transmitter: bus.get(self.h.tx_enabled) > 0.5,
            link_loss: bus.get(self.h.fault_radio_loss) > 0.5,
        }
    }

    /// Starts recording the sticks and link state to a new CSV at `path` (the server names it): a row whenever any
    /// of it changes, in `play_inputs`' format. One at a time; an existing file is never truncated.
    pub fn start_inputs_recording(&mut self, path: &Path) -> Result<(), RecordError> {
        if self.inputs_recorder.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        let state = self.inputs_state();
        self.inputs_recorder = Some(InputsRecorder::start(path, self.scheduler.base_hz(), self.scheduler.tick(), &state)?);
        Ok(())
    }

    /// Finishes the running inputs recording's file.
    pub fn stop_inputs_recording(&mut self) -> Result<InputsRecordingSummary, RecordError> {
        self.inputs_recorder.take().ok_or(RecordError::NotRecording)?.finish()
    }

    pub fn is_recording_inputs(&self) -> bool {
        self.inputs_recorder.is_some()
    }

    /// Tests: runs `recorder` (over an injected writer) as the inputs recording.
    #[cfg(test)]
    pub(crate) fn set_inputs_recorder(&mut self, recorder: InputsRecorder) {
        self.inputs_recorder = Some(recorder);
    }

    /// Why the inputs recording died on its own (a write failed), reported once; the recording is over.
    pub fn take_inputs_recording_failure(&mut self) -> Option<String> {
        let message = self.inputs_recorder.as_ref()?.failure()?.to_owned();
        self.inputs_recorder = None;
        Some(message)
    }

    /// Why the last recording stopped on its own (a write failed), reported once.
    pub fn take_recording_failure(&mut self) -> Option<String> {
        self.recorder.take_failure()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_goggles_latency_is_reported_without_a_transmitting_vtx() {
        // m3e debt: glass-to-glass latency belongs to the goggles, not the VTX; read() zeroed it when nothing
        // transmitted (no [vtx], pit mode, a dead VTX).
        let mut world = WorldConfig::open_field();
        world.receiver.latency_ms = 60;
        let mut bus = Bus::new();
        let video = VideoHandles::register(&mut bus, &world);
        let info = video.read(&bus);
        assert!(!info.present, "nothing set video.present");
        assert_eq!(info.latency_ms, 60);
    }

    #[test]
    fn the_latency_quantizes_down_to_whole_fields() {
        assert_eq!(quantize_latency_ms(40), 40, "the default is already two fields");
        assert_eq!(quantize_latency_ms(37), 20, "37 ms delivers one field");
        assert_eq!(quantize_latency_ms(0), 0);
        assert_eq!(quantize_latency_ms(1015), 1000);
    }

    fn shipped_quad() -> QuadConfig {
        ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap()
    }

    #[test]
    fn the_shipped_quad_wires_esc_telemetry_osd_and_vtx_to_their_uarts() {
        let cfg = shipped_quad();
        let mut bus = Bus::new();
        let video = video_devices(&cfg, 4, 8, &mut bus).unwrap();
        let serial: Vec<u8> = video.serial.iter().map(|l| l.uart_index).collect();
        let taps: Vec<u8> = video.taps.iter().map(|t| t.uart_index).collect();
        assert_eq!(serial, vec![2, 4], "ESC telemetry on UART3 and the VTX replies on UART5");
        assert_eq!(taps, vec![3, 4], "DisplayPort from UART4 and SmartAudio requests from UART5");
        assert_eq!(video.before_fc.len(), 1, "the ESC telemetry runs before the flight controller");
        assert_eq!(video.after_fc.len(), 2, "the OSD and the VTX run after it");
        assert!(video.osd.is_some());
    }

    #[test]
    fn a_quad_without_video_sections_wires_nothing() {
        let mut cfg = shipped_quad();
        (cfg.esc_telemetry, cfg.osd, cfg.vtx) = (None, None, None);
        let mut bus = Bus::new();
        let video = video_devices(&cfg, 4, 8, &mut bus).unwrap();
        assert!(video.serial.is_empty() && video.taps.is_empty() && video.osd.is_none());
        assert!(video.before_fc.is_empty() && video.after_fc.is_empty());
    }

    fn flat_world() -> WorldConfig {
        world_cfg::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap()
    }

    fn open_loop(cfg: &QuadConfig, world: WorldConfig) -> Vehicle {
        let opts = BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-unit-test-data"),
            fc_override: Some(FcKind::OpenLoop),
            world,
        };
        build(cfg, &opts, None).unwrap()
    }

    #[test]
    fn an_open_loop_vehicle_has_its_vtx_at_the_power_up_channel_but_no_osd() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert_eq!(state.vtx, VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false });
        assert_eq!(state.serial_dropped_bytes, 0);
        assert!(vehicle.osd_frame().is_none());
        assert!(state.video.present);
        assert_eq!(state.video.sync, VideoSync::Locked, "{:?}", state.video);
        assert_eq!(state.video.active_antenna, "omni");
        assert_eq!(vehicle.world().name, "open field");
    }

    #[test]
    fn a_quad_without_a_vtx_has_no_video_link() {
        let mut cfg = shipped_quad();
        cfg.vtx = None;
        let mut vehicle = open_loop(&cfg, flat_world());
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert!(!state.vtx.present);
        // No link, but the goggles keep their latency (close-out: it is not VTX-conditional).
        let latency_ms = quantize_latency_ms(flat_world().receiver.latency_ms);
        assert_eq!(state.video, VideoInfo { latency_ms, ..VideoInfo::default() });
    }

    #[test]
    fn a_mixed_motor_quad_builds_and_spins_its_motors_differently() {
        let mut cfg = shipped_quad();
        let mut other = cfg.motors[0].clone();
        other.kv_rpm_per_v = 1000.0; // a slower motor on M3 and M4
        cfg.motors[2] = other.clone();
        cfg.motors[3] = other;
        let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
        vehicle.set_sticks(&Sticks { throttle: 1.0, ..Default::default() }).unwrap();
        vehicle.run_for(0.05).unwrap();
        let s = vehicle.state();
        // From rest the ESC current sits on its 45 A limit, where the torque is ke*I: the 1000KV motors
        // (double the ke) out-accelerate the 1950KV ones. The pin: each motor gets its own parameters.
        assert!(s.motor_rpm[2] > s.motor_rpm[0], "the 1000KV motors out-accelerate the 1950KV ones: {:?}", s.motor_rpm);
    }

    #[test]
    fn an_open_loop_quad_reports_no_fc_telemetry() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.1).unwrap();
        let fc = vehicle.state().fc_telemetry;
        assert_eq!(fc.frame_count, 0);
        assert_eq!(fc.age_s, f64::INFINITY);
        assert!(fc.flight_mode.is_empty());
    }

    #[test]
    fn fault_signals_start_at_their_identities() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.05).unwrap();
        let b = vehicle.scheduler.bus();
        for i in 0..4 {
            assert_eq!(b.get(vehicle.h.fault_motor_scale[i]), 1.0, "motor {i} scale");
            assert_eq!(b.get(vehicle.h.fault_prop_thrust_scale[i]), 1.0, "prop {i} thrust scale");
            assert_eq!(b.get(vehicle.h.prop_imbalance[i]), 0.0, "prop {i} imbalance (the shipped parts carry none)");
        }
        assert_eq!(b.get(vehicle.h.fault_imu_noise_scale), 1.0);
        assert_eq!(b.get(vehicle.h.fault_radio_loss_fraction), 0.0);
        assert_eq!(b.get(vehicle.h.fault_radio_downlink_loss_fraction), 0.0);
        assert_eq!(b.get(vehicle.h.fault_battery_weak_cell), 0.0);
    }

    #[test]
    fn clearing_prop_damage_restores_the_part_imbalance() {
        // A quad whose prop part carries imbalance (the shipped quad, mutated like a temp part file).
        let mut cfg = shipped_quad();
        cfg.props[0].imbalance_kgm = 2.0e-4;
        let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
        vehicle.run_for(0.01).unwrap();
        assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 2.0e-4, "the part data starts");
        vehicle.set_fault(Fault::PropDamage { prop: 0, thrust_scale: 0.9, imbalance_kgm: 5.0e-4 }, true).unwrap();
        assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 5.0e-4);
        vehicle.clear_faults();
        assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 2.0e-4, "the part baseline is restored");
        assert_eq!(vehicle.scheduler.bus().get(vehicle.h.fault_prop_thrust_scale[0]), 1.0);
    }

    #[test]
    fn downlink_loss_drops_telemetry_but_not_the_control_link() {
        let mut clean = open_loop(&shipped_quad(), WorldConfig::open_field());
        let mut faulted = open_loop(&shipped_quad(), WorldConfig::open_field());
        let sticks = Sticks { throttle: 0.5, ..Sticks::default() };
        clean.set_sticks(&sticks).unwrap();
        faulted.set_sticks(&sticks).unwrap();
        clean.run_for(0.05).unwrap();
        faulted.run_for(0.05).unwrap();
        assert_eq!(clean.state().radio.downlink_lq_pct, 100.0);
        faulted.set_fault(Fault::DownlinkLoss { fraction: 1.0 }, true).unwrap();
        assert_eq!(faulted.scheduler.bus().get(faulted.h.fault_radio_downlink_loss_fraction), 1.0);
        clean.run_for(0.25).unwrap();
        faulted.run_for(0.25).unwrap();
        assert_eq!(faulted.state().radio.downlink_lq_pct, 0.0, "the handset hears no telemetry");
        assert_eq!(faulted.state().radio.lq_pct, 100.0, "the uplink is untouched");
        assert_eq!(faulted.state().pos_ned_m, clean.state().pos_ned_m, "the sticks still fly the quad");
        assert_eq!(faulted.state().motor_cmd, clean.state().motor_cmd);
        faulted.clear_faults();
        clean.run_for(0.25).unwrap();
        faulted.run_for(0.25).unwrap();
        assert_eq!(faulted.state().radio.downlink_lq_pct, 100.0, "clearing restores the downlink");
        assert_eq!(faulted.state().pos_ned_m, clean.state().pos_ned_m);
    }

    #[test]
    fn a_downlink_loss_fraction_outside_the_unit_interval_is_refused() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for fraction in [1.5, -0.1, f64::NAN] {
            let err = vehicle.set_fault(Fault::DownlinkLoss { fraction }, true).unwrap_err();
            assert!(err.to_string().contains("downlink loss fraction"), "{fraction}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
            assert_eq!(vehicle.scheduler.bus().get(vehicle.h.fault_radio_downlink_loss_fraction), 0.0);
        }
    }

    #[test]
    fn clear_faults_returns_every_fault_signal_to_its_identity() {
        // The honest re-join pin: a physically diverged trajectory cannot re-join a clean run's
        // digest, so the pin is exhaustive over the fault signals instead — every one of them.
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.set_fault(Fault::MotorFailure { motor: 1, scale: 0.0 }, true).unwrap();
        vehicle.set_fault(Fault::PropDamage { prop: 2, thrust_scale: 0.8, imbalance_kgm: 3.0e-4 }, true).unwrap();
        vehicle.set_fault(Fault::RadioLoss { fraction: 0.7 }, true).unwrap();
        vehicle.set_fault(Fault::DownlinkLoss { fraction: 0.7 }, true).unwrap();
        vehicle.set_fault(Fault::BurstLoss { on_s: 1.0, off_s: 0.5 }, true).unwrap();
        vehicle.set_fault(Fault::GyroFault { bias_radps: DVec3::new(0.1, 0.0, 0.0), noise_scale: 2.0 }, true).unwrap();
        vehicle.set_fault(Fault::WeakCell { cell: 3, r_offset_ohm: 0.01, v_offset_v: 0.1 }, true).unwrap();
        vehicle.run_for(0.01).unwrap(); // the models consume the faulted values before the clear
        vehicle.clear_faults();
        let b = vehicle.scheduler.bus();
        for i in 0..4 {
            assert_eq!(b.get(vehicle.h.fault_motor_scale[i]), 1.0, "motor {i} scale");
            assert_eq!(b.get(vehicle.h.fault_prop_thrust_scale[i]), 1.0, "prop {i} thrust scale");
            assert_eq!(b.get(vehicle.h.prop_imbalance[i]), 0.0, "prop {i} imbalance (the shipped parts carry none)");
        }
        assert_eq!(b.get(vehicle.h.fault_radio_loss), 0.0);
        assert_eq!(b.get(vehicle.h.fault_radio_loss_fraction), 0.0);
        assert_eq!(b.get(vehicle.h.fault_radio_downlink_loss_fraction), 0.0);
        assert_eq!(b.get(vehicle.h.fault_burst_on), 0.0);
        assert_eq!(b.get(vehicle.h.fault_burst_off), 0.0);
        assert_eq!(b.get(vehicle.h.fault_battery_r_offset), 0.0);
        assert_eq!(b.get(vehicle.h.fault_battery_v_offset), 0.0);
        assert_eq!(b.get(vehicle.h.fault_battery_weak_cell), 0.0);
        assert_eq!(b.get(vehicle.h.fault_imu_gyro_bias), DVec3::ZERO);
        assert_eq!(b.get(vehicle.h.fault_imu_noise_scale), 1.0);
        assert!(vehicle.active_fault_names().is_empty());
    }

    #[test]
    fn disarming_for_shutdown_is_a_no_op_without_sitl() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.set_sticks(&Sticks { throttle: 0.7, ..Default::default() }).unwrap();
        vehicle.run_for(0.1).unwrap();
        let (time, entries, throttle) = (vehicle.time_s(), vehicle.journal().entries().len(), vehicle.signal("rc.throttle"));
        vehicle.disarm_for_shutdown();
        assert_eq!(vehicle.time_s(), time, "no simulated time passes");
        assert_eq!(vehicle.journal().entries().len(), entries);
        assert_eq!(vehicle.signal("rc.throttle"), throttle, "the sticks are left alone");
    }

    #[test]
    fn an_unknown_motor_index_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        let err = vehicle.set_fault(Fault::MotorFailure { motor: 7, scale: 0.0 }, true).unwrap_err();
        assert!(err.to_string().contains("motor 7"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
        assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
    }

    #[test]
    fn an_unknown_cell_index_is_invalid_argument() {
        // The weak-cell fault now lands on a real cell of the series string: a cell the pack lacks must be refused,
        // not silently applied to nothing.
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        let err = vehicle.set_fault(Fault::WeakCell { cell: 6, r_offset_ohm: 0.02, v_offset_v: 0.1 }, true).unwrap_err();
        assert!(err.to_string().contains("cell 6"), "{err}");
        assert!(err.to_string().contains("does not exist"), "{err}");
        assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        vehicle.set_fault(Fault::WeakCell { cell: 5, r_offset_ohm: 0.02, v_offset_v: 0.1 }, true).expect("the last cell exists");
    }

    #[test]
    fn a_non_finite_or_non_positive_burst_window_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for (on_s, off_s) in [(f64::NAN, 1.0), (1.0, f64::NAN), (f64::INFINITY, 1.0), (1.0, 0.0)] {
            let err = vehicle.set_fault(Fault::BurstLoss { on_s, off_s }, true).unwrap_err();
            assert!(err.to_string().contains("burst on and off windows"), "{on_s} / {off_s}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        }
    }

    #[test]
    fn a_non_finite_or_negative_prop_damage_parameter_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for (thrust_scale, imbalance_kgm) in
            [(f64::NAN, 1.0e-4), (0.5, f64::NAN), (f64::INFINITY, 1.0e-4), (-0.5, 1.0e-4)]
        {
            let err = vehicle
                .set_fault(Fault::PropDamage { prop: 0, thrust_scale, imbalance_kgm }, true)
                .unwrap_err();
            assert!(err.to_string().contains("finite"), "{thrust_scale} / {imbalance_kgm}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        }
        let b = vehicle.scheduler.bus();
        assert_eq!(b.get(vehicle.h.fault_prop_thrust_scale[0]), 1.0, "a rejected fault leaves nothing behind");
        assert_eq!(b.get(vehicle.h.prop_imbalance[0]), 0.0, "a rejected fault leaves nothing behind");
    }

    #[test]
    fn a_non_finite_weak_cell_offset_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for (r_offset_ohm, v_offset_v) in [(f64::NAN, 0.1), (0.01, f64::NAN)] {
            let err = vehicle
                .set_fault(Fault::WeakCell { cell: 2, r_offset_ohm, v_offset_v }, true)
                .unwrap_err();
            assert!(err.to_string().contains("finite"), "{r_offset_ohm} / {v_offset_v}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        }
    }

    #[test]
    fn a_non_finite_or_negative_gyro_noise_scale_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for (bias_radps, noise_scale) in [(DVec3::ZERO, f64::NAN), (DVec3::ZERO, f64::INFINITY), (DVec3::ZERO, -1.0)] {
            let err = vehicle
                .set_fault(Fault::GyroFault { bias_radps, noise_scale }, true)
                .unwrap_err();
            assert!(err.to_string().contains("finite"), "{bias_radps:?} / {noise_scale}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        }
        let b = vehicle.scheduler.bus();
        assert_eq!(b.get(vehicle.h.fault_imu_gyro_bias), DVec3::ZERO, "a rejected fault leaves nothing behind");
        assert_eq!(b.get(vehicle.h.fault_imu_noise_scale), 1.0, "a rejected fault leaves nothing behind");
    }

    #[test]
    fn vtx_interference_without_a_vtx_is_invalid_argument() {
        let mut cfg = shipped_quad();
        cfg.vtx = None;
        let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
        let err = vehicle
            .set_fault(Fault::VtxInterference { freq_mhz: 5700.0, power_mw: 100.0, position_ned_m: DVec3::X }, true)
            .unwrap_err();
        assert!(err.to_string().contains("[vtx]"), "{err}");
    }

    #[test]
    fn a_non_finite_or_non_positive_vtx_parameter_is_invalid_argument() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        for (freq_mhz, power_mw) in
            [(f64::NAN, 100.0), (5700.0, f64::NAN), (f64::INFINITY, 100.0), (5700.0, f64::INFINITY), (0.0, 100.0)]
        {
            let err = vehicle
                .set_fault(Fault::VtxInterference { freq_mhz, power_mw, position_ned_m: DVec3::X }, true)
                .unwrap_err();
            assert!(err.to_string().contains("positive"), "{freq_mhz} / {power_mw}: {err}");
            assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
        }
    }

    #[test]
    fn injected_and_cleared_faults_are_named_in_order() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.set_fault(Fault::WeakCell { cell: 2, r_offset_ohm: 0.01, v_offset_v: 0.05 }, true).unwrap();
        vehicle.set_fault(Fault::RadioLinkLoss, true).unwrap();
        assert_eq!(vehicle.active_fault_names(), vec!["radio_link_loss".to_string(), "weak_cell".to_string()]);
        vehicle.clear_faults();
        assert!(vehicle.active_fault_names().is_empty());
    }

    #[test]
    fn the_active_faults_list_replaces_removes_and_sorts() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        // Injected out of the spec table's order...
        for fault in [
            Fault::GyroFault { bias_radps: DVec3::ZERO, noise_scale: 2.0 },
            Fault::RadioLinkLoss,
            Fault::DownlinkLoss { fraction: 0.5 },
            Fault::WeakCell { cell: 2, r_offset_ohm: 0.01, v_offset_v: 0.05 },
        ] {
            vehicle.set_fault(fault, true).unwrap();
        }
        // ...the names still come back in the spec table's order.
        assert_eq!(
            vehicle.active_fault_names(),
            ["radio_link_loss", "downlink_loss", "weak_cell", "gyro_fault"].map(str::to_string)
        );
        // A re-inject updates the entry instead of duplicating it.
        vehicle.set_fault(Fault::RadioLinkLoss, true).unwrap();
        assert_eq!(
            vehicle.active_fault_names(),
            ["radio_link_loss", "downlink_loss", "weak_cell", "gyro_fault"].map(str::to_string)
        );
        // Deactivating removes the name.
        vehicle.set_fault(Fault::RadioLinkLoss, false).unwrap();
        assert_eq!(vehicle.active_fault_names(), ["downlink_loss", "weak_cell", "gyro_fault"].map(str::to_string));
    }

    #[test]
    fn motors_integrate_theta() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.set_sticks(&Sticks { throttle: 1.0, ..Default::default() }).unwrap();
        vehicle.run_for(0.10).unwrap();
        let b = vehicle.scheduler.bus();
        let theta = b.get(vehicle.h.motor_theta[0]);
        assert!(theta > 0.0 && theta < 2.0 * std::f64::consts::PI, "wrapped into [0, 2π): {theta}");
        let omega = b.get(vehicle.h.omega[0]);
        // 0.10 s at a rising rpm: theta is in the same ballpark as omega·dt, never wildly off.
        assert!(theta < omega * 0.11 + 2.0 * std::f64::consts::PI, "theta consistent with omega: {theta} vs {omega}");
    }

    #[test]
    fn the_link_parameters_follow_the_world_file() {
        let world = flat_world();
        let params = link_params(shipped_quad().vtx.as_ref().unwrap(), &world, None, &[]).unwrap();
        let w = &params.world;
        assert_eq!(w.pilot_position, DVec3::new(-3.0, 2.0, -1.7));
        assert_eq!(w.antennas.len(), 2);
        assert!((w.antennas[0].antenna.axis - DVec3::NEG_Z).length() < 1e-12, "the omni stands upright");
        let patch = w.antennas[1].antenna;
        assert_eq!(patch.kind, AntennaKind::Patch { beamwidth_deg: 60.0 });
        let expected = DVec3::new(10f64.to_radians().cos(), 0.0, -10f64.to_radians().sin());
        assert!((patch.axis - expected).length() < 1e-12, "the patch looks north, 10 degrees up: {}", patch.axis);
        assert_eq!(w.obstacles.len(), 4, "only the buildings take signal");
        assert_eq!(w.emitters.len(), 1);
        assert_eq!(w.emitters[0].freq_mhz, 5695.0, "R2");
        assert!((params.vtx_antenna.axis - DVec3::new(-0.5, 0.0, -1.0).normalize()).length() < 1e-12);
        assert_eq!(params.pit_power_mw, 0.1);
        assert!(params.fading && params.ground_bounce);
        assert!((aim_ned(90.0, 0.0) - DVec3::Y).length() < 1e-12, "heading 90 is east");
    }

    /// A world with the baked default-ish hills (amplitude 8 m) and the grid it bakes to.
    fn hilly_world() -> (WorldConfig, Terrain) {
        let mut world = WorldConfig::open_field();
        world.terrain = Some(ofs_config::TerrainSection { amplitude_m: 8.0, ..Default::default() });
        let terrain = world.terrain.unwrap().build();
        (world, terrain)
    }

    /// A 10 m building with an RF loss whose bottom sits at `bottom_d` (NED down) over the footprint at `at`.
    fn building(at: (f64, f64), bottom_d: f64) -> world_cfg::ObjectSection {
        let mut o = flat_world().objects.into_iter().find(|o| o.rf_loss_db > 0.0).unwrap();
        o.shape = world_cfg::Shape::Box;
        o.size_m = Some([10.0, 10.0, 10.0]);
        o.center_ned_m = [at.0, at.1, bottom_d - 5.0];
        o
    }

    /// The first spot on a coarse grid around home where the ground is at least 1 m above (`up`) or below the plane.
    fn spot(terrain: &Terrain, up: bool) -> (f64, f64) {
        let grid = (-40..=40).flat_map(|i| (-40..=40).map(move |j| (f64::from(i) * 5.0, f64::from(j) * 5.0)));
        grid.into_iter().find(|&(n, e)| if up { terrain.ground_d(n, e) < -1.0 } else { terrain.ground_d(n, e) > 1.0 }).unwrap()
    }

    /// Is `p`, 100 m under the object's bottom, inside its obstacle (that is, is it rooted down to there)?
    fn rooted_under(world: &WorldConfig, terrain: Option<&Terrain>, at: (f64, f64), bottom_d: f64) -> bool {
        let obstacles = world_obstacles(world, terrain);
        assert_eq!(obstacles.len(), 1);
        obstacles[0].shape.signed_distance(DVec3::new(at.0, at.1, bottom_d + 100.0)) < 0.0
    }

    #[test]
    fn objects_on_hilly_ground_root_to_the_terrain() {
        let (mut world, terrain) = hilly_world();
        let hill = spot(&terrain, true);
        let hill_d = terrain.ground_d(hill.0, hill.1);
        world.objects = vec![building(hill, hill_d)];
        assert!(rooted_under(&world, Some(&terrain), hill, hill_d), "a building on the hill stands on the ground");
        assert!(!rooted_under(&world, None, hill, hill_d), "on the plane it would hang above z = 0");

        // Where the ground is lower than the plane, a building at z = 0 floats; one on the valley floor stands.
        let valley = spot(&terrain, false);
        world.objects = vec![building(valley, 0.0)];
        assert!(!rooted_under(&world, Some(&terrain), valley, 0.0), "it floats above the valley");
        assert!(rooted_under(&world, None, valley, 0.0), "the plane alone would say it stands");
        let valley_d = terrain.ground_d(valley.0, valley.1);
        world.objects = vec![building(valley, valley_d)];
        assert!(rooted_under(&world, Some(&terrain), valley, valley_d));
    }

    #[test]
    fn objects_carry_the_yaw_and_360_is_identity() {
        // Canonicalization is at object_shape: the file's degrees map through rem_euclid(360).to_radians(),
        // so a full turn (or an explicit zero) lands on exactly the identity yaw the fast path branches on.
        let yawed = |deg: f64| {
            let mut world = WorldConfig::open_field();
            let mut o = building((0.0, 0.0), 0.0);
            o.yaw_deg = Some(deg);
            world.objects = vec![o];
            world
        };
        let Shape::Box { yaw_rad, .. } = world_objects(&yawed(135.0))[0].shape else { panic!("a box") };
        assert_eq!(yaw_rad, 135f64.to_radians());
        for full_turn in [360.0, 0.0] {
            let Shape::Box { yaw_rad, .. } = world_objects(&yawed(full_turn))[0].shape else { panic!("a box") };
            assert_eq!(yaw_rad, 0.0, "{full_turn} degrees is exactly the identity yaw");
        }
        let Shape::Box { yaw_rad, .. } = world_obstacles(&yawed(135.0), None)[0].shape else { panic!("a box") };
        assert_eq!(yaw_rad, 135f64.to_radians(), "the rooted obstacle keeps the yaw");
    }

    #[test]
    fn a_world_without_terrain_roots_exactly_as_before() {
        let world = flat_world();
        let expected: Vec<_> = world.objects.iter().filter(|o| o.rf_loss_db > 0.0).map(|o| (object_shape(o).rooted(), o.rf_loss_db)).collect();
        let got: Vec<_> = world_obstacles(&world, None).into_iter().map(|o| (o.shape, o.rf_loss_db)).collect();
        assert_eq!(got.len(), 4);
        assert_eq!(got, expected);
    }

    #[test]
    fn the_links_and_the_body_share_one_grid() {
        let hills = world_cfg::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/hills.toml"))).unwrap();
        let vehicle = open_loop(&shipped_quad(), hills);
        let terrain = vehicle.terrain.as_ref().expect("the hills world bakes a grid");
        // The vehicle's own handle, the body's airframe, the ELRS link's parameters and the video link's world: no copies.
        assert_eq!(Arc::strong_count(terrain), 4);
    }

    fn hall_world() -> WorldConfig {
        world_cfg::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/hall.toml"))).unwrap()
    }

    #[test]
    fn a_mesh_world_shares_one_mesh_with_physics_and_both_links() {
        let vehicle = open_loop(&shipped_quad(), hall_world());
        let data = &vehicle.meshes()[0];
        assert_eq!(data.name, "hall");
        // The vehicle's own data, the body's airframe, the ELRS link's parameters and the video link's world: no copies.
        assert_eq!(Arc::strong_count(&data.mesh), 4);
    }

    #[test]
    fn a_world_without_meshes_has_none_anywhere() {
        let vehicle = open_loop(&shipped_quad(), flat_world());
        assert!(vehicle.meshes().is_empty());
        assert!(link_params(shipped_quad().vtx.as_ref().unwrap(), &flat_world(), None, &[]).unwrap().world.meshes.is_empty());
    }

    #[test]
    fn a_world_whose_mesh_file_vanished_fails_to_build_instead_of_panicking() {
        let mut world = hall_world();
        world.source_path = std::env::temp_dir().join("ofs-no-such-dir").join("hall.toml");
        let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-unit-test-data"), fc_override: Some(FcKind::OpenLoop), world };
        assert!(matches!(build(&shipped_quad(), &opts, None), Err(SimError::InvalidArgument(_))));
    }

    #[test]
    fn a_world_without_terrain_has_no_terrain_in_the_links() {
        let vehicle = open_loop(&shipped_quad(), flat_world());
        assert!(vehicle.terrain.is_none());
        assert!(link_params(shipped_quad().vtx.as_ref().unwrap(), &flat_world(), None, &[]).unwrap().world.terrain.is_none());
    }

    #[test]
    fn a_hill_between_the_pilot_and_the_quad_cuts_the_video_in_the_real_world() {
        // A 10 m ridge across the north-south line from the pilot, crest 100 m north of home.
        let heights = (0..41usize * 41).map(|k| (10.0f32 * (1.0 - (k / 41).abs_diff(20) as f32 / 10.0)).max(0.0)).collect();
        let ridge = Arc::new(Terrain::from_grid([-100.0, -200.0], 10.0, 41, 41, heights).unwrap());
        let world = world_cfg::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/hills.toml"))).unwrap();
        let params = link_params(shipped_quad().vtx.as_ref().unwrap(), &world, Some(ridge), &[]).unwrap();
        let obstruction = |north: f64, east: f64| {
            ofs_video::link::vtx_paths(&params, DVec3::new(north, east, -1.7), DQuat::IDENTITY, 5800.0)[0].obstruction_db
        };
        let (behind, beside) = (obstruction(200.0, 0.0), obstruction(0.0, 300.0));
        assert!(behind > beside + 10.0, "behind the ridge {behind} dB, beside it {beside} dB");
    }

    /// The quad resting on the ground at `north`, `east` in the flat world (or the same world without objects).
    fn snr_at(north: f64, east: f64, objects: bool, power_index: usize) -> VideoInfo {
        let mut cfg = shipped_quad();
        cfg.initial.position_ned_m = [north, east, -0.03];
        cfg.vtx.as_mut().unwrap().default_power_index = power_index;
        let mut world = flat_world();
        if !objects {
            world.objects.clear();
        }
        let mut vehicle = open_loop(&cfg, world);
        vehicle.run_for(0.2).unwrap();
        vehicle.state().video
    }

    #[test]
    fn the_picture_is_clean_near_the_pilot_and_lost_far_away() {
        let near = snr_at(10.0, 0.0, true, 1);
        assert_eq!((near.sync, near.noise, near.chroma), (VideoSync::Locked, 0.0, 1.0), "10 m out at 200 mW: {near:?}");
        let far = snr_at(4000.0, 0.0, true, 0);
        assert_eq!(far.sync, VideoSync::Lost, "4 km out at 25 mW: {far:?}");
        assert!(far.noise > 0.9, "static: {far:?}");
        assert_eq!(far.active_antenna, "patch", "out in front the patch hears it best");
    }

    #[test]
    fn building_b_shadows_the_quad_behind_it() {
        // Twice as far from the pilot as building B's centre, on the same bearing: the building is in the way.
        let (north, east) = (-3.0 + 2.0 * 78.0, 2.0 + 2.0 * 36.0);
        let open = snr_at(north, east, false, 1);
        let shadowed = snr_at(north, east, true, 1);
        assert!(open.snr_db - shadowed.snr_db > 15.0, "open {} dB, behind the building {} dB", open.snr_db, shadowed.snr_db);
    }

    #[test]
    fn cell_voltages_are_equal_shares_and_the_weak_cell_carries_the_delta() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.1).unwrap();
        let clean = vehicle.state();
        assert_eq!(clean.battery_cell_voltages.len(), 6);
        let share = clean.battery_voltage_v / 6.0;
        assert!(clean.battery_cell_voltages.iter().all(|c| (c - share).abs() < 1e-9));
        vehicle.set_fault(Fault::WeakCell { cell: 0, r_offset_ohm: 0.02, v_offset_v: 0.1 }, true).unwrap();
        vehicle.run_for(0.002).unwrap(); // the battery model republishes every 8th tick
        let weak = vehicle.state();
        assert!(weak.battery_cell_voltages[0] < share - 0.05, "cell 0 carries the delta: {:?}", weak.battery_cell_voltages);
        assert!((weak.battery_cell_voltages[3] - share).abs() < 1e-9, "the healthy cells are unchanged");
        vehicle.clear_faults();
        vehicle.run_for(0.002).unwrap();
        let back = vehicle.state();
        assert!(back.battery_cell_voltages.iter().all(|c| (c - share).abs() < 1e-9));
    }

    #[test]
    fn the_injected_emitter_reaches_the_state_snapshot() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.2).unwrap();
        let clean = vehicle.state().video;
        vehicle
            .set_fault(
                Fault::VtxInterference {
                    freq_mhz: 5700.0,
                    power_mw: 500.0,
                    position_ned_m: DVec3::new(5.0, 0.0, -1.7),
                },
                true,
            )
            .unwrap();
        vehicle.run_for(0.2).unwrap();
        let noisy = vehicle.state().video;
        assert!(
            noisy.interference_dbm > clean.interference_dbm + 3.0,
            "{:?} -> {:?}",
            clean.interference_dbm,
            noisy.interference_dbm
        );
        assert!(noisy.snr_db < clean.snr_db);
        vehicle.clear_faults();
        vehicle.run_for(0.2).unwrap();
        let back = vehicle.state().video;
        assert!((back.interference_dbm - clean.interference_dbm).abs() < 0.5, "clears back: {:?}", back.interference_dbm);
    }
}
