//! Quad description files (TOML): parsing and validation that reports every problem at once.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub mod world;
pub use world::{AntennaKind, Polarization, WorldConfig};

/// 3: adds the optional `[esc_telemetry]`, `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 3;
/// Oldest schema this build still reads (schema-2 files have no video sections).
pub const MIN_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuadConfig {
    pub schema_version: u32,
    pub name: String,
    pub sim: SimSection,
    pub frame: FrameSection,
    pub ground: GroundSection,
    pub motor: MotorSection,
    pub prop: PropSection,
    pub esc: EscSection,
    pub battery: BatterySection,
    pub imu: ImuSection,
    pub baro: BaroSection,
    pub home: HomeSection,
    pub initial: InitialSection,
    pub fc: FcSection,
    pub radio: RadioSection,
    #[serde(default)]
    pub esc_telemetry: Option<EscTelemetrySection>,
    #[serde(default)]
    pub osd: Option<OsdSection>,
    #[serde(default)]
    pub vtx: Option<VtxSection>,
    #[serde(skip)]
    pub source_path: PathBuf,
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSection {
    pub diameter_m: f64,
    pub inertia_kgm2: f64,
    pub ct_table: Vec<[f64; 2]>,
    pub cp_table: Vec<[f64; 2]>,
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

/// The pilot's radio link. The receiver's CRSF output is wired to a Betaflight UART.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioSection {
    pub kind: RadioKind,
    pub packet_rate_hz: u32,
    /// Betaflight UART number the receiver is wired to (1-based, as in the Configurator's Ports tab).
    pub uart: u8,
    #[serde(default = "default_latency_packets")]
    pub latency_packets: u32,
    /// Per-packet loss probability in the good and the bad (burst) channel state.
    #[serde(default)]
    pub loss_good: f64,
    #[serde(default)]
    pub loss_bad: f64,
    /// Per-packet probability of entering and of leaving the bad state (Gilbert-Elliott burst loss).
    #[serde(default)]
    pub p_good_to_bad: f64,
    #[serde(default = "default_p_bad_to_good")]
    pub p_bad_to_good: f64,
    #[serde(default = "default_rssi_dbm")]
    pub rssi_dbm: f64,
    #[serde(default = "default_snr_db")]
    pub snr_db: f64,
    #[serde(default = "default_link_stats_interval_packets")]
    pub link_stats_interval_packets: u32,
    /// Reported as-is in CRSF link statistics.
    #[serde(default)]
    pub rf_mode: u8,
    #[serde(default)]
    pub tx_power: u8,
}

fn default_latency_packets() -> u32 {
    1
}

fn default_p_bad_to_good() -> f64 {
    1.0
}

fn default_rssi_dbm() -> f64 {
    -50.0
}

fn default_snr_db() -> f64 {
    10.0
}

fn default_link_stats_interval_packets() -> u32 {
    50
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

fn default_pit_power_mw() -> f64 {
    0.1
}

/// The video link model runs once per PAL field.
pub const VIDEO_FIELD_RATE_HZ: u32 = 50;

fn default_vtx_band() -> String {
    "R".into()
}

fn default_vtx_channel() -> u8 {
    1
}

fn default_vtx_reply_latency_ms() -> f64 {
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
        Some(v @ ..=1) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}); schema 2 adds the required [radio] section \
                 and the CRSF receiver lines in the quad's betaflight.diff, see quads/opendrone-5f-freestyle.toml and \
                 quads/opendrone-5f-freestyle.betaflight.diff"
            );
            return Err(parse_err(message));
        }
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION})"))),
        None => return Err(parse_err("missing integer schema_version".into())),
    }
    let mut cfg: QuadConfig = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    cfg.source_path = path.to_path_buf();
    let problems = cfg.validate();
    if !problems.is_empty() {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    }
    Ok(cfg)
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

        c.positive(self.motor.kv_rpm_per_v, "motor.kv_rpm_per_v");
        c.positive(self.motor.resistance_ohm, "motor.resistance_ohm");
        c.non_negative(self.motor.no_load_current_a, "motor.no_load_current_a");
        c.positive(self.motor.rotor_inertia_kgm2, "motor.rotor_inertia_kgm2");

        c.positive(self.prop.diameter_m, "prop.diameter_m");
        c.non_negative(self.prop.inertia_kgm2, "prop.inertia_kgm2");
        c.table(&self.prop.ct_table, "prop.ct_table");
        c.table(&self.prop.cp_table, "prop.cp_table");

        c.positive(self.esc.response_tau_s, "esc.response_tau_s");
        c.positive(self.esc.current_limit_a, "esc.current_limit_a");

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
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
        for (p, field) in [
            (r.loss_good, "radio.loss_good"),
            (r.loss_bad, "radio.loss_bad"),
            (r.p_good_to_bad, "radio.p_good_to_bad"),
            (r.p_bad_to_good, "radio.p_bad_to_good"),
        ] {
            c.check((0.0..=1.0).contains(&p), field, format!("must be a probability in [0, 1] (got {p})"));
        }
        c.check(r.rssi_dbm.is_finite() && r.rssi_dbm <= 0.0, "radio.rssi_dbm", format!("must be <= 0 dBm (got {})", r.rssi_dbm));
        c.check(r.snr_db.is_finite(), "radio.snr_db", "must be finite");
        c.check(r.link_stats_interval_packets > 0, "radio.link_stats_interval_packets", "must be > 0");
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
        .last()
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
