//! World description files (TOML): where the pilot stands, the goggles' antennas, the objects on the field and other
//! transmitters. The simulator uses them for the video link; the Godot client draws the objects. Positions are NED
//! metres from home, like the quad file's (up is a negative `d`); sizes are `[north, east, height]`.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ofs_core::mesh::TriMesh;
use serde::Deserialize;

use crate::mesh::{load_glb_with, MAX_MESHES, MAX_MESH_TRIANGLES};
use crate::{Checker, ConfigError, Problem, TerrainSection, MAX_TERRAIN_SLOPE_DEG};

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
    /// What the video receiver makes of the SNR; absent = the defaults (docs/research/video-link.md).
    #[serde(default)]
    pub video: VideoSection,
    /// The weather the quad flies in; absent = the defaults.
    #[serde(default)]
    pub environment: EnvironmentSection,
    #[serde(default)]
    pub objects: Vec<ObjectSection>,
    #[serde(default)]
    pub emitters: Vec<EmitterSection>,
    /// The race course, in order: gate 0 is the start and finish (docs/race-timing.md).
    #[serde(default)]
    pub gates: Vec<GateSection>,
    /// The race's rules; absent = race forever.
    #[serde(default)]
    pub race: RaceSection,
    /// Named courses over the declared gates; absent = one course of all the gates in file order.
    #[serde(default)]
    pub courses: Vec<CourseSection>,
    /// The ground's relief; absent = flat ground (the plane d = 0).
    #[serde(default)]
    pub terrain: Option<TerrainSection>,
    /// Triangle-mesh `.glb` files placed in the world; they collide and block the signal.
    #[serde(default)]
    pub meshes: Vec<MeshSection>,
    /// The RF loss of the materials the meshes name.
    #[serde(default)]
    pub materials: Vec<MaterialSection>,
    #[serde(skip)]
    pub source_path: PathBuf,
}

/// A `.glb` placed in the world: its origin goes to `position_ned_m`, turned by `heading_deg` (clockwise from north).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSection {
    /// Letters, digits, `_` and `-`, unique among the meshes.
    pub name: String,
    /// Relative to the world file's directory, no `..`, ending `.glb`.
    pub file: String,
    #[serde(default)]
    pub position_ned_m: [f64; 3],
    #[serde(default)]
    pub heading_deg: f64,
}

/// The loss, in dB, of a signal crossing a surface of the `.glb` material called `name`; a material not listed loses 0.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSection {
    pub name: String,
    #[serde(default)]
    pub rf_loss_db: f64,
}

/// A loaded `[[meshes]]` entry. `loss_db[m]` is the loss of the mesh's material `m` (0 if `[[materials]]` omits it);
/// `path` is absolute, for the Godot client.
#[derive(Debug, Clone)]
pub struct WorldMeshData {
    pub name: String,
    pub path: PathBuf,
    pub digest: u64,
    pub mesh: Arc<TriMesh>,
    pub loss_db: Vec<f64>,
}

/// The rules of the race the gates make.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceSection {
    /// The race ends when this many laps are completed (absent = no limit).
    #[serde(default)]
    pub laps: Option<u32>,
    /// The course a fresh session starts in (absent = the first `[[courses]]` entry).
    #[serde(default)]
    pub course: Option<String>,
}

/// How a race is won.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaceMode {
    /// Fly the laps (the world's or the course's limit, or forever).
    #[default]
    Laps,
    /// As many laps as fit before `time_limit_s` runs out.
    TimeAttack,
}

/// A named course: some of the declared gates, in order. The first is the start and finish.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseSection {
    /// Letters, digits, `_` and `-`, unique among the courses.
    pub name: String,
    /// Declared gate names, in course order, each at most once.
    pub gates: Vec<String>,
    #[serde(default)]
    pub mode: RaceMode,
    /// A lap-mode course's own lap limit; absent = the world's `[race] laps`.
    #[serde(default)]
    pub laps: Option<u32>,
    /// A time-attack course's clock, from the first pass of gate 0.
    #[serde(default)]
    pub time_limit_s: Option<f64>,
}

/// A course as the race runs it: gate indices into `WorldConfig::gates`, and its rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    /// Empty for the implicit course of a world without `[[courses]]`.
    pub name: String,
    pub gates: Vec<usize>,
    pub mode: RaceMode,
    /// The lap limit that applies (the course's own, else the world's); `None` for time attack and "forever".
    pub laps: Option<u32>,
    pub time_limit_s: Option<f64>,
}

/// The most gates a world may declare.
pub const MAX_GATES: usize = 64;

/// A gate: an opening the quad flies through, in the direction of `heading_deg` (upright unless tilted). A trigger
/// only: it does not collide with the quad and blocks no signal (a frame made of posts is a set of `[[objects]]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSection {
    /// Letters, digits, `_` and `-`, unique among the gates.
    pub name: String,
    /// The middle of the opening.
    pub center_ned_m: [f64; 3],
    /// The direction of travel through the gate: 0 = north, 90 = east, clockwise.
    pub heading_deg: f64,
    /// The opening's size across the direction of travel, and its height.
    pub width_m: f64,
    pub height_m: f64,
    /// Tilt, aerospace Z-Y-X after the heading: a positive pitch lifts the direction of travel (the gate leans back,
    /// its top toward the approaching quad), a positive roll turns the opening clockwise seen from the approach.
    /// Both default to 0, an upright gate.
    #[serde(default)]
    pub pitch_deg: f64,
    #[serde(default)]
    pub roll_deg: f64,
}

impl GateSection {
    /// The gate's axes in NED: the direction of travel, across the opening and the opening's own "down" (the height
    /// axis). An upright gate gives exactly `(cos h, sin h, 0)`, `(-sin h, cos h, 0)` and `(0, 0, 1)`.
    pub fn axes(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let (sh, ch) = self.heading_deg.to_radians().sin_cos();
        let (sp, cp) = self.pitch_deg.to_radians().sin_cos();
        let (sr, cr) = self.roll_deg.to_radians().sin_cos();
        (
            [ch * cp, sh * cp, -sp],
            [ch * sp * sr - sh * cr, sh * sp * sr + ch * cr, cp * sr],
            [ch * sp * cr + sh * sr, sh * sp * cr - ch * sr, cp * cr],
        )
    }
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
    /// Glass-to-glass video latency in ms (camera + VTX + air + goggles), quantized down to whole fields by the
    /// simulator. Default: the 2-field estimate in docs/research/video-link.md.
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u32,
    pub antennas: Vec<AntennaSection>,
}

/// The analog receiver's thresholds, in dB of SNR: how the picture degrades, when sync is unstable, lost and
/// relocked, and how often a marginal field is dropped. The defaults are the estimates in docs/research/video-link.md
/// and must equal the constants in `ofs-video`'s `link.rs` (a test in `ofs-sim` checks it).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VideoSection {
    /// At or above this the picture is clean.
    pub clean_snr_db: f64,
    /// Grain reaches half of full static here, where sparkles start.
    pub sparkle_snr_db: f64,
    /// Sparkles cover the picture at or below this.
    pub sparkle_full_snr_db: f64,
    /// Colour starts to fade below this and is gone at `color_lost_snr_db`.
    pub color_fade_snr_db: f64,
    pub color_lost_snr_db: f64,
    /// Below this sync is unstable; a lost sync relocks only above it.
    pub unstable_snr_db: f64,
    /// Below this for `lost_after_fields` fields in a row, sync is lost.
    pub lost_snr_db: f64,
    pub lost_after_fields: u32,
    /// Fields in a row at or above `unstable_snr_db` that a lost sync needs to relock.
    pub relock_after_fields: u32,
    /// The chance a field is dropped at the lost edge of the marginal band.
    pub field_drop_p_max: f64,
}

impl Default for VideoSection {
    fn default() -> Self {
        Self {
            clean_snr_db: 25.0,
            sparkle_snr_db: 12.0,
            sparkle_full_snr_db: 4.0,
            color_fade_snr_db: 8.0,
            color_lost_snr_db: 5.0,
            unstable_snr_db: 6.0,
            lost_snr_db: 3.0,
            lost_after_fields: 3,
            relock_after_fields: 5,
            field_drop_p_max: 0.5,
        }
    }
}

/// The environment the quad flies in: the ambient air temperature the thermal model starts from and cools to, and the
/// earth's magnetic field the compass reads.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentSection {
    /// Ambient air temperature in °C; allowed -40 to 60.
    pub ambient_c: f64,
    /// Total intensity of the earth's field in µT; allowed above 0 up to 100 (the real field spans about 25 to 65).
    pub magnetic_field_ut: f64,
    /// Angle of the field below the horizontal, degrees (positive = pointing down, as in the northern hemisphere).
    pub magnetic_inclination_deg: f64,
    /// Angle of magnetic north east of true north, degrees (positive = east).
    pub magnetic_declination_deg: f64,
}

impl Default for EnvironmentSection {
    fn default() -> Self {
        use ofs_core::consts::*;
        Self {
            ambient_c: DEFAULT_AMBIENT_C,
            magnetic_field_ut: DEFAULT_MAG_FIELD_UT,
            magnetic_inclination_deg: DEFAULT_MAG_INCLINATION_DEG,
            magnetic_declination_deg: DEFAULT_MAG_DECLINATION_DEG,
        }
    }
}

fn default_noise_floor_dbm() -> f64 {
    -93.0
}

fn default_latency_ms() -> u32 {
    40
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
                latency_ms: default_latency_ms(),
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
            video: VideoSection::default(),
            environment: EnvironmentSection::default(),
            objects: Vec::new(),
            emitters: Vec::new(),
            gates: Vec::new(),
            race: RaceSection::default(),
            courses: Vec::new(),
            terrain: None,
            meshes: Vec::new(),
            materials: Vec::new(),
            source_path: PathBuf::new(),
        }
    }

    /// The courses a race can run: the `[[courses]]` resolved against the gates, or one unnamed course of all the
    /// gates in file order. Empty without gates. Call it on a validated config.
    pub fn resolved_courses(&self) -> Vec<Course> {
        if self.gates.is_empty() {
            return Vec::new();
        }
        if self.courses.is_empty() {
            return vec![Course { name: String::new(), gates: (0..self.gates.len()).collect(), mode: RaceMode::Laps, laps: self.race.laps, time_limit_s: None }];
        }
        self.courses
            .iter()
            .map(|c| Course {
                name: c.name.clone(),
                gates: c.gates.iter().filter_map(|n| self.gates.iter().position(|g| &g.name == n)).collect(),
                mode: c.mode,
                laps: if c.mode == RaceMode::Laps { c.laps.or(self.race.laps) } else { None },
                time_limit_s: c.time_limit_s,
            })
            .collect()
    }

    /// The index in [`resolved_courses`](Self::resolved_courses) a fresh session starts in.
    pub fn default_course(&self) -> usize {
        self.race.course.as_ref().and_then(|n| self.courses.iter().position(|c| &c.name == n)).unwrap_or(0)
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
        c.check(r.latency_ms <= 1000, "receiver.latency_ms", format!("must be at most 1000 ms (got {})", r.latency_ms));
        c.check(!r.antennas.is_empty(), "receiver.antennas", "needs at least one antenna");
        let mut names = HashSet::new();
        for (i, a) in r.antennas.iter().enumerate() {
            let field = |f: &str| format!("receiver.antennas[{i}].{f}");
            check_name(&mut c, &a.name, &field("name"), &mut names);
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(finite(&[a.aim_az_deg, a.aim_el()]), &field("aim_az_deg"), "aims must be finite");
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
        }
        let ambient = self.environment.ambient_c;
        c.check(
            ambient.is_finite() && (-40.0..=60.0).contains(&ambient),
            "environment.ambient_c",
            format!("must be between -40 and 60 degrees C (got {ambient})"),
        );
        let e = &self.environment;
        c.check(
            e.magnetic_field_ut.is_finite() && e.magnetic_field_ut > 0.0 && e.magnetic_field_ut <= 100.0,
            "environment.magnetic_field_ut",
            format!("must be above 0 and at most 100 µT (got {})", e.magnetic_field_ut),
        );
        c.check(
            (-90.0..=90.0).contains(&e.magnetic_inclination_deg),
            "environment.magnetic_inclination_deg",
            format!("must be between -90 and 90 degrees (got {})", e.magnetic_inclination_deg),
        );
        c.check(
            (-180.0..=180.0).contains(&e.magnetic_declination_deg),
            "environment.magnetic_declination_deg",
            format!("must be between -180 and 180 degrees (got {})", e.magnetic_declination_deg),
        );
        let v = &self.video;
        let snrs = [
            ("clean_snr_db", v.clean_snr_db),
            ("sparkle_snr_db", v.sparkle_snr_db),
            ("sparkle_full_snr_db", v.sparkle_full_snr_db),
            ("color_fade_snr_db", v.color_fade_snr_db),
            ("color_lost_snr_db", v.color_lost_snr_db),
            ("unstable_snr_db", v.unstable_snr_db),
            ("lost_snr_db", v.lost_snr_db),
            ("field_drop_p_max", v.field_drop_p_max),
        ];
        for (name, value) in snrs {
            c.check(value.is_finite(), &format!("video.{name}"), format!("must be finite (got {value})"));
        }
        if snrs.iter().all(|(_, value)| value.is_finite()) {
            c.check(v.sparkle_snr_db > 0.0, "video.sparkle_snr_db", "must be > 0 (grain's second ramp runs from here down to 0 dB)");
            c.check(v.clean_snr_db > v.sparkle_snr_db, "video.clean_snr_db", "must be greater than sparkle_snr_db");
            c.check(v.sparkle_snr_db > v.sparkle_full_snr_db, "video.sparkle_snr_db", "must be greater than sparkle_full_snr_db");
            c.check(v.color_fade_snr_db > v.color_lost_snr_db, "video.color_fade_snr_db", "must be greater than color_lost_snr_db");
            c.check(v.unstable_snr_db > v.lost_snr_db, "video.unstable_snr_db", "must be greater than lost_snr_db");
            c.check((0.0..=1.0).contains(&v.field_drop_p_max), "video.field_drop_p_max", format!("must be in [0, 1] (got {})", v.field_drop_p_max));
        }
        c.check(v.lost_after_fields >= 1, "video.lost_after_fields", "must be at least 1");
        c.check(v.relock_after_fields >= 1, "video.relock_after_fields", "must be at least 1");
        let h = &self.handset;
        let handset_at = h.position(p);
        // A given position is the handset's own fault; a derived one inherits the pilot's, whose finite
        // check already fired — report the pilot only.
        if h.position_ned_m.is_some() {
            c.check(handset_at.iter().all(|v| v.is_finite()), "handset.position_ned_m", "values must be finite");
        }
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
        if let Some(t) = &self.terrain {
            self.check_terrain(&mut c, t, handset_at);
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
        if let Some(laps) = self.race.laps {
            c.check(laps >= 1, "race.laps", "must be >= 1 (leave it out for no limit)");
            c.check(!self.gates.is_empty(), "race.laps", "a lap limit needs at least one [[gates]] entry");
        }
        c.check(self.gates.len() <= MAX_GATES, "gates", format!("at most {MAX_GATES} gates (got {})", self.gates.len()));
        let mut names = HashSet::new();
        for (i, g) in self.gates.iter().enumerate() {
            let field = |f: &str| format!("gates[{i}].{f}");
            check_name(&mut c, &g.name, &field("name"), &mut names);
            c.check(finite(&g.center_ned_m), &field("center_ned_m"), "values must be finite");
            c.check(g.heading_deg.is_finite(), &field("heading_deg"), "must be finite");
            c.check(g.width_m.is_finite() && g.width_m > 0.0, &field("width_m"), format!("must be > 0 (got {})", g.width_m));
            c.check(g.height_m.is_finite() && g.height_m > 0.0, &field("height_m"), format!("must be > 0 (got {})", g.height_m));
            c.check(g.pitch_deg.is_finite() && g.pitch_deg.abs() < 90.0, &field("pitch_deg"), format!("must be in (-90, 90) (got {})", g.pitch_deg));
            c.check(g.roll_deg.is_finite() && g.roll_deg.abs() <= 180.0, &field("roll_deg"), format!("must be in [-180, 180] (got {})", g.roll_deg));
            // A NaN is reported by the finite checks above, not here. The lowest corner of a tilted opening reaches
            // `|across_d|·width/2 + |down_d|·height/2` below its centre.
            let (_, across, down) = g.axes();
            let bottom_d = g.center_ned_m[2] + across[2].abs() * g.width_m / 2.0 + down[2].abs() * g.height_m / 2.0;
            c.check(
                bottom_d.is_nan() || bottom_d <= 1e-9, // a quarter-turn roll leaves a 1e-16 of rounding
                &field("center_ned_m"),
                "the gate's opening must not reach below the ground (center d + its lowest corner's drop must be <= 0)",
            );
        }
        if !self.courses.is_empty() {
            c.check(!self.gates.is_empty(), "courses", "a course needs [[gates]] to pick from");
        }
        let mut names = HashSet::new();
        for (i, course) in self.courses.iter().enumerate() {
            let field = |f: &str| format!("courses[{i}].{f}");
            check_name(&mut c, &course.name, &field("name"), &mut names);
            c.check(!course.gates.is_empty(), &field("gates"), "a course needs at least one gate");
            let mut picked = HashSet::new();
            for (j, gate) in course.gates.iter().enumerate() {
                c.check(self.gates.iter().any(|g| &g.name == gate), &format!("courses[{i}].gates[{j}]"), format!("no gate named {gate:?}"));
                c.check(picked.insert(gate.as_str()), &format!("courses[{i}].gates[{j}]"), format!("{gate:?} is already in this course"));
            }
            match course.mode {
                RaceMode::Laps => {
                    c.check(course.laps.is_none_or(|l| l >= 1), &field("laps"), "must be >= 1 (leave it out for the world's limit)");
                    c.check(course.time_limit_s.is_none(), &field("time_limit_s"), "only a time_attack course has a time limit");
                }
                RaceMode::TimeAttack => {
                    c.check(course.time_limit_s.is_some_and(|t| t.is_finite() && t > 0.0), &field("time_limit_s"), "a time_attack course needs time_limit_s > 0");
                    c.check(course.laps.is_none(), &field("laps"), "a time_attack course ends on the clock, not on laps");
                }
            }
        }
        if let Some(name) = &self.race.course {
            c.check(self.courses.iter().any(|x| &x.name == name), "race.course", format!("no course named {name:?}"));
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
        c.check(self.meshes.len() <= MAX_MESHES, "meshes", format!("at most {MAX_MESHES} meshes (got {})", self.meshes.len()));
        let mut names = HashSet::new();
        for (i, m) in self.meshes.iter().enumerate() {
            let field = |f: &str| format!("meshes[{i}].{f}");
            check_name(&mut c, &m.name, &field("name"), &mut names);
            c.check(!m.file.starts_with(['/', '\\']) && !m.file.contains(':'), &field("file"), format!("must be a relative path (got {:?})", m.file));
            c.check(!m.file.split(['/', '\\']).any(|s| s == ".."), &field("file"), format!("must not contain '..' (got {:?})", m.file));
            c.check(m.file.ends_with(".glb"), &field("file"), format!("must end in .glb (got {:?})", m.file));
            c.check(finite(&m.position_ned_m), &field("position_ned_m"), "values must be finite");
            c.check(m.heading_deg.is_finite(), &field("heading_deg"), "must be finite");
        }
        let mut names = HashSet::new();
        for (i, m) in self.materials.iter().enumerate() {
            // glTF material names are free-form (`Material.001`, `Wall Concrete`): only non-empty and unique.
            let field = format!("materials[{i}].name");
            c.check(!m.name.is_empty(), &field, "must not be empty");
            c.check(names.insert(m.name.clone()), &field, format!("{:?} is used twice", m.name));
            c.non_negative(m.rf_loss_db, &format!("materials[{i}].rf_loss_db"));
        }
        c.0
    }

    /// [`load_meshes_with`](Self::load_meshes_with) at the default triangle cap.
    pub fn load_meshes(&self) -> Result<Vec<WorldMeshData>, String> {
        self.load_meshes_with(MAX_MESH_TRIANGLES)
    }

    /// Reads every `[[meshes]]` file (resolved against the world file's directory) into NED, at most
    /// `max_total_triangles` in all. A world not loaded from disk has no meshes to load. Call it on a validated config.
    pub fn load_meshes_with(&self, max_total_triangles: usize) -> Result<Vec<WorldMeshData>, String> {
        self.load_meshes_checked(max_total_triangles).map_err(|p| format!("{}: {}", p.field, p.message))
    }

    fn load_meshes_checked(&self, max_total_triangles: usize) -> Result<Vec<WorldMeshData>, Problem> {
        if self.source_path.as_os_str().is_empty() {
            return Ok(Vec::new());
        }
        let dir = self.source_path.parent().unwrap_or(Path::new(""));
        let (mut out, mut used) = (Vec::new(), 0usize);
        let mut loaded_materials: Vec<Vec<String>> = Vec::new();
        for (i, m) in self.meshes.iter().enumerate() {
            let problem = |message: String| Problem { field: format!("meshes[{i}].file"), message };
            let joined = dir.join(&m.file);
            let path = joined.canonicalize().map(strip_verbatim).map_err(|e| problem(format!("{}: cannot read: {e}", joined.display())))?;
            let g = load_glb_with(&path, m.position_ned_m, m.heading_deg, max_total_triangles - used).map_err(|e| {
                let note = if used > 0 { format!(" (all meshes together: at most {max_total_triangles} triangles)") } else { String::new() };
                problem(format!("{e}{note}"))
            })?;
            used += g.mesh.len();
            loaded_materials.push(g.material_names);
            out.push(WorldMeshData { name: m.name.clone(), path, digest: g.digest, mesh: Arc::new(g.mesh), loss_db: Vec::new() });
        }
        // A listed material no mesh uses is a typo that would silently lose nothing.
        for (i, mat) in self.materials.iter().enumerate() {
            if !loaded_materials.iter().flatten().any(|n| *n == mat.name) {
                return Err(Problem { field: format!("materials[{i}].name"), message: format!("no loaded mesh has a material called {:?}", mat.name) });
            }
        }
        for (data, names) in out.iter_mut().zip(&loaded_materials) {
            data.loss_db = names.iter().map(|n| self.materials.iter().find(|m| &m.name == n).map_or(0.0, |m| m.rf_loss_db)).collect();
        }
        Ok(out)
    }
}

/// Windows' `canonicalize` returns `\\?\C:\...`, which the Godot client cannot open.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

impl WorldConfig {
    /// The terrain's rules. The grid is baked (for the slope cap and the places) only when the numbers passed.
    fn check_terrain(&self, c: &mut Checker, t: &TerrainSection, handset_at: [f64; 3]) {
        let before = c.0.len();
        t.check(c);
        if c.0.len() != before {
            return;
        }
        let ground = t.build();
        let slope = ground.max_slope();
        c.check(
            within_slope_cap(slope),
            "terrain.amplitude_m",
            format!("the steepest slope is {:.1} degrees, over the {MAX_TERRAIN_SLOPE_DEG} degree limit: lower amplitude_m or raise wavelength_m", slope.atan().to_degrees()),
        );
        // A d above 0 is already reported as below the flat ground; a non-finite place is reported as such.
        let below = |at: [f64; 3]| at.iter().all(|v| v.is_finite()) && at[2] <= 0.0 && at[2] > ground.ground_d(at[0], at[1]);
        let pilot_at = self.pilot.position_ned_m;
        let pilot_below = below(pilot_at);
        c.check(!pilot_below, "pilot.position_ned_m", format!("the pilot must not be below the terrain (d = {}, the ground is at d = {})", pilot_at[2], ground.ground_d(pilot_at[0], pilot_at[1])));
        // A derived handset inherits the pilot's fault: report the pilot only.
        if self.handset.position_ned_m.is_some() || !pilot_below {
            c.check(
                !below(handset_at),
                "handset.position_ned_m",
                format!("the handset must not be below the terrain (d = {}, the ground is at d = {})", handset_at[2], ground.ground_d(handset_at[0], handset_at[1])),
            );
        }
    }
}

/// The slope cap decides without libm: tan²60° = 3 exactly, and an IEEE multiply is correctly rounded, so
/// `slope * slope <= 3.0` is the same bits on every OS. `atan` — which may differ by an ulp between OSes — is
/// display only, for the message's degrees.
fn within_slope_cap(slope: f64) -> bool {
    slope * slope <= 3.0
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

/// The file a session's world names. A name (`flat`: no path separator, no `.toml`) is `worlds/<name>.toml` beside
/// the quad's directory (`<quad dir>/../worlds`, as parts resolve against `library/`); anything else is a path.
pub fn resolve(world: &str, quad_path: &Path) -> PathBuf {
    if world.contains(['/', '\\']) || world.ends_with(".toml") {
        return PathBuf::from(world);
    }
    let quads = quad_path.parent().unwrap_or(Path::new("."));
    quads.parent().unwrap_or(Path::new(".")).join("worlds").join(format!("{world}.toml"))
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
    world.load_meshes_checked(MAX_MESH_TRIANGLES).map_err(|p| ConfigError::Invalid { path: path.to_path_buf(), problems: vec![p] })?;
    Ok(world)
}

#[cfg(test)]
mod slope_cap_tests {
    use super::within_slope_cap;

    /// The cap decides at the f64 neighbours of tan 60 degrees = sqrt(3), the closest any world can get to the limit:
    /// the last double below is inside, the rounded sqrt(3) itself squares (rounded) to under 3, the first above is out.
    #[test]
    fn the_cap_decides_exactly_at_the_ulp_neighbours_of_tan_60() {
        let (below, at, above) = (
            f64::from_bits(3.0f64.sqrt().to_bits() - 1),
            3.0f64.sqrt(),
            f64::from_bits(3.0f64.sqrt().to_bits() + 1),
        );
        assert!(within_slope_cap(below), "the last f64 below tan 60 degrees is inside the cap");
        assert!(within_slope_cap(at), "the rounded sqrt(3) squares to under 3, so tan 60 itself is inside");
        assert!(!within_slope_cap(above), "the first f64 above tan 60 degrees is outside the cap");
    }
}
