//! Component-library part files (TOML): one file per part, every parameter's provenance checked.
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{
    BaroSection, CollisionSection, ConfigError, EscSection, FrameSection, GroundSection, ImuSection, MotorSection,
    Problem, PropSection, VtxAntennaSection, VtxKind,
};

/// 1: first shape (M4a). Part files version independently of quad files.
pub const PART_SCHEMA_VERSION: u32 = 1;
pub const MIN_PART_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceSource {
    Datasheet,
    Measured,
    #[default]
    Estimated,
}

impl ProvenanceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Datasheet => "datasheet",
            Self::Measured => "measured",
            Self::Estimated => "estimated",
        }
    }
}

/// One library part a quad loaded, with the provenance of each of its parameters (keyed by dotted path).
#[derive(Debug, Clone)]
pub struct PartProvenance {
    /// The library directory's part kind: `frame`, `motor`, `prop`, `esc`, `battery`, `imu`, `baro` or `vtx`.
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub parameters: BTreeMap<String, Provenance>,
}

/// Where one parameter's value came from. `link` (not `ref` — a Rust keyword) is a URL or doc id.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: ProvenanceSource,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FramePart {
    pub schema_version: u32,
    pub name: String,
    pub frame: FrameSection,
    pub ground: GroundSection,
    #[serde(default)]
    pub collision: Option<CollisionSection>,
    pub provenance: BTreeMap<String, Provenance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorPart {
    pub schema_version: u32,
    pub name: String,
    pub motor: MotorSection,
    pub provenance: BTreeMap<String, Provenance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropPart {
    pub schema_version: u32,
    pub name: String,
    pub prop: PropSection,
    pub provenance: BTreeMap<String, Provenance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscPart {
    pub schema_version: u32,
    pub name: String,
    pub esc: EscSection,
    pub provenance: BTreeMap<String, Provenance>,
}

/// Battery hardware only — `initial_soc` and `rate_hz` are the quad's wiring.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatteryHardware {
    pub cells: u32,
    pub capacity_mah: f64,
    pub r0_ohm: f64,
    pub r1_ohm: f64,
    pub c1_f: f64,
    pub ocv_table: Vec<[f64; 2]>,
    /// Per-cell multipliers of each cell's share of `r0_ohm` and `capacity_mah` (a tired cell, a mismatched pack);
    /// empty = every cell 1.0, otherwise one entry per cell.
    #[serde(default)]
    pub cell_r0_scale: Vec<f64>,
    #[serde(default)]
    pub cell_capacity_scale: Vec<f64>,
    /// Ageing: `1 − capacity_fade_per_cycle·cycles` of the capacity is left and `R0` has grown by
    /// `resistance_growth_per_cycle·cycles` (both default 0).
    #[serde(default)]
    pub cycles: u32,
    #[serde(default)]
    pub capacity_fade_per_cycle: f64,
    #[serde(default)]
    pub resistance_growth_per_cycle: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatteryPart {
    pub schema_version: u32,
    pub name: String,
    pub battery: BatteryHardware,
    pub provenance: BTreeMap<String, Provenance>,
}

/// VTX hardware only — `uart`, band, channel and power index are the quad's wiring.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VtxHardware {
    pub kind: VtxKind,
    pub power_levels_mw: Vec<u32>,
    pub power_levels_dbm: Vec<u8>,
    #[serde(default = "crate::default_vtx_reply_latency_ms")]
    pub reply_latency_ms: f64,
    #[serde(default)]
    pub antenna: VtxAntennaSection,
    #[serde(default = "crate::default_pit_power_mw")]
    pub pit_power_mw: f64,
    #[serde(default)]
    pub thermal: Option<crate::VtxThermalSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VtxPart {
    pub schema_version: u32,
    pub name: String,
    pub vtx: VtxHardware,
    pub provenance: BTreeMap<String, Provenance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImuPart {
    pub schema_version: u32,
    pub name: String,
    pub imu: ImuSection,
    pub provenance: BTreeMap<String, Provenance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaroPart {
    pub schema_version: u32,
    pub name: String,
    pub baro: BaroSection,
    pub provenance: BTreeMap<String, Provenance>,
}

fn check_part_schema(raw: &toml::Value) -> Result<(), String> {
    match raw.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v >= i64::from(MIN_PART_SCHEMA_VERSION) && v <= i64::from(PART_SCHEMA_VERSION) => Ok(()),
        Some(v) => Err(format!("unsupported schema_version {v} (this build reads part schema {MIN_PART_SCHEMA_VERSION} to {PART_SCHEMA_VERSION})")),
        None => Err("missing integer schema_version".into()),
    }
}

/// Every leaf path under the part's data tables. Tables recurse; arrays are single leaves
/// (`prop.ct_table` is one parameter).
fn leaf_paths(value: &toml::Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                leaf_paths(v, &path, out);
            }
        }
        _ => out.push(prefix.to_string()),
    }
}

/// Every present data parameter carries provenance, and no provenance key is invented.
pub fn check_provenance(raw: &toml::Value, provenance: &BTreeMap<String, Provenance>) -> Vec<Problem> {
    let mut required = Vec::new();
    if let Some(t) = raw.as_table() {
        for (k, v) in t {
            if k == "schema_version" || k == "name" || k == "provenance" {
                continue;
            }
            leaf_paths(v, k, &mut required);
        }
    }
    let mut problems = Vec::new();
    for path in &required {
        if !provenance.contains_key(path) {
            problems.push(Problem { field: path.clone(), message: "missing provenance (datasheet | measured | estimated)".into() });
        }
    }
    for path in provenance.keys() {
        if !required.iter().any(|r| r == path) {
            problems.push(Problem { field: path.clone(), message: "unknown path (no such parameter in this part)".into() });
        }
    }
    problems
}

fn parse_part<T: serde::de::DeserializeOwned>(path: &Path, text: &str) -> Result<(T, toml::Value), ConfigError> {
    let raw: toml::Value = toml::from_str(text)
        .map_err(|e| ConfigError::Parse { path: path.to_path_buf(), message: e.to_string() })?;
    if let Err(message) = check_part_schema(&raw) {
        return Err(ConfigError::Parse { path: path.to_path_buf(), message });
    }
    let part = toml::from_str(text).map_err(|e| ConfigError::Parse { path: path.to_path_buf(), message: e.to_string() })?;
    Ok((part, raw))
}

macro_rules! part_loader {
    ($(#[$doc:meta])* $fn_name:ident, $part:ty) => {
        $(#[$doc])*
        pub fn $fn_name(path: &Path) -> Result<$part, ConfigError> {
            let text = std::fs::read_to_string(path)
                .map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
            let (part, raw) = parse_part::<$part>(path, &text)?;
            let problems = check_provenance(&raw, &part.provenance);
            if problems.is_empty() {
                Ok(part)
            } else {
                Err(ConfigError::Invalid { path: path.to_path_buf(), problems })
            }
        }
    };
}

part_loader!(/// Loads a frame part (`[frame]`, `[ground]`, optional `[collision]`).
             load_frame, FramePart);
part_loader!(load_motor, MotorPart);
part_loader!(load_prop, PropPart);
part_loader!(load_esc, EscPart);
part_loader!(load_battery, BatteryPart);
part_loader!(load_vtx, VtxPart);
part_loader!(load_imu, ImuPart);
part_loader!(load_baro, BaroPart);
