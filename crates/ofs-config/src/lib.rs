//! Quad description files (TOML): parsing and validation that reports every problem at once.
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub mod world;
pub use world::{AntennaKind, Polarization, WorldConfig};

pub mod part;

/// 5: quads assemble from library parts (M4a) — frame/motor/prop/esc/battery/vtx hardware moves to
/// `library/`, every part parameter carries provenance. 4: the real radio link and [collision] (M3c).
/// 3: [esc_telemetry]/[osd]/[vtx] (M3a). 2: the required [radio] section (M2).
pub const SCHEMA_VERSION: u32 = 5;
/// Oldest schema this build still reads: only 5 (the hardware sections live in library/ part files, so older
/// files cannot be mapped onto it).
pub const MIN_SCHEMA_VERSION: u32 = 5;

/// The resolved quad: the aircraft's own sections merged with the library parts it referenced.
#[derive(Debug, Clone)]
pub struct QuadConfig {
    pub schema_version: u32,
    pub name: String,
    pub sim: SimSection,
    pub frame: FrameSection,
    pub ground: GroundSection,
    pub motors: Vec<MotorSection>,
    pub props: Vec<PropSection>,
    pub escs: Vec<EscSection>,
    pub battery: BatterySection,
    pub imu: ImuSection,
    pub baro: BaroSection,
    pub home: HomeSection,
    pub initial: InitialSection,
    pub fc: FcSection,
    pub radio: RadioSection,
    pub collision: Option<CollisionSection>,
    pub esc_telemetry: Option<EscTelemetrySection>,
    pub osd: Option<OsdSection>,
    pub vtx: Option<VtxSection>,
    /// The part ids the quad referenced, kept for error messages and introspection.
    pub parts: PartRefs,
    pub source_path: PathBuf,
}

/// The quad file as written: part references plus the aircraft's own wiring.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuadFile {
    schema_version: u32,
    name: String,
    sim: SimSection,
    frame: String,
    motors: Vec<String>,
    props: Vec<String>,
    escs: Vec<String>,
    battery: BatteryWiring,
    imu: ImuSection,
    baro: BaroSection,
    home: HomeSection,
    initial: InitialSection,
    fc: FcSection,
    radio: RadioSection,
    #[serde(default)]
    esc_telemetry: Option<EscTelemetrySection>,
    #[serde(default)]
    osd: Option<OsdSection>,
    #[serde(default)]
    vtx: Option<VtxWiring>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatteryWiring {
    part: String,
    #[serde(default = "default_initial_soc")]
    initial_soc: f64,
    #[serde(default = "default_battery_rate_hz")]
    rate_hz: u32,
}

fn default_initial_soc() -> f64 {
    1.0
}

fn default_battery_rate_hz() -> u32 {
    1000
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VtxWiring {
    part: String,
    uart: u8,
    #[serde(default = "default_vtx_band")]
    default_band: String,
    #[serde(default = "default_vtx_channel")]
    default_channel: u8,
    #[serde(default)]
    default_power_index: usize,
}

/// The part ids a quad referenced, kept for error messages and introspection.
#[derive(Debug, Clone, PartialEq)]
pub struct PartRefs {
    pub frame: String,
    pub motors: Vec<String>,
    pub props: Vec<String>,
    pub escs: Vec<String>,
    pub battery: String,
    pub vtx: Option<String>,
}

/// The library root: the `OFS_LIBRARY` override when given, else `library/` beside the quads.
pub fn library_root(env: Option<&str>, quad_dir: &Path) -> PathBuf {
    match env.map(str::trim).filter(|s| !s.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => quad_dir.parent().unwrap_or_else(|| Path::new(".")).join("library"),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSection {
    pub base_hz: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSection {
    pub mass_kg: f64,
    pub inertia_kgm2: [f64; 3],
    pub cda_m2: [f64; 3],
    pub angular_damping_nms: f64,
    pub motor_positions_frd_m: Vec<[f64; 3]>,
    pub motor_spin: Vec<i8>,
    pub contact_points_frd_m: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundSection {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub friction_coeff: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorSection {
    pub kv_rpm_per_v: f64,
    pub resistance_ohm: f64,
    pub no_load_current_a: f64,
    pub rotor_inertia_kgm2: f64,
    /// Electrical revolutions per mechanical revolution; Betaflight's `motor_poles` / 2 (M4a). Required —
    /// Betaflight takes it as data, so the simulator must not guess it.
    pub pole_pairs: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSection {
    pub diameter_m: f64,
    pub inertia_kgm2: f64,
    pub ct_table: Vec<[f64; 2]>,
    pub cp_table: Vec<[f64; 2]>,
    /// Static imbalance (mass × radius) the M4b vibration model turns into IMU disturbance. 0 = none.
    #[serde(default)]
    pub imbalance_kgm: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscSection {
    pub response_tau_s: f64,
    pub current_limit_a: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatterySection {
    pub cells: u32,
    pub capacity_mah: f64,
    pub r0_ohm: f64,
    pub r1_ohm: f64,
    pub c1_f: f64,
    pub initial_soc: f64,
    pub rate_hz: u32,
    pub ocv_table: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImuSection {
    pub gyro_noise_std_radps: f64,
    pub gyro_bias_radps: [f64; 3],
    pub accel_noise_std_mps2: f64,
    pub accel_bias_mps2: [f64; 3],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaroSection {
    pub noise_std_pa: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeSection {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialSection {
    pub position_ned_m: [f64; 3],
    pub yaw_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FcKind {
    Sitl,
    OpenLoop,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FcSection {
    pub kind: FcKind,
    pub exchange_hz: u32,
    pub launch: Vec<String>,
    #[serde(default)]
    pub cleanup: Vec<String>,
    pub betaflight_diff: String,
    #[serde(default = "default_reply_timeout_ms")]
    pub reply_timeout_ms: u64,
    /// The first exchange waits longer: TCP 5761 accepting does not mean the main loop is running.
    #[serde(default = "default_first_reply_timeout_ms")]
    pub first_reply_timeout_ms: u64,
    #[serde(default = "default_startup_timeout_ms")]
    pub startup_timeout_ms: u64,
}

fn default_reply_timeout_ms() -> u64 {
    500
}

fn default_first_reply_timeout_ms() -> u64 {
    5_000
}

fn default_startup_timeout_ms() -> u64 {
    15_000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioKind {
    /// ExpressLRS, behavioural model (fidelity level 1).
    Elrs,
}

/// The ExpressLRS band (`kind = "elrs"`); absent means 2.4 GHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum BandKind {
    #[serde(rename = "2g4")]
    G2_4,
    #[serde(rename = "900")]
    Mhz900,
}

impl BandKind {
    pub fn name(self) -> &'static str {
        match self {
            BandKind::G2_4 => "2g4",
            BandKind::Mhz900 => "900",
        }
    }

    /// The packet rates the band's LoRa modes offer. `ofs-radio`'s `Band` is the model's own copy; a test in
    /// `ofs-sim` keeps the two equal.
    pub fn packet_rates(self) -> &'static [u32] {
        match self {
            BandKind::G2_4 => &[50, 150, 250, 500],
            BandKind::Mhz900 => &[25, 50, 100, 200, 250],
        }
    }

    fn label(self) -> &'static str {
        match self {
            BandKind::G2_4 => "2.4 GHz",
            BandKind::Mhz900 => "900 MHz",
        }
    }
}

impl RadioSection {
    pub fn band_or_default(&self) -> BandKind {
        self.band.unwrap_or(BandKind::G2_4)
    }
}

/// The pilot's radio link. The receiver's CRSF output is wired to a Betaflight UART.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioSection {
    pub kind: RadioKind,
    /// The ExpressLRS LoRa modes of the band: 2.4 GHz 50, 150, 250 or 500; 900 MHz 25, 50, 100, 200 or 250 packets per second.
    pub packet_rate_hz: u32,
    /// Betaflight UART number the receiver is wired to (1-based, as in the Configurator's Ports tab).
    pub uart: u8,
    /// `kind = "elrs"` only: the ExpressLRS band, `"2g4"` (default) or `"900"`; it fixes which packet rates are valid.
    #[serde(default)]
    pub band: Option<BandKind>,
    #[serde(default = "default_latency_packets")]
    pub latency_packets: u32,
    /// The handset's TX power in mW: one of 10, 25, 50, 100, 250, 500, 1000.
    #[serde(default = "default_tx_power_mw")]
    pub tx_power_mw: u32,
    #[serde(default = "default_link_stats_interval_packets")]
    pub link_stats_interval_packets: u32,
    /// The receiver's antennas: one, or two for receiver diversity.
    #[serde(default = "default_radio_antennas")]
    pub antennas: Vec<RadioAntennaSection>,
}

fn default_latency_packets() -> u32 {
    1
}

fn default_tx_power_mw() -> u32 {
    250
}

fn default_link_stats_interval_packets() -> u32 {
    50
}

fn default_radio_antennas() -> Vec<RadioAntennaSection> {
    vec![RadioAntennaSection::default()]
}

/// A receiver antenna on the quad: a 2.4 GHz dipole.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioAntennaSection {
    /// Unique; it names the antenna in the state stream.
    pub name: String,
    #[serde(default)]
    pub kind: AntennaKind,
    #[serde(default = "default_radio_antenna_gain_dbi")]
    pub gain_dbi: f64,
    #[serde(default = "default_radio_antenna_polarization")]
    pub polarization: Polarization,
    /// The dipole's axis in the body frame: FRD, x forward, y right, z down.
    #[serde(default = "default_radio_antenna_mount_frd")]
    pub mount_frd: [f64; 3],
}

impl Default for RadioAntennaSection {
    fn default() -> Self {
        Self {
            name: "antenna".into(),
            kind: AntennaKind::Omni,
            gain_dbi: default_radio_antenna_gain_dbi(),
            polarization: default_radio_antenna_polarization(),
            mount_frd: default_radio_antenna_mount_frd(),
        }
    }
}

fn default_radio_antenna_gain_dbi() -> f64 {
    2.0
}

fn default_radio_antenna_polarization() -> Polarization {
    Polarization::Linear
}

/// Up and back, like the VTX antenna: a typical 2.4 GHz dipole on the back of the frame.
fn default_radio_antenna_mount_frd() -> [f64; 3] {
    [-0.5, 0.0, -1.0]
}

/// The battery as Betaflight's ESC sensor: KISS telemetry frames into a Betaflight UART.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscTelemetrySection {
    /// Betaflight UART number (1-based).
    pub uart: u8,
    #[serde(default = "default_esc_rate_hz")]
    pub rate_hz: u32,
}

fn default_esc_rate_hz() -> u32 {
    100
}

/// Betaflight's OSD over MSP DisplayPort, decoded into a character grid.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OsdSection {
    /// Betaflight UART number (1-based).
    pub uart: u8,
    #[serde(default = "default_osd_cols")]
    pub cols: u32,
    #[serde(default = "default_osd_rows")]
    pub rows: u32,
}

fn default_osd_cols() -> u32 {
    30
}

fn default_osd_rows() -> u32 {
    16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VtxKind {
    /// SmartAudio v2.1 (fidelity level 1).
    Smartaudio,
    /// Tramp (fidelity level 1).
    Tramp,
}

/// The video transmitter, controlled by Betaflight over SmartAudio.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VtxSection {
    pub kind: VtxKind,
    /// Betaflight UART number (1-based).
    pub uart: u8,
    /// Power-up band: A, B, E, F, R (Raceband) or L (Lowband).
    #[serde(default = "default_vtx_band")]
    pub default_band: String,
    /// Power-up channel, 1..=8.
    #[serde(default = "default_vtx_channel")]
    pub default_channel: u8,
    /// Output power of each level in mW and in dBm (SmartAudio v2.1 reports dBm); the same length. Betaflight builds
    /// its power list from the dBm values the VTX reports (the SITL build has no vtxtable).
    pub power_levels_mw: Vec<u32>,
    pub power_levels_dbm: Vec<u8>,
    /// Power-up level (0-based index into the lists).
    #[serde(default)]
    pub default_power_index: usize,
    /// How long the VTX takes to answer a request.
    #[serde(default = "default_vtx_reply_latency_ms")]
    pub reply_latency_ms: f64,
    /// The antenna on the quad (default: a 2 dBi RHCP omni pointing up and back).
    #[serde(default)]
    pub antenna: VtxAntennaSection,
    /// Output power in pit mode.
    #[serde(default = "default_pit_power_mw")]
    pub pit_power_mw: f64,
}

/// The VTX antenna, for the video link model.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VtxAntennaSection {
    #[serde(default = "default_vtx_antenna_kind")]
    pub kind: AntennaKind,
    #[serde(default = "default_vtx_antenna_gain_dbi")]
    pub gain_dbi: f64,
    /// Patch only.
    #[serde(default)]
    pub beamwidth_deg: Option<f64>,
    #[serde(default = "default_vtx_polarization")]
    pub polarization: Polarization,
    /// The omni's axis (or the patch's boresight) in the body frame: FRD, x forward, y right, z down.
    #[serde(default = "default_vtx_mount_frd")]
    pub mount_frd: [f64; 3],
}

impl Default for VtxAntennaSection {
    fn default() -> Self {
        Self {
            kind: default_vtx_antenna_kind(),
            gain_dbi: default_vtx_antenna_gain_dbi(),
            beamwidth_deg: None,
            polarization: default_vtx_polarization(),
            mount_frd: default_vtx_mount_frd(),
        }
    }
}

fn default_vtx_antenna_kind() -> AntennaKind {
    AntennaKind::Omni
}

fn default_vtx_antenna_gain_dbi() -> f64 {
    2.0
}

fn default_vtx_polarization() -> Polarization {
    Polarization::Rhcp
}

/// Up and back, about 27 degrees from upright, as on a typical 5" quad.
fn default_vtx_mount_frd() -> [f64; 3] {
    [-0.5, 0.0, -1.0]
}

pub(crate) fn default_pit_power_mw() -> f64 {
    0.1
}

/// How the quad collides with the world. Optional: the defaults generate a sphere per prop tip and one for the body.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionSection {
    #[serde(default = "default_restitution")]
    pub restitution: f64,
    #[serde(default = "default_friction_coeff")]
    pub friction_coeff: f64,
    /// Collision spheres as `[x, y, z, radius]` in the body frame (FRD, metres). Default: generated from the
    /// motor positions and the prop diameter (see [`QuadConfig::collision_spheres`]).
    #[serde(default)]
    pub spheres_frd_m: Option<Vec<[f64; 4]>>,
}

fn default_restitution() -> f64 {
    0.3
}

fn default_friction_coeff() -> f64 {
    0.5
}

/// The radius of each generated prop-tip sphere.
pub const PROP_SPHERE_RADIUS_M: f64 = 0.02;
/// The radius of the generated body sphere at the centre of mass.
pub const BODY_SPHERE_RADIUS_M: f64 = 0.04;
/// Generated spheres per prop tip circle.
pub const SPHERES_PER_PROP: usize = 6;

/// The video link model runs once per PAL field.
pub use ofs_core::consts::VIDEO_FIELD_RATE_HZ;

fn default_vtx_band() -> String {
    "R".into()
}

fn default_vtx_channel() -> u8 {
    1
}

pub(crate) fn default_vtx_reply_latency_ms() -> f64 {
    5.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub field: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: cannot read file: {source}", path = .path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {message}", path = .path.display())]
    Parse { path: PathBuf, message: String },
    #[error("{path}: {count} problem(s):\n{list}", path = .path.display(), count = .problems.len(), list = format_problems(.problems))]
    Invalid { path: PathBuf, problems: Vec<Problem> },
}

fn format_problems(problems: &[Problem]) -> String {
    let mut out = String::new();
    for p in problems {
        let _ = writeln!(out, "  - {}: {}", p.field, p.message);
    }
    out
}

pub fn load(path: &Path) -> Result<QuadConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
    let parse_err = |message: String| ConfigError::Parse { path: path.to_path_buf(), message };
    let raw: toml::Value = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    match raw.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v >= i64::from(MIN_SCHEMA_VERSION) && v <= i64::from(SCHEMA_VERSION) => {}
        Some(4) => {
            let message = "unsupported schema_version 4 (this build reads schema 5); schema 5 moves the [frame], \
                           [ground], [motor], [prop], [esc], [battery] and [vtx] hardware sections into library/ part \
                           files with per-parameter provenance and the quad references them by id — see the \
                           \"Component library\" section of README.md".to_string();
            return Err(parse_err(message));
        }
        Some(v @ ..=3) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads schema {SCHEMA_VERSION}); schema 4 removes radio.rssi_dbm, radio.snr_db, \
                 radio.loss_good, radio.loss_bad, radio.p_good_to_bad and radio.p_bad_to_good, and adds radio.tx_power_mw, the \
                 [[radio.antennas]] list and the optional [collision] section, see quads/opendrone-5f-freestyle.toml"
            );
            return Err(parse_err(message));
        }
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION})"))),
        None => return Err(parse_err("missing integer schema_version".into())),
    }
    let file: QuadFile = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    let root = library_root(std::env::var("OFS_LIBRARY").ok().as_deref(), path.parent().unwrap_or(Path::new(".")));
    let mut problems = Vec::new();
    let resolve = |kind: &str, id: &str, field: &str, problems: &mut Vec<Problem>| -> Option<PathBuf> {
        let dir = root.join(kind);
        let path = dir.join(format!("{id}.toml"));
        if path.is_file() {
            return Some(path);
        }
        let message = if dir.is_dir() {
            let mut have: Vec<String> = fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
                        .collect()
                })
                .unwrap_or_default();
            have.sort();
            format!("part '{id}' not found in {} (have: {})", dir.display(), have.join(", "))
        } else {
            format!("library kind directory not found: {} (library root {}; set OFS_LIBRARY to override)", dir.display(), root.display())
        };
        problems.push(Problem { field: field.to_string(), message });
        None
    };

    let Some(frame_path) = resolve("frames", &file.frame, "frame", &mut problems) else {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    };
    let frame_part = part::load_frame(&frame_path)?;
    let n = frame_part.frame.motor_positions_frd_m.len();
    for (field, list) in [("motors", &file.motors), ("props", &file.props), ("escs", &file.escs)] {
        if list.len() != 1 && list.len() != n {
            problems.push(Problem { field: field.to_string(), message: format!("needs 1 or {n} entries (got {})", list.len()) });
        }
    }
    let mut motor_paths = Vec::with_capacity(file.motors.len());
    for (i, id) in file.motors.iter().enumerate() {
        if let Some(p) = resolve("motors", id, &format!("motors[{i}]"), &mut problems) {
            motor_paths.push(p);
        }
    }
    let mut prop_paths = Vec::with_capacity(file.props.len());
    for (i, id) in file.props.iter().enumerate() {
        if let Some(p) = resolve("props", id, &format!("props[{i}]"), &mut problems) {
            prop_paths.push(p);
        }
    }
    let mut esc_paths = Vec::with_capacity(file.escs.len());
    for (i, id) in file.escs.iter().enumerate() {
        if let Some(p) = resolve("escs", id, &format!("escs[{i}]"), &mut problems) {
            esc_paths.push(p);
        }
    }
    let battery_path = resolve("batteries", &file.battery.part, "battery.part", &mut problems);
    let mut vtx_path = None;
    if let Some(wiring) = &file.vtx {
        vtx_path = resolve("vtx", &wiring.part, "vtx.part", &mut problems);
    }
    if !problems.is_empty() {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    }

    // Every id resolved above (any failure returned with the problems), so the optional paths are present.
    let battery_part = part::load_battery(&battery_path.expect("the battery id resolved"))?;
    let motors: Vec<MotorSection> =
        broadcast(&motor_paths, n, part::load_motor)?.into_iter().map(|p| p.motor).collect();
    let props: Vec<PropSection> =
        broadcast(&prop_paths, n, part::load_prop)?.into_iter().map(|p| p.prop).collect();
    let escs: Vec<EscSection> =
        broadcast(&esc_paths, n, part::load_esc)?.into_iter().map(|p| p.esc).collect();
    let battery = BatterySection {
        cells: battery_part.battery.cells,
        capacity_mah: battery_part.battery.capacity_mah,
        r0_ohm: battery_part.battery.r0_ohm,
        r1_ohm: battery_part.battery.r1_ohm,
        c1_f: battery_part.battery.c1_f,
        ocv_table: battery_part.battery.ocv_table,
        initial_soc: file.battery.initial_soc,
        rate_hz: file.battery.rate_hz,
    };
    let vtx = match (vtx_path, &file.vtx) {
        (Some(vtx_path), Some(wiring)) => {
            let hardware = part::load_vtx(&vtx_path)?.vtx;
            Some(VtxSection {
                kind: hardware.kind,
                uart: wiring.uart,
                default_band: wiring.default_band.clone(),
                default_channel: wiring.default_channel,
                power_levels_mw: hardware.power_levels_mw,
                power_levels_dbm: hardware.power_levels_dbm,
                default_power_index: wiring.default_power_index,
                reply_latency_ms: hardware.reply_latency_ms,
                antenna: hardware.antenna,
                pit_power_mw: hardware.pit_power_mw,
            })
        }
        _ => None,
    };
    let cfg = QuadConfig {
        schema_version: file.schema_version,
        name: file.name,
        sim: file.sim,
        frame: frame_part.frame,
        ground: frame_part.ground,
        collision: frame_part.collision,
        motors,
        props,
        escs,
        battery,
        imu: file.imu,
        baro: file.baro,
        home: file.home,
        initial: file.initial,
        fc: file.fc,
        radio: file.radio,
        esc_telemetry: file.esc_telemetry,
        osd: file.osd,
        vtx,
        parts: PartRefs {
            frame: file.frame,
            motors: file.motors,
            props: file.props,
            escs: file.escs,
            battery: file.battery.part,
            vtx: file.vtx.as_ref().map(|v| v.part.clone()),
        },
        source_path: path.to_path_buf(),
    };
    let problems = cfg.validate();
    if !problems.is_empty() {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    }
    Ok(cfg)
}

/// Loads every path's part; a single part broadcasts to `n` entries (one part id covering every motor,
/// prop or ESC of the aircraft).
fn broadcast<P: Clone>(paths: &[PathBuf], n: usize, load: impl Fn(&Path) -> Result<P, ConfigError>) -> Result<Vec<P>, ConfigError> {
    let parts = paths.iter().map(|p| load(p)).collect::<Result<Vec<_>, ConfigError>>()?;
    if parts.len() == 1 {
        return Ok(vec![parts[0].clone(); n]);
    }
    Ok(parts)
}

struct Checker(Vec<Problem>);

impl Checker {
    fn check(&mut self, ok: bool, field: &str, message: impl Into<String>) {
        if !ok {
            self.0.push(Problem { field: field.to_string(), message: message.into() });
        }
    }

    fn positive(&mut self, v: f64, field: &str) {
        self.check(v.is_finite() && v > 0.0, field, format!("must be > 0 (got {v})"));
    }

    fn non_negative(&mut self, v: f64, field: &str) {
        self.check(v.is_finite() && v >= 0.0, field, format!("must be >= 0 (got {v})"));
    }

    fn table(&mut self, t: &[[f64; 2]], field: &str) {
        self.check(!t.is_empty(), field, "must have at least one row");
        self.check(t.windows(2).all(|w| w[1][0] > w[0][0]), field, "first column must be strictly increasing");
        self.check(t.iter().flatten().all(|v| v.is_finite()), field, "values must be finite");
    }

    fn finite<'a>(&mut self, values: impl IntoIterator<Item = &'a f64>, field: &str) {
        self.check(values.into_iter().all(|v| v.is_finite()), field, "values must be finite");
    }

    fn divides(&mut self, base_hz: u32, hz: u32, field: &str) {
        self.check(hz > 0 && base_hz % hz == 0, field, format!("must be > 0 and divide sim.base_hz {base_hz} (got {hz})"));
    }
}

impl QuadConfig {
    pub fn source_dir(&self) -> &Path {
        self.source_path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// Resolves a path written in the quad file relative to the quad file's directory.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        self.source_dir().join(rel)
    }

    /// The collision spheres: the file's list, or the generated default — 6 spheres of radius 2 cm evenly
    /// spaced on each prop's tip circle (in the motor's plane), plus one body sphere of radius 4 cm at the
    /// centre of mass.
    pub fn collision_spheres(&self) -> Vec<[f64; 4]> {
        if let Some(spheres) = self.collision.as_ref().and_then(|c| c.spheres_frd_m.clone()) {
            return spheres;
        }
        let mut spheres = Vec::with_capacity(self.frame.motor_positions_frd_m.len() * SPHERES_PER_PROP + 1);
        for (m, p) in self.frame.motor_positions_frd_m.iter().zip(&self.props) {
            let tip = p.diameter_m / 2.0;
            for k in 0..SPHERES_PER_PROP {
                let a = 2.0 * std::f64::consts::PI * (k as f64) / SPHERES_PER_PROP as f64;
                spheres.push([m[0] + tip * a.cos(), m[1] + tip * a.sin(), m[2], PROP_SPHERE_RADIUS_M]);
            }
        }
        spheres.push([0.0, 0.0, 0.0, BODY_SPHERE_RADIUS_M]);
        spheres
    }

    /// The restitution and friction coefficients of contacts, defaults filled in.
    pub fn collision_contact(&self) -> (f64, f64) {
        match &self.collision {
            Some(c) => (c.restitution, c.friction_coeff),
            None => (default_restitution(), default_friction_coeff()),
        }
    }

    pub fn validate(&self) -> Vec<Problem> {
        let mut c = Checker(Vec::new());
        c.check(self.sim.base_hz > 0, "sim.base_hz", "must be > 0");

        let f = &self.frame;
        c.positive(f.mass_kg, "frame.mass_kg");
        f.inertia_kgm2.iter().for_each(|v| c.positive(*v, "frame.inertia_kgm2"));
        f.cda_m2.iter().for_each(|v| c.non_negative(*v, "frame.cda_m2"));
        c.non_negative(f.angular_damping_nms, "frame.angular_damping_nms");
        c.check(
            f.motor_positions_frd_m.len() == 4,
            "frame.motor_positions_frd_m",
            format!("v1 supports exactly 4 motors (Betaflight SITL outputs 4), got {}", f.motor_positions_frd_m.len()),
        );
        c.check(f.motor_spin.len() == f.motor_positions_frd_m.len(), "frame.motor_spin", "needs one entry per motor");
        c.check(f.motor_spin.iter().all(|s| *s == 1 || *s == -1), "frame.motor_spin", "entries must be 1 or -1");
        c.check(!f.contact_points_frd_m.is_empty(), "frame.contact_points_frd_m", "needs at least one point");
        c.finite(f.motor_positions_frd_m.iter().flatten(), "frame.motor_positions_frd_m");
        c.finite(f.contact_points_frd_m.iter().flatten(), "frame.contact_points_frd_m");
        c.finite(&self.imu.gyro_bias_radps, "imu.gyro_bias_radps");
        c.finite(&self.imu.accel_bias_mps2, "imu.accel_bias_mps2");
        c.finite(&self.initial.position_ned_m, "initial.position_ned_m");
        c.finite([&self.initial.yaw_deg], "initial.yaw_deg");
        c.finite([&self.home.alt_m], "home.alt_m");

        c.positive(self.ground.stiffness_npm, "ground.stiffness_npm");
        c.non_negative(self.ground.damping_nspm, "ground.damping_nspm");
        c.non_negative(self.ground.friction_coeff, "ground.friction_coeff");

        for (i, m) in self.motors.iter().enumerate() {
            c.positive(m.kv_rpm_per_v, &format!("motors[{i}].kv_rpm_per_v"));
            c.positive(m.resistance_ohm, &format!("motors[{i}].resistance_ohm"));
            c.non_negative(m.no_load_current_a, &format!("motors[{i}].no_load_current_a"));
            c.positive(m.rotor_inertia_kgm2, &format!("motors[{i}].rotor_inertia_kgm2"));
            c.check(
                (1..=32).contains(&m.pole_pairs),
                &format!("motors[{i}].pole_pairs"),
                format!("must be 1..=32 (got {})", m.pole_pairs),
            );
        }
        for (i, p) in self.props.iter().enumerate() {
            c.positive(p.diameter_m, &format!("props[{i}].diameter_m"));
            c.non_negative(p.inertia_kgm2, &format!("props[{i}].inertia_kgm2"));
            c.table(&p.ct_table, &format!("props[{i}].ct_table"));
            c.table(&p.cp_table, &format!("props[{i}].cp_table"));
            c.non_negative(p.imbalance_kgm, &format!("props[{i}].imbalance_kgm"));
        }
        for (i, e) in self.escs.iter().enumerate() {
            c.positive(e.response_tau_s, &format!("escs[{i}].response_tau_s"));
            c.positive(e.current_limit_a, &format!("escs[{i}].current_limit_a"));
        }

        let b = &self.battery;
        c.check((1..=14).contains(&b.cells), "battery.cells", format!("must be 1..=14 (got {})", b.cells));
        c.positive(b.capacity_mah, "battery.capacity_mah");
        c.non_negative(b.r0_ohm, "battery.r0_ohm");
        c.positive(b.r1_ohm, "battery.r1_ohm");
        c.positive(b.c1_f, "battery.c1_f");
        c.check((0.0..=1.0).contains(&b.initial_soc), "battery.initial_soc", format!("must be in [0, 1] (got {})", b.initial_soc));
        c.table(&b.ocv_table, "battery.ocv_table");
        c.divides(self.sim.base_hz, b.rate_hz, "battery.rate_hz");

        c.non_negative(self.imu.gyro_noise_std_radps, "imu.gyro_noise_std_radps");
        c.non_negative(self.imu.accel_noise_std_mps2, "imu.accel_noise_std_mps2");
        c.non_negative(self.baro.noise_std_pa, "baro.noise_std_pa");

        c.check((-90.0..=90.0).contains(&self.home.lat_deg), "home.lat_deg", "must be in [-90, 90]");
        c.check((-180.0..=180.0).contains(&self.home.lon_deg), "home.lon_deg", "must be in [-180, 180]");

        c.divides(self.sim.base_hz, self.fc.exchange_hz, "fc.exchange_hz");
        for (ms, field) in [
            (self.fc.reply_timeout_ms, "fc.reply_timeout_ms"),
            (self.fc.first_reply_timeout_ms, "fc.first_reply_timeout_ms"),
            (self.fc.startup_timeout_ms, "fc.startup_timeout_ms"),
        ] {
            c.check(ms > 0, field, "must be > 0");
        }
        let r = &self.radio;
        let band = r.band_or_default();
        let rates = band.packet_rates();
        let listed = match rates.split_last() {
            Some((last, rest)) => format!("{} or {last}", rest.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")),
            None => String::new(),
        };
        c.check(
            rates.contains(&r.packet_rate_hz),
            "radio.packet_rate_hz",
            format!("must be one of the ExpressLRS {} LoRa modes: {listed} (got {})", band.label(), r.packet_rate_hz),
        );
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
        c.check(
            matches!(r.tx_power_mw, 10 | 25 | 50 | 100 | 250 | 500 | 1000),
            "radio.tx_power_mw",
            format!("must be one of 10, 25, 50, 100, 250, 500 or 1000 mW (got {})", r.tx_power_mw),
        );
        c.check(r.link_stats_interval_packets > 0, "radio.link_stats_interval_packets", "must be > 0");
        c.check(
            (1..=2).contains(&r.antennas.len()),
            "radio.antennas",
            format!("needs one or two antennas (two = receiver diversity), got {}", r.antennas.len()),
        );
        for (i, a) in r.antennas.iter().enumerate() {
            let field = |f: &str| format!("radio.antennas[{i}].{f}");
            c.check(matches!(a.kind, AntennaKind::Omni), &field("kind"), "only omni (dipole) antennas are modelled for ELRS");
            c.check(
                r.antennas.iter().take(i).all(|other| other.name != a.name),
                &field("name"),
                format!("{:?} is used twice", a.name),
            );
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(
                a.mount_frd.iter().all(|x| x.is_finite()) && a.mount_frd.iter().any(|x| *x != 0.0),
                &field("mount_frd"),
                "must be a finite, non-zero direction",
            );
        }
        if let Some(x) = &self.collision {
            c.check(
                (0.0..=1.0).contains(&x.restitution),
                "collision.restitution",
                format!("must be in [0, 1] (got {})", x.restitution),
            );
            c.non_negative(x.friction_coeff, "collision.friction_coeff");
            if let Some(spheres) = &x.spheres_frd_m {
                c.check(!spheres.is_empty(), "collision.spheres_frd_m", "must not be empty when given");
                c.check(spheres.iter().flatten().all(|v| v.is_finite()), "collision.spheres_frd_m", "values must be finite");
                c.check(spheres.iter().all(|s| s[3] > 0.0), "collision.spheres_frd_m", "radii must be > 0");
            }
        }
        if self.schema_version < 3 && (self.esc_telemetry.is_some() || self.osd.is_some() || self.vtx.is_some()) {
            c.check(false, "schema_version", "the [esc_telemetry], [osd] and [vtx] sections need schema_version = 3");
        }
        let uart_ok = |u: u8| (2..=8).contains(&u);
        let range = |what: &str, u: u8| format!("must be 2..=8; UART1 is Betaflight's MSP port, tcp:5761 (got {u}) in {what}");
        c.check(uart_ok(r.uart), "radio.uart", range("radio.uart", r.uart));
        let mut uarts: Vec<(&str, u8)> = vec![("radio.uart", r.uart)];
        if let Some(e) = &self.esc_telemetry {
            c.check(uart_ok(e.uart), "esc_telemetry.uart", range("esc_telemetry.uart", e.uart));
            c.divides(self.sim.base_hz, e.rate_hz, "esc_telemetry.rate_hz");
            c.check(e.rate_hz <= 1000, "esc_telemetry.rate_hz", format!("must be <= 1000 (got {})", e.rate_hz));
            uarts.push(("esc_telemetry.uart", e.uart));
        }
        if let Some(o) = &self.osd {
            c.check(uart_ok(o.uart), "osd.uart", range("osd.uart", o.uart));
            c.check((1..=64).contains(&o.cols), "osd.cols", format!("must be 1..=64 (got {})", o.cols));
            c.check((1..=32).contains(&o.rows), "osd.rows", format!("must be 1..=32 (got {})", o.rows));
            uarts.push(("osd.uart", o.uart));
        }
        if let Some(v) = &self.vtx {
            c.check(uart_ok(v.uart), "vtx.uart", range("vtx.uart", v.uart));
            uarts.push(("vtx.uart", v.uart));
            c.check(
                matches!(v.default_band.as_str(), "A" | "B" | "E" | "F" | "R" | "L"),
                "vtx.default_band",
                format!("must be one of A, B, E, F, R, L (got {})", v.default_band),
            );
            c.check((1..=8).contains(&v.default_channel), "vtx.default_channel", format!("must be 1..=8 (got {})", v.default_channel));
            c.check(
                !v.power_levels_mw.is_empty() && v.power_levels_mw.len() <= 8 && v.power_levels_mw.iter().all(|m| *m > 0),
                "vtx.power_levels_mw",
                "must have 1 to 8 entries, all > 0",
            );
            c.check(
                v.power_levels_dbm.len() == v.power_levels_mw.len()
                    && v.power_levels_dbm.iter().all(|d| (1..=40).contains(d))
                    && v.power_levels_dbm.windows(2).all(|w| w[1] > w[0]),
                "vtx.power_levels_dbm",
                format!("needs one entry per power_levels_mw entry ({}), strictly increasing, each in 1..=40 dBm", v.power_levels_mw.len()),
            );
            c.check(
                v.default_power_index < v.power_levels_mw.len(),
                "vtx.default_power_index",
                format!("must be < {} (got {})", v.power_levels_mw.len(), v.default_power_index),
            );
            c.check(
                v.reply_latency_ms.is_finite() && (0.0..=100.0).contains(&v.reply_latency_ms),
                "vtx.reply_latency_ms",
                format!("must be in [0, 100] ms (got {})", v.reply_latency_ms),
            );
            c.check(
                self.sim.base_hz % VIDEO_FIELD_RATE_HZ == 0,
                "sim.base_hz",
                format!("must be a multiple of {VIDEO_FIELD_RATE_HZ} when the quad has [vtx]: the video link runs once per PAL field (got {})", self.sim.base_hz),
            );
            c.positive(v.pit_power_mw, "vtx.pit_power_mw");
            let a = &v.antenna;
            c.check(a.gain_dbi.is_finite(), "vtx.antenna.gain_dbi", "must be finite");
            c.check(
                a.mount_frd.iter().all(|x| x.is_finite()) && a.mount_frd.iter().any(|x| *x != 0.0),
                "vtx.antenna.mount_frd",
                "must be a finite, non-zero direction",
            );
            world::check_beamwidth(&mut c, a.kind, a.beamwidth_deg, "vtx.antenna.beamwidth_deg");
        }
        for (i, (field, uart)) in uarts.iter().enumerate() {
            if let Some((other, _)) = uarts[..i].iter().find(|(_, u)| u == uart) {
                c.check(false, field, format!("UART{uart} is already used by {other}"));
            }
        }
        if self.fc.kind == FcKind::Sitl {
            c.check(!self.fc.launch.is_empty(), "fc.launch", "must not be empty for kind = \"sitl\"");
            let diff = self.resolve(&self.fc.betaflight_diff);
            c.check(diff.is_file(), "fc.betaflight_diff", format!("file not found: {}", diff.display()));
            if let Ok(text) = std::fs::read_to_string(&diff) {
                let vtx_uart = self.vtx.as_ref().map(|v| v.uart);
                let esc_uart = self.esc_telemetry.as_ref().map(|e| e.uart);
                let osd_uart = self.osd.as_ref().map(|o| o.uart);
                let msp_displayport = diff_setting(&text, "osd_displayport_device").is_some_and(|v| v.eq_ignore_ascii_case("MSP"));
                let mut esc_sensor = false;
                for (index, functions) in serial_functions(&text) {
                    let uart = index + 1;
                    if functions & SERIAL_FUNCTION_ESC_SENSOR != 0 {
                        esc_sensor = true;
                        c.check(
                            esc_uart == Some(uart),
                            "fc.betaflight_diff",
                            format!(
                                "the diff enables the ESC sensor on UART{uart} but the quad has no [esc_telemetry] section with                                  uart = {uart}; the battery would read 0 V in Betaflight"
                            ),
                        );
                    }
                    if msp_displayport && uart != 1 && functions & SERIAL_FUNCTION_MSP != 0 {
                        c.check(
                            osd_uart == Some(uart),
                            "fc.betaflight_diff",
                            format!(
                                "the diff sends the OSD over MSP DisplayPort on UART{uart} but the quad has no [osd] section                                  with uart = {uart}; the OSD would not be drawn"
                            ),
                        );
                    }
                    if functions & SERIAL_FUNCTION_SMARTAUDIO != 0 && vtx_uart != Some(index + 1) {
                        c.check(
                            false,
                            "fc.betaflight_diff",
                            format!(
                                "the diff enables SmartAudio on UART{0} but the quad has no [vtx] section with uart = {0}; \
                                 Betaflight SITL crashes when its OSD shows the VTX channel and no VTX answers",
                                index + 1
                            ),
                        );
                    }
                }
                if esc_sensor {
                    let cells = self.battery.cells.to_string();
                    let forced = diff_setting(&text, "force_battery_cell_count");
                    c.check(
                        forced == Some(cells.as_str()),
                        "fc.betaflight_diff",
                        format!(
                            "with the battery as the ESC sensor the diff must `set force_battery_cell_count = {cells}` (battery.cells,                              got {}); Betaflight otherwise guesses the cell count from the first voltage it sees",
                            forced.unwrap_or("no such line")
                        ),
                    );
                }
                let pole_pairs = self.motors.iter().map(|m| m.pole_pairs).collect::<Vec<_>>();
                // no motors (the frame's motor-count problem already reports it): the SITL diff checks are meaningless without them
                if let Some(&first) = pole_pairs.first() {
                    if pole_pairs.iter().any(|p| *p != first) {
                        c.check(false, "motors", "SITL quads need uniform motor pole_pairs: Betaflight has one motor_poles setting, and the KISS battery frames carry one pack eRPM");
                    }
                    let poles = 2 * first;
                    match diff_setting(&text, "motor_poles").map(str::trim).and_then(|v| v.parse::<u32>().ok().map(|p| (v, p))) {
                        Some((raw, set)) => c.check(
                            set == poles,
                            "fc.betaflight_diff",
                            format!("the diff sets motor_poles = {raw} but the motors have {poles} poles (2 x pole_pairs); the RPM filter would mis-scale"),
                        ),
                        None => c.check(
                            poles == 14,
                            "motors",
                            format!("the diff does not set motor_poles, so Betaflight uses its default 14, but the motors have {poles} poles; set pole_pairs = 7 or add `set motor_poles = {poles}` to the diff"),
                        ),
                    }
                }
            }
        }
        c.0
    }
}

/// Betaflight's serial function bits: MSP, the ESC sensor (KISS telemetry) and a SmartAudio VTX.
const SERIAL_FUNCTION_MSP: u32 = 1;
const SERIAL_FUNCTION_ESC_SENSOR: u32 = 1024;
const SERIAL_FUNCTION_SMARTAUDIO: u32 = 2048;

/// The value of a `set <name> = <value>` line of a Betaflight diff (the last one wins, as in Betaflight).
fn diff_setting<'a>(diff: &'a str, name: &str) -> Option<&'a str> {
    diff.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("set ")?;
            let (key, value) = rest.split_once('=')?;
            (key.trim() == name).then(|| value.trim())
        })
        .next_back()
}

/// The `serial <index> <function mask> ...` lines of a Betaflight diff: (0-based UART index, function mask).
fn serial_functions(diff: &str) -> Vec<(u8, u32)> {
    diff.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next()? == "serial").then_some(())?;
            let index = words.next()?.parse().ok()?;
            let functions = words.next()?.parse().ok()?;
            Some((index, functions))
        })
        .collect()
}
