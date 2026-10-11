# M4a: component library — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Quads assemble from `library/` part files — one TOML per part, every parameter's provenance recorded — and motor pole pairs become data (closing the M3d `POLE_PAIRS`-constant debt).

**Architecture:** `ofs-config` gains a `part` module (six part kinds, a generic provenance-completeness walker); quad files become part references plus their own wiring (`schema_version = 5`, hard cutover), resolved at load time into the same section structs the vehicle consumes — motors/props/escs become per-motor vectors. Both shipped quads migrate value-preserving, so no physics number, digest or pinned test expectation changes.

**Tech Stack:** Rust workspace (`ofs-config`, `ofs-physics`, `ofs-fc`, `ofs-sim`). No protocol change (the spec's M4a row); the Python client and Godot are untouched (quad paths are unchanged). Betaflight SITL stays pinned at 2026.6.2 — `scripts/build-sitl.sh` and `third_party/betaflight/` are not touched.

**Spec:** `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md` §2 (component library), §1 decisions 1 and 2 (hard cutover, mandatory provenance), §8's M4a row — the plan argues from the spec; executors read both.

## Global Constraints

- **No wire changes.** `PROTOCOL_VERSION` stays 7 (M3e). No Python or GDScript file is edited this milestone.
- **Value-preserving migration.** Every resolved section value of both shipped quads equals today's inline values bit-for-bit. Consequences, all pinned by existing tests that must pass *unchanged*: the vehicle tests that pin VtxInfo / UART wiring / picture behaviour; the lockstep digest tests (no new signal, no new RNG draw, no arithmetic reorder); the live SITL hover test.
- **Rust style.** Never run bare `rustfmt` (it churns against the repo's compact house style); hand-indent new code. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` after every task that lands Rust — CI's first gate is clippy and local `cargo test` never runs it.
- **Determinism.** No new randomness, no new bus signals, no changed arithmetic order anywhere in the tick. The digest of a given quad + seed is identical before and after this milestone.
- **Live SITL tests** run inside WSL via `uv run --with grpcio,protobuf,pytest` + a Linux-built `ofs-sim`, `PYTHONPATH=python` (docs/dev-setup.md option 2). Run `python/tests/test_sitl_hover.py` after Tasks 3 and 4; the whole sitl suite after Task 5.
- **Commits:** one per task, conventional style (`feat(config): ...`), after the task's tests pass. Task 3 is an atomic cutover — its steps do not compile independently and land as one commit.
- **Part data license:** `library/README.md` states the part data files are CC-BY-SA-4.0 (parent spec §10) — Task 5.

## Rulings and facts verified in advance

The spec pins behaviour; these rulings pin names, seams and shapes, all verified against the merged M3e tree (`99443f6`, protocol 7, `quads/opendrone-5f-tramp.toml` with `[vtx] uart = 6`, `power_levels_mw = [25, 100, 200, 400, 600]`, `default_power_index = 2`) while the plan was written. They are the contract where the spec is silent.

1. **The frame part carries `[frame]`, `[ground]` and the optional `[collision]`** (spec §2.2 "motor positions, spin, contact, mass, drag"); `[imu]` and `[baro]` stay quad-level (spec §2.1 ships exactly six part kinds; sensors become parts when they have hardware identity — carried debt).
2. **References with wiring are sections; the rest are bare ids.** `frame = "…"`, `motors = ["…"]`, `props = ["…"]`, `escs = ["…"]`; `[battery] part = "…" + initial_soc + rate_hz`; `[vtx] part = "…" + uart + default_band + default_channel + default_power_index`. This refines the spec §2.3 example, which showed `battery`/`vtx` as bare strings but left their wiring homeless. Each of `motors`/`props`/`escs` independently broadcasts: length 1 or the frame's motor count (4, validated since M1).
3. **`rotor_inertia_kgm2` in `AirframeParams` becomes `Vec<f64>`** — `rigid_body.rs:204` already indexes it per motor (`self.p.rotor_inertia_kgm2 * bus.get(self.omega_dot[k])`); a mixed build deserves per-motor truth, a uniform build gets the identical number in every slot.
4. **SITL quads require uniform `pole_pairs`.** The KISS ESC-sensor frames carry one pack eRPM (`esc_telemetry.rs`: `mean_rpm * POLE_PAIRS`) and Betaflight has one `motor_poles`, so mixed pairs cannot be represented honestly — a config error says so. The eRPM datagram block (`codec.rs:erpm_block_payload`) is per-motor and takes per-motor pole pairs; with the shipped uniform parts its outputs are byte-identical to today's.
5. **Betaflight's `motor_poles` default is 14** (the codebase's own record in `codec.rs`'s `erpm_block_payload` doc). The diff check: when the diff sets `motor_poles` it must equal `2 × pole_pairs`; when it does not, `2 × pole_pairs` must equal 14. SITL quads only — the existing diff checks' gate.
6. **Part ids are file stems** (`library/frames/opendrone-5f.toml` → `opendrone-5f`); kind directories are `frames/ motors/ props/ escs/ batteries/ vtx/` (the parent spec §6.1's layout, `vtx` singular). Parts do not reference parts.
7. **The library root is `<quad dir>/../library`, overridable by `OFS_LIBRARY`** (one path). Resolution is a pure function `library_root(env: Option<&str>, quad_dir: &Path) -> PathBuf` so tests never touch the process environment (parallel `std::env::set_var` is a race); `load()` reads the env var and passes it in.
8. **Unknown part ids are collected on the quad's error** (fields `frame`, `motors[i]`, `props[i]`, `escs[i]`, `battery.part`, `vtx.part`), each message listing the searched directory and the ids it holds; a *missing kind directory* is its own message naming the library root and `OFS_LIBRARY`. Part *content* errors (parse, provenance, schema) are `ConfigError`s carrying the part file's own path — one file's problems are gathered per file, the all-at-once contract stays per-file.
9. **Provenance entries are `{ source, link?, note? }`** with `source` one of `datasheet | measured | estimated` (serde lowercase). The key is `link`, not `ref` (a Rust keyword). The walker is generic over the parsed TOML: every leaf path under the part's data tables (tables recurse, **arrays are single leaves** — `prop.ct_table` is one parameter) must appear in `[provenance]`, and every `[provenance]` key must be such a path. `schema_version`, `name` and `provenance` themselves are skipped. Absent optional fields (e.g. `vtx.antenna.beamwidth_deg`) need no provenance; the shipped part files write all values explicitly anyway.
10. **`pole_pairs` (u32, default 7) and `imbalance_kgm` (f64, default 0.0) land in Task 1 serde-defaulted** so the schema-4 quads keep loading until the Task 3 cutover; Task 4 removes the `pole_pairs` default (it becomes required — the Task 2 part files set it from the start). `imbalance_kgm` is the M4b vibration model's input; 0.0 keeps this milestone's runs bit-identical.
11. **The schema-4 refusal message names what moved**, like the schema-3 one before it: schema 5 moves `[frame]`/`[ground]`/`[motor]`/`[prop]`/`[esc]`/`[battery]`/`[vtx]` hardware into `library/` parts, and points at README's "Component library" section.
12. **`QuadConfig` loses `Deserialize`** — after the cutover it is built by `load()` from `QuadFile` (the raw references) plus parts, and is no longer the shape of any TOML file. Its section structs keep their derives (the part structs reuse them). Validation field names stay stable where the section survives: `frame.*`, `ground.*`, `battery.*` (merged hardware + wiring); `motor.*`/`prop.*`/`esc.*` become `motors[i].*`/`props[i].*`/`escs[i].*`.

## Review Focus

The five inputs the spec implies but no task's tests fully exercise, most likely first — each is pinned by the named test:

1. **A schema-4 quad file after the cutover** (every existing muscle memory, every stale doc): an explicit refusal naming the moved sections, not a serde "invalid type: map, expected a string" — `a_schema_4_quad_file_is_refused_with_the_migration_message` (Task 3).
2. **Provenance completeness at depth and in excess**: a missing `vtx.antenna.gain_dbi` is named, an invented `[provenance."motor.does_not_exist"]` is named, a `ct_table` is one path not six — `missing_provenance_names_the_field`, `unknown_provenance_path_is_rejected`, `table_valued_parameters_are_one_path` (Task 1).
3. **Library location failures**: absent library root vs empty kind directory vs unknown id, each its own actionable message; `OFS_LIBRARY` honored without touching the process env in tests — `a_missing_kind_directory_names_the_library_root`, `an_unknown_part_id_lists_what_the_directory_holds`, `the_library_root_honours_the_override` (Tasks 1/3).
4. **List-length rules and mixed builds**: `motors = ["a", "b"]` (length 2) refused; `props` broadcasting while `motors` is mixed accepted; the per-motor wiring actually reached (different kVs spin different omegas) — `a_wrong_length_part_list_is_refused`, `a_mixed_motor_quad_builds_and_spins_its_motors_differently` (Tasks 3).
5. **`motor_poles` agreement and the KISS/eRPM numerics**: a diff setting `motor_poles = 12` against `pole_pairs = 7` refused; a silent diff with `pole_pairs = 6` refused (default 14); agreement passes; the codec/esc-telemetry golden values are unchanged for `pole_pairs = 7` — `a_diffs_motor_poles_must_match_the_motors`, `pole_pairs_without_a_diff_line_must_match_the_default_14`, `the_golden_erpm_values_survive_the_pole_pairs_argument` (Task 4).

## File Structure

```
crates/ofs-config/src/part.rs        new   part structs, provenance walker, per-kind loaders
crates/ofs-config/src/lib.rs         SCHEMA 5, QuadFile/wiring, resolution, QuadConfig rebuilt, validation loops
crates/ofs-config/tests/part.rs      new   part + provenance tests
crates/ofs-config/tests/config.rs    helpers write a temp library; mutations re-pointed (frame/vtx power → part files)
crates/ofs-physics/src/rigid_body.rs AirframeParams.rotor_inertia_kgm2: Vec<f64> (+ its tests)
crates/ofs-fc/src/sitl/esc_telemetry.rs  POLE_PAIRS const dies; EscTelemetry carries pole_pairs
crates/ofs-fc/src/sitl/codec.rs      erpm_block_payload(&omega, &pole_pairs)
crates/ofs-fc/src/sitl/bridge.rs     BridgeConfig.pole_pairs: Vec<f64>
crates/ofs-fc/tests/sitl_codec.rs    erpm calls gain the argument (golden bytes unchanged)
crates/ofs-fc/tests/esc_telemetry.rs constructions gain the argument
crates/ofs-sim/src/vehicle.rs        per-motor params, rotor inertia Vec, pole-pairs plumbing
library/README.md                    new   CC-BY-SA-4.0 data note (Task 5)
library/frames/opendrone-5f.toml     new   (Task 2; consumed in Task 3)
library/motors/opendrone-openmotor-2207.toml  new
library/props/opendrone-5in-prop.toml         new
library/escs/opendrone-4in1-esc.toml          new
library/batteries/opendrone-6s-1300.toml      new
library/vtx/opendrone-smartaudio-1w.toml      new
library/vtx/opendrone-tramp-600.toml          new
quads/opendrone-5f-freestyle.toml   rewritten (Task 3; values unchanged)
quads/opendrone-5f-tramp.toml       rewritten (Task 3; values unchanged)
docs/research/component-library.md  new   every shipped parameter's source (Task 5)
docs/superpowers/m4a-carried-debt.md new   (Task 5)
README.md, docs/dev-setup.md        library section, schema notes updated (Task 5)
```

---

### Task 1: `ofs-config::part` — part files, provenance walker, per-kind loaders

The part module in isolation: it loads and validates one part file given a path. Nothing consumes it yet; the shipped quads still load (the new `pole_pairs`/`imbalance_kgm` fields are serde-defaulted, ruling 10).

**Files:**
- Create: `crates/ofs-config/src/part.rs`
- Modify: `crates/ofs-config/src/lib.rs` (module declaration; `MotorSection.pole_pairs`, `PropSection.imbalance_kgm`)
- Create: `crates/ofs-config/tests/part.rs`

**Interfaces:**
- Produces: `ofs_config::part::{FramePart, MotorPart, PropPart, EscPart, BatteryPart, VtxPart, Provenance, ProvenanceSource, PART_SCHEMA_VERSION, MIN_PART_SCHEMA_VERSION}`; loaders `load_frame(path: &Path) -> Result<FramePart, ConfigError>` and the five siblings, all by **path** (the quad-side resolver — Task 3 — owns directories, listing and unknown-id messages).
- `BatteryPart`/`VtxPart` carry hardware only: `BatteryPart { battery: BatteryHardware }` where `pub struct BatteryHardware { cells, capacity_mah, r0_ohm, r1_ohm, c1_f, ocv_table }` (new, in `part.rs`; `initial_soc`/`rate_hz` are quad wiring); `VtxPart { vtx: VtxHardware }` where `pub struct VtxHardware { kind: VtxKind, power_levels_mw, power_levels_dbm, reply_latency_ms, antenna: VtxAntennaSection, pit_power_mw }` (the part file's `[vtx]` table; `uart`/band/channel/index are quad wiring).

- [ ] **Step 1: Write the failing tests in `crates/ofs-config/tests/part.rs`**

```rust
use std::fs;
use std::path::Path;

use ofs_config::part::{load_frame, load_motor, load_prop, load_vtx, ProvenanceSource};

const FRAME: &str = r#"
schema_version = 1
name = "Test 5F frame"

[frame]
mass_kg = 0.65
inertia_kgm2 = [0.0025, 0.0025, 0.0045]
cda_m2 = [0.010, 0.010, 0.020]
angular_damping_nms = 0.0001
motor_positions_frd_m = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]]
motor_spin = [1, -1, -1, 1]
contact_points_frd_m = [[-0.08, 0.08, 0.03], [0.08, 0.08, 0.03], [-0.08, -0.08, 0.03], [0.08, -0.08, 0.03]]

[ground]
stiffness_npm = 3000.0
damping_nspm = 40.0
friction_coeff = 0.6

[provenance."frame.mass_kg"]
source = "estimated"
[provenance."frame.inertia_kgm2"]
source = "estimated"
[provenance."frame.cda_m2"]
source = "estimated"
[provenance."frame.angular_damping_nms"]
source = "estimated"
[provenance."frame.motor_positions_frd_m"]
source = "measured"
[provenance."frame.motor_spin"]
source = "estimated"
[provenance."frame.contact_points_frd_m"]
source = "estimated"
[provenance."ground.stiffness_npm"]
source = "estimated"
[provenance."ground.damping_nspm"]
source = "estimated"
[provenance."ground.friction_coeff"]
source = "estimated"
"#;

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

const PROP: &str = r#"
schema_version = 1
name = "Test prop"

[prop]
diameter_m = 0.127
inertia_kgm2 = 6.0e-6
ct_table = [[0.0, 0.11], [30000.0, 0.11]]
cp_table = [[0.0, 0.045], [30000.0, 0.045]]
imbalance_kgm = 0.0

[provenance."prop.diameter_m"]
source = "estimated"
[provenance."prop.inertia_kgm2"]
source = "estimated"
[provenance."prop.ct_table"]
source = "estimated"
[provenance."prop.cp_table"]
source = "estimated"
[provenance."prop.imbalance_kgm"]
source = "estimated"
"#;

#[test]
fn a_frame_part_loads_with_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let part = load_frame(&write(dir.path(), "f.toml", FRAME)).unwrap();
    assert_eq!(part.name, "Test 5F frame");
    assert_eq!(part.frame.mass_kg, 0.65);
    assert_eq!(part.ground.friction_coeff, 0.6);
    assert!(part.collision.is_none());
    assert_eq!(part.provenance["frame.motor_positions_frd_m"].source, ProvenanceSource::Measured);
}

#[test]
fn missing_provenance_names_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let missing = FRAME.replace("\n[provenance.\"ground.friction_coeff\"]\nsource = \"estimated\"\n", "\n");
    let err = load_frame(&write(dir.path(), "f.toml", &missing)).unwrap_err();
    assert!(err.to_string().contains("ground.friction_coeff"), "{err}");
}

#[test]
fn unknown_provenance_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let extra = format!("{FRAME}\n[provenance.\"frame.does_not_exist\"]\nsource = \"estimated\"\n");
    let err = load_frame(&write(dir.path(), "f.toml", &extra)).unwrap_err();
    assert!(err.to_string().contains("frame.does_not_exist"), "{err}");
}

#[test]
fn a_bad_source_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = FRAME.replace("source = \"estimated\"", "source = \"guessed\"").replace("source = \"measured\"", "source = \"guessed\"");
    let err = load_frame(&write(dir.path(), "f.toml", &bad)).unwrap_err();
    assert!(err.to_string().contains("guessed"), "{err}");
}

#[test]
fn an_unknown_part_schema_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = load_frame(&write(dir.path(), "f.toml", &FRAME.replace("schema_version = 1", "schema_version = 2"))).unwrap_err();
    assert!(err.to_string().contains("schema_version 2"), "{err}");
}

#[test]
fn an_unknown_data_field_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = load_frame(&write(dir.path(), "f.toml", &FRAME.replace("mass_kg = 0.65", "mass_kg = 0.65\nmas_kg = 1.0"))).unwrap_err();
    assert!(err.to_string().contains("mas_kg"), "{err}");
}

const MOTOR: &str = r#"
schema_version = 1
name = "Test motor"

[motor]
kv_rpm_per_v = 1950.0
resistance_ohm = 0.07
no_load_current_a = 1.2
rotor_inertia_kgm2 = 6.0e-6
pole_pairs = 7

[provenance."motor.kv_rpm_per_v"]
source = "estimated"
[provenance."motor.resistance_ohm"]
source = "estimated"
[provenance."motor.no_load_current_a"]
source = "estimated"
[provenance."motor.rotor_inertia_kgm2"]
source = "estimated"
[provenance."motor.pole_pairs"]
source = "estimated"
"#;

#[test]
fn a_motor_part_carries_pole_pairs_and_the_sections_default_the_new_fields() {
    let dir = tempfile::tempdir().unwrap();
    let motor = load_motor(&write(dir.path(), "m.toml", MOTOR)).unwrap();
    assert_eq!(motor.motor.pole_pairs, 7);
    let prop = load_prop(&write(dir.path(), "p.toml", PROP)).unwrap();
    assert_eq!(prop.prop.imbalance_kgm, 0.0);
    // The schema-4 quads still load against the defaulted fields until the Task 3 cutover
    // (this access moves to cfg.motors[0]/cfg.props[0] there — see Task 3's mapping table).
    let cfg = ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap();
    assert_eq!(cfg.motor.pole_pairs, 7);
    assert_eq!(cfg.prop.imbalance_kgm, 0.0);
}

#[test]
fn table_valued_parameters_are_one_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = PROP.replace("[provenance.\"prop.ct_table\"]\nsource = \"estimated\"\n", "\n");
    let err = load_prop(&write(dir.path(), "p.toml", &missing)).unwrap_err();
    let shown = err.to_string();
    assert!(shown.contains("prop.ct_table"), "{shown}");
    assert_eq!(shown.matches("prop.ct_table").count(), 1, "the table is one parameter, not one problem per row: {shown}");
}
```

(Task 2's completeness test adds `load_esc` and `load_battery` to these imports.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-config --test part`
Expected: FAIL — `unresolved module`/`no function or associated item named load_frame`.

- [ ] **Step 3: Implement `crates/ofs-config/src/part.rs`**

```rust
//! Component-library part files (TOML): one file per part, every parameter's provenance checked.
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::{
    BatterySection, CollisionSection, ConfigError, EscSection, FrameSection, GroundSection, MotorSection,
    Problem, PropSection, VtxAntennaSection, VtxKind,
};

/// 1: first shape (M4a). Part files version independently of quad files.
pub const PART_SCHEMA_VERSION: u32 = 1;
pub const MIN_PART_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceSource {
    Datasheet,
    Measured,
    Estimated,
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

impl Provenance {
    fn estimated() -> Self {
        Self { source: ProvenanceSource::Estimated, ..Default::default() }
    }
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VtxPart {
    pub schema_version: u32,
    pub name: String,
    pub vtx: VtxHardware,
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
```

`Provenance::estimated()` is used by Task 3's tests; if clippy flags it unused until then, keep it with `#[allow(dead_code)]` removed in Task 3. The `VtxHardware` defaults reference `crate::default_vtx_reply_latency_ms`/`crate::default_pit_power_mw` — make those two `pub(crate)` in `lib.rs`.

- [ ] **Step 4: Add the fields to `lib.rs`'s sections (ruling 10)**

```rust
pub struct MotorSection {
    pub kv_rpm_per_v: f64,
    pub resistance_ohm: f64,
    pub no_load_current_a: f64,
    pub rotor_inertia_kgm2: f64,
    /// Electrical revolutions per mechanical revolution; Betaflight's `motor_poles` / 2 (M4a).
    #[serde(default = "default_pole_pairs")]
    pub pole_pairs: u32,
}

fn default_pole_pairs() -> u32 {
    7
}

pub struct PropSection {
    pub diameter_m: f64,
    pub inertia_kgm2: f64,
    pub ct_table: Vec<[f64; 2]>,
    pub cp_table: Vec<[f64; 2]>,
    /// Static imbalance (mass × radius) the M4b vibration model turns into IMU disturbance. 0 = none.
    #[serde(default)]
    pub imbalance_kgm: f64,
}
```

Declare the module (`pub mod part;`) and re-export nothing yet — Task 3 decides what `lib.rs` re-exports.

- [ ] **Step 5: Run the tests and clippy**

Run: `cargo test -p ofs-config && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: part tests PASS; the existing config tests still PASS (the defaulted fields change nothing); clippy clean.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-config/src/part.rs crates/ofs-config/src/lib.rs crates/ofs-config/tests/part.rs
git commit -m "feat(config): the component-library part module - six part kinds, mandatory provenance"
```

---

### Task 2: The shipped library — seven part files with provenance

Pure data, verified loadable through Task 1's API. Values are today's shipped values verbatim (the Tramp file's comment headers and the freestyle ones move with their data); every provenance entry is `source = "estimated"` with the M1 note — the point is the shape and the honesty, not new numbers (spec §2.3).

**Files:**
- Create: `library/README.md` (one line now, extended in Task 5: "Part data files are CC-BY-SA-4.0 (the parent design §10).")
- Create: `library/frames/opendrone-5f.toml`, `library/motors/opendrone-openmotor-2207.toml`, `library/props/opendrone-5in-prop.toml`, `library/escs/opendrone-4in1-esc.toml`, `library/batteries/opendrone-6s-1300.toml`, `library/vtx/opendrone-smartaudio-1w.toml`, `library/vtx/opendrone-tramp-600.toml`
- Test: `crates/ofs-config/tests/part.rs` (add)

**Interfaces:**
- Produces: the seven part ids `opendrone-5f`, `opendrone-openmotor-2207`, `opendrone-5in-prop`, `opendrone-4in1-esc`, `opendrone-6s-1300`, `opendrone-smartaudio-1w`, `opendrone-tramp-600` — Task 3's quads reference exactly these.

- [ ] **Step 1: Write the part files**

`library/frames/opendrone-5f.toml`:

```toml
# OpenDrone 5" freestyle frame — M1 estimates. Units are in the key suffixes (SI).
schema_version = 1
name = "OpenDrone 5F frame"

[frame]
mass_kg = 0.65
inertia_kgm2 = [0.0025, 0.0025, 0.0045]
cda_m2 = [0.010, 0.010, 0.020]
angular_damping_nms = 0.0001
# Betaflight Quad-X order: M1 rear-right, M2 front-right, M3 rear-left, M4 front-left (FRD: x fwd, y right, z down)
motor_positions_frd_m = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]]
# +1 = clockwise seen from above. Betaflight default: M1 CW, M2 CCW, M3 CCW, M4 CW.
motor_spin = [1, -1, -1, 1]
contact_points_frd_m = [[-0.08, 0.08, 0.03], [0.08, 0.08, 0.03], [-0.08, -0.08, 0.03], [0.08, -0.08, 0.03]]

[ground]
stiffness_npm = 3000.0
damping_nspm = 40.0
friction_coeff = 0.6

[provenance."frame.mass_kg"]
source = "estimated"
note = "M1 estimate"
[provenance."frame.inertia_kgm2"]
source = "estimated"
note = "M1 estimate"
[provenance."frame.cda_m2"]
source = "estimated"
note = "M1 estimate"
[provenance."frame.angular_damping_nms"]
source = "estimated"
note = "M1 estimate"
[provenance."frame.motor_positions_frd_m"]
source = "estimated"
note = "M1 estimate, a 5\" X-frame's arm length"
[provenance."frame.motor_spin"]
source = "datasheet"
link = "https://betaflight.com/docs/development/Mixer"
note = "Betaflight Quad-X order"
[provenance."frame.contact_points_frd_m"]
source = "estimated"
note = "M1 estimate, legs under the arms"
[provenance."ground.stiffness_npm"]
source = "estimated"
note = "M1 estimate"
[provenance."ground.damping_nspm"]
source = "estimated"
note = "M1 estimate"
[provenance."ground.friction_coeff"]
source = "estimated"
note = "M1 estimate"
```

`library/motors/opendrone-openmotor-2207.toml`:

```toml
# OpenDrone OpenMotor 2207 1950KV — M1 estimates standing in for the OpenMotor datasheet values.
schema_version = 1
name = "OpenMotor 2207 1950KV"

[motor]
kv_rpm_per_v = 1950.0
resistance_ohm = 0.07
no_load_current_a = 1.2
rotor_inertia_kgm2 = 6.0e-6
pole_pairs = 7

[provenance."motor.kv_rpm_per_v"]
source = "estimated"
note = "M1 estimate; the OpenMotor datasheet value replaces this when transcribed"
[provenance."motor.resistance_ohm"]
source = "estimated"
note = "M1 estimate"
[provenance."motor.no_load_current_a"]
source = "estimated"
note = "M1 estimate"
[provenance."motor.rotor_inertia_kgm2"]
source = "estimated"
note = "M1 estimate"
[provenance."motor.pole_pairs"]
source = "estimated"
note = "14-pole motor; matches Betaflight's motor_poles default of 14"
```

`library/props/opendrone-5in-prop.toml`:

```toml
# OpenDrone 5" propeller — M1 estimates. Units are in the key suffixes (SI).
schema_version = 1
name = "OpenDrone 5in prop"

[prop]
diameter_m = 0.127
inertia_kgm2 = 6.0e-6
ct_table = [[0.0, 0.11], [30000.0, 0.11]]
cp_table = [[0.0, 0.045], [30000.0, 0.045]]
imbalance_kgm = 0.0

[provenance."prop.diameter_m"]
source = "estimated"
note = "5 inch"
[provenance."prop.inertia_kgm2"]
source = "estimated"
note = "M1 estimate"
[provenance."prop.ct_table"]
source = "estimated"
note = "M1 flat-table estimate"
[provenance."prop.cp_table"]
source = "estimated"
note = "M1 flat-table estimate"
[provenance."prop.imbalance_kgm"]
source = "estimated"
note = "no measurement; 0 until the M4b vibration model lands"
```

`library/escs/opendrone-4in1-esc.toml`:

```toml
# OpenDrone 4-in-1 ESC (AM32) — M1 behavioural estimates; firmware-in-the-loop is fidelity level 2.
schema_version = 1
name = "OpenDrone 4-in-1 ESC"

[esc]
response_tau_s = 0.002
current_limit_a = 45.0

[provenance."esc.response_tau_s"]
source = "estimated"
note = "M1 estimate"
[provenance."esc.current_limit_a"]
source = "estimated"
note = "M1 estimate"
```

`library/batteries/opendrone-6s-1300.toml`:

```toml
# OpenDrone 6S 1300 mAh pack — M1 estimates. Hardware only; initial_soc and rate_hz are the quad's wiring.
schema_version = 1
name = "OpenDrone 6S 1300 mAh"

[battery]
cells = 6
capacity_mah = 1300.0
r0_ohm = 0.024
r1_ohm = 0.010
c1_f = 2000.0
ocv_table = [[0.0, 3.27], [0.05, 3.61], [0.1, 3.69], [0.2, 3.73], [0.3, 3.77], [0.4, 3.79],
             [0.5, 3.82], [0.6, 3.87], [0.7, 3.92], [0.8, 3.97], [0.9, 4.06], [1.0, 4.20]]

[provenance."battery.cells"]
source = "estimated"
note = "6S"
[provenance."battery.capacity_mah"]
source = "estimated"
note = "M1 estimate"
[provenance."battery.r0_ohm"]
source = "estimated"
note = "M1 estimate"
[provenance."battery.r1_ohm"]
source = "estimated"
note = "M1 estimate"
[provenance."battery.c1_f"]
source = "estimated"
note = "M1 estimate"
[provenance."battery.ocv_table"]
source = "estimated"
note = "M1 estimate, a generic LiPo curve"
```

`library/vtx/opendrone-smartaudio-1w.toml`:

```toml
# A SmartAudio v2.1 VTX. Betaflight builds its power list from the dBm values reported (SITL has no vtxtable).
schema_version = 1
name = "OpenDrone SmartAudio VTX (1 W)"

[vtx]
kind = "smartaudio"
power_levels_mw = [25, 200, 600, 1000]
power_levels_dbm = [14, 23, 28, 30]
reply_latency_ms = 5.0
pit_power_mw = 0.1

[vtx.antenna]
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
# Up and back, on the back of the frame.
mount_frd = [-0.5, 0.0, -1.0]

[provenance."vtx.kind"]
source = "estimated"
note = "SmartAudio v2.1 behavioural model (M3a)"
[provenance."vtx.power_levels_mw"]
source = "estimated"
note = "M1 estimate"
[provenance."vtx.power_levels_dbm"]
source = "estimated"
note = "dBm equivalents of the mW table, our own conversion"
[provenance."vtx.reply_latency_ms"]
source = "estimated"
note = "M3a estimate"
[provenance."vtx.pit_power_mw"]
source = "estimated"
note = "M3a estimate: a trickle next to the pilot"
[provenance."vtx.antenna.kind"]
source = "estimated"
note = "M3b estimate"
[provenance."vtx.antenna.gain_dbi"]
source = "estimated"
note = "M3b estimate"
[provenance."vtx.antenna.polarization"]
source = "estimated"
note = "M3b estimate"
[provenance."vtx.antenna.mount_frd"]
source = "estimated"
note = "M3b estimate"
```

(Fix applied above: the `power_levels_dbm` provenance is our own conversion — `estimated` — not a datasheet citation.)

`library/vtx/opendrone-tramp-600.toml`:

```toml
# A Tramp VTX. Power levels are the FC's built-in Tramp table; the dBm column is a label - Tramp talks mW
# and the codec never sends it (M3e rulings 3-4).
schema_version = 1
name = "OpenDrone Tramp VTX (600 mW)"

[vtx]
kind = "tramp"
power_levels_mw = [25, 100, 200, 400, 600]
power_levels_dbm = [14, 20, 23, 26, 28]
reply_latency_ms = 5.0
pit_power_mw = 0.1

[vtx.antenna]
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
mount_frd = [-0.5, 0.0, -1.0]

[provenance."vtx.kind"]
source = "estimated"
note = "Tramp behavioural model (M3e)"
[provenance."vtx.power_levels_mw"]
source = "datasheet"
link = "https://github.com/betaflight/betaflight/blob/master/src/main/io/vtx_tramp.c"
note = "the FC driver's built-in trampPowerTable"
[provenance."vtx.power_levels_dbm"]
source = "estimated"
note = "labels only; the codec never sends dBm"
[provenance."vtx.reply_latency_ms"]
source = "estimated"
note = "M3e estimate"
[provenance."vtx.pit_power_mw"]
source = "estimated"
note = "default 0.1 (M3a)"
[provenance."vtx.antenna.kind"]
source = "estimated"
note = "same antenna as the SmartAudio quad"
[provenance."vtx.antenna.gain_dbi"]
source = "estimated"
note = "M3b estimate"
[provenance."vtx.antenna.polarization"]
source = "estimated"
note = "M3b estimate"
[provenance."vtx.antenna.mount_frd"]
source = "estimated"
note = "M3b estimate"
```

- [ ] **Step 2: Add the completeness test to `crates/ofs-config/tests/part.rs`**

```rust
#[test]
fn every_shipped_part_loads_and_names_its_id() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"));
    let frame = load_frame(&root.join("frames/opendrone-5f.toml")).unwrap();
    assert_eq!(frame.frame.mass_kg, 0.65);
    assert_eq!(frame.frame.motor_positions_frd_m.len(), 4);
    assert!(frame.collision.is_none());
    let motor = load_motor(&root.join("motors/opendrone-openmotor-2207.toml")).unwrap();
    assert_eq!(motor.motor.pole_pairs, 7);
    let prop = load_prop(&root.join("props/opendrone-5in-prop.toml")).unwrap();
    assert_eq!(prop.prop.imbalance_kgm, 0.0);
    load_esc(&root.join("escs/opendrone-4in1-esc.toml")).unwrap();
    load_battery(&root.join("batteries/opendrone-6s-1300.toml")).unwrap();
    let sa = load_vtx(&root.join("vtx/opendrone-smartaudio-1w.toml")).unwrap();
    assert_eq!(sa.vtx.power_levels_mw, vec![25, 200, 600, 1000]);
    let tramp = load_vtx(&root.join("vtx/opendrone-tramp-600.toml")).unwrap();
    assert_eq!(tramp.vtx.power_levels_mw, vec![25, 100, 200, 400, 600]);
    assert_eq!(tramp.vtx.kind, ofs_config::VtxKind::Tramp);
}
```

- [ ] **Step 3: Run and commit**

Run: `cargo test -p ofs-config --test part && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS (any provenance typo surfaces here, named by field).

```bash
git add library/
git commit -m "feat(library): the seven shipped OpenDrone part files with per-parameter provenance"
```

---

### Task 3: The cutover — quad schema 5, resolution, per-motor vehicle wiring, both quads migrated

The atomic one: after this task a quad file is references + wiring, the loader resolves parts against the library root, and the vehicle consumes per-motor vectors. **The steps do not compile independently** (the `QuadConfig` shape changes under `ofs-sim`); the commit lands once, with the whole workspace green. The tests are rewritten first so their expectations are the contract, not the residue.

**Files:**
- Modify: `crates/ofs-config/src/lib.rs` (SCHEMA 5, `QuadFile`, wiring structs, `PartRefs`, resolution, `QuadConfig` rebuilt, `validate()` loops, `collision_spheres()`)
- Modify: `crates/ofs-sim/src/vehicle.rs` (per-motor params, rotor-inertia vector, `collision_spheres` consumer unchanged)
- Modify: `crates/ofs-physics/src/rigid_body.rs` (`AirframeParams.rotor_inertia_kgm2: Vec<f64>`; use site; its tests)
- Modify: `quads/opendrone-5f-freestyle.toml`, `quads/opendrone-5f-tramp.toml` (rewritten)
- Modify: `crates/ofs-config/tests/config.rs` (helpers + migrations)

**Interfaces:**
- Consumes: Task 1's `part::{load_frame, load_motor, load_prop, load_esc, load_battery, load_vtx}`; Task 2's part ids.
- Produces: `QuadConfig { frame: FrameSection, ground: GroundSection, collision: Option<CollisionSection>, motors: Vec<MotorSection>, props: Vec<PropSection>, escs: Vec<EscSection>, battery: BatterySection, vtx: Option<VtxSection>, …, parts: PartRefs }` (Task 4 consumes `motors[i].pole_pairs`; M4b consumes `props[i].imbalance_kgm`); `ofs_config::library_root(env: Option<&str>, quad_dir: &Path) -> PathBuf`.

- [ ] **Step 1: Rewrite the test helpers in `crates/ofs-config/tests/config.rs`**

The old helper wrote a quad beside its diff; the new one writes the repo layout in the temp dir — quad at `{dir}/quads/`, the shipped library copied to `{dir}/library/` — so the default root (`{quad dir}/../library`) finds it without touching the process env (ruling 7).

```rust
use std::fs;
use std::path::Path;

use ofs_config::{load, ConfigError, FcKind, RadioKind, VtxKind};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library");

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap().replace("\r\n", "\n")
}

fn part_text(kind: &str, id: &str) -> String {
    fs::read_to_string(Path::new(LIBRARY).join(kind).join(format!("{id}.toml"))).unwrap().replace("\r\n", "\n")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// The quad at {dir}/quads/quad.toml, its diff beside it, the shipped library at {dir}/library/.
/// Re-copies the library every call — mutate parts with `write_part` only AFTER this, never before.
fn write_quad(dir: &Path, text: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join("quads")).unwrap();
    let path = dir.join("quads/quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.join("quads/opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    copy_dir(Path::new(LIBRARY), &dir.join("library"));
    path
}

/// Replaces one part file's text (after `write_quad`).
fn write_part(dir: &Path, kind: &str, id: &str, text: &str) {
    fs::write(dir.join("library").join(kind).join(format!("{id}.toml")), text).unwrap();
}
```

- [ ] **Step 2: Migrate the existing tests' mutations**

Mapping (assertions' field names do not change — the resolved sections keep them). Three existing tests mix mutations that now live in **different files**; a load reports one file's problems, so those tests **split** — their quad-half and part-half each keep the multi-problem assertion:

| Old mutation (quad text) | New location |
|---|---|
| `mass_kg = 0.65`, `motor_spin = …`, `mas_kg`, `motor_positions_frd_m`/`contact_points_frd_m` non-finite | `part_text("frames", "opendrone-5f")` + `write_part(dir, "frames", "opendrone-5f", …)` — always **after** `write_quad` |
| `initial_soc = 2.0`, `gyro_bias_radps`, `accel_bias_mps2`, `position_ned_m`, `yaw_deg`, `alt_m`, `packet_rate_hz`, `uart = 2`, `tx_power_mw`, fc timeouts, `[[radio.antennas]]`, osd/esc_telemetry sections | unchanged — still in the quad text |
| `power_levels_dbm = [14, 23, 28]`, vtx power tables | `part_text("vtx", "opendrone-smartaudio-1w")` + `write_part` |
| `[vtx]\nkind = "smartaudio"\nuart = 5` | `[vtx]\npart = "opendrone-smartaudio-1w"\nuart = 5` (the quad text's own shape, Task 3 Step 5) |
| `schema_version = 4` asserts | `schema_version = 5`; `unsupported_schema_version_is_explicit` keeps testing 1 and now 4 (Step 4's message) |
| `the_old_radio_fields_are_unknown_in_schema_4` | rename `…_in_schema_5`; the mutation is unchanged (`rssi_dbm` is unknown in the quad file) |
| `every_problem_is_reported_at_once` | **splits**: part-half mutates `mass_kg` + `motor_spin` in the frame part (one `Invalid` listing both, on the part's path); quad-half mutates `initial_soc` with, e.g., `packet_rate_hz` and keeps the multi-problem assertion |
| `non_finite_vectors_are_rejected` | **splits**: `motor_positions_frd_m`/`contact_points_frd_m` nan/inf belong to the frame part; `gyro_bias`/`accel_bias`/`position`/`yaw`/`alt_m` stay quad-level |
| `video_problems_are_reported_together` | **splits**: `power_levels_dbm` truncation moves to the vtx part; `osd.uart` collision and `default_band = "Z"` stay quad-level |
| Task 1's `a_motor_part_carries_pole_pairs…` shipped-quad accesses | `cfg.motor` → `cfg.motors[0]`, `cfg.prop` → `cfg.props[0]` (same assertions) |

The reference test becomes:

```rust
#[test]
fn reference_quad_loads_and_validates() {
    let cfg = load(Path::new(QUAD)).unwrap();
    assert_eq!(cfg.schema_version, 5);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart), (500, 2));
    assert_eq!(cfg.frame.motor_positions_frd_m.len(), 4);
    assert_eq!(cfg.motors.len(), 4, "the single motor part broadcasts to every motor");
    assert_eq!(cfg.motors[0].pole_pairs, 7);
    assert_eq!(cfg.parts.battery, "opendrone-6s-1300");
    assert_eq!(cfg.fc.kind, FcKind::Sitl);
    assert_eq!(cfg.vtx.as_ref().unwrap().kind, VtxKind::Smartaudio);
    assert!(cfg.validate().is_empty());
}
```

Add the resolution tests:

```rust
#[test]
fn a_schema_4_quad_file_is_refused_with_the_migration_message() {
    let dir = tempfile::tempdir().unwrap();
    let old = quad_text()
        .replace("schema_version = 5", "schema_version = 4")
        .replace("frame = \"opendrone-5f\"", "[frame]\nmass_kg = 0.65");
    let err = load(&write_quad(dir.path(), &old)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("schema_version 4"), "{message}");
    assert!(message.contains("library"), "{message}");
}

#[test]
fn an_unknown_part_id_lists_what_the_directory_holds() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]", "motors = [\"no-such-motor\"]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("no-such-motor"), "{err}");
    assert!(err.to_string().contains("opendrone-openmotor-2207"), "lists the ids it holds: {err}");
}

#[test]
fn a_missing_kind_directory_names_the_library_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::remove_dir_all(dir.path().join("library/frames")).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("frames"), "{err}");
    assert!(err.to_string().contains("OFS_LIBRARY"), "{err}");
}

#[test]
fn a_wrong_length_part_list_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]",
                                   "motors = [\"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\"]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("motors"), "{err}");
    assert!(err.to_string().contains("1 or 4"), "{err}");
}

#[test]
fn the_library_root_honours_the_override() {
    let quad_dir = Path::new("C:/anywhere/quads");
    assert_eq!(ofs_config::library_root(None, quad_dir), Path::new("C:/anywhere/library"));
    assert_eq!(ofs_config::library_root(Some("C:/my-library"), quad_dir), Path::new("C:/my-library"));
}

#[test]
fn a_missing_vtx_part_is_reported_on_the_vtx_section() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("part = \"opendrone-smartaudio-1w\"", "part = \"no-such-vtx\"");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("vtx.part"), "{err}");
}
```

- [ ] **Step 3: Rewrite `lib.rs`'s loading half**

```rust
/// 5: quads assemble from library parts (M4a) — frame/motor/prop/esc/battery/vtx hardware moves to
/// `library/`, every part parameter carries provenance. 4: the real radio link and [collision] (M3c).
/// 3: [esc_telemetry]/[osd]/[vtx] (M3a). 2: the required [radio] section (M2).
pub const SCHEMA_VERSION: u32 = 5;
pub const MIN_SCHEMA_VERSION: u32 = 5;
```

In `load()`'s schema gate, above the existing `Some(v @ ..=3)` arm:

```rust
Some(4) => {
    let message = "unsupported schema_version 4 (this build reads schema 5); schema 5 moves the [frame], \
                   [ground], [motor], [prop], [esc], [battery] and [vtx] hardware sections into library/ part \
                   files with per-parameter provenance and the quad references them by id — see the \
                   \"Component library\" section of README.md".to_string();
    return Err(parse_err(message));
}
```

(The `..=3` arm keeps its message; adjust its wording only if it claims to be the oldest.)

The raw file shape and the resolved one:

```rust
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
```

(`[frame]`/`[ground]`/`[motor]`/`[prop]`/`[esc]`/`[collision]` tables in a quad file are parse errors through `deny_unknown_fields` — the hardware lives in parts; `collision` belongs to the frame part, ruling 1.)

```rust
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
```

`QuadConfig` loses `Deserialize` and gains the vectors (ruling 12): replace `motor: MotorSection`, `prop: PropSection`, `esc: EscSection` with `motors: Vec<MotorSection>`, `props: Vec<PropSection>`, `escs: Vec<EscSection>`; add `pub parts: PartRefs`; `frame`/`ground`/`collision`/`battery`/`vtx`/everything else keeps its name and type.

The resolver — after the schema gate, replacing today's `toml::from_str::<QuadConfig>`:

```rust
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
        let mut have: Vec<String> = fs::read_dir(&dir).unwrap_or_default()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        have.sort();
        format!("part '{id}' not found in {} (have: {})", dir.display(), have.join(", "))
    } else {
        format!("library kind directory not found: {} (library root {}; set OFS_LIBRARY to override)", dir.display(), root.display())
    };
    problems.push(Problem { field: field.to_string(), message });
    None
};

let frame_path = resolve("frames", &file.frame, "frame", &mut problems);
```

With `resolve` in hand, the loader runs one pass (problems collected, the frame part first — its motor count validates the lists):

1. Resolve + `load_frame` → `FramePart` (its own load errors propagate as the part file's `ConfigError`).
2. `let n = frame.frame.motor_positions_frd_m.len();` — validate `motors`/`props`/`escs` lengths are 1 or n: `problems.push(Problem { field: "motors", message: format!("needs 1 or {n} entries (got {})", file.motors.len()) })`.
3. Resolve every id in the three lists (fields `motors[i]`, `props[i]`, `escs[i]`), plus `battery.part` and `vtx.part`; if any problems, return `ConfigError::Invalid { path, problems }`.
4. Load each part; broadcast lists of length 1 to n entries.
5. Merge: `battery: BatterySection { cells, capacity_mah, r0_ohm, r1_ohm, c1_f, ocv_table, initial_soc: file.battery.initial_soc, rate_hz: file.battery.rate_hz }`; `vtx: Option<VtxSection>` assembled from `VtxHardware` + `VtxWiring` field by field (kind, power_levels_mw, power_levels_dbm, reply_latency_ms, antenna, pit_power_mw from the part; uart, default_band, default_channel, default_power_index from the wiring).
6. Build `QuadConfig { …, parts: PartRefs { frame: file.frame, motors: file.motors, props: file.props, escs: file.escs, battery: file.battery.part, vtx: file.vtx.as_ref().map(|v| v.part.clone()) }, source_path: path.to_path_buf() }`, then `validate()` as today.

- [ ] **Step 4: Update `validate()` and `collision_spheres()` for the vectors**

The scalar checks become loops with `motors[i]`/`props[i]`/`escs[i]` field names (ruling 12):

```rust
for (i, m) in self.motors.iter().enumerate() {
    c.positive(m.kv_rpm_per_v, &format!("motors[{i}].kv_rpm_per_v"));
    c.positive(m.resistance_ohm, &format!("motors[{i}].resistance_ohm"));
    c.non_negative(m.no_load_current_a, &format!("motors[{i}].no_load_current_a"));
    c.positive(m.rotor_inertia_kgm2, &format!("motors[{i}].rotor_inertia_kgm2"));
    c.check((1..=32).contains(&m.pole_pairs), &format!("motors[{i}].pole_pairs"), format!("must be 1..=32 (got {})", m.pole_pairs));
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
```

`collision_spheres()` uses each motor's own prop diameter:

```rust
for (m, p) in self.frame.motor_positions_frd_m.iter().zip(&self.props) {
    let tip = p.diameter_m / 2.0;
    for k in 0..SPHERES_PER_PROP {
        let a = 2.0 * std::f64::consts::PI * (k as f64) / SPHERES_PER_PROP as f64;
        spheres.push([m[0] + tip * a.cos(), m[1] + tip * a.sin(), m[2], PROP_SPHERE_RADIUS_M]);
    }
}
```

(The `esc_telemetry` diff check that reads `self.battery.cells` is unchanged — the merged `BatterySection` still carries `cells`.)

- [ ] **Step 5: Rewrite the two quad files**

`quads/opendrone-5f-freestyle.toml`:

```toml
# OpenDrone 5" freestyle — assembled from library parts (M1 estimates live in the part files now).
schema_version = 5
name = "OpenDrone 5F freestyle"

frame = "opendrone-5f"
motors = ["opendrone-openmotor-2207"]
props = ["opendrone-5in-prop"]
escs = ["opendrone-4in1-esc"]

[battery]
part = "opendrone-6s-1300"
initial_soc = 1.0
rate_hz = 1000

[sim]
base_hz = 8000

[imu]
gyro_noise_std_radps = 0.002
gyro_bias_radps = [0.0, 0.0, 0.0]
accel_noise_std_mps2 = 0.05
accel_bias_mps2 = [0.0, 0.0, 0.0]

[baro]
noise_std_pa = 2.0

[home]
lat_deg = 50.85
lon_deg = 4.35
alt_m = 30.0

[initial]
position_ned_m = [0.0, 0.0, -0.03]
yaw_deg = 0.0

[fc]
kind = "sitl"
exchange_hz = 1000
# Overridden by the OFS_SITL_LAUNCH / OFS_SITL_CLEANUP environment variables (space-separated argv).
# An empty cleanup defaults to "pkill -x betaflight_SITL" on Linux, and under WSL (launch[0] = wsl.exe)
# to "<wsl prefix> pkill -x betaflight_SITL".
launch = ["betaflight_SITL.elf"]
cleanup = []
betaflight_diff = "opendrone-5f-freestyle.betaflight.diff"
reply_timeout_ms = 500
first_reply_timeout_ms = 5000
startup_timeout_ms = 15000

[radio]
# ExpressLRS 2.4 GHz. The receiver's CRSF output is wired to Betaflight's UART2; the betaflight.diff selects
# the CRSF receiver there. RSSI, SNR and packet loss come from the geometry of the world file (M3c); the
# default receiver antenna is a 2 dBi linear dipole leaning up and back (mount_frd = [-0.5, 0.0, -1.0]).
kind = "elrs"
packet_rate_hz = 500
uart = 2
latency_packets = 1
tx_power_mw = 250

# M3a: video. Betaflight UART numbers are 1-based (as in the Configurator's Ports tab); UART1 is the MSP port.
# The battery as Betaflight's ESC sensor (KISS telemetry frames, 4 identical frames carry a quarter of the pack
# current and consumed charge each, see docs/superpowers/specs/2026-10-08-m3a-osd-vtx-design.md section 5).
[esc_telemetry]
uart = 3
rate_hz = 100

# Betaflight's OSD over MSP DisplayPort; 30x16 is the analog PAL grid (the diff sets vcd_video_system = PAL).
[osd]
uart = 4
cols = 30
rows = 16

# The VTX part is the hardware (power table, antenna); the wiring below is this aircraft's.
[vtx]
part = "opendrone-smartaudio-1w"
uart = 5
default_band = "R"
default_channel = 1
default_power_index = 1
```

`quads/opendrone-5f-tramp.toml`: identical except `name = "OpenDrone 5F freestyle (Tramp VTX)"`, `betaflight_diff = "opendrone-5f-tramp.betaflight.diff"`, and:

```toml
[vtx]
part = "opendrone-tramp-600"
uart = 6
default_band = "R"
default_channel = 1
default_power_index = 2
```

The two `.betaflight.diff` files are untouched.

- [ ] **Step 6: Consume the vectors in `vehicle.rs` and `rigid_body.rs`**

`vehicle.rs`'s build (today's lines 528–575), per-motor:

```rust
for i in 0..n {
    let prop = PropParams {
        diameter_m: cfg.props[i].diameter_m,
        ct_table: pairs(&cfg.props[i].ct_table),
        cp_table: pairs(&cfg.props[i].cp_table),
    };
    models.push(Box::new(Propeller::new(i, prop, &mut bus)));
}
for i in 0..n {
    let motor = MotorParams {
        kv_rpm_per_v: cfg.motors[i].kv_rpm_per_v,
        resistance_ohm: cfg.motors[i].resistance_ohm,
        no_load_current_a: cfg.motors[i].no_load_current_a,
        rotor_inertia_kgm2: cfg.motors[i].rotor_inertia_kgm2,
    };
    let esc = EscParams { response_tau_s: cfg.escs[i].response_tau_s, current_limit_a: cfg.escs[i].current_limit_a };
    models.push(Box::new(EscMotor::new(i, esc, motor, cfg.props[i].inertia_kgm2, &mut bus)));
}
```

`AirframeParams`: `rotor_inertia_kgm2: Vec<f64>` (ruling 3), built as

```rust
rotor_inertia_kgm2: (0..n).map(|i| cfg.motors[i].rotor_inertia_kgm2 + cfg.props[i].inertia_kgm2).collect(),
```

`rigid_body.rs:204` becomes `self.p.rotor_inertia_kgm2[k]`, and `rigid_body.rs`'s tests change their `AirframeParams` literals from a scalar to `vec![<same number>; <motor count>]` — same values, so every pinned expectation holds.

`vehicle.rs`'s own tests: `shipped_quad()` and the `(cfg.esc_telemetry, cfg.osd, cfg.vtx) = (None, None, None)` mutation keep working unchanged. Add the mixed-build pin:

```rust
#[test]
fn a_mixed_motor_quad_builds_and_spins_its_motors_differently() {
    let mut cfg = shipped_quad();
    let mut other = cfg.motors[0].clone();
    other.kv_rpm_per_v = 1000.0; // a slower motor on M3 and M4
    cfg.motors[2] = other.clone();
    cfg.motors[3] = other;
    let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
    vehicle.set_sticks(&Sticks { throttle: 1.0, ..Default::default() });
    vehicle.run_for(0.05).unwrap();
    let s = vehicle.state();
    assert!(s.motor_rpm[0] > s.motor_rpm[2], "the 1950KV motors spin faster than the 1000KV ones: {:?}", s.motor_rpm);
}
```

- [ ] **Step 7: Run everything, including the live hover**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings`
Then (WSL, dev-setup option 2): `uv run --with grpcio,protobuf,pytest pytest python/tests/test_sitl_hover.py`
Expected: all PASS — the value-preservation constraint is exactly what these suites pin (the vehicle's VtxInfo/UART/picture tests unchanged, the hover achieved, digests stable).

- [ ] **Step 8: Commit**

```bash
git add crates/ofs-config/src/lib.rs crates/ofs-config/tests/config.rs crates/ofs-sim/src/vehicle.rs crates/ofs-physics/src/rigid_body.rs quads/
git commit -m "feat(config)!: quad schema 5 - quads assemble from library parts (the cutover)"
```

---

### Task 4: Pole pairs as data — the constant dies, the diff agrees

`pole_pairs` becomes required in motor parts; `ofs-fc` takes it as data everywhere; the diff must agree with Betaflight's `motor_poles` (or its default 14), closing the M3d debt loudly.

**Files:**
- Modify: `crates/ofs-config/src/lib.rs` (remove the default; the diff agreement checks)
- Modify: `crates/ofs-fc/src/sitl/esc_telemetry.rs`, `crates/ofs-fc/src/sitl/codec.rs`, `crates/ofs-fc/src/sitl/bridge.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs` (plumb the values)
- Modify: `crates/ofs-fc/tests/sitl_codec.rs`, `crates/ofs-fc/tests/esc_telemetry.rs`, `crates/ofs-config/tests/config.rs`

**Interfaces:**
- Consumes: `QuadConfig.motors[i].pole_pairs: u32` (Task 3).
- Produces: `EscTelemetry::new(uart: Wire, motors: usize, divisor: u32, pole_pairs: f64, bus: &mut Bus)`; `erpm_block_payload(motor_omega_radps: &[f64], pole_pairs: &[f64]) -> [u8; 8]`; `BridgeConfig { pole_pairs: Vec<f64>, … }`. The `POLE_PAIRS` const is deleted — nothing outside `ofs-fc` referenced it (verified: `codec.rs:100` and `esc_telemetry.rs:83` only).

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-config/tests/config.rs`:

```rust
#[test]
fn a_diffs_motor_poles_must_match_the_motors() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let diff = dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff");
    let mismatched = "feature -GPS\nset motor_poles = 12\n";
    fs::write(&diff, mismatched).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("motor_poles"), "{err}");
    assert!(err.to_string().contains("14"), "names the motors' pole count: {err}");
    fs::write(&diff, "feature -GPS\nset motor_poles = 14\n").unwrap();
    assert!(load(&path).is_ok());
}

#[test]
fn pole_pairs_without_a_diff_line_must_match_the_default_14() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text()); // the library lands first; mutate parts after it
    let six = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7", "pole_pairs = 6");
    write_part(dir.path(), "motors", "opendrone-openmotor-2207", &six);
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("motor_poles"), "{err}");
    assert!(err.to_string().contains("12 poles"), "{err}");
}

#[test]
fn mixed_pole_pairs_on_a_sitl_quad_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]",
        "motors = [\"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\", \"opendrone-openmotor-6pp\"]");
    let path = write_quad(dir.path(), &text);
    let six = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7", "pole_pairs = 6");
    write_part(dir.path(), "motors", "opendrone-openmotor-6pp", &six);
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("uniform"), "{err}");
}

#[test]
fn a_motor_part_without_pole_pairs_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let missing = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7\n", "\n");
    write_part(dir.path(), "motors", "opendrone-openmotor-2207", &missing);
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("pole_pairs"), "{err}");
}
```

In `crates/ofs-fc/tests/sitl_codec.rs`, update the four `erpm_block_payload(…)` calls to `erpm_block_payload(&[…], &[7.0; 4])` (and `&[7.0; 4]`-length-matched variants where the omega slice is shorter — `pole_pairs` is indexed per motor, so pass `&[7.0; omega.len()]`), keeping the exact expected bytes; add:

```rust
#[test]
fn the_golden_erpm_values_survive_the_pole_pairs_argument() {
    assert_eq!(erpm_block_payload(&[104.71975511965977], &[7.0]), [70, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(erpm_block_payload(&[104.71975511965977], &[3.0]), [30, 0, 0, 0, 0, 0, 0, 0], "6 poles halve the eRPM");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-config --test config && cargo test -p ofs-fc`
Expected: FAIL — `pole_pairs` still defaults (no parse error), no `motor_poles` check, arity mismatch on `erpm_block_payload`.

- [ ] **Step 3: Implement**

`lib.rs`: delete `default_pole_pairs` and the `#[serde(default = "default_pole_pairs")]` attribute (the field becomes required — part files without it are parse errors). Inside `validate()`'s existing `if self.fc.kind == FcKind::Sitl` block, after the `force_battery_cell_count` check and still inside the `if let Ok(text)` diff guard:

```rust
let pole_pairs = self.motors.iter().map(|m| m.pole_pairs).collect::<Vec<_>>();
if pole_pairs.iter().any(|p| *p != pole_pairs[0]) {
    c.check(false, "motors", "SITL quads need uniform motor pole_pairs: Betaflight has one motor_poles setting, and the KISS battery frames carry one pack eRPM");
}
let poles = 2 * pole_pairs[0];
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
```

`esc_telemetry.rs`: delete `pub const POLE_PAIRS`; add `pole_pairs: f64` to the struct and `pole_pairs: f64` parameter to `new()`; the step's `mean_rpm * POLE_PAIRS` becomes `mean_rpm * self.pole_pairs`; the doc comment on the struct says "eRPM = mechanical rpm × pole pairs (uniform on SITL quads; enforced by ofs-config)".

`codec.rs`:

```rust
pub fn erpm_block_payload(motor_omega_radps: &[f64], pole_pairs: &[f64]) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, omega) in motor_omega_radps.iter().take(4).enumerate() {
        let erpm = omega * 60.0 / (2.0 * std::f64::consts::PI) * pole_pairs.get(i).copied().unwrap_or(pole_pairs[0]) / 100.0;
        let value = erpm.round().clamp(0.0, 65535.0) as u16;
        out[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    out
}
```

(Update its doc comment: pole pairs come from the quad's motor parts — the m3d constant's replacement.)

`bridge.rs`: `BridgeConfig` gains `pub pole_pairs: Vec<f64>`; `SitlBridge` stores it at start-up beside its cloned `erpm` signals (the `let erpm = cfg.erpm.clone();` site, ~line 150); the send site (~line 412) becomes `erpm_block_payload(&omega, &self.pole_pairs)`.

`vehicle.rs`: `EscTelemetry::new(uart.clone(), motors, cfg.sim.base_hz / e.rate_hz, pp, bus)` where `let pp = cfg.motors[0].pole_pairs as f64;` (uniformity validated); `BridgeConfig` gains `pole_pairs: cfg.motors.iter().map(|m| f64::from(m.pole_pairs)).collect()`.

- [ ] **Step 4: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings`
Then (WSL): `uv run --with grpcio,protobuf,pytest pytest python/tests/test_sitl_hover.py python/tests/test_sitl_telemetry.py`
Expected: all PASS — the shipped quad's `pole_pairs = 7` reproduces today's bytes exactly.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-config/src/lib.rs crates/ofs-fc/src/sitl/esc_telemetry.rs crates/ofs-fc/src/sitl/codec.rs crates/ofs-fc/src/sitl/bridge.rs crates/ofs-sim/src/vehicle.rs crates/ofs-config/tests/config.rs crates/ofs-fc/tests/sitl_codec.rs crates/ofs-fc/tests/esc_telemetry.rs
git commit -m "feat(fc): pole pairs are motor data - the POLE_PAIRS constant dies and the diff must agree with motor_poles"
```

---

### Task 5: Docs, research note, carried debt

**Files:**
- Modify: `README.md` (a "Component library" section: the layout, `OFS_LIBRARY`, provenance, the schema-4→5 migration), `docs/dev-setup.md` (the schema notes at its §"Schema-3 quad files" and the radio-section paragraph name schema 5 and the library)
- Create: `docs/research/component-library.md` (every shipped parameter's source — all `estimated` M1 values; `pole_pairs = 7` from the 14-pole motors and Betaflight's default; `imbalance_kgm = 0` until M4b; the OpenDrone datasheets as the follow-up)
- Extend: `library/README.md` (CC-BY-SA-4.0 note; how to add a part)
- Create: `docs/superpowers/m4a-carried-debt.md`

**Interfaces:** none (documents only).

- [ ] **Step 1: Write `docs/research/component-library.md`**

A table per part file: parameter, value, source, note — transcribing the part files' provenance with a short intro stating the M4a position: every shipped value is an M1 estimate recorded in the open; datasheet transcription (OpenMotor, OpenVTX) is community follow-up and lands by editing the part file and its provenance entry, nothing else.

- [ ] **Step 2: Write `docs/superpowers/m4a-carried-debt.md`**

```markdown
# M4a carried debt

## Deferred features
- **imu/baro are not parts** — they stay quad-level sections without provenance; they become parts when the
  sensor models have hardware identity (an MPU6000 vs an ICM42688 behaves differently).
- **fcs/, receivers/, cameras/ part kinds do not exist** — no model consumes them (the [fc] section is sim
  plumbing; receiver antennas are quad wiring; there is no camera model).
- **Provenance is validated, not exposed** — no RPC or API surfaces a part's provenance yet; it is file data.
- **OFS_LIBRARY is one path** — no search path list; point it at one library root.

## Rulings worth remembering
- SITL quads refuse mixed motor pole_pairs (one Betaflight motor_poles, one pack eRPM in the KISS frames).
- The eRPM datagram block takes per-motor pole pairs, so an open-loop mixed build is honest even though
  SITL cannot be.
- `rotor_inertia_kgm2` in AirframeParams is per-motor (a Vec) since M4a; uniform quads put the same number
  in every slot.
```

- [ ] **Step 3: README and dev-setup**

README's component-library section shows the tree from the spec §2.1, the `OFS_LIBRARY` override, one provenance example, and the migration sentence (schema 4's seven hardware sections → the part references of schema 5, pointing at the shipped quads as the worked example). dev-setup.md's schema paragraphs updated to name schema 5 and where the hardware sections went.

- [ ] **Step 4: Full verification and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings`, the Godot headless suite (`scripts/run-godot-tests.sh`), and (WSL) the full python sitl suite.
Expected: all PASS.

```bash
git add README.md docs/dev-setup.md docs/research/component-library.md docs/superpowers/m4a-carried-debt.md library/README.md
git commit -m "docs: M4a - the component library, its provenance ledger and the schema-5 migration"
```
