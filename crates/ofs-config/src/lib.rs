//! Quad description files (TOML): parsing and validation that reports every problem at once.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 2: adds the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 2;

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
        Some(v) if v == i64::from(SCHEMA_VERSION) => {}
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {SCHEMA_VERSION})"))),
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
        c.check(
            (2..=8).contains(&r.uart),
            "radio.uart",
            format!("must be 2..=8; UART1 is Betaflight's MSP port, tcp:5761 (got {})", r.uart),
        );
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
        if self.fc.kind == FcKind::Sitl {
            c.check(!self.fc.launch.is_empty(), "fc.launch", "must not be empty for kind = \"sitl\"");
            let diff = self.resolve(&self.fc.betaflight_diff);
            c.check(diff.is_file(), "fc.betaflight_diff", format!("file not found: {}", diff.display()));
        }
        c.0
    }
}
