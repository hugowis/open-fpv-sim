//! World description files (TOML): where the pilot stands, the goggles' antennas, the objects on the field and other
//! transmitters. The simulator uses them for the video link; the Godot client draws the objects. Positions are NED
//! metres from home, like the quad file's (up is a negative `d`); sizes are `[north, east, height]`.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Checker, ConfigError, Problem};

pub const WORLD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AntennaKind {
    #[default]
    Omni,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarization {
    Rhcp,
    Lhcp,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Axis-aligned box: `size_m`.
    Box,
    /// Vertical cylinder: `radius_m` and `height_m`.
    Cylinder,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldConfig {
    pub schema_version: u32,
    pub name: String,
    pub pilot: PilotSection,
    pub receiver: ReceiverSection,
    /// The pilot's handset; the ELRS uplink transmits from here.
    #[serde(default)]
    pub handset: HandsetSection,
    #[serde(default)]
    pub objects: Vec<ObjectSection>,
    #[serde(default)]
    pub emitters: Vec<EmitterSection>,
    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotSection {
    /// Where the goggles are (head height included).
    pub position_ned_m: [f64; 3],
    /// The heading the pilot looks along (0 = north, 90 = east); the antenna aims are relative to it.
    #[serde(default)]
    pub facing_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverSection {
    #[serde(default = "default_noise_floor_dbm")]
    pub noise_floor_dbm: f64,
    /// Use the antenna with the best signal (otherwise the first one).
    #[serde(default = "default_true")]
    pub diversity: bool,
    pub antennas: Vec<AntennaSection>,
}

fn default_noise_floor_dbm() -> f64 {
    -93.0
}

fn default_true() -> bool {
    true
}

/// The pilot's handset: where it is and what transmits from it. Optional: the default sits 0.5 m below the
/// goggles with one vertical 2 dBi linear dipole.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsetSection {
    /// Default: the pilot's position, 0.5 m lower.
    #[serde(default)]
    pub position_ned_m: Option<[f64; 3]>,
    /// One or two, like the receiver's.
    #[serde(default)]
    pub antennas: Vec<AntennaSection>,
}

impl Default for HandsetSection {
    fn default() -> Self {
        Self {
            position_ned_m: None,
            antennas: vec![AntennaSection {
                name: "handset".into(),
                kind: AntennaKind::Omni,
                gain_dbi: 2.0,
                beamwidth_deg: None,
                polarization: Polarization::Linear,
                aim_az_deg: 0.0,
                aim_el_deg: None,
            }],
        }
    }
}

impl HandsetSection {
    /// Where the handset is: its position, or 0.5 m below the pilot's goggles.
    pub fn position(&self, pilot: &PilotSection) -> [f64; 3] {
        self.position_ned_m
            .unwrap_or([pilot.position_ned_m[0], pilot.position_ned_m[1], pilot.position_ned_m[2] + 0.5])
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntennaSection {
    /// Unique; it names the antenna's RSSI signal and its entry in the state stream.
    pub name: String,
    pub kind: AntennaKind,
    pub gain_dbi: f64,
    /// Patch only: the width of its main lobe at -3 dB.
    #[serde(default)]
    pub beamwidth_deg: Option<f64>,
    pub polarization: Polarization,
    /// Where the patch points (or the omni's axis leans), relative to the pilot's facing: azimuth clockwise,
    /// elevation up. Default: a patch straight ahead and level, an omni upright.
    #[serde(default)]
    pub aim_az_deg: f64,
    #[serde(default)]
    pub aim_el_deg: Option<f64>,
}

impl AntennaSection {
    pub fn aim_el(&self) -> f64 {
        self.aim_el_deg.unwrap_or(match self.kind {
            AntennaKind::Omni => 90.0,
            AntennaKind::Patch => 0.0,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectSection {
    pub name: String,
    pub shape: Shape,
    pub center_ned_m: [f64; 3],
    /// Box: `[north, east, height]`.
    #[serde(default)]
    pub size_m: Option<[f64; 3]>,
    /// Cylinder.
    #[serde(default)]
    pub radius_m: Option<f64>,
    #[serde(default)]
    pub height_m: Option<f64>,
    /// `[r, g, b]`, each 0 to 1.
    pub color: [f64; 3],
    /// The loss when the object fully blocks the path; 0 (the default) lets the signal through.
    #[serde(default)]
    pub rf_loss_db: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmitterSection {
    pub name: String,
    pub position_ned_m: [f64; 3],
    /// Either `freq_mhz`, or `band` (A, B, E, F, R, L) and `channel` (1..=8).
    #[serde(default)]
    pub freq_mhz: Option<f64>,
    #[serde(default)]
    pub band: Option<String>,
    #[serde(default)]
    pub channel: Option<u8>,
    pub power_mw: f64,
    /// Its omni antenna, upright.
    #[serde(default = "default_emitter_gain_dbi")]
    pub gain_dbi: f64,
    #[serde(default = "default_rhcp")]
    pub polarization: Polarization,
}

fn default_emitter_gain_dbi() -> f64 {
    2.0
}

fn default_rhcp() -> Polarization {
    Polarization::Rhcp
}

/// Emitter frequencies the simulator accepts: the 5.8 GHz bands, Lowband included.
pub const EMITTER_FREQ_RANGE_MHZ: std::ops::RangeInclusive<f64> = 5300.0..=6000.0;

impl WorldConfig {
    /// The world used when a session names none: the pilot 1.7 m up at home with one upright 2 dBi RHCP omni, no
    /// objects and no other transmitters.
    pub fn open_field() -> WorldConfig {
        WorldConfig {
            schema_version: WORLD_SCHEMA_VERSION,
            name: "open field".into(),
            pilot: PilotSection { position_ned_m: [0.0, 0.0, -1.7], facing_deg: 0.0 },
            receiver: ReceiverSection {
                noise_floor_dbm: default_noise_floor_dbm(),
                diversity: true,
                antennas: vec![AntennaSection {
                    name: "omni".into(),
                    kind: AntennaKind::Omni,
                    gain_dbi: 2.0,
                    beamwidth_deg: None,
                    polarization: Polarization::Rhcp,
                    aim_az_deg: 0.0,
                    aim_el_deg: None,
                }],
            },
            handset: HandsetSection::default(),
            objects: Vec::new(),
            emitters: Vec::new(),
            source_path: PathBuf::new(),
        }
    }

    pub fn validate(&self) -> Vec<Problem> {
        let mut c = Checker(Vec::new());
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        let p = &self.pilot;
        c.check(finite(&p.position_ned_m), "pilot.position_ned_m", "values must be finite");
        c.check(p.position_ned_m[2].is_nan() || p.position_ned_m[2] <= 0.0, "pilot.position_ned_m", format!("the pilot must not be below the ground (d = {} > 0)", p.position_ned_m[2]));
        c.check(p.facing_deg.is_finite(), "pilot.facing_deg", "must be finite");
        let r = &self.receiver;
        c.check(r.noise_floor_dbm.is_finite(), "receiver.noise_floor_dbm", "must be finite");
        c.check(!r.antennas.is_empty(), "receiver.antennas", "needs at least one antenna");
        let mut names = HashSet::new();
        for (i, a) in r.antennas.iter().enumerate() {
            let field = |f: &str| format!("receiver.antennas[{i}].{f}");
            check_name(&mut c, &a.name, &field("name"), &mut names);
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(finite(&[a.aim_az_deg, a.aim_el()]), &field("aim_az_deg"), "aims must be finite");
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
        }
        let h = &self.handset;
        let handset_at = h.position(&p);
        c.check(handset_at.iter().all(|v| v.is_finite()), "handset.position_ned_m", "values must be finite");
        c.check(
            handset_at[2].is_nan() || handset_at[2] <= 0.0,
            "handset.position_ned_m",
            format!("the handset must not be below the ground (d = {} > 0)", handset_at[2]),
        );
        let mut handset_names = HashSet::new();
        c.check(!h.antennas.is_empty(), "handset.antennas", "needs at least one antenna");
        for (i, a) in h.antennas.iter().enumerate() {
            let field = |f: &str| format!("handset.antennas[{i}].{f}");
            check_name(&mut c, &a.name, &field("name"), &mut handset_names);
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(finite(&[a.aim_az_deg, a.aim_el()]), &field("aim_az_deg"), "aims must be finite");
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
        }
        let mut names = HashSet::new();
        for (i, o) in self.objects.iter().enumerate() {
            let field = |f: &str| format!("objects[{i}].{f}");
            check_name(&mut c, &o.name, &field("name"), &mut names);
            c.check(finite(&o.center_ned_m), &field("center_ned_m"), "values must be finite");
            c.check(o.color.iter().all(|v| (0.0..=1.0).contains(v)), &field("color"), "each value must be in [0, 1]");
            c.non_negative(o.rf_loss_db, &field("rf_loss_db"));
            match o.shape {
                Shape::Box => {
                    c.check(o.size_m.is_some_and(|s| s.iter().all(|v| v.is_finite() && *v > 0.0)), &field("size_m"), "a box needs size_m, every value > 0");
                    c.check(o.radius_m.is_none() && o.height_m.is_none(), &field("shape"), "radius_m and height_m are for a cylinder");
                }
                Shape::Cylinder => {
                    c.check(o.radius_m.is_some_and(|v| v.is_finite() && v > 0.0), &field("radius_m"), "a cylinder needs radius_m > 0");
                    c.check(o.height_m.is_some_and(|v| v.is_finite() && v > 0.0), &field("height_m"), "a cylinder needs height_m > 0");
                    c.check(o.size_m.is_none(), &field("shape"), "size_m is for a box");
                }
            }
        }
        let mut names = HashSet::new();
        for (i, e) in self.emitters.iter().enumerate() {
            let field = |f: &str| format!("emitters[{i}].{f}");
            check_name(&mut c, &e.name, &field("name"), &mut names);
            c.check(finite(&e.position_ned_m), &field("position_ned_m"), "values must be finite");
            c.positive(e.power_mw, &field("power_mw"));
            c.check(e.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            match (e.freq_mhz, &e.band, e.channel) {
                (Some(f), None, None) => c.check(
                    EMITTER_FREQ_RANGE_MHZ.contains(&f),
                    &field("freq_mhz"),
                    format!("must be in {}..={} MHz (got {f})", EMITTER_FREQ_RANGE_MHZ.start(), EMITTER_FREQ_RANGE_MHZ.end()),
                ),
                (None, Some(band), Some(channel)) => {
                    c.check(
                        matches!(band.as_str(), "A" | "B" | "E" | "F" | "R" | "L"),
                        &field("band"),
                        format!("must be one of A, B, E, F, R, L (got {band})"),
                    );
                    c.check((1..=8).contains(&channel), &field("channel"), format!("must be 1..=8 (got {channel})"));
                }
                _ => c.check(false, &field("freq_mhz"), "give either freq_mhz, or band and channel"),
            }
        }
        c.0
    }
}

/// Names become signal names and dictionary keys: letters, digits, `_` and `-`, unique within their list.
fn check_name(c: &mut Checker, name: &str, field: &str, seen: &mut HashSet<String>) {
    c.check(
        !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
        field,
        format!("must be letters, digits, '_' or '-' (got {name:?})"),
    );
    c.check(seen.insert(name.to_string()), field, format!("{name:?} is used twice"));
}

/// A patch needs its beamwidth (0 to 180 degrees); an omni has none.
pub(crate) fn check_beamwidth(c: &mut Checker, kind: AntennaKind, beamwidth_deg: Option<f64>, field: &str) {
    match (kind, beamwidth_deg) {
        (AntennaKind::Patch, Some(b)) => c.check(b.is_finite() && b > 0.0 && b < 180.0, field, format!("must be in (0, 180) degrees (got {b})")),
        (AntennaKind::Patch, None) => c.check(false, field, "a patch needs beamwidth_deg"),
        (AntennaKind::Omni, Some(_)) => c.check(false, field, "an omni has no beamwidth_deg"),
        (AntennaKind::Omni, None) => {}
    }
}

pub fn load(path: &Path) -> Result<WorldConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
    let parse_err = |message: String| ConfigError::Parse { path: path.to_path_buf(), message };
    let raw: toml::Value = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    match raw.get("schema_version").and_then(toml::Value::as_integer) {
        Some(v) if v == i64::from(WORLD_SCHEMA_VERSION) => {}
        Some(v) => return Err(parse_err(format!("unsupported world schema_version {v} (this build reads {WORLD_SCHEMA_VERSION})"))),
        None => return Err(parse_err("missing integer schema_version".into())),
    }
    let mut world: WorldConfig = toml::from_str(&text).map_err(|e| parse_err(e.to_string()))?;
    world.source_path = path.to_path_buf();
    let problems = world.validate();
    if !problems.is_empty() {
        return Err(ConfigError::Invalid { path: path.to_path_buf(), problems });
    }
    Ok(world)
}
