# M3b: Analog Video Link and Degradation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A 5.8 GHz analog link model turns the VTX state of M3a and the quad's pose in a world file into per-field signal quality, and a shader between Betaflight's OSD and the HUD breaks the FPV picture up the way analog video does.

**Architecture:** Pure propagation functions (`ofs-video::propagation`: path loss, antenna patterns, polarization, knife-edge diffraction, ground bounce) feed a `VideoLink` model (`ofs-video::link`) that runs once per PAL field, adds fading, interference and diversity, and maps the SNR to a picture and a sync state on the bus. A world file (`worlds/flat.toml`, parsed by `ofs-config::world`) gives the pilot's goggles, the objects and other emitters; the server owns it, serves it over a new `GetWorld` call and carries the link in `State.video` (protocol 4). The Godot client builds the field from the server's world and draws the link's numbers with a shader on canvas layer 3.

**Tech Stack:** Rust 1.85+ (workspace: glam, rand_chacha, rand_distr, serde/toml, tonic/prost), Python 3.10+ client (grpcio), Godot 4.7.2 + godot-rust.

**Spec:** `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md` (read it first; this plan implements it). Predecessor: `docs/superpowers/specs/2026-10-08-m3a-osd-vtx-design.md` (merged as c1f3292). Parent: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`: no language or std features newer than 1.85 in any crate except `ofs-godot` (the msrv CI job runs `cargo check --workspace --all-targets --locked --exclude ofs-godot`).
- Python >= 3.10. Godot **4.7.2** only (GL Compatibility). License GPL-3.0-or-later.
- Determinism: same quad + world + seed + inputs in lockstep give identical output. The link's fading draws only from `model_rng(seed, "video.link")`; the shader's grain is cosmetic and may differ between runs.
- Protocol: `PROTOCOL_VERSION = 4` in `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` together (a test compares them). Quad schema stays 3 (the new `[vtx]` fields are optional). World files have `schema_version = 1`.
- Frames: world files use the quad file's frame, NED metres from home (up is a negative `d`), sizes `[north, east, height]`. Only `ofs-client` converts to Godot's frame (`frames::vec_to_godot`); GDScript does no NED arithmetic.
- The link model runs at 50 Hz (`FIELD_RATE_HZ`, one PAL field); `sim.base_hz` must be a multiple of 50 when the quad has `[vtx]`.
- Every text file ends with a newline. Commit the `.uid` files Godot creates next to new `.gd` and `.gdshader` files. Never stage `.superpowers/`, `graphify-out/`, `shots/` or `build/`.
- Commit messages end with the trailer `Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>`.
- Never use `pkill -f` (use `pkill -x betaflight_SITL`); never run `wsl --shutdown`; do not touch Windows Firewall settings (ask the user).
- This plan downloads nothing.
- Task 1 changes the dependency graph (`ofs-video` gains glam, rand, rand_chacha, rand_distr): run its first cargo command without `--locked` so `Cargo.lock` updates, and commit `Cargo.lock` with the task.
- Files that contain backslashes must be written with the editor tools (Write/Edit), not Bash heredocs (heredocs collapse `\\`).
- Every edit below is "In `file`, replace: ... with: ...": the old text appears exactly once in the file at that point of the plan. Apply the edits of a step in the order given.

## Rulings and facts verified in advance

Every task of this plan was written as code in a scratch copy of the repository (at 997d3cb) and run there, including against real Betaflight SITL, before the plan was written; the code blocks below are that code. A script then replayed the plan's edits, task by task, onto a fresh copy and checked that each task's tests pass and that the result equals the tested tree. Observed, not assumed:

- **Results of the verified tree.** Windows: 275 Rust tests (workspace), 19 Python tests (+ 11 SITL ones skipped without `OFS_SITL_LAUNCH`), 273 Godot unit checks, the open-loop and error end-to-end runs. With Betaflight SITL: the 8 Python video tests (6 from M3a, 2 new), the Godot game flying real Betaflight. Linux (WSL): the workspace plus the 9 live Rust SITL tests (`sitl_live`, `vtx_live`).
- **The M3a-to-M3b chain works live.** Through `MSP_SET_VTX_CONFIG` (the Configurator's VTX tab), 25 -> 600 mW raised the SNR by 13.8 dB; pit mode lost the picture and raised `video_lost`; moving the VTX onto the parked quad's channel (R2) raised the interference by 22.8 dB, and R8 dropped it by 40 dB.
- **Link numbers in the shipped world** (quad on the ground straight ahead of the pilot, diversity goggles): 25 mW is clean (SNR >= 25 dB) to about 100 m and loses sync by 2 km; 200 mW, 300 m and 4 km; 600 mW, 500 m and beyond 4 km. On the launch pad the SNR is about 55 dB.
- **The picture.** The shots script renders grain, tearing with colour loss and sparkles, and full static; Betaflight's OSD (layer 2) breaks up with the picture and the HUD (layer 4) stays sharp.
- **Ruling: the VTX transmits in open loop.** The spec's open-loop tests need a transmitting VTX, and a powered VTX transmits whether or not a flight controller talks to it. So an open-loop quad with `[vtx]` gets the VTX model (power-up channel and power, no SmartAudio traffic) and the link. M3a tests that expected no VTX in open loop change in Task 4 (Rust server and client, Python, Godot e2e). Cost if wrong: open-loop sessions show a VTX line and a link; reverting is one `if` in `vehicle::build`.
- **Ruling: the obstruction is found by a search, not by 32 samples.** A golden-section search on the convex shape's signed distance finds the deepest point of the path exactly, so a thin post on a long path is never stepped over (the spec's risk table named this). Test: `a_thin_object_on_a_long_path_is_still_found`.
- **Ruling: objects standing on the ground are rooted below it** (`Shape::rooted`). Without it the nearest face of a building to a low path is its bottom face, so the knife edge measured under the building (16.8 dB instead of 25 for a path 1.7 m up).
- **Ruling: emitter frequencies may be 5300 to 6000 MHz** (spec: 5600 to 6000), because Lowband (5362 to 5621 MHz) is one of the VTX's own bands.
- **Ruling: the open-loop sync-loss test is at 4 km and 25 mW** (spec: 2 km): with the shipped 8 dBi patch, 25 mW at 2 km is marginal (SNR about 0 dB) rather than reliably lost.
- **Ruling: protocol details.** `VideoSync` has `VIDEO_SYNC_UNSPECIFIED = 0` (proto3 needs a zero value; it means "no video link"), then `LOCKED`, `UNSTABLE`, `LOST`. The bus gains `video.present` (not in the spec's list; `VehicleState` needs it). World kinds travel as strings ("omni", "patch", "rhcp", "box", ...), which Python and GDScript read directly.
- **Ruling: "Video effects" is saved in `user://ofs_client.cfg`** (`[client] video_effects`), the file the other client settings come from, by a new `AppSettings.save_value`; the box sits on the F2 controls screen.
- **The world file reproduces the old scenery.** `worlds/flat.toml` carries every object `godot/world/world.gd` drew (same positions, sizes, colours; a test checks building A lands at Godot `(-45, 7, -110)` size `(14, 14, 12)`).

## How to run things (Windows host, Git Bash)

- Rust tests: `cargo test -p <crate> --locked` from the repository root; the whole workspace: `cargo test --workspace --locked`.
- Live SITL tests run in WSL (Windows Firewall blocks freshly built test executables that must receive SITL datagrams). Define once per shell:

```bash
wsl_run() { wsl.exe -d Ubuntu -e bash -lc "cd /mnt$(pwd) && export CARGO_TARGET_DIR=\$HOME/ofs/target OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf && $*"; }
```

  Example: `wsl_run cargo test -p ofs-fc -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`. (`wsl.exe` prints a harmless `.wslconfig` warning.)
- Python tests run on Windows: `cargo build -p ofs-sim` then `python -m pytest python/tests -q`. The SITL ones need `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf"` and an `ofs-sim.exe` the firewall allows (see `docs/dev-setup.md`, "Troubleshooting (Windows)"; `OFS_SIM_BIN` points at it).
- Godot tests: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh [unit|e2e|all]` (it builds `ofs-sim` and `ofs-godot` into `target/` first; Godot loads the extension from `target/debug`, so keep `CARGO_TARGET_DIR` at its default for these).
- Regenerate the Python stubs after a proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`.

## Review Focus

Failure modes the spec implies but a straight reading of the tasks would not cover; each has a pinning test in the named task.

1. **A world file with mistakes** (a duplicate antenna or object name, a patch without a beamwidth, a box without a size, a cylinder with one, NaN or infinity, a pilot below the ground, an emitter with no frequency or two, an unknown kind, a newer schema): one config error at load naming every problem, the session not loaded. Tests: Task 3 (`world.rs`), Task 5 (`a_bad_world_path_is_a_config_error_and_loads_nothing`), Task 7 (`a_missing_world_file_fails_with_a_config_error`).
2. **The quad at odd places**: exactly at the goggles (zero distance), inside a building, below the ground plane in a crash, very far away. The link must stay finite (the simulator stops on any non-finite signal). Test: Task 2 (`odd_positions_give_finite_values`).
3. **A quad without `[vtx]`** (an M2-era or schema-2 quad file): no video link, `State.video.present = false`, a clean picture in the game, nothing crashes. Tests: Task 4 (`a_quad_without_a_vtx_has_no_video_link`), Task 5 (`the_video_message_says_absent_without_a_vtx`), Task 9 (`test_the_layer_draws_only_over_a_degraded_fpv_picture`).
4. **Reloads and world changes**: a reload fetches the world again, a load without a world goes back to the open field, the game's field is rebuilt without leftovers. Tests: Task 5 (`a_session_flies_in_the_world_it_was_loaded_with`), Task 7 (`the_world_of_the_settings_arrives_with_the_load_and_again_after_a_reload`), Task 8 (`test_a_new_world_replaces_the_old_one`).
5. **The VTX changed by Betaflight mid-flight** (power, pit mode, channel from the OSD menu or the Configurator): the link follows within a second, `video_lost`/`video_restored` mark sync changes. Tests: Task 2 (pit mode, no VTX), Task 5 (the events), Task 6 (live, against Betaflight).

## File Structure

Create:
- `crates/ofs-video/src/propagation.rs` — pure propagation functions and the `Antenna`, `Shape`, `Obstacle`, `Endpoint` types.
- `crates/ofs-video/src/link.rs` — the receiver (picture, sync, diversity, fading) and the `VideoLink` model.
- `crates/ofs-video/tests/{propagation,link}.rs`.
- `crates/ofs-config/src/world.rs`, `crates/ofs-config/tests/world.rs`, `worlds/flat.toml`.
- `godot/ui/video.gd`, `godot/ui/video.gdshader`, `godot/tests/test_video.gd` (+ their `.uid` files).
- `docs/research/video-link.md`, `docs/superpowers/m3b-carried-debt.md`.

Modify: `crates/ofs-video/{Cargo.toml,src/lib.rs}`, `Cargo.lock`, `crates/ofs-core/src/names.rs`, `crates/ofs-config/src/lib.rs`, `quads/opendrone-5f-freestyle.toml`, `crates/ofs-sim/src/{vehicle,session,server}.rs`, `crates/ofs-sim/tests/{open_loop_vehicle,sitl_live,grpc}.rs`, `proto/ofs/v1/sim.proto`, `crates/ofs-proto/{src/lib.rs,tests/messages.rs}`, `python/ofs/{client.py,__init__.py}` and the regenerated `python/ofs/v1/*`, `python/tests/{conftest,test_client,test_sitl_video}.py`, `crates/ofs-client/src/{model,worker,client}.rs`, `crates/ofs-client/tests/{model,session}.rs`, `crates/ofs-godot/src/lib.rs`, `godot/{main.tscn,scripts/app.gd,scripts/settings.gd,world/world.gd,ui/hud.gd,ui/controls_menu.gd,ui/osd.gd}`, `godot/tests/{run_tests,test_scene,test_settings,test_extension,test_hud,test_controls_menu,e2e_open_loop,e2e_betaflight,shots}.gd`, `docs/dev-setup.md`, `README.md`.

---

### Task 1: Propagation maths (`ofs-video::propagation`)

**Files:**
- Create: `crates/ofs-video/src/propagation.rs`, `crates/ofs-video/tests/propagation.rs`
- Modify: `crates/ofs-video/Cargo.toml`, `crates/ofs-video/src/lib.rs`, `Cargo.lock`

**Interfaces:**
- Consumes: nothing new (glam's `DVec3`).
- Produces (used by Tasks 2 and 4), all in `ofs_video::propagation`:
  - `pub fn wavelength_m(freq_mhz: f64) -> f64`, `pub fn fspl_db(distance_m: f64, freq_mhz: f64) -> f64`, `pub fn mw_to_dbm(mw: f64) -> f64`, `pub fn dbm_to_mw(dbm: f64) -> f64`, `pub fn power_sum_dbm(dbm: impl IntoIterator<Item = f64>) -> f64`
  - `pub enum Polarization { Rhcp, Lhcp, Linear }` (`reflected()`), `pub enum AntennaKind { Omni, Patch { beamwidth_deg: f64 } }`, `pub struct Antenna { kind, gain_dbi, polarization, axis: DVec3 }` (`gain_towards(dir)`, `field_direction()`)
  - `pub fn polarization_loss_db(tx: &Antenna, tx_polarization: Polarization, rx: &Antenna, path: DVec3) -> f64`, `pub fn knife_edge_loss_db(v: f64) -> f64`, `pub fn fresnel_v(h, d1, d2, wavelength_m) -> f64`
  - `pub enum Shape { Box { center: DVec3, half: DVec3 }, Cylinder { center: DVec3, radius: f64, half_height: f64 } }` (`signed_distance(p)`, `rooted()`), `pub struct Obstacle { shape: Shape, rf_loss_db: f64 }`, `pub fn obstruction_loss_db(&Obstacle, a, b, wavelength_m) -> f64`, `pub fn deepest_point(&Shape, a, b) -> f64`
  - `pub struct Endpoint { position: DVec3, antenna: Antenna }`, `pub struct PathGain { gain_db: f64, obstruction_db: f64 }`, `pub fn path_gain(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, obstacles: &[Obstacle], ground_bounce: bool) -> PathGain`
  - `pub fn adjacent_channel_rejection_db(offset_mhz: f64) -> f64`
  - Constants `SPEED_OF_LIGHT_MPS`, `PATTERN_FLOOR_DB` (20), `CROSS_POLARIZATION_DB` (20), `CIRCULAR_TO_LINEAR_DB` (3), `MIN_DISTANCE_M` (1).

Positions are NED metres (down is +z; the ground is z = 0). Every function is pure: no bus, no state, so the ELRS link can adopt them later.

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-video/tests/propagation.rs`:

```rust
use glam::DVec3;
use ofs_video::propagation::*;

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

fn omni(axis: DVec3, polarization: Polarization) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization, axis }
}

#[test]
fn free_space_path_loss_matches_the_formula() {
    close(fspl_db(100.0, 5800.0), 87.72, 0.01, "100 m at 5800 MHz");
    close(fspl_db(1000.0, 5800.0) - fspl_db(100.0, 5800.0), 20.0, 1e-9, "20 dB per decade");
    close(fspl_db(0.1, 5800.0), fspl_db(1.0, 5800.0), 1e-12, "clamped at 1 m");
    close(wavelength_m(5800.0), 0.05169, 1e-5, "wavelength");
}

#[test]
fn power_helpers_round_trip() {
    close(mw_to_dbm(25.0), 13.98, 0.01, "25 mW");
    close(dbm_to_mw(30.0), 1000.0, 1e-9, "30 dBm");
    close(power_sum_dbm([-90.0, -90.0]), -86.99, 0.01, "two equal powers add 3 dB");
}

#[test]
fn an_omni_peaks_across_its_axis_and_nulls_along_it() {
    let a = omni(DVec3::NEG_Z, Polarization::Rhcp);
    close(a.gain_towards(DVec3::X), 2.0, 1e-9, "broadside");
    close(a.gain_towards(DVec3::NEG_Z), 2.0 - PATTERN_FLOOR_DB, 1e-9, "along the axis: the floor");
    close(a.gain_towards(DVec3::new(1.0, 0.0, -1.0).normalize()), 2.0 - 3.0103, 0.001, "45 degrees off the axis: sin^2");
}

#[test]
fn a_patch_is_3_db_down_at_half_its_beamwidth_and_floored_behind() {
    let a = Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Rhcp, axis: DVec3::X };
    close(a.gain_towards(DVec3::X), 8.0, 1e-9, "boresight");
    let half = 30f64.to_radians();
    close(a.gain_towards(DVec3::new(half.cos(), half.sin(), 0.0)), 8.0 - 3.0103, 1e-4, "half power at 30 degrees");
    close(a.gain_towards(DVec3::NEG_X), 8.0 - PATTERN_FLOOR_DB, 1e-9, "behind");
    close(a.gain_towards(DVec3::Y), 8.0 - PATTERN_FLOOR_DB, 1e-9, "at 90 degrees");
}

#[test]
fn polarization_losses_follow_the_table() {
    let path = DVec3::X;
    let rhcp = omni(DVec3::NEG_Z, Polarization::Rhcp);
    let lhcp = omni(DVec3::NEG_Z, Polarization::Lhcp);
    let vertical = omni(DVec3::NEG_Z, Polarization::Linear);
    let horizontal = omni(DVec3::Y, Polarization::Linear);
    let tilted = omni(DVec3::new(0.0, 1.0, -1.0).normalize(), Polarization::Linear);
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &rhcp, path), 0.0, 1e-12, "same hand");
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &lhcp, path), CROSS_POLARIZATION_DB, 1e-12, "opposite hands");
    close(polarization_loss_db(&rhcp, Polarization::Rhcp, &vertical, path), CIRCULAR_TO_LINEAR_DB, 1e-12, "circular to linear");
    close(polarization_loss_db(&vertical, Polarization::Linear, &vertical, path), 0.0, 1e-9, "aligned dipoles");
    close(polarization_loss_db(&vertical, Polarization::Linear, &tilted, path), 3.0103, 0.001, "45 degrees apart");
    close(polarization_loss_db(&vertical, Polarization::Linear, &horizontal, path), CROSS_POLARIZATION_DB, 1e-9, "crossed: capped");
    assert_eq!(Polarization::Rhcp.reflected(), Polarization::Lhcp);
    assert_eq!(Polarization::Linear.reflected(), Polarization::Linear);
}

#[test]
fn knife_edge_loss_matches_itu_r_p526() {
    close(knife_edge_loss_db(0.0), 6.03, 0.01, "grazing: about 6 dB");
    close(knife_edge_loss_db(-0.78), 0.0, 1e-12, "clear");
    close(knife_edge_loss_db(-2.0), 0.0, 1e-12, "well clear");
    close(knife_edge_loss_db(1.0), 13.96, 0.05, "v = 1");
    close(knife_edge_loss_db(2.4), 20.6, 0.1, "v = 2.4");
}

#[test]
fn signed_distances_of_the_shapes() {
    let b = Shape::Box { center: DVec3::ZERO, half: DVec3::new(2.0, 1.0, 1.0) };
    close(b.signed_distance(DVec3::new(5.0, 0.0, 0.0)), 3.0, 1e-12, "outside along north");
    close(b.signed_distance(DVec3::ZERO), -1.0, 1e-12, "the centre is 1 m from the nearest face");
    let c = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -5.0), radius: 1.0, half_height: 5.0 };
    close(c.signed_distance(DVec3::new(3.0, 0.0, -5.0)), 2.0, 1e-12, "beside it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -12.0)), 2.0, 1e-12, "above it");
    close(c.signed_distance(DVec3::new(0.0, 0.0, -5.0)), -1.0, 1e-12, "on its axis");
}

#[test]
fn a_shape_standing_on_the_ground_is_rooted_below_it_and_a_floating_one_is_not() {
    let wall = Shape::Box { center: DVec3::new(0.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    let p = DVec3::new(0.0, 0.0, -1.0); // inside the wall, 1 m above the ground and 5 m from its sides
    close(wall.signed_distance(p), -1.0, 1e-12, "unrooted: the bottom face is nearest");
    close(wall.rooted().signed_distance(p), -5.0, 1e-9, "rooted: the side faces are");
    close(wall.rooted().signed_distance(DVec3::new(0.0, 0.0, -25.0)), 5.0, 1e-9, "the top stays where it was");
    let bar = Shape::Box { center: DVec3::new(0.0, 0.0, -2.5), half: DVec3::new(0.1, 1.6, 0.06) };
    assert_eq!(bar.rooted(), bar, "a gate's top bar floats: unchanged");
    let pylon = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -1.5), radius: 0.15, half_height: 1.5 };
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -4.0)), 1.0, 1e-9, "a rooted cylinder keeps its top");
    close(pylon.rooted().signed_distance(DVec3::new(0.0, 0.0, -0.5)), -0.15, 1e-9, "and its radius");
}

#[test]
fn a_building_on_the_path_costs_its_loss_and_a_clear_one_costs_nothing() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) }.rooted(), rf_loss_db: 25.0 };
    let lambda = wavelength_m(5800.0);
    let (a, b) = (DVec3::new(0.0, 0.0, -2.0), DVec3::new(100.0, 0.0, -2.0));
    close(obstruction_loss_db(&building, a, b, lambda), 25.0, 1e-9, "straight through: capped at rf_loss_db");
    let beside = DVec3::new(100.0, 20.0, -2.0);
    let past = DVec3::new(0.0, 20.0, -2.0);
    close(obstruction_loss_db(&building, past, beside, lambda), 0.0, 1e-9, "15 m clear of it");
    let transparent = Obstacle { rf_loss_db: 0.0, ..building };
    close(obstruction_loss_db(&transparent, a, b, lambda), 0.0, 1e-12, "a 0 dB object never blocks");
}

#[test]
fn the_loss_fades_in_over_a_few_metres_at_a_buildings_edge() {
    // The receiver is 100 m south of a 10 m wide building; the quad moves east behind it, 100 m north of it.
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) }.rooted(), rf_loss_db: 30.0 };
    let lambda = wavelength_m(5800.0);
    let rx = DVec3::new(0.0, 0.0, -2.0);
    let loss_at = |east: f64| obstruction_loss_db(&building, rx, DVec3::new(200.0, east, -2.0), lambda);
    let losses: Vec<f64> = [30.0, 14.0, 10.0, 6.0, 0.0].iter().map(|e| loss_at(*e)).collect();
    assert!(losses[0] < 0.5, "well clear: {losses:?}");
    assert!(losses.windows(2).all(|w| w[1] >= w[0]), "rises steadily as the path goes in: {losses:?}");
    assert!(losses[2] > 3.0 && losses[2] < 9.0, "the line just grazes the edge: about 6 dB: {losses:?}");
    // Directly behind, the path is 5 m from the nearest side: v = 4.4, J(v) = 25.7 dB, under the 30 dB cap.
    close(losses[4], 25.70, 0.01, "fully behind: diffraction around the sides");
}

#[test]
fn a_thin_object_on_a_long_path_is_still_found() {
    // A 0.5 m post exactly between two ends 4 km apart: a fixed sampling would step over it.
    let post = Obstacle { shape: Shape::Box { center: DVec3::new(2000.0, 0.0, -5.0), half: DVec3::new(0.25, 0.25, 5.0) }.rooted(), rf_loss_db: 10.0 };
    let loss = obstruction_loss_db(&post, DVec3::new(0.0, 0.0, -2.0), DVec3::new(4000.0, 0.0, -2.0), wavelength_m(5800.0));
    assert!(loss > 5.0, "the post is in the way: {loss}");
}

#[test]
fn circular_antennas_reject_most_of_the_ground_bounce() {
    let freq = 5800.0;
    let spread = |polarization: Polarization| {
        let mut gains = Vec::new();
        for i in 0..400 {
            let d = 100.0 + f64::from(i) * 0.5;
            let tx = Endpoint { position: DVec3::new(d, 0.0, -10.0), antenna: omni(DVec3::NEG_Z, polarization) };
            let rx = Endpoint { position: DVec3::new(0.0, 0.0, -1.7), antenna: omni(DVec3::NEG_Z, polarization) };
            let with = path_gain(&tx, &rx, freq, &[], true).gain_db;
            let without = path_gain(&tx, &rx, freq, &[], false).gain_db;
            gains.push(with - without);
        }
        let max = gains.iter().cloned().fold(f64::MIN, f64::max);
        let min = gains.iter().cloned().fold(f64::MAX, f64::min);
        (max, min)
    };
    let (circular_max, circular_min) = spread(Polarization::Rhcp);
    let (linear_max, linear_min) = spread(Polarization::Linear);
    assert!(circular_max < 1.5 && circular_min > -1.5, "circular: a ripple under 1.5 dB ({circular_min}..{circular_max})");
    assert!(linear_min < -10.0 && linear_max > 4.0, "linear: deep fades and peaks ({linear_min}..{linear_max})");
}

#[test]
fn the_direct_path_adds_gains_and_subtracts_the_path_loss() {
    let tx = Endpoint { position: DVec3::new(100.0, 0.0, -2.0), antenna: omni(DVec3::NEG_Z, Polarization::Rhcp) };
    let rx = Endpoint { position: DVec3::new(0.0, 0.0, -2.0), antenna: omni(DVec3::NEG_Z, Polarization::Rhcp) };
    let p = path_gain(&tx, &rx, 5800.0, &[], false);
    close(p.gain_db, 2.0 + 2.0 - fspl_db(100.0, 5800.0), 1e-9, "two broadside omnis");
    assert_eq!(p.obstruction_db, 0.0);
}

#[test]
fn adjacent_channel_rejection_follows_its_curve() {
    close(adjacent_channel_rejection_db(0.0), 0.0, 1e-12, "same channel");
    close(adjacent_channel_rejection_db(10.0), 5.0, 1e-12, "halfway to 20 MHz");
    close(adjacent_channel_rejection_db(-20.0), 10.0, 1e-12, "either side");
    close(adjacent_channel_rejection_db(40.0), 25.0, 1e-12, "40 MHz");
    close(adjacent_channel_rejection_db(37.0), 22.75, 1e-12, "37 MHz (the next Raceband channel)");
    close(adjacent_channel_rejection_db(500.0), 40.0, 1e-12, "far away");
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-video --test propagation`
Expected: FAIL: does not compile, ``unresolved import `ofs_video::propagation` ``.

- [ ] **Step 3: Add the dependencies and the module**

In `crates/ofs-video/Cargo.toml`, replace:

```toml

[dependencies]
ofs-core.workspace = true
ofs-fc.workspace = true

[dev-dependencies]
glam.workspace = true
tempfile.workspace = true
```

with:

```toml

[dependencies]
glam.workspace = true
ofs-core.workspace = true
ofs-fc.workspace = true
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

In `crates/ofs-video/src/lib.rs`, replace:

```rust
//! Video-side models: the SmartAudio VTX and Betaflight's OSD over MSP DisplayPort.
pub mod osd;
pub mod smartaudio;
```

with:

```rust
//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles.
pub mod osd;
pub mod propagation;
pub mod smartaudio;
```

(`rand`, `rand_chacha` and `rand_distr` are for Task 2's fading; adding them now keeps the dependency change in one task.)

- [ ] **Step 4: Write the propagation module**

Create `crates/ofs-video/src/propagation.rs`:

```rust
//! Radio propagation as pure functions: free-space path loss, antenna patterns, polarization, knife-edge
//! diffraction around obstacles and the two-ray ground bounce. No bus and no state, so the video link uses them
//! today and the ELRS link can adopt them later. Positions and directions are NED metres (down is +z, the ground
//! is the plane z = 0).
use glam::DVec3;

pub const SPEED_OF_LIGHT_MPS: f64 = 299_792_458.0;
/// Antenna patterns never fall more than this below their peak (real nulls are filled by reflections).
pub const PATTERN_FLOOR_DB: f64 = 20.0;
/// Loss between opposite-hand circular antennas, and the cap of a crossed linear pair (estimated).
pub const CROSS_POLARIZATION_DB: f64 = 20.0;
/// Loss between a circular and a linear antenna (half the power is in the other polarization).
pub const CIRCULAR_TO_LINEAR_DB: f64 = 3.0;
/// Shortest distance used by the path loss and the Fresnel geometry (the far-field formulas fail closer in).
pub const MIN_DISTANCE_M: f64 = 1.0;

pub fn wavelength_m(freq_mhz: f64) -> f64 {
    SPEED_OF_LIGHT_MPS / (freq_mhz * 1e6)
}

/// Free-space path loss in dB: `20 log10(d) + 20 log10(f) - 27.55` (d in metres, f in MHz), d at least 1 m.
pub fn fspl_db(distance_m: f64, freq_mhz: f64) -> f64 {
    20.0 * distance_m.max(MIN_DISTANCE_M).log10() + 20.0 * freq_mhz.log10() - 27.55
}

pub fn mw_to_dbm(mw: f64) -> f64 {
    10.0 * mw.log10()
}

pub fn dbm_to_mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

/// The sum of powers given in dBm, in dBm.
pub fn power_sum_dbm(dbm: impl IntoIterator<Item = f64>) -> f64 {
    mw_to_dbm(dbm.into_iter().map(dbm_to_mw).sum::<f64>())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarization {
    Rhcp,
    Lhcp,
    Linear,
}

impl Polarization {
    /// A reflection off the ground reverses the hand of a circular wave.
    pub fn reflected(self) -> Polarization {
        match self {
            Polarization::Rhcp => Polarization::Lhcp,
            Polarization::Lhcp => Polarization::Rhcp,
            Polarization::Linear => Polarization::Linear,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AntennaKind {
    /// A dipole-like doughnut around its axis.
    Omni,
    /// A directional antenna: a main lobe around its boresight, `beamwidth_deg` wide at -3 dB.
    Patch { beamwidth_deg: f64 },
}

/// An antenna placed in the world: `axis` is the omni's axis or the patch's boresight (a unit vector, NED).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Antenna {
    pub kind: AntennaKind,
    pub gain_dbi: f64,
    pub polarization: Polarization,
    pub axis: DVec3,
}

impl Antenna {
    /// Gain in dBi towards the unit vector `dir` (from this antenna to the other end).
    pub fn gain_towards(&self, dir: DVec3) -> f64 {
        let cos_theta = self.axis.dot(dir).clamp(-1.0, 1.0);
        let relative = match self.kind {
            AntennaKind::Omni => {
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                20.0 * sin_theta.max(1e-12).log10()
            }
            AntennaKind::Patch { beamwidth_deg } => {
                if cos_theta <= 0.0 {
                    -PATTERN_FLOOR_DB
                } else {
                    10.0 * patch_exponent(beamwidth_deg) * cos_theta.log10()
                }
            }
        };
        self.gain_dbi + relative.max(-PATTERN_FLOOR_DB)
    }

    /// The direction of a linear antenna's electric field: an omni's axis, or for a patch the world's up made
    /// square to its boresight (a vertically polarized patch).
    pub fn field_direction(&self) -> DVec3 {
        match self.kind {
            AntennaKind::Omni => self.axis,
            AntennaKind::Patch { .. } => {
                let up = DVec3::NEG_Z;
                let across = up - self.axis * up.dot(self.axis);
                across.try_normalize().unwrap_or(DVec3::X)
            }
        }
    }
}

/// The exponent n of a `cos^n` main lobe that is 3 dB down at half the beamwidth.
pub fn patch_exponent(beamwidth_deg: f64) -> f64 {
    let half = (beamwidth_deg * 0.5).to_radians();
    0.5f64.ln() / half.cos().ln()
}

/// Polarization mismatch loss in dB between a transmitting and a receiving antenna along the unit vector `path`.
pub fn polarization_loss_db(tx: &Antenna, tx_polarization: Polarization, rx: &Antenna, path: DVec3) -> f64 {
    use Polarization::*;
    match (tx_polarization, rx.polarization) {
        (Rhcp, Rhcp) | (Lhcp, Lhcp) => 0.0,
        (Rhcp, Lhcp) | (Lhcp, Rhcp) => CROSS_POLARIZATION_DB,
        (Linear, Linear) => {
            let across = |v: DVec3| (v - path * v.dot(path)).try_normalize();
            match (across(tx.field_direction()), across(rx.field_direction())) {
                (Some(a), Some(b)) => (-20.0 * a.dot(b).abs().max(1e-12).log10()).min(CROSS_POLARIZATION_DB),
                // A field along the path: the pattern's null already accounts for it.
                _ => 0.0,
            }
        }
        _ => CIRCULAR_TO_LINEAR_DB,
    }
}

/// ITU-R P.526 single knife-edge diffraction loss J(v) in dB (0 for a clear path, v <= -0.78).
pub fn knife_edge_loss_db(v: f64) -> f64 {
    if v <= -0.78 {
        0.0
    } else {
        6.9 + 20.0 * (((v - 0.1).powi(2) + 1.0).sqrt() + v - 0.1).log10()
    }
}

/// The Fresnel-Kirchhoff parameter v of an edge `h` metres into the path (negative: clear by `-h`), `d1` and
/// `d2` metres from the two ends.
pub fn fresnel_v(h: f64, d1: f64, d2: f64, wavelength_m: f64) -> f64 {
    let (d1, d2) = (d1.max(MIN_DISTANCE_M), d2.max(MIN_DISTANCE_M));
    h * (2.0 * (d1 + d2) / (wavelength_m * d1 * d2)).sqrt()
}

/// A solid in the world, by its signed distance (negative inside).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Axis-aligned box: `half` is half the size along north, east and down.
    Box { center: DVec3, half: DVec3 },
    /// Vertical cylinder.
    Cylinder { center: DVec3, radius: f64, half_height: f64 },
}

/// How far below the ground a rooted shape reaches (any depth works: nothing travels under the ground).
const ROOT_DEPTH_M: f64 = 1000.0;
/// A shape whose bottom is within this of the ground stands on it.
const ON_GROUND_M: f64 = 0.05;

impl Shape {
    /// The shape as an obstacle: one that stands on the ground continues below it, so a signal diffracts over its
    /// top and around its sides, never underneath (its nearest face is never the bottom one).
    pub fn rooted(self) -> Shape {
        match self {
            Shape::Box { center, half } if center.z + half.z >= -ON_GROUND_M => {
                let top = center.z - half.z;
                let half_z = (ROOT_DEPTH_M - top) * 0.5;
                Shape::Box { center: DVec3::new(center.x, center.y, top + half_z), half: DVec3::new(half.x, half.y, half_z) }
            }
            Shape::Cylinder { center, radius, half_height } if center.z + half_height >= -ON_GROUND_M => {
                let top = center.z - half_height;
                let half_z = (ROOT_DEPTH_M - top) * 0.5;
                Shape::Cylinder { center: DVec3::new(center.x, center.y, top + half_z), radius, half_height: half_z }
            }
            other => other,
        }
    }

    pub fn signed_distance(&self, p: DVec3) -> f64 {
        match *self {
            Shape::Box { center, half } => {
                let q = (p - center).abs() - half;
                q.max(DVec3::ZERO).length() + q.max_element().min(0.0)
            }
            Shape::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let radial = (d.x * d.x + d.y * d.y).sqrt() - radius;
                let vertical = d.z.abs() - half_height;
                let outside = (radial.max(0.0).powi(2) + vertical.max(0.0).powi(2)).sqrt();
                outside + radial.max(vertical).min(0.0)
            }
        }
    }
}

/// An object that weakens a signal passing through or near it, by up to `rf_loss_db`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub shape: Shape,
    pub rf_loss_db: f64,
}

/// Golden-section search steps: the bracket shrinks to 0.618^60 of the path, far below a millimetre.
const SEARCH_STEPS: usize = 60;

/// The point of the segment `a`..`b` that goes deepest into (or passes closest to) `shape`, as its fraction t of
/// the way from `a`. A convex shape's signed distance is convex along a line, so a golden-section search finds the
/// minimum exactly, however thin the object and however long the path.
pub fn deepest_point(shape: &Shape, a: DVec3, b: DVec3) -> f64 {
    let f = |t: f64| shape.signed_distance(a.lerp(b, t));
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..SEARCH_STEPS {
        let m1 = hi - ratio * (hi - lo);
        let m2 = lo + ratio * (hi - lo);
        if f(m1) < f(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    (lo + hi) * 0.5
}

/// Diffraction loss of one obstacle on the path `a`..`b`, capped at the obstacle's `rf_loss_db`.
pub fn obstruction_loss_db(obstacle: &Obstacle, a: DVec3, b: DVec3, wavelength_m: f64) -> f64 {
    if obstacle.rf_loss_db <= 0.0 {
        return 0.0;
    }
    let t = deepest_point(&obstacle.shape, a, b);
    let length = a.distance(b);
    let depth = -obstacle.shape.signed_distance(a.lerp(b, t));
    let v = fresnel_v(depth, t * length, (1.0 - t) * length, wavelength_m);
    knife_edge_loss_db(v).min(obstacle.rf_loss_db)
}

/// One end of a radio path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Endpoint {
    pub position: DVec3,
    pub antenna: Antenna,
}

/// What a path does to a signal, in dB (gains positive, losses negative in `gain_db`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathGain {
    /// Antenna gains, path loss, polarization, ground bounce and obstruction together.
    pub gain_db: f64,
    /// The obstruction part alone (>= 0), for the fading model.
    pub obstruction_db: f64,
}

fn unit(v: DVec3) -> DVec3 {
    v.try_normalize().unwrap_or(DVec3::X)
}

/// Gain of one ray from `tx` to `rx` that leaves `tx` along `out_dir` and arrives travelling along `in_dir`.
fn ray_gain_db(tx: &Endpoint, tx_polarization: Polarization, rx: &Endpoint, out_dir: DVec3, in_dir: DVec3, length_m: f64, freq_mhz: f64) -> f64 {
    tx.antenna.gain_towards(out_dir) + rx.antenna.gain_towards(-in_dir) - fspl_db(length_m, freq_mhz)
        - polarization_loss_db(&tx.antenna, tx_polarization, &rx.antenna, in_dir)
}

/// The path from `tx` to `rx` at `freq_mhz`: the direct ray, plus (with `ground_bounce`) the ray reflected off the
/// ground (coefficient -1, which reverses circular polarization), added with their phase difference; then the
/// diffraction losses of the obstacles on the direct ray.
pub fn path_gain(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, obstacles: &[Obstacle], ground_bounce: bool) -> PathGain {
    let lambda = wavelength_m(freq_mhz);
    let direct_vec = rx.position - tx.position;
    let direct_len = direct_vec.length();
    let dir = unit(direct_vec);
    let direct_db = ray_gain_db(tx, tx.antenna.polarization, rx, dir, dir, direct_len, freq_mhz);
    let mut gain_db = direct_db;
    // Both ends above the ground (z < 0): the image of tx below the ground gives the reflected ray.
    if ground_bounce && tx.position.z < 0.0 && rx.position.z < 0.0 {
        let image = DVec3::new(tx.position.x, tx.position.y, -tx.position.z);
        let reflected_vec = rx.position - image;
        let reflected_len = reflected_vec.length();
        let t = -image.z / (rx.position.z - image.z); // where the image-to-rx line crosses z = 0
        let bounce = image.lerp(rx.position, t);
        let out_dir = unit(bounce - tx.position);
        let in_dir = unit(rx.position - bounce);
        let reflected_db = ray_gain_db(tx, tx.antenna.polarization.reflected(), rx, out_dir, in_dir, reflected_len, freq_mhz);
        let (a, b) = (10f64.powf(direct_db / 20.0), 10f64.powf(reflected_db / 20.0));
        let phase = 2.0 * std::f64::consts::PI * (reflected_len - direct_len) / lambda + std::f64::consts::PI;
        let power = a * a + b * b + 2.0 * a * b * phase.cos();
        gain_db = 10.0 * power.max(1e-30).log10();
    }
    let obstruction_db: f64 = obstacles.iter().map(|o| obstruction_loss_db(o, tx.position, rx.position, lambda)).sum();
    PathGain { gain_db: gain_db - obstruction_db, obstruction_db }
}

/// Receiver rejection of a transmitter `offset_mhz` away from the tuned channel (estimated for analog 5.8 GHz
/// receivers): 0 dB on channel, 10 dB at 20 MHz, 25 dB at 40 MHz, 40 dB at 60 MHz and beyond, linear in between.
pub fn adjacent_channel_rejection_db(offset_mhz: f64) -> f64 {
    const POINTS: [(f64, f64); 4] = [(0.0, 0.0), (20.0, 10.0), (40.0, 25.0), (60.0, 40.0)];
    let x = offset_mhz.abs();
    for pair in POINTS.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    POINTS[POINTS.len() - 1].1
}
```

Notes for the reader:
- `deepest_point` uses a golden-section search: a convex shape's signed distance is convex along a line, so the search finds the true minimum (the deepest point of the path in the object, or its closest pass) however thin the object is.
- `rooted` makes an object that stands on the ground continue below it. Without it the nearest face of a building to a path 1.7 m up is its bottom face, and the knife edge would be measured under the building.
- The ground bounce flips circular polarization (`Polarization::reflected`), which is why circular antennas reject it.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p ofs-video --test propagation`
Expected: PASS, 14 tests (this first run updates `Cargo.lock`; it has no `--locked` on purpose).

Run: `cargo test -p ofs-video --locked`
Expected: every ofs-video test passes (the M3a ones too).

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/ofs-video/Cargo.toml crates/ofs-video/src/lib.rs crates/ofs-video/src/propagation.rs crates/ofs-video/tests/propagation.rs
git commit -m "feat(video): 5.8 GHz propagation: path loss, antennas, polarization, knife edge, ground bounce"
```

### Task 2: The video link model (`ofs-video::link`)

**Files:**
- Create: `crates/ofs-video/src/link.rs`, `crates/ofs-video/tests/link.rs`
- Modify: `crates/ofs-core/src/names.rs`, `crates/ofs-video/src/lib.rs`

**Interfaces:**
- Consumes: Task 1's `propagation` API; M3a's bus signals `vtx.present`, `vtx.freq_mhz`, `vtx.power_mw`, `vtx.pit_mode`; the physics' `body.pos_ned_m`, `body.att_q`.
- Produces (used by Tasks 4 and 5):
  - Signal names in `ofs_core::names`: `VIDEO_PRESENT` ("video.present"), `VIDEO_SNR`, `VIDEO_ANTENNA`, `VIDEO_INTERFERENCE`, `VIDEO_NOISE`, `VIDEO_SPARKLES`, `VIDEO_CHROMA`, `VIDEO_SYNC` (0 locked, 1 unstable, 2 lost) and `pub fn video_rssi(antenna: &str) -> String` ("video.rssi_dbm.<name>").
  - In `ofs_video::link`: `pub const MODEL_NAME` ("video.link"), `FIELD_RATE_HZ` (50), `NO_SIGNAL_DBM` (-150), `BODY_SHADOW_DB`, the receiver thresholds (`CLEAN_SNR_DB` 25, `SPARKLE_SNR_DB` 12, `SPARKLE_FULL_SNR_DB` 4, `COLOR_FADE_SNR_DB` 8, `COLOR_LOST_SNR_DB` 5, `UNSTABLE_SNR_DB` 6, `LOST_SNR_DB` 3, `LOST_AFTER_FIELDS` 3, `RELOCK_AFTER_FIELDS` 5), `DIVERSITY_HYSTERESIS_DB` (2), `LOS_K_DB` (10).
  - `pub enum VideoSync { Locked = 0, Unstable = 1, Lost = 2 }` (`from_signal(f64)`), `pub struct Picture { noise, sparkles, chroma }`, `pub fn picture(snr_db) -> Picture`, `pub struct SyncTracker` (`new()`, `update(snr_db) -> VideoSync`), `pub struct Diversity` (`choose(&[f64]) -> usize`), `pub fn snr_db(signal_dbm, noise_floor_dbm, interference_dbm) -> f64`, `pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, receiver_pos: DVec3) -> f64`.
  - `pub struct ReceiverAntenna { name: String, antenna: Antenna }`, `pub struct Emitter { position, freq_mhz, power_mw, antenna }`, `pub struct LinkWorld { pilot_position, antennas, noise_floor_dbm, diversity, obstacles, emitters }`, `pub struct LinkParams { world: LinkWorld, vtx_antenna: Antenna /* axis in body FRD */, pit_power_mw: f64, fading: bool, ground_bounce: bool }`.
  - `pub fn vtx_paths(&LinkParams, quad_pos, att, freq_mhz) -> Vec<PathGain>`, `pub fn interference_dbm(&LinkWorld, freq_mhz, ground_bounce) -> Vec<f64>`.
  - `pub struct VideoLink` (a `Model`): `VideoLink::new(params: LinkParams, divisor: u32, seed: u64, bus: &mut Bus)`; it writes the `video.*` signals once per field.

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-video/tests/link.rs`:

```rust
use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, StepCtx};
use ofs_video::link::*;
use ofs_video::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization, Shape};

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

fn omni(axis: DVec3) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Rhcp, axis }
}

/// The open field: the goggles 1.7 m up at home with one upright 2 dBi omni, no objects, no emitters.
fn open_field() -> LinkWorld {
    LinkWorld {
        pilot_position: DVec3::new(0.0, 0.0, -1.7),
        antennas: vec![ReceiverAntenna { name: "omni".into(), antenna: omni(DVec3::NEG_Z) }],
        noise_floor_dbm: -93.0,
        diversity: true,
        obstacles: Vec::new(),
        emitters: Vec::new(),
    }
}

/// A VTX omni standing straight up on the quad (FRD -z is up); bounce and fading off: the bare link budget.
fn bare(world: LinkWorld) -> LinkParams {
    LinkParams { world, vtx_antenna: omni(DVec3::NEG_Z), pit_power_mw: 0.1, fading: false, ground_bounce: false }
}

fn snr_at(params: &LinkParams, power_mw: f64, north_m: f64) -> f64 {
    let path = vtx_paths(params, DVec3::new(north_m, 0.0, -1.7), DQuat::IDENTITY, 5800.0)[0];
    snr_db(10.0 * power_mw.log10() + path.gain_db, params.world.noise_floor_dbm, NO_SIGNAL_DBM)
}

#[test]
fn twenty_five_milliwatts_reach_8_db_snr_at_580_m_in_the_open() {
    let params = bare(open_field());
    close(snr_at(&params, 25.0, 580.0), 8.0, 0.1, "25 mW at 580 m");
    let reach_600 = 580.0 * 10f64.powf((10.0 * (600.0f64 / 25.0).log10()) / 20.0);
    assert!(reach_600 > 2700.0 && reach_600 < 3000.0, "600 mW reaches about 3 km: {reach_600}");
    close(snr_at(&params, 600.0, reach_600), 8.0, 0.1, "600 mW at its reach");
    close(snr_at(&params, 25.0, 58.0) - snr_at(&params, 25.0, 580.0), 20.0, 1e-6, "ten times closer: 20 dB better");
    close(fspl_db(580.0, 5800.0), 102.99, 0.01, "the path loss behind it");
}

#[test]
fn the_picture_follows_the_receiver_thresholds() {
    assert_eq!(picture(30.0), Picture { noise: 0.0, sparkles: 0.0, chroma: 1.0 }, "clean");
    let p = picture(12.0);
    close(p.noise, 0.5, 1e-12, "grain at 12 dB");
    close(p.sparkles, 0.0, 1e-12, "sparkles start at 12 dB");
    close(picture(8.0).sparkles, 0.5, 1e-12, "half way to 4 dB");
    close(picture(4.0).sparkles, 1.0, 1e-12, "full at 4 dB");
    close(picture(8.0).chroma, 1.0, 1e-12, "colour holds at 8 dB");
    close(picture(6.5).chroma, 0.5, 1e-12, "half gone at 6.5 dB");
    close(picture(5.0).chroma, 0.0, 1e-12, "gone at 5 dB");
    close(picture(0.0).noise, 1.0, 1e-12, "static at 0 dB");
    close(picture(-20.0).noise, 1.0, 1e-12, "and below");
    let mut last = picture(40.0);
    for tenth in (-100..400).rev() {
        let p = picture(f64::from(tenth) / 10.0);
        assert!(p.noise >= last.noise && p.sparkles >= last.sparkles && p.chroma <= last.chroma, "monotonic at {tenth}");
        last = p;
    }
}

#[test]
fn sync_is_lost_after_three_bad_fields_and_relocks_after_five_good_ones() {
    let mut t = SyncTracker::new();
    assert_eq!(t.update(20.0), VideoSync::Locked);
    assert_eq!(t.update(5.0), VideoSync::Unstable, "below 6 dB: tearing");
    assert_eq!(t.update(2.0), VideoSync::Unstable, "one field below 3 dB");
    assert_eq!(t.update(2.0), VideoSync::Unstable, "two");
    assert_eq!(t.update(2.0), VideoSync::Lost, "three in a row: lost");
    for i in 0..4 {
        assert_eq!(t.update(10.0), VideoSync::Lost, "good field {i}: still relocking");
    }
    assert_eq!(t.update(10.0), VideoSync::Locked, "the fifth good field relocks");
    assert_eq!(t.update(2.0), VideoSync::Unstable);
    assert_eq!(t.update(20.0), VideoSync::Locked, "a good field resets the count");
    assert_eq!(t.update(2.0), VideoSync::Unstable);
    assert_eq!(t.update(2.0), VideoSync::Unstable, "only two in a row");
}

#[test]
fn a_relock_needs_five_good_fields_in_a_row() {
    let mut t = SyncTracker::new();
    assert_eq!(t.update(-5.0), VideoSync::Lost, "the first field starts lost at once");
    for _ in 0..4 {
        t.update(10.0);
    }
    assert_eq!(t.update(4.0), VideoSync::Lost, "a weak field breaks the run");
    for _ in 0..4 {
        assert_eq!(t.update(10.0), VideoSync::Lost);
    }
    assert_eq!(t.update(5.0 + UNSTABLE_SNR_DB), VideoSync::Locked);
    assert_eq!(VideoSync::from_signal(2.0), VideoSync::Lost);
    assert_eq!(VideoSync::from_signal(1.0), VideoSync::Unstable);
    assert_eq!(VideoSync::from_signal(0.0), VideoSync::Locked);
}

#[test]
fn diversity_switches_only_for_a_clearly_better_antenna() {
    let mut d = Diversity::default();
    assert_eq!(d.choose(&[10.0, 11.0]), 0, "1 dB better is not enough");
    assert_eq!(d.choose(&[10.0, 12.5]), 1, "2.5 dB better: switch");
    assert_eq!(d.choose(&[11.0, 10.0]), 1, "and stay while the other is only 1 dB better");
    assert_eq!(d.choose(&[13.0, 10.0]), 0, "back when it is 3 dB better");
    assert_eq!(d.choose(&[5.0]), 0, "a single antenna");
}

#[test]
fn the_frame_shadows_the_vtx_when_the_pilot_is_ahead_and_below() {
    let level = DQuat::IDENTITY;
    let quad = DVec3::new(0.0, 0.0, -20.0);
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, 0.0)), BODY_SHADOW_DB, 1e-9, "straight below");
    close(body_shadow_db(level, quad, DVec3::new(-100.0, 0.0, -20.0)), 0.0, 1e-12, "behind, level");
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, -40.0)), 0.0, 1e-12, "above");
    let ahead = body_shadow_db(level, quad, DVec3::new(100.0, 0.0, -20.0));
    assert!(ahead > 4.0 && ahead < BODY_SHADOW_DB, "ahead, level: most of it ({ahead})");
    let turned = DQuat::from_rotation_z(std::f64::consts::PI);
    close(body_shadow_db(turned, quad, DVec3::new(100.0, 0.0, -20.0)), 0.0, 1e-9, "turned away: the antenna sees the pilot");
}

fn emitter_world(offset_mhz: f64) -> LinkWorld {
    let mut world = open_field();
    world.emitters.push(Emitter {
        position: DVec3::new(0.0, 150.0, -1.0),
        freq_mhz: 5800.0 + offset_mhz,
        power_mw: 25.0,
        antenna: omni(DVec3::NEG_Z),
    });
    world
}

#[test]
fn an_emitter_interferes_less_the_further_its_channel_is() {
    let same = interference_dbm(&emitter_world(0.0), 5800.0, false)[0];
    let next = interference_dbm(&emitter_world(37.0), 5800.0, false)[0];
    let far = interference_dbm(&emitter_world(100.0), 5800.0, false)[0];
    close(same, 13.98 + 4.0 - fspl_db(150.0, 5800.0), 0.05, "same channel: its whole power");
    // 22.75 dB of rejection, plus 0.06 dB more path loss at the higher frequency.
    close(same - next, 22.75 + 20.0 * (5837.0f64 / 5800.0).log10(), 1e-3, "37 MHz away (the next Raceband channel)");
    close(same - far, 40.0 + 20.0 * (5900.0f64 / 5800.0).log10(), 1e-3, "far away");
    close(interference_dbm(&open_field(), 5800.0, false)[0], NO_SIGNAL_DBM, 1e-9, "no emitters");
    let quad_snr = |world: LinkWorld| {
        let params = bare(world);
        let path = vtx_paths(&params, DVec3::new(300.0, 0.0, -1.7), DQuat::IDENTITY, 5800.0)[0];
        snr_db(10.0 * 25f64.log10() + path.gain_db, -93.0, interference_dbm(&params.world, 5800.0, false)[0])
    };
    assert!(quad_snr(emitter_world(0.0)) < 0.0, "a same-channel emitter close to the pilot swamps a quad 300 m out");
    assert!(quad_snr(emitter_world(100.0)) > quad_snr(open_field()) - 1.0, "a far channel costs under a dB");
}

#[test]
fn a_building_between_the_quad_and_the_pilot_costs_its_loss() {
    let mut world = open_field();
    let building = Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    world.obstacles.push(Obstacle { shape: building.rooted(), rf_loss_db: 25.0 });
    let params = bare(world);
    let open = bare(open_field());
    let at = |p: &LinkParams, east: f64| vtx_paths(p, DVec3::new(200.0, east, -1.7), DQuat::IDENTITY, 5800.0)[0];
    close(at(&open, 0.0).gain_db - at(&params, 0.0).gain_db, 25.0, 1e-6, "behind it");
    close(at(&params, 0.0).obstruction_db, 25.0, 1e-9, "reported as obstruction");
    close(at(&open, 40.0).gain_db - at(&params, 40.0).gain_db, 0.0, 1e-9, "beside it");
}

struct Rig {
    bus: Bus,
    link: VideoLink,
    tick: u64,
}

impl Rig {
    fn new(params: LinkParams, seed: u64) -> Rig {
        let mut bus = Bus::new();
        let link = VideoLink::new(params, 160, seed, &mut bus);
        let mut rig = Rig { bus, link, tick: 0 };
        rig.set_vtx(true, 5800.0, 25.0, false);
        rig.set_pose(DVec3::new(50.0, 0.0, -10.0));
        rig
    }

    fn set_vtx(&mut self, present: bool, freq_mhz: f64, power_mw: f64, pit: bool) {
        let b = &mut self.bus;
        let s = |b: &mut Bus, name: &str, v: f64| {
            let sig = b.signal::<f64>(name);
            b.set(sig, v);
        };
        s(b, names::VTX_PRESENT, if present { 1.0 } else { 0.0 });
        s(b, names::VTX_FREQ_MHZ, freq_mhz);
        s(b, names::VTX_POWER_MW, power_mw);
        s(b, names::VTX_PIT, if pit { 1.0 } else { 0.0 });
    }

    fn set_pose(&mut self, pos: DVec3) {
        let p = self.bus.signal::<DVec3>(names::BODY_POS_NED);
        self.bus.set(p, pos);
        let a = self.bus.signal::<DQuat>(names::BODY_ATT);
        self.bus.set(a, DQuat::IDENTITY);
    }

    fn field(&mut self) {
        let ctx = StepCtx { tick: self.tick, time_s: self.tick as f64 / 8000.0, dt_s: 0.02 };
        self.link.step(&ctx, &mut self.bus).unwrap();
        self.tick += 160;
    }

    fn get(&self, name: &str) -> f64 {
        self.bus.get(self.bus.lookup::<f64>(name).unwrap())
    }
}

fn faded(world: LinkWorld) -> LinkParams {
    LinkParams { fading: true, ground_bounce: true, ..bare(world) }
}

#[test]
fn a_hovering_quad_sees_a_steady_signal_and_a_moving_one_a_fading_one() {
    let mut rig = Rig::new(faded(open_field()), 7);
    let mut still = Vec::new();
    for _ in 0..20 {
        rig.field();
        still.push(rig.get(&names::video_rssi("omni")));
    }
    assert!(still.windows(2).all(|w| w[0] == w[1]), "standing still: {still:?}");
    let mut moving = Vec::new();
    for i in 0..50 {
        rig.set_pose(DVec3::new(100.0 + f64::from(i) * 0.4, 0.0, -10.0)); // 20 m/s
        rig.field();
        moving.push(rig.get(&names::video_rssi("omni")));
    }
    let max = moving.iter().cloned().fold(f64::MIN, f64::max);
    let min = moving.iter().cloned().fold(f64::MAX, f64::min);
    assert!(max - min > 3.0, "fast flight flickers: {min}..{max}");
}

#[test]
fn the_same_seed_gives_the_same_fades() {
    let run = |seed: u64| {
        let mut rig = Rig::new(faded(open_field()), seed);
        (0..30)
            .map(|i| {
                rig.set_pose(DVec3::new(100.0 + f64::from(i), 0.0, -10.0));
                rig.field();
                rig.get(names::VIDEO_SNR)
            })
            .collect::<Vec<f64>>()
    };
    assert_eq!(run(3), run(3));
    assert_ne!(run(3), run(4));
}

#[test]
fn the_model_publishes_the_link_and_follows_the_vtx() {
    let mut rig = Rig::new(bare(open_field()), 1);
    rig.field();
    assert_eq!(rig.get(names::VIDEO_PRESENT), 1.0);
    let full = rig.get(names::VIDEO_SNR);
    assert!(full > CLEAN_SNR_DB, "25 mW at 50 m is clean: {full}");
    assert_eq!(rig.get(names::VIDEO_SYNC), 0.0, "locked");
    assert_eq!(rig.get(names::VIDEO_NOISE), 0.0);
    assert_eq!(rig.get(names::VIDEO_CHROMA), 1.0);
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 0.0);
    close(rig.get(names::VIDEO_INTERFERENCE), NO_SIGNAL_DBM, 1e-9, "no emitters");
    rig.set_vtx(true, 5800.0, 25.0, true);
    rig.field();
    close(full - rig.get(names::VIDEO_SNR), 10.0 * (25.0f64 / 0.1).log10(), 1e-6, "pit mode: 0.1 mW");
    rig.set_vtx(false, 0.0, 0.0, false);
    for _ in 0..3 {
        rig.field();
    }
    close(rig.get(&names::video_rssi("omni")), NO_SIGNAL_DBM, 1e-9, "no VTX: nothing received");
    assert_eq!(rig.get(names::VIDEO_SYNC), 2.0, "and the sync is lost");
    assert_eq!(rig.get(names::VIDEO_NOISE), 1.0, "static");
}

#[test]
fn diversity_picks_the_patch_when_the_quad_is_out_in_front() {
    let mut world = open_field();
    world.antennas.push(ReceiverAntenna {
        name: "patch".into(),
        antenna: Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Rhcp, axis: DVec3::X },
    });
    let mut rig = Rig::new(bare(world), 1);
    rig.set_pose(DVec3::new(300.0, 0.0, -10.0));
    rig.field();
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 1.0, "in front: the 8 dBi patch");
    let gain = rig.get(&names::video_rssi("patch")) - rig.get(&names::video_rssi("omni"));
    assert!(gain > 5.0, "the patch hears it {gain} dB better");
    rig.set_pose(DVec3::new(-300.0, 0.0, -10.0));
    rig.field();
    assert_eq!(rig.get(names::VIDEO_ANTENNA), 0.0, "behind the pilot: the omni");
    let mut single = rig_without_diversity();
    single.set_pose(DVec3::new(300.0, 0.0, -10.0));
    single.field();
    assert_eq!(single.get(names::VIDEO_ANTENNA), 0.0, "no diversity: always the first antenna");
}

fn rig_without_diversity() -> Rig {
    let mut world = open_field();
    world.diversity = false;
    world.antennas.push(ReceiverAntenna { name: "patch".into(), antenna: omni(DVec3::NEG_Z) });
    Rig::new(bare(world), 1)
}

#[test]
fn odd_positions_give_finite_values() {
    // At the goggles (zero distance), inside a building, below the ground plane (a crash can dip there): the
    // simulator's per-step check stops on any non-finite signal, so the link must never produce one.
    let mut world = open_field();
    let building = Shape::Box { center: DVec3::new(100.0, 0.0, -10.0), half: DVec3::new(5.0, 5.0, 10.0) };
    world.obstacles.push(Obstacle { shape: building.rooted(), rf_loss_db: 25.0 });
    let mut rig = Rig::new(faded(world), 1);
    for pos in [DVec3::new(0.0, 0.0, -1.7), DVec3::new(100.0, 0.0, -5.0), DVec3::new(10.0, 0.0, 0.5), DVec3::new(1e5, 0.0, -10.0)] {
        rig.set_pose(pos);
        rig.field();
        for name in [names::VIDEO_SNR, names::VIDEO_NOISE, names::VIDEO_SPARKLES, names::VIDEO_CHROMA, names::VIDEO_INTERFERENCE] {
            assert!(rig.get(name).is_finite(), "{name} at {pos}: {}", rig.get(name));
        }
        assert!(rig.get(&names::video_rssi("omni")).is_finite(), "rssi at {pos}");
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-video --test link --locked`
Expected: FAIL: does not compile, ``unresolved import `ofs_video::link` ``.

- [ ] **Step 3: Add the signal names**

In `crates/ofs-core/src/names.rs`, replace:

```rust
pub const FC_SERIAL_DROPPED: &str = "fc.serial_dropped_bytes";

/// 1.0 when a VTX is wired to the flight controller.
pub const VTX_PRESENT: &str = "vtx.present";
/// Band 1..=6 (A, B, E, F, R, L); 0 while the VTX is in user-frequency mode.
```

with:

```rust
pub const FC_SERIAL_DROPPED: &str = "fc.serial_dropped_bytes";

/// 1.0 when the quad has a VTX (it transmits from load on, with or without Betaflight).
pub const VTX_PRESENT: &str = "vtx.present";
/// Band 1..=6 (A, B, E, F, R, L); 0 while the VTX is in user-frequency mode.
```

In `crates/ofs-core/src/names.rs`, replace:

```rust
/// 1.0 while the VTX is in pit mode.
pub const VTX_PIT: &str = "vtx.pit_mode";
```

with:

```rust
/// 1.0 while the VTX is in pit mode.
pub const VTX_PIT: &str = "vtx.pit_mode";

/// 1.0 when the quad has a VTX, so the video link model runs.
pub const VIDEO_PRESENT: &str = "video.present";
/// Signal-to-noise ratio in dB at the receiver antenna in use.
pub const VIDEO_SNR: &str = "video.snr_db";
/// Index (into the world's receiver antennas) of the antenna in use.
pub const VIDEO_ANTENNA: &str = "video.antenna";
/// Other emitters' power after the receiver's channel filter, at the antenna in use, in dBm.
pub const VIDEO_INTERFERENCE: &str = "video.interference_dbm";
/// Picture grain, 0 (clean) to 1 (static).
pub const VIDEO_NOISE: &str = "video.noise";
/// FM threshold sparkles, 0 to 1.
pub const VIDEO_SPARKLES: &str = "video.sparkles";
/// Colour saturation, 1 (full colour) to 0 (black and white).
pub const VIDEO_CHROMA: &str = "video.chroma";
/// 0 locked, 1 unstable (tearing), 2 lost (rolling, static).
pub const VIDEO_SYNC: &str = "video.sync";

/// Received power in dBm at one receiver antenna, by its name in the world file.
pub fn video_rssi(antenna: &str) -> String {
    format!("video.rssi_dbm.{antenna}")
}
```

- [ ] **Step 4: Write the link model**

In `crates/ofs-video/src/lib.rs`, replace:

```rust
//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles.
pub mod osd;
pub mod propagation;
```

with:

```rust
//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles.
pub mod link;
pub mod osd;
pub mod propagation;
```

Create `crates/ofs-video/src/link.rs`:

```rust
//! The analog video link, from the quad's VTX to the pilot's goggles, one PAL field (50 Hz) at a time: received
//! power at each goggle antenna, interference from other emitters, diversity, then what the receiver makes of the
//! signal (grain, sparkles, colour, sync). Deterministic: the fading draws from the model's own seeded stream.
//!
//! Every threshold below is an estimate for a typical analog 5.8 GHz receiver; docs/research/video-link.md lists
//! them with their provenance.
use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

use crate::propagation::{
    adjacent_channel_rejection_db, mw_to_dbm, path_gain, power_sum_dbm, wavelength_m, Antenna, Endpoint, Obstacle,
    PathGain,
};

pub const MODEL_NAME: &str = "video.link";
/// One PAL field.
pub const FIELD_RATE_HZ: u32 = 50;
/// Received power reported when nothing arrives (no transmitter, or no other emitters).
pub const NO_SIGNAL_DBM: f64 = -150.0;
/// The most the frame, battery and stack take from the VTX signal when they sit between its antenna and the pilot.
pub const BODY_SHADOW_DB: f64 = 8.0;
/// The body direction (FRD, unit) the frame shadows most: forward and down, through the stack and the battery.
const SHADOW_DIRECTION: DVec3 = DVec3::new(0.6, 0.0, 0.8);
/// The receiver changes antenna only when another one is better by this much.
pub const DIVERSITY_HYSTERESIS_DB: f64 = 2.0;
/// Rician K-factor with a clear line of sight; obstruction lowers it dB for dB (towards Rayleigh fading).
pub const LOS_K_DB: f64 = 10.0;
/// Lowest K-factor (deep behind an obstacle: practically Rayleigh).
pub const MIN_K_DB: f64 = -20.0;

/// At or above this SNR the picture is clean.
pub const CLEAN_SNR_DB: f64 = 25.0;
/// Grain reaches half of full static here, where sparkles start.
pub const SPARKLE_SNR_DB: f64 = 12.0;
/// Sparkles cover the picture at or below this SNR.
pub const SPARKLE_FULL_SNR_DB: f64 = 4.0;
/// Colour starts to fade below this SNR and is gone at `COLOR_LOST_SNR_DB`.
pub const COLOR_FADE_SNR_DB: f64 = 8.0;
pub const COLOR_LOST_SNR_DB: f64 = 5.0;
/// Below this SNR sync is unstable (tearing); a lost sync relocks only above it.
pub const UNSTABLE_SNR_DB: f64 = 6.0;
/// Below this SNR for `LOST_AFTER_FIELDS` fields in a row, sync is lost.
pub const LOST_SNR_DB: f64 = 3.0;
pub const LOST_AFTER_FIELDS: u32 = 3;
/// Fields in a row at or above `UNSTABLE_SNR_DB` that a lost sync needs to relock.
pub const RELOCK_AFTER_FIELDS: u32 = 5;

/// The receiver's hold on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSync {
    Locked = 0,
    /// Tearing and line jitter.
    Unstable = 1,
    /// Rolling, then static.
    Lost = 2,
}

impl VideoSync {
    pub fn from_signal(value: f64) -> VideoSync {
        match value.round() as i64 {
            0 => VideoSync::Locked,
            1 => VideoSync::Unstable,
            _ => VideoSync::Lost,
        }
    }
}

/// What the picture looks like, each value 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picture {
    /// Grain; 1 is full static.
    pub noise: f64,
    pub sparkles: f64,
    /// Colour saturation; 0 is black and white.
    pub chroma: f64,
}

/// How far `x` has gone from `from` towards `to` (`from > to`), clamped to 0..=1.
fn ramp(x: f64, from: f64, to: f64) -> f64 {
    ((from - x) / (from - to)).clamp(0.0, 1.0)
}

/// The picture at a given SNR: grain rises from 25 dB (0.5 at 12 dB, full static at 0 dB), sparkles from 12 dB
/// (full at 4 dB), colour fades from 8 dB (gone at 5 dB).
pub fn picture(snr_db: f64) -> Picture {
    let noise = 0.5 * ramp(snr_db, CLEAN_SNR_DB, SPARKLE_SNR_DB) + 0.5 * ramp(snr_db, SPARKLE_SNR_DB, 0.0);
    Picture {
        noise,
        sparkles: ramp(snr_db, SPARKLE_SNR_DB, SPARKLE_FULL_SNR_DB),
        chroma: 1.0 - ramp(snr_db, COLOR_FADE_SNR_DB, COLOR_LOST_SNR_DB),
    }
}

/// The receiver's sync, field by field: unstable below 6 dB, lost after 3 fields below 3 dB, relocked after 5 fields
/// at 6 dB or more. The first field decides the starting state without delay.
#[derive(Debug, Clone, Default)]
pub struct SyncTracker {
    state: Option<VideoSync>,
    low_fields: u32,
    good_fields: u32,
}

impl SyncTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, snr_db: f64) -> VideoSync {
        let live = if snr_db < UNSTABLE_SNR_DB { VideoSync::Unstable } else { VideoSync::Locked };
        let next = match self.state {
            None if snr_db < LOST_SNR_DB => VideoSync::Lost,
            None => live,
            Some(VideoSync::Lost) => {
                self.good_fields = if snr_db >= UNSTABLE_SNR_DB { self.good_fields + 1 } else { 0 };
                if self.good_fields >= RELOCK_AFTER_FIELDS { live } else { VideoSync::Lost }
            }
            Some(_) => {
                self.low_fields = if snr_db < LOST_SNR_DB { self.low_fields + 1 } else { 0 };
                if self.low_fields >= LOST_AFTER_FIELDS { VideoSync::Lost } else { live }
            }
        };
        if (next == VideoSync::Lost) != (self.state == Some(VideoSync::Lost)) {
            self.low_fields = 0;
            self.good_fields = 0;
        }
        self.state = Some(next);
        next
    }
}

/// Diversity: the antenna with the best SNR, changed only for a 2 dB better one.
#[derive(Debug, Clone, Default)]
pub struct Diversity {
    active: usize,
}

impl Diversity {
    /// The antenna to use, given each antenna's SNR (at least one).
    pub fn choose(&mut self, snr_db: &[f64]) -> usize {
        self.active = self.active.min(snr_db.len().saturating_sub(1));
        let best = (0..snr_db.len()).max_by(|a, b| snr_db[*a].total_cmp(&snr_db[*b])).unwrap_or(0);
        if snr_db[best] > snr_db[self.active] + DIVERSITY_HYSTERESIS_DB {
            self.active = best;
        }
        self.active
    }
}

/// `SNR = signal - (noise floor + interference)`, powers added in mW.
pub fn snr_db(signal_dbm: f64, noise_floor_dbm: f64, interference_dbm: f64) -> f64 {
    signal_dbm - power_sum_dbm([noise_floor_dbm, interference_dbm])
}

/// How much of the frame, battery and stack lies between the VTX antenna and the receiver, as a loss in dB:
/// up to `BODY_SHADOW_DB` when the receiver is forward and below the quad, nothing when it is behind or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, receiver_pos: DVec3) -> f64 {
    let Some(dir_world) = (receiver_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
}

/// A goggle antenna, placed in the world (its axis in NED).
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiverAntenna {
    pub name: String,
    pub antenna: Antenna,
}

/// Another transmitter on the field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub position: DVec3,
    pub freq_mhz: f64,
    pub power_mw: f64,
    pub antenna: Antenna,
}

/// Everything the link needs to know about the world.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkWorld {
    /// Where the goggles are.
    pub pilot_position: DVec3,
    pub antennas: Vec<ReceiverAntenna>,
    pub noise_floor_dbm: f64,
    pub diversity: bool,
    pub obstacles: Vec<Obstacle>,
    pub emitters: Vec<Emitter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub world: LinkWorld,
    /// The VTX antenna on the quad: its axis in the body frame (FRD).
    pub vtx_antenna: Antenna,
    /// Output power in pit mode.
    pub pit_power_mw: f64,
    /// Fading and the ground bounce; tests that check the bare link budget turn them off.
    pub fading: bool,
    pub ground_bounce: bool,
}

/// The VTX signal's path to each receiver antenna, without fading: body shadow included.
pub fn vtx_paths(params: &LinkParams, quad_pos: DVec3, att: DQuat, freq_mhz: f64) -> Vec<PathGain> {
    let tx = Endpoint { position: quad_pos, antenna: Antenna { axis: (att * params.vtx_antenna.axis).normalize(), ..params.vtx_antenna } };
    let shadow = body_shadow_db(att, quad_pos, params.world.pilot_position);
    params
        .world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: params.world.pilot_position, antenna: a.antenna };
            let p = path_gain(&tx, &rx, freq_mhz, &params.world.obstacles, params.ground_bounce);
            PathGain { gain_db: p.gain_db - shadow, obstruction_db: p.obstruction_db }
        })
        .collect()
}

/// Interference at each receiver antenna for a receiver tuned to `freq_mhz`, in dBm.
pub fn interference_dbm(world: &LinkWorld, freq_mhz: f64, ground_bounce: bool) -> Vec<f64> {
    world
        .antennas
        .iter()
        .map(|a| {
            let rx = Endpoint { position: world.pilot_position, antenna: a.antenna };
            let powers = world.emitters.iter().map(|e| {
                let tx = Endpoint { position: e.position, antenna: e.antenna };
                mw_to_dbm(e.power_mw) + path_gain(&tx, &rx, e.freq_mhz, &world.obstacles, ground_bounce).gain_db
                    - adjacent_channel_rejection_db(e.freq_mhz - freq_mhz)
            });
            power_sum_dbm(powers.chain([NO_SIGNAL_DBM]))
        })
        .collect()
}

/// Rician fading of one antenna: an AR(1) complex Gaussian scatter (unit mean power) added to the line of sight.
#[derive(Debug, Clone, Copy, Default)]
struct Fader {
    re: f64,
    im: f64,
}

impl Fader {
    /// The fade in dB for this field. `rho` is the correlation with the previous field (1 = the quad stood still).
    fn next(&mut self, rng: &mut ChaCha8Rng, rho: f64, k_db: f64) -> f64 {
        let fresh = ((1.0 - rho * rho).max(0.0) * 0.5).sqrt();
        let (n1, n2): (f64, f64) = (StandardNormal.sample(rng), StandardNormal.sample(rng));
        self.re = rho * self.re + fresh * n1;
        self.im = rho * self.im + fresh * n2;
        let k = 10f64.powf(k_db.max(MIN_K_DB) / 10.0);
        let (los, scatter) = ((k / (k + 1.0)).sqrt(), (1.0 / (k + 1.0)).sqrt());
        let (re, im) = (los + scatter * self.re, scatter * self.im);
        10.0 * (re * re + im * im).max(1e-6).log10()
    }
}

struct Outputs {
    present: Signal<f64>,
    snr: Signal<f64>,
    rssi: Vec<Signal<f64>>,
    antenna: Signal<f64>,
    interference: Signal<f64>,
    noise: Signal<f64>,
    sparkles: Signal<f64>,
    chroma: Signal<f64>,
    sync: Signal<f64>,
}

pub struct VideoLink {
    params: LinkParams,
    divisor: u32,
    rng: ChaCha8Rng,
    faders: Vec<Fader>,
    last_pos: Option<DVec3>,
    tracker: SyncTracker,
    diversity: Diversity,
    /// Interference per antenna for the frequency it was computed for (the emitters and the goggles do not move).
    interference: Option<(f64, Vec<f64>)>,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    vtx_present: Signal<f64>,
    vtx_freq: Signal<f64>,
    vtx_power: Signal<f64>,
    vtx_pit: Signal<f64>,
    out: Outputs,
}

impl VideoLink {
    /// `divisor` is `base_hz / FIELD_RATE_HZ`.
    pub fn new(params: LinkParams, divisor: u32, seed: u64, bus: &mut Bus) -> Self {
        assert!(!params.world.antennas.is_empty(), "the receiver needs at least one antenna");
        let out = Outputs {
            present: bus.signal(names::VIDEO_PRESENT),
            snr: bus.signal(names::VIDEO_SNR),
            rssi: params.world.antennas.iter().map(|a| bus.signal(&names::video_rssi(&a.name))).collect(),
            antenna: bus.signal(names::VIDEO_ANTENNA),
            interference: bus.signal(names::VIDEO_INTERFERENCE),
            noise: bus.signal(names::VIDEO_NOISE),
            sparkles: bus.signal(names::VIDEO_SPARKLES),
            chroma: bus.signal(names::VIDEO_CHROMA),
            sync: bus.signal(names::VIDEO_SYNC),
        };
        let mut rng = model_rng(seed, MODEL_NAME);
        let faders = params
            .world
            .antennas
            .iter()
            .map(|_| {
                let mut f = Fader::default();
                f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first field
                f
            })
            .collect();
        Self {
            divisor,
            rng,
            faders,
            last_pos: None,
            tracker: SyncTracker::new(),
            diversity: Diversity::default(),
            interference: None,
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            vtx_present: bus.signal(names::VTX_PRESENT),
            vtx_freq: bus.signal(names::VTX_FREQ_MHZ),
            vtx_power: bus.signal(names::VTX_POWER_MW),
            vtx_pit: bus.signal(names::VTX_PIT),
            out,
            params,
        }
    }

    fn interference_for(&mut self, freq_mhz: f64) -> Vec<f64> {
        match &self.interference {
            Some((f, values)) if *f == freq_mhz => values.clone(),
            _ => {
                let values = interference_dbm(&self.params.world, freq_mhz, self.params.ground_bounce);
                self.interference = Some((freq_mhz, values.clone()));
                values
            }
        }
    }
}

impl Model for VideoLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.divisor
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let freq_mhz = bus.get(self.vtx_freq);
        let power_mw = if bus.get(self.vtx_pit) > 0.5 { self.params.pit_power_mw } else { bus.get(self.vtx_power) };
        let transmitting = bus.get(self.vtx_present) > 0.5 && power_mw > 0.0 && freq_mhz > 0.0;
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let interference = if freq_mhz > 0.0 { self.interference_for(freq_mhz) } else { vec![NO_SIGNAL_DBM; self.faders.len()] };
        let mut rssi = vec![NO_SIGNAL_DBM; self.faders.len()];
        if transmitting {
            let rho = (-moved / (wavelength_m(freq_mhz) * 0.5)).exp();
            for (i, path) in vtx_paths(&self.params, pos, att, freq_mhz).into_iter().enumerate() {
                let fade = if self.params.fading {
                    self.faders[i].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
                } else {
                    0.0
                };
                rssi[i] = (mw_to_dbm(power_mw) + path.gain_db + fade).max(NO_SIGNAL_DBM);
            }
        }
        let snr: Vec<f64> = rssi
            .iter()
            .zip(&interference)
            .map(|(s, i)| snr_db(*s, self.params.world.noise_floor_dbm, *i))
            .collect();
        let active = if self.params.world.diversity { self.diversity.choose(&snr) } else { 0 };
        let sync = self.tracker.update(snr[active]);
        let picture = picture(snr[active]);
        let o = &self.out;
        bus.set(o.present, 1.0);
        bus.set(o.snr, snr[active]);
        for (signal, value) in o.rssi.iter().zip(&rssi) {
            bus.set(*signal, *value);
        }
        bus.set(o.antenna, active as f64);
        bus.set(o.interference, interference[active]);
        bus.set(o.noise, picture.noise);
        bus.set(o.sparkles, picture.sparkles);
        bus.set(o.chroma, picture.chroma);
        bus.set(o.sync, sync as i32 as f64);
        Ok(())
    }
}
```

Notes for the reader:
- `SyncTracker` resets its counters only when the state enters or leaves `Lost`, so the fields below 3 dB count from `Locked` and from `Unstable` alike (3 fields to lose sync).
- The fading's correlation follows how far the quad moved (`rho = exp(-moved / (λ/2))`): a hovering quad keeps its fade, fast flight redraws it every field.
- Interference depends only on the VTX frequency (the goggles and the emitters do not move), so it is computed once per frequency and cached.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p ofs-video --locked`
Expected: PASS: 13 link tests, 14 propagation tests and the M3a tests.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-core/src/names.rs crates/ofs-video/src/lib.rs crates/ofs-video/src/link.rs crates/ofs-video/tests/link.rs
git commit -m "feat(video): the analog link model: receiver thresholds, sync, diversity, fading, interference"
```

### Task 3: World files and the VTX antenna (`ofs-config`)

**Files:**
- Create: `crates/ofs-config/src/world.rs`, `crates/ofs-config/tests/world.rs`, `worlds/flat.toml`
- Modify: `crates/ofs-config/src/lib.rs`, `quads/opendrone-5f-freestyle.toml`

**Interfaces:**
- Consumes: the crate's `Checker`, `Problem` and `ConfigError` (private `Checker` is visible to the child module `world`).
- Produces (used by Tasks 4, 5):
  - `ofs_config::world::{WorldConfig, PilotSection, ReceiverSection, AntennaSection, ObjectSection, EmitterSection, AntennaKind, Polarization, Shape, WORLD_SCHEMA_VERSION, EMITTER_FREQ_RANGE_MHZ}`, `pub fn load(path: &Path) -> Result<WorldConfig, ConfigError>`, `WorldConfig::open_field()`, `WorldConfig::validate()`, `AntennaSection::aim_el() -> f64` (default 90 for an omni, 0 for a patch).
  - Re-exported at the crate root: `AntennaKind`, `Polarization`, `WorldConfig`.
  - `VtxSection` gains `antenna: VtxAntennaSection { kind, gain_dbi, beamwidth_deg: Option<f64>, polarization, mount_frd: [f64; 3] }` (default a 2 dBi RHCP omni, `mount_frd = [-0.5, 0.0, -1.0]`) and `pit_power_mw: f64` (default 0.1); `pub const VIDEO_FIELD_RATE_HZ: u32 = 50`.

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-config/tests/world.rs`:

```rust
use std::fs;
use std::path::Path;

use ofs_config::world::{self, AntennaKind, Shape, WorldConfig};
use ofs_config::{load, ConfigError, Polarization};

const WORLD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml");
const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn world_text() -> String {
    fs::read_to_string(WORLD).unwrap().replace("\r\n", "\n")
}

fn write(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("world.toml");
    fs::write(&path, text).unwrap();
    path
}

fn problems(text: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    match world::load(&write(dir.path(), text)) {
        Err(ConfigError::Invalid { problems, .. }) => problems.into_iter().map(|p| format!("{}: {}", p.field, p.message)).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn the_shipped_world_loads() {
    let w = world::load(Path::new(WORLD)).unwrap();
    assert_eq!(w.schema_version, 1);
    assert_eq!(w.pilot.position_ned_m, [-3.0, 2.0, -1.7]);
    let names: Vec<&str> = w.receiver.antennas.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["omni", "patch"]);
    assert!(w.receiver.diversity);
    assert_eq!(w.receiver.antennas[1].kind, AntennaKind::Patch);
    assert_eq!(w.receiver.antennas[1].beamwidth_deg, Some(60.0));
    assert_eq!(w.receiver.antennas[0].aim_el(), 90.0, "an omni stands upright by default");
    assert_eq!(w.receiver.antennas[1].aim_el(), 10.0);
    assert_eq!(w.objects.len(), 1 + 16 + 9 + 4, "pad, pylons, gates, buildings");
    let b = w.objects.iter().find(|o| o.name == "BuildingB").unwrap();
    assert_eq!((b.shape, b.size_m, b.rf_loss_db), (Shape::Box, Some([10.0, 10.0, 22.0]), 30.0));
    let pylon = w.objects.iter().find(|o| o.name == "Pylon1L").unwrap();
    assert_eq!((pylon.shape, pylon.radius_m, pylon.height_m, pylon.rf_loss_db), (Shape::Cylinder, Some(0.15), Some(3.0), 0.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!((w.emitters[0].band.as_deref(), w.emitters[0].channel), (Some("R"), Some(2)));
    assert_eq!(w.emitters[0].polarization, Polarization::Rhcp);
}

#[test]
fn the_open_field_is_valid_and_bare() {
    let w = WorldConfig::open_field();
    assert!(w.validate().is_empty(), "{:?}", w.validate());
    assert_eq!(w.receiver.antennas.len(), 1);
    assert!(w.objects.is_empty() && w.emitters.is_empty());
    assert_eq!(w.pilot.position_ned_m, [0.0, 0.0, -1.7]);
}

#[test]
fn every_world_problem_is_reported_at_once() {
    let text = world_text()
        .replace("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-3.0, 2.0, 1.0]")
        .replace("name = \"patch\"", "name = \"omni\"")
        .replace("beamwidth_deg = 60.0\n", "")
        .replace("size_m = [10.0, 10.0, 22.0]", "size_m = [10.0, 0.0, 22.0]")
        .replace("rf_loss_db = 20.0", "rf_loss_db = -1.0")
        .replace("color = [0.62, 0.64, 0.66]", "color = [0.62, 1.5, 0.66]")
        .replace("band = \"R\"\nchannel = 2", "band = \"Z\"\nchannel = 9");
    let found = problems(&text);
    for expected in [
        "pilot.position_ned_m: the pilot must not be below the ground",
        "receiver.antennas[1].name: \"omni\" is used twice",
        "receiver.antennas[1].beamwidth_deg: a patch needs beamwidth_deg",
        "objects[27].size_m",
        "objects[28].rf_loss_db",
        "objects[26].color",
        "emitters[0].band",
        "emitters[0].channel",
    ] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
}

#[test]
fn shapes_need_their_own_sizes() {
    let text = world_text()
        .replace("radius_m = 0.15\nheight_m = 3.0\ncolor = [0.92, 0.92, 0.9]\n\n[[objects]]\nname = \"Pylon1R\"", "size_m = [1.0, 1.0, 1.0]\ncolor = [0.92, 0.92, 0.9]\n\n[[objects]]\nname = \"Pylon1R\"")
        .replace("shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nsize_m = [3.0, 3.0, 0.02]", "shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nradius_m = 1.0");
    let found = problems(&text);
    for expected in ["objects[1].radius_m", "objects[1].height_m", "objects[1].shape: size_m is for a box", "objects[0].size_m", "objects[0].shape: radius_m"] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
}

#[test]
fn an_emitter_needs_one_frequency_in_range() {
    let both = world_text().replace("band = \"R\"\nchannel = 2", "band = \"R\"\nchannel = 2\nfreq_mhz = 5800.0");
    assert!(problems(&both).iter().any(|p| p.starts_with("emitters[0].freq_mhz: give either")), "both");
    let neither = world_text().replace("band = \"R\"\nchannel = 2\n", "");
    assert!(problems(&neither).iter().any(|p| p.starts_with("emitters[0].freq_mhz: give either")), "neither");
    let low = world_text().replace("band = \"R\"\nchannel = 2", "freq_mhz = 2400.0");
    assert!(problems(&low).iter().any(|p| p.starts_with("emitters[0].freq_mhz: must be in 5300..=6000")), "2.4 GHz");
    let ok = world_text().replace("band = \"R\"\nchannel = 2", "freq_mhz = 5695.0");
    let dir = tempfile::tempdir().unwrap();
    assert!(world::load(&write(dir.path(), &ok)).is_ok());
}

#[test]
fn names_must_be_signal_friendly() {
    let text = world_text().replace("name = \"patch\"", "name = \"the patch\"");
    assert!(problems(&text).iter().any(|p| p.starts_with("receiver.antennas[1].name: must be letters")), "a space");
}

#[test]
fn non_finite_numbers_are_rejected() {
    let text = world_text().replace("center_ned_m = [110.0, -45.0, -7.0]", "center_ned_m = [nan, -45.0, -7.0]").replace("facing_deg = 0.0", "facing_deg = inf");
    let found = problems(&text);
    assert!(found.iter().any(|p| p.starts_with("objects[") && p.contains("center_ned_m")), "{found:#?}");
    assert!(found.iter().any(|p| p.starts_with("pilot.facing_deg")), "{found:#?}");
}

#[test]
fn a_world_needs_an_antenna_and_a_known_schema() {
    let none = world_text().split("\n[[receiver.antennas]]").next().unwrap().to_string() + "\nantennas = []\n";
    assert!(problems(&none).iter().any(|p| p.starts_with("receiver.antennas: needs at least one antenna")));
    let dir = tempfile::tempdir().unwrap();
    let err = world::load(&write(dir.path(), &world_text().replace("schema_version = 1", "schema_version = 2"))).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }) && err.to_string().contains("world schema_version 2"), "{err}");
    let err = world::load(&write(dir.path(), &world_text().replace("kind = \"omni\"", "kind = \"yagi\""))).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }) && err.to_string().contains("yagi"), "{err}");
    let err = world::load(Path::new("does/not/exist.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::Io { .. }), "{err}");
}

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap().replace("\r\n", "\n")
}

fn write_quad(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.join("opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    path
}

#[test]
fn the_shipped_quad_describes_its_vtx_antenna() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let vtx = cfg.vtx.as_ref().unwrap();
    assert_eq!(vtx.pit_power_mw, 0.1);
    assert_eq!((vtx.antenna.kind, vtx.antenna.gain_dbi, vtx.antenna.polarization), (AntennaKind::Omni, 2.0, Polarization::Rhcp));
    assert_eq!(vtx.antenna.mount_frd, [-0.5, 0.0, -1.0]);
}

#[test]
fn the_vtx_antenna_and_pit_power_have_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text();
    let bare = text.split("# Pit mode").next().unwrap().to_string();
    let cfg = load(&write_quad(dir.path(), &bare)).unwrap();
    let vtx = cfg.vtx.unwrap();
    assert_eq!(vtx.pit_power_mw, 0.1);
    assert_eq!(vtx.antenna, ofs_config::VtxAntennaSection::default());
    assert_eq!(vtx.antenna.mount_frd, [-0.5, 0.0, -1.0]);
}

#[test]
fn video_link_problems_in_the_quad_file_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("base_hz = 8000", "base_hz = 8010")
        .replace("pit_power_mw = 0.1", "pit_power_mw = 0.0")
        .replace("kind = \"omni\"\ngain_dbi = 2.0", "kind = \"patch\"\ngain_dbi = 2.0")
        .replace("mount_frd = [-0.5, 0.0, -1.0]", "mount_frd = [0.0, 0.0, 0.0]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["sim.base_hz", "vtx.pit_power_mw", "vtx.antenna.beamwidth_deg", "vtx.antenna.mount_frd"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
    assert!(err.to_string().contains("multiple of 50"), "{err}");
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-config --test world --locked`
Expected: FAIL: does not compile, ``unresolved import `ofs_config::world` ``.

- [ ] **Step 3: The VTX antenna, pit power and the 50 Hz check in the quad file's schema**

In `crates/ofs-config/src/lib.rs`, replace:

```rust

use serde::Deserialize;

/// 3: adds the optional `[esc_telemetry]`, `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
```

with:

```rust

use serde::Deserialize;

pub mod world;
pub use world::{AntennaKind, Polarization, WorldConfig};

/// 3: adds the optional `[esc_telemetry]`, `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
    #[serde(default = "default_vtx_reply_latency_ms")]
    pub reply_latency_ms: f64,
}

fn default_vtx_band() -> String {
```

with:

```rust
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
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
                format!("must be in [0, 100] ms (got {})", v.reply_latency_ms),
            );
        }
        for (i, (field, uart)) in uarts.iter().enumerate() {
```

with:

```rust
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
```

- [ ] **Step 4: The world file module**

Create `crates/ofs-config/src/world.rs`:

```rust
//! World description files (TOML): where the pilot stands, the goggles' antennas, the objects on the field and other
//! transmitters. The simulator uses them for the video link; the Godot client draws the objects. Positions are NED
//! metres from home, like the quad file's (up is a negative `d`); sizes are `[north, east, height]`.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Checker, ConfigError, Problem};

pub const WORLD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AntennaKind {
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
        c.check(p.position_ned_m[2] <= 0.0, "pilot.position_ned_m", format!("the pilot must not be below the ground (d = {} > 0)", p.position_ned_m[2]));
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
```

- [ ] **Step 5: The shipped world and the quad's VTX antenna**

Create `worlds/flat.toml`:

```toml
# The grey-box field: a launch pad, two rows of pylons, three gates and four buildings on flat ground.
# The simulator uses it for the video link (line of sight, diffraction around the buildings, interference); the Godot
# client draws the objects from it. Nothing here collides with the drone: the simulator's ground is the plane d = 0.
#
# Positions are NED metres from home (north, east, down: up is negative); sizes are [north, east, height].
# All values are estimates.
schema_version = 1
name = "Flat field"

[pilot]
# Beside the launch pad, goggles 1.7 m up, looking north along the course.
position_ned_m = [-3.0, 2.0, -1.7]
facing_deg = 0.0

[receiver]
noise_floor_dbm = -93.0
diversity = true

# Diversity goggles: an upright omni and a patch aimed ahead, tilted 10 degrees up.
[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"

[[receiver.antennas]]
name = "patch"
kind = "patch"
gain_dbi = 8.0
beamwidth_deg = 60.0
polarization = "rhcp"
aim_az_deg = 0.0
aim_el_deg = 10.0

# Another quad's VTX, left on at the west edge of the field on R2, next to the default R1.
[[emitters]]
name = "parked-quad"
position_ned_m = [30.0, -60.0, -1.0]
band = "R"
channel = 2
power_mw = 25.0

[[objects]]
name = "LaunchPad"
shape = "box"
center_ned_m = [0.0, 0.0, -0.01]
size_m = [3.0, 3.0, 0.02]
color = [0.78, 0.78, 0.76]

# Two rows of pylons along the course, every 15 m, 6 m either side of it.
[[objects]]
name = "Pylon1L"
shape = "cylinder"
center_ned_m = [15.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon1R"
shape = "cylinder"
center_ned_m = [15.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon2L"
shape = "cylinder"
center_ned_m = [30.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon2R"
shape = "cylinder"
center_ned_m = [30.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon3L"
shape = "cylinder"
center_ned_m = [45.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon3R"
shape = "cylinder"
center_ned_m = [45.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon4L"
shape = "cylinder"
center_ned_m = [60.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon4R"
shape = "cylinder"
center_ned_m = [60.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon5L"
shape = "cylinder"
center_ned_m = [75.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon5R"
shape = "cylinder"
center_ned_m = [75.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon6L"
shape = "cylinder"
center_ned_m = [90.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon6R"
shape = "cylinder"
center_ned_m = [90.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon7L"
shape = "cylinder"
center_ned_m = [105.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

[[objects]]
name = "Pylon7R"
shape = "cylinder"
center_ned_m = [105.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon8L"
shape = "cylinder"
center_ned_m = [120.0, -6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [1.0, 0.45, 0.1]

[[objects]]
name = "Pylon8R"
shape = "cylinder"
center_ned_m = [120.0, 6.0, -1.5]
radius_m = 0.15
height_m = 3.0
color = [0.92, 0.92, 0.9]

# Three gates on the course: two posts and a top bar each.
[[objects]]
name = "Gate0PostL"
shape = "box"
center_ned_m = [30.0, -1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate0PostR"
shape = "box"
center_ned_m = [30.0, 1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate0Bar"
shape = "box"
center_ned_m = [30.0, 0.0, -2.5]
size_m = [0.12, 3.32, 0.12]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate1PostL"
shape = "box"
center_ned_m = [60.0, -1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.95, 0.8, 0.15]

[[objects]]
name = "Gate1PostR"
shape = "box"
center_ned_m = [60.0, 1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.95, 0.8, 0.15]

[[objects]]
name = "Gate1Bar"
shape = "box"
center_ned_m = [60.0, 0.0, -2.5]
size_m = [0.12, 3.32, 0.12]
color = [0.95, 0.8, 0.15]

[[objects]]
name = "Gate2PostL"
shape = "box"
center_ned_m = [90.0, -1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate2PostR"
shape = "box"
center_ned_m = [90.0, 1.6, -1.25]
size_m = [0.12, 0.12, 2.5]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate2Bar"
shape = "box"
center_ned_m = [90.0, 0.0, -2.5]
size_m = [0.12, 3.32, 0.12]
color = [0.2, 0.8, 0.35]

# Concrete buildings: they block the video signal.
[[objects]]
name = "BuildingA"
shape = "box"
center_ned_m = [110.0, -45.0, -7.0]
size_m = [12.0, 14.0, 14.0]
color = [0.62, 0.64, 0.66]
rf_loss_db = 25.0

[[objects]]
name = "BuildingB"
shape = "box"
center_ned_m = [75.0, 38.0, -11.0]
size_m = [10.0, 10.0, 22.0]
color = [0.527, 0.544, 0.561]
rf_loss_db = 30.0

[[objects]]
name = "BuildingC"
shape = "box"
center_ned_m = [140.0, 70.0, -5.0]
size_m = [18.0, 30.0, 10.0]
color = [0.658, 0.676, 0.694]
rf_loss_db = 20.0

[[objects]]
name = "BuildingD"
shape = "box"
center_ned_m = [30.0, -80.0, -9.0]
size_m = [12.0, 12.0, 18.0]
color = [0.589, 0.608, 0.627]
rf_loss_db = 25.0
```

It carries every object `godot/world/world.gd` draws today, converted to NED (Godot `(x, y, z)` is NED `(-z, x, -y)`; a Godot box size `(x, y, z)` is `[z, x, y]` here). Building colours are the ones `world.gd` computes (`CONCRETE.darkened(0.15)` is `[0.527, 0.544, 0.561]`, and so on).

In `quads/opendrone-5f-freestyle.toml`, replace:

```toml
default_power_index = 1
reply_latency_ms = 5.0
```

with:

```toml
default_power_index = 1
reply_latency_ms = 5.0
# Pit mode (0 dBm in SmartAudio) leaves a trickle: a picture only next to the pilot.
pit_power_mw = 0.1

# The video link model's view of the VTX antenna: a 2 dBi RHCP omni on the back of the frame, leaning back.
[vtx.antenna]
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
mount_frd = [-0.5, 0.0, -1.0]
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p ofs-config --locked`
Expected: PASS: 16 config tests and 11 world tests.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-config/src/lib.rs crates/ofs-config/src/world.rs crates/ofs-config/tests/world.rs worlds/flat.toml quads/opendrone-5f-freestyle.toml
git commit -m "feat(config): world files (pilot, goggle antennas, objects, emitters) and the quad's VTX antenna"
```

### Task 4: The link in the vehicle (`ofs-sim`)

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs` (test helper), `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/tests/open_loop_vehicle.rs`, `crates/ofs-sim/tests/sitl_live.rs`
- Modify (M3a expectations, see the ruling): `crates/ofs-client/tests/session.rs`, `python/tests/test_client.py`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 2's `VideoLink`, `LinkParams`, `LinkWorld`, `ReceiverAntenna`, `Emitter`, `VideoSync`, `FIELD_RATE_HZ`; Task 1's `Antenna`, `AntennaKind`, `Obstacle`, `Polarization`, `Shape`; Task 3's `WorldConfig` and `VtxSection` fields.
- Produces (used by Task 5):
  - `BuildOptions` gains `pub world: WorldConfig` (every construction passes one; tests use `WorldConfig::open_field()`).
  - `pub struct VideoInfo { present, snr_db, interference_dbm, rssi_dbm: Vec<(String, f64)>, active_antenna: String, noise, sparkles, chroma, sync: VideoSync }` (`Default`: absent, clean), `VehicleState.video: VideoInfo`.
  - `Vehicle::world(&self) -> &WorldConfig`.
  - `pub fn link_params(vtx: &VtxSection, world: &WorldConfig) -> Result<VideoLinkParams, SimError>`, `pub fn aim_ned(heading_deg, elevation_deg) -> DVec3`, `pub(crate) fn emitter_freq_mhz(&EmitterSection) -> Result<f64, SimError>`.

**Ruling (from "Rulings and facts verified in advance"): the VTX transmits in open loop.** `build` adds the VTX model to open-loop vehicles too (fresh wires, no SmartAudio traffic: it stays on its power-up channel and power), and the `VideoLink` model whenever the quad has `[vtx]`. Four M3a tests asserted "no VTX in open loop"; Step 5 updates them.

- [ ] **Step 1: Write the failing tests**

The vehicle's unit tests (the test module of `vehicle.rs`):

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    }

    #[test]
    fn an_open_loop_vehicle_has_no_osd_and_no_vtx() {
        let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-unit-test-data"), fc_override: Some(FcKind::OpenLoop) };
        let mut vehicle = build(&shipped_quad(), &opts).unwrap();
        vehicle.run_for(0.05).unwrap();
        let state = vehicle.state();
        assert_eq!(state.vtx, VtxInfo { present: false, band: 0, channel: 0, freq_mhz: 0, power_mw: 0, pit_mode: false });
        assert_eq!(state.serial_dropped_bytes, 0);
        assert!(vehicle.osd_frame().is_none());
    }
}
```

with:

```rust
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
        build(cfg, &opts).unwrap()
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
        assert_eq!(state.video, VideoInfo::default());
    }

    #[test]
    fn the_link_parameters_follow_the_world_file() {
        let world = flat_world();
        let params = link_params(shipped_quad().vtx.as_ref().unwrap(), &world).unwrap();
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
}
```

Every other `BuildOptions` construction gets a world:

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, replace:

```rust
use std::path::Path;

use ofs_config::{load, FcKind};
use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Fault, Sticks, Vehicle};

```

with:

```rust
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Fault, Sticks, Vehicle};

```

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, replace:

```rust
fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop) };
    build(&cfg, &opts).unwrap()
}
```

with:

```rust
fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}
```

In `crates/ofs-sim/tests/sitl_live.rs`, replace:

```rust
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(Path::new(QUAD)).unwrap();
        let v = build(&cfg, &BuildOptions { seed: 1, data_dir: dir.path().to_path_buf(), fc_override: None }).unwrap();
        Self { v, _dir: dir }
    }
```

with:

```rust
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(Path::new(QUAD)).unwrap();
        let opts = BuildOptions { seed: 1, data_dir: dir.path().to_path_buf(), fc_override: None, world: ofs_config::WorldConfig::open_field() };
        let v = build(&cfg, &opts).unwrap();
        Self { v, _dir: dir }
    }
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
        let quad = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
        let cfg = ofs_config::load(std::path::Path::new(quad)).unwrap();
        let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-unit-test-data"), fc_override: Some(FcKind::OpenLoop) };
        Session::new(vehicle::build(&cfg, &opts).unwrap(), mode, OverrunPolicy::Warn, false)
    }
```

with:

```rust
        let quad = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
        let cfg = ofs_config::load(std::path::Path::new(quad)).unwrap();
        let opts = BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-unit-test-data"),
            fc_override: Some(FcKind::OpenLoop),
            world: ofs_config::WorldConfig::open_field(),
        };
        Session::new(vehicle::build(&cfg, &opts).unwrap(), mode, OverrunPolicy::Warn, false)
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-sim --lib --locked`
Expected: FAIL: does not compile (`BuildOptions` has no field `world`, `VideoInfo` and `link_params` do not exist).

- [ ] **Step 3: Wire the world, the open-loop VTX and the link into the vehicle**

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust

use glam::{DQuat, DVec3};
use ofs_config::{FcKind, QuadConfig};
use ofs_core::rng::fnv1a64;
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
```

with:

```rust

use glam::{DQuat, DVec3};
use ofs_config::world::{self as world_cfg, WorldConfig};
use ofs_config::{FcKind, QuadConfig, VtxSection};
use ofs_core::rng::fnv1a64;
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_video::vtx::{band_index, VtxModel, VtxParams};

#[derive(Debug, Clone)]
```

with:

```rust
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};
use ofs_video::link::{Emitter, LinkParams as VideoLinkParams, LinkWorld, ReceiverAntenna, VideoLink, VideoSync, FIELD_RATE_HZ};
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_video::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
use ofs_video::vtx::{band_index, VtxModel, VtxParams, FREQUENCIES_MHZ};

#[derive(Debug, Clone)]
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
}

```

with:

```rust
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
    /// The field the quad flies in: the pilot's goggles, objects and other transmitters (the video link's world).
    pub world: WorldConfig,
}

```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
}

/// Faults a script can inject (spec §6.2). The v1 catalog completes in M4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

with:

```rust
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
        }
    }
}

/// Faults a script can inject (spec §6.2). The v1 catalog completes in M4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    /// TX bytes Betaflight's UART capture dropped because a consumer fell behind.
    pub serial_dropped_bytes: u64,
}

```

with:

```rust
    /// TX bytes Betaflight's UART capture dropped because a consumer fell behind.
    pub serial_dropped_bytes: u64,
    pub video: VideoInfo,
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
        }
    }

    fn read(&self, b: &Bus) -> VideoInfo {
        if b.get(self.present) < 0.5 {
            return VideoInfo::default();
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
        }
    }
}

```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    vtx_pit: Signal<f64>,
    serial_dropped: Signal<f64>,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
```

with:

```rust
    vtx_pit: Signal<f64>,
    serial_dropped: Signal<f64>,
    video: VideoHandles,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize, world: &WorldConfig) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
            vtx_pit: bus.signal(names::VTX_PIT),
            serial_dropped: bus.signal(names::FC_SERIAL_DROPPED),
        }
    }
```

with:

```rust
            vtx_pit: bus.signal(names::VTX_PIT),
            serial_dropped: bus.signal(names::FC_SERIAL_DROPPED),
            video: VideoHandles::register(bus, world),
        }
    }
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    sitl: bool,
    osd: Option<OsdHandle>,
}

```

with:

```rust
    sitl: bool,
    osd: Option<OsdHandle>,
    world: WorldConfig,
}

```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        let requests = Wire::new(VIDEO_TAP_CAPACITY);
        let replies = Wire::new(VIDEO_TAP_CAPACITY);
        let params = VtxParams {
            power_levels_mw: x.power_levels_mw.clone(),
            power_levels_dbm: x.power_levels_dbm.clone(),
            default_band: band_index(&x.default_band)
                .ok_or_else(|| SimError::InvalidArgument(format!("vtx.default_band {:?} is not a band letter", x.default_band)))?,
            default_channel: x.default_channel,
            default_power_index: x.default_power_index,
            reply_latency_s: x.reply_latency_ms / 1000.0,
        };
        v.taps.push(SerialTap { uart_index: x.uart - 1, tx: requests.clone() });
        v.serial.push(SerialLink { uart_index: x.uart - 1, rx: replies.clone() });
        v.after_fc.push(Box::new(VtxModel::new(params, requests, replies, fc_divisor, bus)));
    }
    Ok(v)
}

```

with:

```rust
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
    Ok(VtxModel::new(params, requests, replies, divisor, bus))
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
    let (h, e) = (heading_deg.to_radians(), elevation_deg.to_radians());
    DVec3::new(e.cos() * h.cos(), e.cos() * h.sin(), -e.sin())
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
pub fn link_params(vtx: &VtxSection, world: &WorldConfig) -> Result<VideoLinkParams, SimError> {
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
    let obstacles = world
        .objects
        .iter()
        .filter(|o| o.rf_loss_db > 0.0)
        .map(|o| {
            let center = v3(o.center_ned_m);
            let shape = match o.shape {
                world_cfg::Shape::Box => {
                    let s = o.size_m.unwrap_or_default();
                    Shape::Box { center, half: DVec3::new(s[0], s[1], s[2]) * 0.5 }
                }
                world_cfg::Shape::Cylinder => {
                    Shape::Cylinder { center, radius: o.radius_m.unwrap_or_default(), half_height: o.height_m.unwrap_or_default() * 0.5 }
                }
            };
            Obstacle { shape: shape.rooted(), rf_loss_db: o.rf_loss_db }
        })
        .collect();
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
    })
}

```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    let mut osd: Option<OsdHandle> = None;
    match fc_kind {
        FcKind::OpenLoop => models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus))),
        FcKind::Sitl => {
            let video = video_devices(cfg, n, fc_divisor, &mut bus)?;
```

with:

```rust
    let mut osd: Option<OsdHandle> = None;
    match fc_kind {
        FcKind::OpenLoop => {
            models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus)));
            // No Betaflight to talk to, but the VTX is on the quad all the same: it transmits its power-up channel.
            if let Some(x) = &cfg.vtx {
                models.push(Box::new(vtx_model(x, Wire::new(VIDEO_TAP_CAPACITY), Wire::new(VIDEO_TAP_CAPACITY), fc_divisor, &mut bus)?));
            }
        }
        FcKind::Sitl => {
            let video = video_devices(cfg, n, fc_divisor, &mut bus)?;
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        }
    }

    let h = Handles::register(&mut bus, n);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd };
    // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default());
```

with:

```rust
        }
    }
    if let Some(x) = &cfg.vtx {
        // Last in the tick: it reads the pose the physics wrote and the channel the VTX published.
        let params = link_params(x, &opts.world)?;
        models.push(Box::new(VideoLink::new(params, base_hz / FIELD_RATE_HZ, opts.seed, &mut bus)));
    }

    let h = Handles::register(&mut bus, n, &opts.world);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd, world: opts.world.clone() };
    // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default());
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
```

with:

```rust
    }

    /// The world the vehicle flies in (the open field when the session named none).
    pub fn world(&self) -> &WorldConfig {
        &self.world
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
            },
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
        }
    }
```

with:

```rust
            },
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
            video: h.video.read(b),
        }
    }
```

Notes for the reader:
- The ELRS link already imports a `LinkParams`, so the video one is imported as `VideoLinkParams`.
- `VideoHandles` registers the `video.*` signals even for a quad without `[vtx]` (they stay zero, `video.present = 0`), so `VehicleState.video` reads as absent then.
- The `VideoLink` model is added last: it reads the pose the physics wrote and the channel the VTX published in the same tick.

- [ ] **Step 4: The server builds with the open field until Task 5 adds `world_path`**

In `crates/ofs-sim/src/server.rs`, replace:

```rust
use ofs_config::FcKind;
```

with:

```rust
use ofs_config::{FcKind, WorldConfig};
```

In `crates/ofs-sim/src/server.rs`, replace:

```rust
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop) };
```

with:

```rust
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop), world: WorldConfig::open_field() };
```

- [ ] **Step 5: Update the M3a tests that expected no VTX in open loop**

In `crates/ofs-sim/src/server.rs`, replace:

```rust
        let state = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner();
        let vtx = state.vtx.expect("State.vtx is always set");
        assert!(!vtx.present);
        assert_eq!(state.serial_dropped_bytes, 0);
```

with:

```rust
        svc.run(Request::new(pb::RunRequest { seconds: 0.05 })).await.unwrap();
        let state = svc.get_state(Request::new(pb::Empty {})).await.unwrap().into_inner();
        let vtx = state.vtx.expect("State.vtx is always set");
        assert!(vtx.present && vtx.freq_mhz == 5658, "open loop: the VTX transmits its power-up channel: {vtx:?}");
        assert_eq!(state.serial_dropped_bytes, 0);
```

In `crates/ofs-client/tests/session.rs`, replace:

```rust
#[test]
fn a_quad_without_firmware_has_an_absent_osd_and_no_vtx() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the OSD stream delivers its first frame", SHORT, |p| p.client.osd().is_some());
    let osd = probe.client.osd().unwrap();
    assert!(!osd.present && osd.cols == 0 && osd.cells.is_empty(), "{osd:?}");
    assert!(probe.client.osd_version() >= 1);
    probe.wait("states arrive", SHORT, |p| p.client.telemetry().is_some());
    assert_eq!(probe.client.telemetry().unwrap().vtx, VtxInfo::default());
}
```

with:

```rust
#[test]
fn a_quad_without_firmware_has_an_absent_osd_but_its_vtx_transmits() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the OSD stream delivers its first frame", SHORT, |p| p.client.osd().is_some());
    let osd = probe.client.osd().unwrap();
    assert!(!osd.present && osd.cols == 0 && osd.cells.is_empty(), "{osd:?}");
    assert!(probe.client.osd_version() >= 1);
    probe.wait("states with the VTX arrive", SHORT, |p| p.client.telemetry().is_some_and(|t| t.vtx.present));
    let t = probe.client.telemetry().unwrap();
    assert_eq!(t.vtx, VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false });
}
```

In `python/tests/test_client.py`, replace:

```python
def test_osd_and_vtx_are_absent_without_firmware(sim):
    sim.load(QUAD, open_loop_fc=True)
    osd = sim.get_osd()
    assert (osd.present, osd.cols, osd.rows) == (False, 0, 0)
    assert osd.rows_text() == [] and osd.text == ""
    state = sim.state()
    assert state.vtx == ofs.Vtx() and not state.vtx.present
    assert state.serial_dropped_bytes == 0
```

with:

```python
def test_without_firmware_the_osd_is_absent_but_the_vtx_transmits(sim):
    sim.load(QUAD, open_loop_fc=True)
    osd = sim.get_osd()
    assert (osd.present, osd.cols, osd.rows) == (False, 0, 0)
    assert osd.rows_text() == [] and osd.text == ""
    state = sim.run(0.1)
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False)
    assert state.serial_dropped_bytes == 0
```

In `godot/tests/e2e_open_loop.gd`, replace:

```gdscript
	_check(t.has("vtx_present") and not t["vtx_present"], "open loop: no VTX")
```

with:

```gdscript
	_check(t.get("vtx_present", false) and t.get("vtx_freq_mhz", 0) == 5658, "open loop: the VTX transmits R1: %s" % str(t.get("vtx_freq_mhz")))
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test --workspace --locked`
Expected: PASS. The vehicle tests show the open-loop VTX, the link parameters from `worlds/flat.toml`, a clean picture 10 m out, a lost one 4 km out at 25 mW, and building B costing more than 15 dB; `same_seed_and_inputs_give_identical_runs` (open_loop_vehicle.rs) now also covers the fading, since the bus digest includes the `video.*` signals.

Run: `cargo build -p ofs-sim --locked && python -m pytest python/tests/test_client.py -q`
Expected: PASS.

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh e2e`
Expected: the open-loop e2e passes with "open loop: the VTX transmits R1" (the Betaflight one skips without `OFS_SITL_LAUNCH`).

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-sim/src/vehicle.rs crates/ofs-sim/src/session.rs crates/ofs-sim/src/server.rs crates/ofs-sim/tests/open_loop_vehicle.rs crates/ofs-sim/tests/sitl_live.rs crates/ofs-client/tests/session.rs python/tests/test_client.py godot/tests/e2e_open_loop.gd
git commit -m "feat(sim): the video link in the vehicle; the VTX transmits in open loop too"
```

### Task 5: Protocol 4: the world and the video link over gRPC

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`, `crates/ofs-sim/src/session.rs`, `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/tests/grpc.rs`, `python/ofs/client.py` (the version only), `python/ofs/v1/*` (regenerated)
- Modify (keep the workspace compiling): `crates/ofs-client/src/model.rs` (the two event kinds and `Settings.world_path`), `crates/ofs-client/src/worker.rs` (the load passes it)

**Interfaces:**
- Consumes: Task 4's `VideoInfo`, `VehicleState.video`, `Vehicle::world()`, `emitter_freq_mhz`; Task 3's `world::load`, `WorldConfig::open_field`.
- Produces (used by Tasks 6, 7, 8):
  - Proto: `LoadRequest.world_path = 7`; `rpc GetWorld(Empty) returns (World)`; `enum VideoSync { VIDEO_SYNC_UNSPECIFIED, VIDEO_SYNC_LOCKED, VIDEO_SYNC_UNSTABLE, VIDEO_SYNC_LOST }`; `message AntennaRssi { name, rssi_dbm }`; `message VideoLink { present, snr_db, interference_dbm, rssi, active_antenna, noise, sparkles, chroma, sync }`; `State.video = 16`; `EVENT_KIND_VIDEO_LOST = 11`, `EVENT_KIND_VIDEO_RESTORED = 12`; `message ReceiverAntenna { name, kind, gain_dbi, beamwidth_deg, polarization, aim_az_deg, aim_el_deg }`, `message WorldObject { name, shape, center_ned_m, size_m, radius_m, height_m, color, rf_loss_db }`, `message Emitter { name, position_ned_m, freq_mhz, power_mw }`, `message World { name, pilot_position_ned_m, pilot_facing_deg, antennas, objects, emitters }`.
  - `PROTOCOL_VERSION = 4` (Rust and Python).
  - Server: `Load` reads `world_path` (empty: the open field; a bad path: `config` error, nothing loaded); `GetWorld` answers the loaded world (`not_loaded` before a load); `video_lost` / `video_restored` events with "SNR -2.0 dB on omni".
  - `ofs_client`: `EventKind::{VideoLost, VideoRestored}` ("video_lost", "video_restored"), `Settings.world_path: String` (empty: the open field).

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-proto/tests/messages.rs`, replace:

```rust
use ofs_proto::pb::{PilotInput, Sticks};
use ofs_proto::PROTOCOL_VERSION;
use prost::Message;
```

with:

```rust
use ofs_proto::pb::{PilotInput, State, Sticks, VideoLink, VideoSync};
use ofs_proto::PROTOCOL_VERSION;
use prost::Message;
```

In `crates/ofs-proto/tests/messages.rs`, replace:

```rust
fn the_protocol_version_matches_the_python_client() {
    // python/ofs/client.py PROTOCOL_VERSION must equal this; bump both together.
    assert_eq!(PROTOCOL_VERSION, 3);
    let python = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../python/ofs/client.py")).unwrap();
    assert!(python.contains(&format!("PROTOCOL_VERSION = {PROTOCOL_VERSION}")), "python client speaks another protocol");
```

with:

```rust
fn the_protocol_version_matches_the_python_client() {
    // python/ofs/client.py PROTOCOL_VERSION must equal this; bump both together.
    assert_eq!(PROTOCOL_VERSION, 4);
    let python = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../python/ofs/client.py")).unwrap();
    assert!(python.contains(&format!("PROTOCOL_VERSION = {PROTOCOL_VERSION}")), "python client speaks another protocol");
```

In `crates/ofs-proto/tests/messages.rs`, replace:

```rust
    assert_eq!(PilotInput::decode(bytes.as_slice()).unwrap(), input);
}
```

with:

```rust
    assert_eq!(PilotInput::decode(bytes.as_slice()).unwrap(), input);
}

#[test]
fn a_state_from_an_older_server_reads_as_no_video_link() {
    let bytes = State { time_s: 1.0, ..Default::default() }.encode_to_vec();
    let state = State::decode(bytes.as_slice()).unwrap();
    assert!(state.video.is_none());
    assert_eq!(VideoLink::default().sync(), VideoSync::Unspecified);
}
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust

    #[test]
    fn the_osd_message_packs_cells_row_major() {
        use ofs_video::osd::{Cell, OsdFrame, OsdGrid};
```

with:

```rust

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
        };
        let video_events = |events: Vec<pb::Event>| {
            events
                .into_iter()
                .filter(|e| e.kind == pb::EventKind::VideoLost as i32 || e.kind == pb::EventKind::VideoRestored as i32)
                .collect::<Vec<_>>()
        };
        assert!(video_events(session.events_for(&s)).is_empty(), "the first sample is not a change");
        s.video.sync = VideoSync::Unstable;
        assert!(video_events(session.events_for(&s)).is_empty(), "tearing is not a loss");
        s.video.sync = VideoSync::Lost;
        s.video.snr_db = -2.04;
        let lost = video_events(session.events_for(&s));
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].kind, pb::EventKind::VideoLost as i32);
        assert_eq!(lost[0].message, "video lost: SNR -2.0 dB on omni");
        assert!(video_events(session.events_for(&s)).is_empty(), "still lost: no new event");
        s.video.sync = VideoSync::Locked;
        s.video.snr_db = 9.0;
        let back = video_events(session.events_for(&s));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].kind, pb::EventKind::VideoRestored as i32);
        assert_eq!(back[0].message, "video restored: SNR 9.0 dB on omni");
    }

    #[test]
    fn the_video_message_says_absent_without_a_vtx() {
        let msg = video_msg(&VideoInfo::default());
        assert!(!msg.present);
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
```

In `crates/ofs-sim/src/server.rs`, replace:

```rust
        assert!(vtx.present && vtx.freq_mhz == 5658, "open loop: the VTX transmits its power-up channel: {vtx:?}");
        assert_eq!(state.serial_dropped_bytes, 0);
        svc.shutdown();
    }
```

with:

```rust
        assert!(vtx.present && vtx.freq_mhz == 5658, "open loop: the VTX transmits its power-up channel: {vtx:?}");
        assert_eq!(state.serial_dropped_bytes, 0);
        let video = state.video.expect("State.video is always set");
        assert!(video.present);
        assert_eq!(video.sync(), pb::VideoSync::Locked, "{video:?}");
        assert_eq!(video.rssi.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["omni"]);
        svc.shutdown();
    }

    #[tokio::test]
    async fn get_world_returns_the_sessions_world() {
        let svc = SimService::new(std::env::temp_dir().join("ofs-unit-test-data"));
        assert_eq!(kind_of(&svc.get_world(Request::new(pb::Empty {})).await.unwrap_err()), "not_loaded");
        *svc.shared.lock() = Some(open_loop_session(RunMode::Lockstep));
        let world = svc.get_world(Request::new(pb::Empty {})).await.unwrap().into_inner();
        assert_eq!(world.name, "open field");
        assert_eq!(world.antennas.len(), 1);
        assert_eq!((world.antennas[0].kind.as_str(), world.antennas[0].aim_el_deg), ("omni", 90.0));
        assert!(world.objects.is_empty() && world.emitters.is_empty());
        svc.shutdown();
    }
```

In `crates/ofs-sim/tests/grpc.rs`, replace:

```rust
use ofs_sim::pb::{
    fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, PilotInput, RadioLinkLoss, RunRequest, Sticks,
    StreamRequest,
};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
```

with:

```rust
use ofs_sim::pb::{
    fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, PilotInput, RadioLinkLoss, RunRequest, Sticks,
    StreamRequest, VideoSync,
};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
```

In `crates/ofs-sim/tests/grpc.rs`, replace:

```rust
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

```

with:

```rust
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

const WORLD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml");

#[tokio::test]
async fn a_bad_world_path_is_a_config_error_and_loads_nothing() {
    let mut c = start().await;
    let req = LoadRequest { world_path: "no/such/world.toml".into(), ..open_loop(Mode::Lockstep) };
    let err = c.load(req).await.unwrap_err();
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("no/such/world.toml"), "{}", err.message());
    assert_eq!(kind(&c.get_world(Empty {}).await.unwrap_err()), "not_loaded");
}

#[tokio::test]
async fn a_session_flies_in_the_world_it_was_loaded_with() {
    let mut c = start().await;
    c.load(LoadRequest { world_path: WORLD.into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
    let world = c.get_world(Empty {}).await.unwrap().into_inner();
    assert_eq!(world.name, "Flat field");
    assert_eq!(world.objects.len(), 30);
    assert_eq!(world.antennas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["omni", "patch"]);
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    let video = s.video.unwrap();
    assert!(video.present);
    assert_eq!(video.rssi.len(), 2);
    assert_eq!(video.sync(), VideoSync::Locked, "on the launch pad: {video:?}");
    assert!(video.snr_db > 40.0, "{video:?}");
    c.load(open_loop(Mode::Lockstep)).await.unwrap();
    assert_eq!(c.get_world(Empty {}).await.unwrap().into_inner().name, "open field", "a load without a world: the open field");
}

```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-proto -p ofs-sim --locked`
Expected: FAIL: does not compile (no `VideoSync`, `VideoLink`, `World`, `world_path`, `get_world`).

- [ ] **Step 3: Protocol 4**

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
package ofs.v1;

// Simulator control, protocol version 3: lockstep and real-time sessions, state and event streams, the
// pilot link, fault injection, and OSD and VTX state. One session (loaded quad) per server.
service Sim {
  rpc Handshake(HandshakeRequest) returns (HandshakeReply);
```

with:

```protobuf
package ofs.v1;

// Simulator control, protocol version 4: lockstep and real-time sessions, state and event streams, the
// pilot link, fault injection, OSD and VTX state, and the world with its analog video link. One session (loaded quad)
// per server.
service Sim {
  rpc Handshake(HandshakeRequest) returns (HandshakeReply);
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  // checked at up to rate_hz (1..60; 0 means 60).
  rpc StreamOsd(StreamRequest) returns (stream OsdFrame);
}

```

with:

```protobuf
  // checked at up to rate_hz (1..60; 0 means 60).
  rpc StreamOsd(StreamRequest) returns (stream OsdFrame);
  // The world the session flies in, as loaded and validated (the built-in open field when Load named none).
  rpc GetWorld(Empty) returns (World);
}

```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  OverrunPolicy overrun_policy = 5;
  bool keep_alive = 6;  // keep the session when its last watcher disconnects
}
message LoadReply {
```

with:

```protobuf
  OverrunPolicy overrun_policy = 5;
  bool keep_alive = 6;  // keep the session when its last watcher disconnects
  string world_path = 7;  // a world file (worlds/flat.toml); empty: the built-in open field
}
message LoadReply {
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
}

message State {
  double time_s = 1;
```

with:

```protobuf
}

// The analog video link at the goggles (present = false without a VTX).
enum VideoSync {
  VIDEO_SYNC_UNSPECIFIED = 0;  // no video link
  VIDEO_SYNC_LOCKED = 1;
  VIDEO_SYNC_UNSTABLE = 2;     // tearing, line jitter
  VIDEO_SYNC_LOST = 3;         // rolling, static
}

message AntennaRssi {
  string name = 1;
  double rssi_dbm = 2;
}

message VideoLink {
  bool present = 1;
  double snr_db = 2;             // at the antenna in use
  double interference_dbm = 3;   // other emitters after the channel filter, at the antenna in use
  repeated AntennaRssi rssi = 4; // every goggle antenna, in the world file's order
  string active_antenna = 5;
  double noise = 6;     // picture grain, 0 (clean) to 1 (static)
  double sparkles = 7;  // 0 to 1
  double chroma = 8;    // colour, 1 (full) to 0 (black and white)
  VideoSync sync = 9;
}

message State {
  double time_s = 1;
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  Vtx vtx = 14;
  uint64 serial_dropped_bytes = 15;  // Betaflight UART output dropped because a consumer fell behind
}

```

with:

```protobuf
  Vtx vtx = 14;
  uint64 serial_dropped_bytes = 15;  // Betaflight UART output dropped because a consumer fell behind
  VideoLink video = 16;
}

```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  EVENT_KIND_VTX_CHANGED = 9;
  EVENT_KIND_SERIAL_OVERFLOW = 10;
}

```

with:

```protobuf
  EVENT_KIND_VTX_CHANGED = 9;
  EVENT_KIND_SERIAL_OVERFLOW = 10;
  EVENT_KIND_VIDEO_LOST = 11;      // the goggles lost sync
  EVENT_KIND_VIDEO_RESTORED = 12;  // and found it again
}

```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  }
}
```

with:

```protobuf
  }
}

// The world file, NED metres from home (up is negative). Strings name the kinds: antenna "omni" or "patch";
// polarization "rhcp", "lhcp" or "linear"; shape "box" or "cylinder".
message ReceiverAntenna {
  string name = 1;
  string kind = 2;
  double gain_dbi = 3;
  double beamwidth_deg = 4;  // patch only, else 0
  string polarization = 5;
  double aim_az_deg = 6;     // relative to the pilot's facing, clockwise
  double aim_el_deg = 7;     // up
}

message WorldObject {
  string name = 1;
  string shape = 2;
  Vec3 center_ned_m = 3;
  Vec3 size_m = 4;      // box: north, east, height
  double radius_m = 5;  // cylinder
  double height_m = 6;  // cylinder
  Vec3 color = 7;       // r, g, b in 0..1
  double rf_loss_db = 8;
}

message Emitter {
  string name = 1;
  Vec3 position_ned_m = 2;
  double freq_mhz = 3;
  double power_mw = 4;
}

message World {
  string name = 1;
  Vec3 pilot_position_ned_m = 2;
  double pilot_facing_deg = 3;  // degrees clockwise from north
  repeated ReceiverAntenna antennas = 4;
  repeated WorldObject objects = 5;
  repeated Emitter emitters = 6;
}
```

In `crates/ofs-proto/src/lib.rs`, replace:

```rust
//! Protocol 3 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
//! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.

```

with:

```rust
//! Protocol 4 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
//! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.

```

In `crates/ofs-proto/src/lib.rs`, replace:

```rust

/// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
pub const PROTOCOL_VERSION: u32 = 3;
```

with:

```rust

/// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
pub const PROTOCOL_VERSION: u32 = 4;
```

The Python client must speak the same version (`the_protocol_version_matches_the_python_client` checks it):

In `python/ofs/client.py`, replace:

```python
from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 3
UNLOAD_TIMEOUT_S = 5.0

```

with:

```python
from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 4
UNLOAD_TIMEOUT_S = 5.0

```

- [ ] **Step 4: The session's video message, events and world message**

In `crates/ofs-sim/src/session.rs`, replace:

```rust
use std::time::Instant;

use ofs_core::SimError;
use ofs_video::osd::OsdFrame;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{Vehicle, VehicleState, VtxInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

with:

```rust
use std::time::Instant;

use ofs_config::world::{self as world_cfg, WorldConfig};
use ofs_config::{AntennaKind, Polarization};
use ofs_core::SimError;
use ofs_video::link::VideoSync;
use ofs_video::osd::OsdFrame;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{VideoInfo, Vehicle, VehicleState, VtxInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
    last_vtx: Option<VtxInfo>,
    last_dropped: u64,
    /// Unit tests only: the next `step` panics (exercises panic containment).
    #[cfg(test)]
```

with:

```rust
    last_vtx: Option<VtxInfo>,
    last_dropped: u64,
    last_video_lost: Option<bool>,
    /// Unit tests only: the next `step` panics (exercises panic containment).
    #[cfg(test)]
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
    };
    format!("{channel} {} MHz {} mW{}", v.freq_mhz, v.power_mw, if v.pit_mode { " (pit mode)" } else { "" })
}

```

with:

```rust
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

/// The world as the protocol carries it. Emitters given by band and channel carry their frequency.
pub(crate) fn world_msg(w: &WorldConfig) -> pb::World {
    pb::World {
        name: w.name.clone(),
        pilot_position_ned_m: vec3_of(w.pilot.position_ned_m),
        pilot_facing_deg: w.pilot.facing_deg,
        antennas: w
            .receiver
            .antennas
            .iter()
            .map(|a| pb::ReceiverAntenna {
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
            })
            .collect(),
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
    }
}

```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            last_vtx: None,
            last_dropped: 0,
            #[cfg(test)]
            panic_on_step: false,
```

with:

```rust
            last_vtx: None,
            last_dropped: 0,
            last_video_lost: None,
            #[cfg(test)]
            panic_on_step: false,
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts,
    /// VTX changes, dropped UART bytes.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let s = self.vehicle.state();
```

with:

```rust

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts,
    /// VTX changes, dropped UART bytes, video sync lost or regained.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let s = self.vehicle.state();
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            self.last_dropped = s.serial_dropped_bytes;
            out.push(event(s.time_s, pb::EventKind::SerialOverflow, format!("{dropped} byte(s) of Betaflight UART output were dropped")));
        }
        out
```

with:

```rust
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
        out
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            }),
            serial_dropped_bytes: s.serial_dropped_bytes,
        }
    }
```

with:

```rust
            }),
            serial_dropped_bytes: s.serial_dropped_bytes,
            video: Some(video_msg(&s.video)),
        }
    }
```

A session raises `video_lost` when the goggles' sync goes to `Lost` and `video_restored` when it comes back; the first sample sets the state without an event (like the VTX's). Unstable sync (tearing) is not an event.

- [ ] **Step 5: `world_path` and `GetWorld` in the server**

In `crates/ofs-sim/src/server.rs`, replace:

```rust
use crate::pb::{self, sim_server::Sim};
use crate::runner;
use crate::session::{event, panic_message, RunMode, Session, Shared, Slot};
use crate::streams;
use crate::vehicle::{self, BuildOptions, Fault, Sticks};
```

with:

```rust
use crate::pb::{self, sim_server::Sim};
use crate::runner;
use crate::session::{event, panic_message, world_msg, RunMode, Session, Shared, Slot};
use crate::streams;
use crate::vehicle::{self, BuildOptions, Fault, Sticks};
```

In `crates/ofs-sim/src/server.rs`, replace:

```rust
        };
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop), world: WorldConfig::open_field() };
        let keep_alive = req.keep_alive;
        self.blocking(move |shared, slot| {
```

with:

```rust
        };
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let world = if req.world_path.is_empty() {
            WorldConfig::open_field()
        } else {
            ofs_config::world::load(Path::new(&req.world_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?
        };
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop), world };
        let keep_alive = req.keep_alive;
        self.blocking(move |shared, slot| {
```

In `crates/ofs-sim/src/server.rs`, replace:

```rust
    async fn get_osd(&self, _req: Request<pb::Empty>) -> Result<Response<pb::OsdFrame>, Status> {
        self.blocking(|_, slot| Ok(loaded(slot)?.osd_msg())).await
    }

```

with:

```rust
    async fn get_osd(&self, _req: Request<pb::Empty>) -> Result<Response<pb::OsdFrame>, Status> {
        self.blocking(|_, slot| Ok(loaded(slot)?.osd_msg())).await
    }

    async fn get_world(&self, _req: Request<pb::Empty>) -> Result<Response<pb::World>, Status> {
        self.blocking(|_, slot| Ok(world_msg(loaded(slot)?.vehicle.world()))).await
    }

```

- [ ] **Step 6: Keep the Rust client compiling**

`ofs-sim`'s tests depend on `ofs-client`, whose event match is exhaustive, and the load request gained a field:

In `crates/ofs-client/src/model.rs`, replace:

```rust
    VtxChanged,
    SerialOverflow,
    /// A kind this client does not know (a newer server).
    Unknown,
```

with:

```rust
    VtxChanged,
    SerialOverflow,
    VideoLost,
    VideoRestored,
    /// A kind this client does not know (a newer server).
    Unknown,
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            EventKind::VtxChanged => "vtx_changed",
            EventKind::SerialOverflow => "serial_overflow",
            EventKind::Unknown => "unknown",
        }
```

with:

```rust
            EventKind::VtxChanged => "vtx_changed",
            EventKind::SerialOverflow => "serial_overflow",
            EventKind::VideoLost => "video_lost",
            EventKind::VideoRestored => "video_restored",
            EventKind::Unknown => "unknown",
        }
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            Ok(pb::EventKind::VtxChanged) => EventKind::VtxChanged,
            Ok(pb::EventKind::SerialOverflow) => EventKind::SerialOverflow,
            Ok(pb::EventKind::Unspecified) | Err(_) => EventKind::Unknown,
        };
```

with:

```rust
            Ok(pb::EventKind::VtxChanged) => EventKind::VtxChanged,
            Ok(pb::EventKind::SerialOverflow) => EventKind::SerialOverflow,
            Ok(pb::EventKind::VideoLost) => EventKind::VideoLost,
            Ok(pb::EventKind::VideoRestored) => EventKind::VideoRestored,
            Ok(pb::EventKind::Unspecified) | Err(_) => EventKind::Unknown,
        };
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
    pub launch: Option<LaunchSpec>,
    pub quad_path: String,
    pub seed: u64,
    /// Fly without Betaflight: motor commands follow the throttle stick (a testing aid).
```

with:

```rust
    pub launch: Option<LaunchSpec>,
    pub quad_path: String,
    /// A world file (`worlds/flat.toml`); empty flies in the server's built-in open field.
    pub world_path: String,
    pub seed: u64,
    /// Fly without Betaflight: motor commands follow the throttle stick (a testing aid).
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            launch: None,
            quad_path: quad_path.into(),
            seed: 1,
            open_loop_fc: false,
```

with:

```rust
            launch: None,
            quad_path: quad_path.into(),
            world_path: String::new(),
            seed: 1,
            open_loop_fc: false,
```

In `crates/ofs-client/src/worker.rs`, replace:

```rust
        keep_alive: false,
    };
```

with:

```rust
        keep_alive: false,
        world_path: settings.world_path.clone(),
    };
```

- [ ] **Step 7: Regenerate the Python stubs**

Run: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`
Expected: `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi` and `sim_pb2_grpc.py` change (the new messages and `GetWorld`).

- [ ] **Step 8: Run the tests to see them pass**

Run: `cargo test --workspace --locked`
Expected: PASS (the new session, server and gRPC tests among them).

Run: `cargo build -p ofs-sim --locked && python -m pytest python/tests -q`
Expected: PASS (the SITL tests skip without `OFS_SITL_LAUNCH`).

- [ ] **Step 9: Commit**

```bash
git add proto/ofs/v1/sim.proto crates/ofs-proto crates/ofs-sim/src/session.rs crates/ofs-sim/src/server.rs crates/ofs-sim/tests/grpc.rs crates/ofs-client/src/model.rs crates/ofs-client/src/worker.rs python/ofs/client.py python/ofs/v1
git commit -m "feat(proto): protocol 4: the world (GetWorld, world_path) and the video link in the state, with sync events"
```

### Task 6: The Python client

**Files:**
- Modify: `python/ofs/client.py`, `python/ofs/__init__.py`, `python/tests/conftest.py`, `python/tests/test_client.py`, `python/tests/test_sitl_video.py`

**Interfaces:**
- Consumes: Task 5's protocol (`GetWorld`, `LoadRequest.world_path`, `State.video`, the two event kinds).
- Produces: `ofs.VideoLink` (`present, snr_db, interference_dbm, rssi: dict, active_antenna, noise, sparkles, chroma, sync: str`), `ofs.World`, `ofs.WorldObject`, `ofs.ReceiverAntenna`, `ofs.Emitter`, `State.video`, `Sim.load(..., world=None)`, `Sim.get_world() -> World`; event kinds "video_lost", "video_restored".

- [ ] **Step 1: Write the failing tests**

In `python/tests/conftest.py`, replace:

```python
REPO = pathlib.Path(__file__).resolve().parents[2]
QUAD = str(REPO / "quads" / "opendrone-5f-freestyle.toml")


```

with:

```python
REPO = pathlib.Path(__file__).resolve().parents[2]
QUAD = str(REPO / "quads" / "opendrone-5f-freestyle.toml")
WORLD = str(REPO / "worlds" / "flat.toml")


```

In `python/tests/test_client.py`, replace:

```python

import ofs
from conftest import QUAD


```

with:

```python

import ofs
from conftest import QUAD, WORLD


```

In `python/tests/test_client.py`, replace:

```python
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False)
    assert state.serial_dropped_bytes == 0


```

with:

```python
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False)
    assert state.serial_dropped_bytes == 0
    assert state.video.present and state.video.sync == "locked", state.video
    assert list(state.video.rssi) == ["omni"], "the open field's single antenna"


def test_a_session_flies_in_its_world(sim):
    sim.load(QUAD, open_loop_fc=True, world=WORLD)
    world = sim.get_world()
    assert world.name == "Flat field"
    assert world.pilot_position_ned_m == (-3.0, 2.0, -1.7)
    assert [a.name for a in world.antennas] == ["omni", "patch"]
    assert len(world.objects) == 30
    b = next(o for o in world.objects if o.name == "BuildingB")
    assert (b.shape, b.size_m, b.rf_loss_db) == ("box", (10.0, 10.0, 22.0), 30.0)
    assert [(e.name, e.freq_mhz) for e in world.emitters] == [("parked-quad", 5695.0)]
    video = sim.run(0.5).video
    assert video.present and video.sync == "locked" and video.snr_db > 40.0, video
    assert list(video.rssi) == ["omni", "patch"] and video.active_antenna in video.rssi
    assert 0.0 <= video.noise <= 1.0 and video.chroma == 1.0
    sim.load(QUAD, open_loop_fc=True)
    assert sim.get_world().name == "open field"


def test_a_bad_world_path_is_a_config_error(sim):
    with pytest.raises(ofs.ConfigError):
        sim.load(QUAD, open_loop_fc=True, world="no/such/world.toml")
    with pytest.raises(ofs.NotLoaded):
        sim.get_world()


def test_video_messages_convert():
    from ofs.client import _event, _video
    from ofs.v1 import sim_pb2 as pb

    m = pb.VideoLink(present=True, snr_db=2.5, sync=pb.VIDEO_SYNC_LOST, active_antenna="patch",
                     rssi=[pb.AntennaRssi(name="omni", rssi_dbm=-90.0), pb.AntennaRssi(name="patch", rssi_dbm=-85.0)])
    v = _video(m)
    assert (v.sync, v.active_antenna, v.rssi) == ("lost", "patch", {"omni": -90.0, "patch": -85.0})
    assert _video(pb.VideoLink()).sync == "" and not _video(pb.VideoLink()).present
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_LOST)).kind == "video_lost"
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_RESTORED)).kind == "video_restored"


```

The live chain against Betaflight (M3a's SmartAudio VTX driving M3b's link): with the pilot 300 m from the launch pad, the Configurator's VTX tab (`MSP_SET_VTX_CONFIG`) moves the power, pit mode and channel.

In `python/tests/test_sitl_video.py`, replace:

```python
"""OSD, VTX and ESC telemetry through the API against Betaflight SITL (spec docs/superpowers/specs/2026-10-08-m3a).
Needs OFS_SITL_LAUNCH."""
import os
import pathlib
```

with:

```python
"""OSD, VTX, ESC telemetry and the analog video link through the API against Betaflight SITL (specs
docs/superpowers/specs/2026-10-08-m3a and 2026-10-09-m3b). Needs OFS_SITL_LAUNCH."""
import os
import pathlib
```

In `python/tests/test_sitl_video.py`, replace:

```python

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
```

with:

```python

import ofs
from conftest import QUAD, WORLD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
```

In `python/tests/test_sitl_video.py`, replace:

```python
    assert sim.get_osd().present, "the OSD never came back after the reboot"
    assert state.vtx.present and state.vtx.freq_mhz == 5658, state.vtx
```

with:

```python
    assert sim.get_osd().present, "the OSD never came back after the reboot"
    assert state.vtx.present and state.vtx.freq_mhz == 5658, state.vtx


def far_pilot_world(tmp_path):
    """The flat field with the pilot 300 m south of the launch pad, looking north at it: far enough for the VTX power
    to matter."""
    text = pathlib.Path(WORLD).read_text().replace("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-300.0, 0.0, -1.7]")
    path = tmp_path / "far.toml"
    path.write_text(text)
    return str(path)


R1, R2, R8 = 32, 33, 39  # SmartAudio channel index: (band - 1) * 8 + (channel - 1), Raceband is band 5


def set_vtx(msp, index, power_level, pit=0):
    """The Configurator's VTX tab: band and channel as one index, power level 1..4 (25, 200, 600, 1000 mW), pit mode."""
    msp.request(MSP_SET_VTX_CONFIG, bytes([index, 0, power_level, pit]))


def test_vtx_power_moves_the_snr_and_pit_mode_loses_the_picture(sim, tmp_path):
    sim.load(QUAD, seed=1, world=far_pilot_world(tmp_path))
    sim.run(6.0)
    sim.events()
    msp = Msp(sim)
    try:
        set_vtx(msp, R1, 1)
        low = sim.run(1.0)
        set_vtx(msp, R1, 3)
        high = sim.run(1.0)
        set_vtx(msp, R1, 3, pit=1)
        pit = sim.run(1.0)
    finally:
        msp.close()
    assert (low.vtx.power_mw, high.vtx.power_mw, pit.vtx.pit_mode) == (25, 600, True), (low.vtx, high.vtx, pit.vtx)
    gained = high.video.snr_db - low.video.snr_db
    assert abs(gained - 13.8) < 0.5, f"25 -> 600 mW is +13.8 dB: got {gained:.2f} ({low.video} -> {high.video})"
    assert low.video.sync == "locked" and high.video.sync == "locked", (low.video, high.video)
    assert pit.video.sync == "lost" and pit.video.noise > 0.9, pit.video
    lost = [e for e in sim.events() if e.kind == "video_lost"]
    assert lost and "SNR" in lost[-1].message, lost


def test_moving_off_the_parked_quads_channel_lowers_the_interference(sim, tmp_path):
    sim.load(QUAD, seed=1, world=far_pilot_world(tmp_path))
    sim.run(6.0)
    msp = Msp(sim)
    try:
        set_vtx(msp, R1, 3)
        r1 = sim.run(1.0)
        set_vtx(msp, R2, 3)  # the parked quad's channel
        r2 = sim.run(1.0)
        set_vtx(msp, R8, 3)
        r8 = sim.run(1.0)
    finally:
        msp.close()
    assert (r1.vtx.freq_mhz, r2.vtx.freq_mhz, r8.vtx.freq_mhz) == (5658, 5695, 5917)
    v1, v2, v8 = r1.video, r2.video, r8.video
    assert abs((v2.interference_dbm - v1.interference_dbm) - 22.8) < 0.5, (v1, v2)  # 37 MHz of rejection: 22.75 dB
    assert v8.interference_dbm < v2.interference_dbm - 39.0, (v2, v8)  # 222 MHz away: 40 dB
    assert v2.snr_db < v1.snr_db - 3.0, "on the parked quad's channel the picture gets worse"
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo build -p ofs-sim --locked && python -m pytest python/tests/test_client.py -q`
Expected: FAIL: `TypeError: load() got an unexpected keyword argument 'world'`, `AttributeError: 'State' object has no attribute 'video'` and `ImportError` for `_video`.

- [ ] **Step 3: The video link and the world in the client**

In `python/ofs/client.py`, replace:

```python

@dataclass(frozen=True)
class Osd:
    """Betaflight's OSD as a character grid. `cells` are row-major, each `char | page << 8 | blink << 10`."""
```

with:

```python

@dataclass(frozen=True)
class VideoLink:
    """The analog video link at the goggles (`present` is False when the quad has no VTX)."""
    present: bool = False
    snr_db: float = 0.0
    interference_dbm: float = 0.0
    rssi: dict = field(default_factory=dict)  # dBm by goggle antenna name, in the world file's order
    active_antenna: str = ""
    noise: float = 0.0  # grain, 0 (clean) to 1 (static)
    sparkles: float = 0.0
    chroma: float = 1.0  # colour, 1 (full) to 0 (black and white)
    sync: str = ""  # "locked", "unstable" (tearing), "lost" (rolling, static); "" without a VTX


@dataclass(frozen=True)
class ReceiverAntenna:
    name: str
    kind: str  # "omni" or "patch"
    gain_dbi: float
    beamwidth_deg: float  # patch only, else 0
    polarization: str  # "rhcp", "lhcp" or "linear"
    aim_az_deg: float
    aim_el_deg: float


@dataclass(frozen=True)
class WorldObject:
    name: str
    shape: str  # "box" or "cylinder"
    center_ned_m: tuple
    size_m: tuple  # box: (north, east, height)
    radius_m: float  # cylinder
    height_m: float  # cylinder
    color: tuple  # (r, g, b) in 0..1
    rf_loss_db: float


@dataclass(frozen=True)
class Emitter:
    name: str
    position_ned_m: tuple
    freq_mhz: float
    power_mw: float


@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters."""
    name: str
    pilot_position_ned_m: tuple
    pilot_facing_deg: float
    antennas: tuple
    objects: tuple
    emitters: tuple


@dataclass(frozen=True)
class Osd:
    """Betaflight's OSD as a character grid. `cells` are row-major, each `char | page << 8 | blink << 10`."""
```

In `python/ofs/client.py`, replace:

```python
    vtx: Vtx = field(default_factory=Vtx)
    serial_dropped_bytes: int = 0

    @property
```

with:

```python
    vtx: Vtx = field(default_factory=Vtx)
    serial_dropped_bytes: int = 0
    video: VideoLink = field(default_factory=VideoLink)

    @property
```

In `python/ofs/client.py`, replace:

```python
class Event:
    time_s: float
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended", "vtx_changed", "serial_overflow"
    message: str


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


```

with:

```python
class Event:
    time_s: float
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended", "vtx_changed", "video_lost"
    message: str


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


_SYNC_NAMES = {pb.VIDEO_SYNC_LOCKED: "locked", pb.VIDEO_SYNC_UNSTABLE: "unstable", pb.VIDEO_SYNC_LOST: "lost"}


def _video(m) -> VideoLink:
    return VideoLink(present=m.present, snr_db=m.snr_db, interference_dbm=m.interference_dbm,
                     rssi={r.name: r.rssi_dbm for r in m.rssi}, active_antenna=m.active_antenna, noise=m.noise,
                     sparkles=m.sparkles, chroma=m.chroma, sync=_SYNC_NAMES.get(m.sync, ""))


def _world(m) -> World:
    return World(
        name=m.name,
        pilot_position_ned_m=_v(m.pilot_position_ned_m),
        pilot_facing_deg=m.pilot_facing_deg,
        antennas=tuple(ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg,
                                       a.aim_el_deg) for a in m.antennas),
        objects=tuple(WorldObject(o.name, o.shape, _v(o.center_ned_m), _v(o.size_m), o.radius_m, o.height_m, _v(o.color),
                                  o.rf_loss_db) for o in m.objects),
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
    )


```

In `python/ofs/client.py`, replace:

```python
        vtx=Vtx(m.vtx.present, m.vtx.band, m.vtx.channel, m.vtx.freq_mhz, m.vtx.power_mw, m.vtx.pit_mode),
        serial_dropped_bytes=m.serial_dropped_bytes,
    )

```

with:

```python
        vtx=Vtx(m.vtx.present, m.vtx.band, m.vtx.channel, m.vtx.freq_mhz, m.vtx.power_mw, m.vtx.pit_mode),
        serial_dropped_bytes=m.serial_dropped_bytes,
        video=_video(m.video),
    )

```

In `python/ofs/client.py`, replace:

```python

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
             overrun_policy: str = "warn", keep_alive: bool = False) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
```

with:

```python

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
             overrun_policy: str = "warn", keep_alive: bool = False, world: str | None = None) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). `world` is a world file (e.g. "worlds/flat.toml"); without one the quad flies in the
        open field. Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
```

In `python/ofs/client.py`, replace:

```python
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive))
        self.configurator_address = reply.configurator_address
        return reply.quad_name
```

with:

```python
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive, world_path=str(world or "")))
        self.configurator_address = reply.configurator_address
        return reply.quad_name
```

In `python/ofs/client.py`, replace:

```python
        """The current OSD frame (`present` is False without an OSD)."""
        return _osd(self._call(self._stub.GetOsd, pb.Empty()))

    def stream_states(self, rate_hz: int = 60):
```

with:

```python
        """The current OSD frame (`present` is False without an OSD)."""
        return _osd(self._call(self._stub.GetOsd, pb.Empty()))

    def get_world(self) -> World:
        """The world the session flies in, as the server loaded it (the open field when `load` named none)."""
        return _world(self._call(self._stub.GetWorld, pb.Empty()))

    def stream_states(self, rate_hz: int = 60):
```

In `python/ofs/__init__.py`, replace:

```python
"""Python client for Open FPV Sim."""
from . import faults
from .client import Event, Osd, RadioLink, Sim, State, Vtx, connect, launch
from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgument, InvalidState, NotLoaded,
                     NumericalError, OfsError, PilotBusy, ProtocolMismatch, ServerUnavailable)

__all__ = [
    "Sim", "State", "RadioLink", "Event", "Osd", "Vtx", "connect", "launch", "faults",
    "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
    "InvalidArgument", "InvalidState", "ServerUnavailable", "PilotBusy", "InternalError",
```

with:

```python
"""Python client for Open FPV Sim."""
from . import faults
from .client import (Emitter, Event, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World, WorldObject,
                     connect, launch)
from .errors import (ConfigError, FirmwareCrashed, InternalError, InvalidArgument, InvalidState, NotLoaded,
                     NumericalError, OfsError, PilotBusy, ProtocolMismatch, ServerUnavailable)

__all__ = [
    "Sim", "State", "RadioLink", "Event", "Osd", "Vtx", "VideoLink", "World", "WorldObject", "ReceiverAntenna", "Emitter",
    "connect", "launch", "faults",
    "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
    "InvalidArgument", "InvalidState", "ServerUnavailable", "PilotBusy", "InternalError",
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `python -m pytest python/tests/test_client.py -q`
Expected: PASS, 14 tests.

Run: `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf" OFS_SIM_BIN=<a firewall-allowed ofs-sim.exe> python -m pytest python/tests/test_sitl_video.py -q`
Expected: PASS, 8 tests (about 45 s). The power test sees +13.8 dB for 25 -> 600 mW, and pit mode loses sync with a `video_lost` event; the channel test sees +22.8 dB of interference on R2 and 40 dB less on R8. Afterwards `wsl.exe -d Ubuntu -e pgrep -x betaflight_SITL` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add python/ofs/client.py python/ofs/__init__.py python/tests/conftest.py python/tests/test_client.py python/tests/test_sitl_video.py
git commit -m "feat(python): the world and the video link in the Python client; live SmartAudio-to-link tests"
```

### Task 7: The Rust client (`ofs-client`)

**Files:**
- Modify: `crates/ofs-client/src/model.rs`, `crates/ofs-client/src/worker.rs`, `crates/ofs-client/src/client.rs`, `crates/ofs-client/tests/model.rs`, `crates/ofs-client/tests/session.rs`

**Interfaces:**
- Consumes: Task 5's protocol and `Settings.world_path`.
- Produces (used by Task 8 and 9 through `ofs-godot`):
  - `pub enum VideoSync { None, Locked, Unstable, Lost }` (`as_str`: "", "locked", "unstable", "lost"), `pub struct VideoInfo { present, snr_db, interference_dbm, rssi_dbm: Vec<(String, f64)>, active_antenna, noise, sparkles, chroma, sync }` (`from_pb`; `Default`: absent, clean), `Telemetry.video: VideoInfo`.
  - `pub struct World { name, pilot_position: DVec3, pilot_facing_deg, antennas: Vec<WorldAntenna>, objects: Vec<WorldObject>, emitters: Vec<WorldEmitter> }` in **Godot's frame** (`World::from_pb`): `WorldAntenna { name, kind, aim: DVec3 }`, `WorldObject { name, shape, center: DVec3, size: DVec3 /* east, height, north */, radius_m, height_m, color: [f64; 3], rf_loss_db }`, `WorldEmitter { name, position: DVec3, freq_mhz, power_mw }`.
  - `Client::world() -> Option<World>` (fetched once after each successful load, `None` while a load is under way), `Client::world_version() -> u64` (changes when it does).

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-client/tests/model.rs`, replace:

```rust
use ofs_client::{ClientError, ErrorKind, Event, EventKind, OsdFrame, Sticks, Telemetry, VtxInfo};
use ofs_proto::pb;
use tonic::metadata::MetadataMap;
```

with:

```rust
use glam::DVec3;
use ofs_client::{ClientError, ErrorKind, Event, EventKind, OsdFrame, Sticks, Telemetry, VideoInfo, VideoSync, VtxInfo, World};
use ofs_proto::pb;
use tonic::metadata::MetadataMap;
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
    assert_eq!(e.kind.as_str(), "serial_overflow");
}
```

with:

```rust
    assert_eq!(e.kind.as_str(), "serial_overflow");
}

#[test]
fn video_messages_convert() {
    let m = pb::VideoLink {
        present: true,
        snr_db: 7.5,
        interference_dbm: -100.0,
        rssi: vec![pb::AntennaRssi { name: "omni".into(), rssi_dbm: -90.0 }, pb::AntennaRssi { name: "patch".into(), rssi_dbm: -85.5 }],
        active_antenna: "patch".into(),
        noise: 0.6,
        sparkles: 0.1,
        chroma: 0.8,
        sync: pb::VideoSync::Unstable as i32,
    };
    let v = VideoInfo::from_pb(&m);
    assert_eq!(v.sync, VideoSync::Unstable);
    assert_eq!(v.sync.as_str(), "unstable");
    assert_eq!(v.rssi_dbm, vec![("omni".to_string(), -90.0), ("patch".to_string(), -85.5)]);
    assert_eq!((v.active_antenna.as_str(), v.snr_db, v.noise, v.chroma), ("patch", 7.5, 0.6, 0.8));
    let absent = VideoInfo::from_pb(&pb::VideoLink::default());
    assert_eq!(absent.sync, VideoSync::None);
    assert_eq!(absent.sync.as_str(), "");
    let t = Telemetry::from_pb(&pb::State::default());
    assert_eq!(t.video, VideoInfo::default(), "a state without video: no link, a clean picture");
    assert_eq!(t.video.chroma, 1.0);
}

#[test]
fn the_world_converts_to_godots_frame() {
    let v = |x, y, z| Some(pb::Vec3 { x, y, z });
    let w = World::from_pb(pb::World {
        name: "Flat field".into(),
        pilot_position_ned_m: v(-3.0, 2.0, -1.7),
        pilot_facing_deg: 0.0,
        antennas: vec![
            pb::ReceiverAntenna { name: "omni".into(), kind: "omni".into(), aim_el_deg: 90.0, ..Default::default() },
            pb::ReceiverAntenna { name: "patch".into(), kind: "patch".into(), aim_az_deg: 0.0, aim_el_deg: 10.0, ..Default::default() },
        ],
        objects: vec![pb::WorldObject {
            name: "BuildingA".into(),
            shape: "box".into(),
            center_ned_m: v(110.0, -45.0, -7.0),
            size_m: v(12.0, 14.0, 14.0),
            color: v(0.62, 0.64, 0.66),
            rf_loss_db: 25.0,
            ..Default::default()
        }],
        emitters: vec![pb::Emitter { name: "parked-quad".into(), position_ned_m: v(30.0, -60.0, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
    });
    assert_eq!(w.pilot_position, DVec3::new(2.0, 1.7, 3.0), "east, up, south");
    let b = &w.objects[0];
    assert_eq!(b.center, DVec3::new(-45.0, 7.0, -110.0), "where world.gd drew building A");
    assert_eq!(b.size, DVec3::new(14.0, 14.0, 12.0), "east, height, north");
    assert_eq!(b.color, [0.62, 0.64, 0.66]);
    assert!((w.antennas[0].aim - DVec3::Y).length() < 1e-12, "the omni stands up: {}", w.antennas[0].aim);
    let up = 10f64.to_radians();
    assert!((w.antennas[1].aim - DVec3::new(0.0, up.sin(), -up.cos())).length() < 1e-12, "the patch looks north (-Z), 10 degrees up");
    assert_eq!(w.emitters[0].position, DVec3::new(-60.0, 1.0, -30.0));
    let east = World::from_pb(pb::World { pilot_facing_deg: 90.0, antennas: vec![pb::ReceiverAntenna { aim_el_deg: 0.0, ..Default::default() }], ..Default::default() });
    assert!((east.antennas[0].aim - DVec3::X).length() < 1e-12, "facing east, aimed ahead: +X");
}

#[test]
fn the_video_event_kinds_have_names() {
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoLost as i32, message: "video lost: SNR -2.0 dB on omni".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::VideoLost, "video_lost"));
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoRestored as i32, message: String::new() });
    assert_eq!(e.kind.as_str(), "video_restored");
}
```

In `crates/ofs-client/tests/session.rs`, replace:

```rust

use common::*;
use ofs_client::{Command, ErrorKind, EventKind, Phase, Settings, Sticks, Update, VtxInfo};

fn flying(server: &TestServer) -> Probe {
```

with:

```rust

use common::*;
use ofs_client::{Command, ErrorKind, EventKind, Phase, Settings, Sticks, Update, VideoSync, VtxInfo};

fn flying(server: &TestServer) -> Probe {
```

In `crates/ofs-client/tests/session.rs`, replace:

```rust
    let t = probe.client.telemetry().unwrap();
    assert_eq!(t.vtx, VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false });
}
```

with:

```rust
    let t = probe.client.telemetry().unwrap();
    assert_eq!(t.vtx, VtxInfo { present: true, band: 5, channel: 1, freq_mhz: 5658, power_mw: 200, pit_mode: false });
    assert!(t.video.present && t.video.sync == VideoSync::Locked, "{:?}", t.video);
    let world = probe.client.world().expect("the world arrives with the load");
    assert_eq!(world.name, "open field", "no world_path: the open field");
}

#[test]
fn the_world_of_the_settings_arrives_with_the_load_and_again_after_a_reload() {
    let server = TestServer::start();
    let world_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml");
    let mut probe = Probe::start(Settings { world_path: world_path.into(), ..server.settings() });
    probe.wait_phase(Phase::Flying, LONG);
    let world = probe.client.world().expect("the world is there once flying");
    assert_eq!(world.name, "Flat field");
    assert_eq!(world.objects.len(), 30);
    assert_eq!(world.antennas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["omni", "patch"]);
    let version = probe.client.world_version();
    probe.wait("telemetry names the antennas", SHORT, |p| p.client.telemetry().is_some_and(|t| t.video.rssi_dbm.len() == 2));
    probe.client.send(Command::Reload);
    probe.wait("the world is fetched again", LONG, |p| p.client.world_version() >= version + 2 && p.client.world().is_some());
}

#[test]
fn a_missing_world_file_fails_with_a_config_error() {
    let server = TestServer::start();
    let mut probe = Probe::start(Settings { world_path: "no/such/world.toml".into(), ..server.settings() });
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Config));
    assert!(detail.contains("no/such/world.toml"), "{detail}");
    assert!(probe.client.world().is_none());
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-client --locked`
Expected: FAIL: does not compile (no `VideoInfo`, `VideoSync`, `World`, `Client::world`).

- [ ] **Step 3: The video link and the world in the client**

In `crates/ofs-client/src/model.rs`, replace:

```rust
use std::time::Duration;

use ofs_proto::pb;

use crate::error::{ClientError, ErrorKind};

pub const AUX_COUNT: usize = 4;
```

with:

```rust
use std::time::Duration;

use glam::DVec3;
use ofs_proto::pb;

use crate::error::{ClientError, ErrorKind};
use crate::frames::vec_to_godot;

pub const AUX_COUNT: usize = 4;
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
}

/// Betaflight's OSD as a character grid. `cells` are row-major, each `char | font_page << 8 | blink << 10`.
#[derive(Debug, Clone, PartialEq, Default)]
```

with:

```rust
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
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
    pub fc_restarts: u32,
    pub vtx: VtxInfo,
    /// Seconds since this state message arrived (filled in when the telemetry is read).
    pub age_s: f64,
```

with:

```rust
    pub fc_restarts: u32,
    pub vtx: VtxInfo,
    pub video: VideoInfo,
    /// Seconds since this state message arrived (filled in when the telemetry is read).
    pub age_s: f64,
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            fc_restarts: s.fc_restarts,
            vtx: s.vtx.as_ref().map(VtxInfo::from_pb).unwrap_or_default(),
            age_s: 0.0,
        }
```

with:

```rust
            fc_restarts: s.fc_restarts,
            vtx: s.vtx.as_ref().map(VtxInfo::from_pb).unwrap_or_default(),
            video: s.video.as_ref().map(VideoInfo::from_pb).unwrap_or_default(),
            age_s: 0.0,
        }
```

In `crates/ofs-client/src/worker.rs`, replace:

```rust
use crate::interp::{Sample, StateBuffer};
use crate::launch::ServerProcess;
use crate::model::{Command, Event, OsdFrame, OverrunPolicy, Phase, Settings, Sticks, Telemetry, Update};

/// The server frees a disconnected pilot's slot asynchronously, so a pilot that reconnects at once (a reload) can
```

with:

```rust
use crate::interp::{Sample, StateBuffer};
use crate::launch::ServerProcess;
use crate::model::{Command, Event, OsdFrame, OverrunPolicy, Phase, Settings, Sticks, Telemetry, Update, World};

/// The server frees a disconnected pilot's slot asynchronously, so a pilot that reconnects at once (a reload) can
```

In `crates/ofs-client/src/worker.rs`, replace:

```rust
    pub osd: Mutex<Option<OsdFrame>>,
    pub osd_version: AtomicU64,
}

```

with:

```rust
    pub osd: Mutex<Option<OsdFrame>>,
    pub osd_version: AtomicU64,
    pub world: Mutex<Option<World>>,
    pub world_version: AtomicU64,
}

```

In `crates/ofs-client/src/worker.rs`, replace:

```rust
        *lock(&self.telemetry) = None;
        self.store_osd(None);
    }

```

with:

```rust
        *lock(&self.telemetry) = None;
        self.store_osd(None);
        self.store_world(None);
    }

    fn store_world(&self, world: Option<World>) {
        *lock(&self.world) = world;
        self.world_version.fetch_add(1, Ordering::AcqRel);
    }

```

In `crates/ofs-client/src/worker.rs`, replace:

```rust
        Err(status) => return Fly::Failed(status_error(status)),
    };
    let _ = shared.updates.send(Update::Session { quad_name: reply.quad_name, configurator_address: reply.configurator_address });
    if let Err(status) = client.start(pb::Empty {}).await {
```

with:

```rust
        Err(status) => return Fly::Failed(status_error(status)),
    };
    match client.get_world(pb::Empty {}).await {
        Ok(world) => shared.store_world(Some(World::from_pb(world.into_inner()))),
        Err(status) => return Fly::Failed(status_error(status)),
    }
    let _ = shared.updates.send(Update::Session { quad_name: reply.quad_name, configurator_address: reply.configurator_address });
    if let Err(status) = client.start(pb::Empty {}).await {
```

In `crates/ofs-client/src/client.rs`, replace:

```rust
use crate::error::{ClientError, ErrorKind};
use crate::interp::{Pose, StateBuffer};
use crate::model::{Command, OsdFrame, Phase, Settings, Sticks, Telemetry, Update};
use crate::worker::{lock, run, Shared};

```

with:

```rust
use crate::error::{ClientError, ErrorKind};
use crate::interp::{Pose, StateBuffer};
use crate::model::{Command, OsdFrame, Phase, Settings, Sticks, Telemetry, Update, World};
use crate::worker::{lock, run, Shared};

```

In `crates/ofs-client/src/client.rs`, replace:

```rust
            osd: Mutex::new(None),
            osd_version: AtomicU64::new(0),
        });
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
```

with:

```rust
            osd: Mutex::new(None),
            osd_version: AtomicU64::new(0),
            world: Mutex::new(None),
            world_version: AtomicU64::new(0),
        });
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
```

In `crates/ofs-client/src/client.rs`, replace:

```rust
    }

    /// Ends the session (unloading it first when this client launched the server) and stops the supervisor.
    /// Also done on drop.
```

with:

```rust
    }

    /// The world of the loaded session, in Godot's frame; `None` before the first load and while one is under way.
    pub fn world(&self) -> Option<World> {
        lock(&self.shared.world).clone()
    }

    /// Counts world updates (and clears): a changed value means [`world`](Self::world) has something new to build.
    pub fn world_version(&self) -> u64 {
        self.shared.world_version.load(Ordering::Acquire)
    }

    /// Ends the session (unloading it first when this client launched the server) and stops the supervisor.
    /// Also done on drop.
```

A failed `GetWorld` right after a successful load fails the flight like a failed `Load` (the server answers it from the session it just built, so it only fails when the server went away).

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ofs-client --locked`
Expected: PASS (model: 10 tests, session: 14 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-client
git commit -m "feat(client): the world (in Godot's frame) and the video link in the Rust client"
```

### Task 8: The world in Godot

**Files:**
- Modify: `crates/ofs-godot/src/lib.rs` (`get_world`, `get_world_version`, the `world_path` setting), `godot/scripts/settings.gd`, `godot/world/world.gd` (rewritten), `godot/scripts/app.gd`
- Test: `godot/tests/test_scene.gd`, `godot/tests/test_settings.gd`, `godot/tests/test_extension.gd`

**Interfaces:**
- Consumes: Task 7's `Client::world()`, `world_version()`, `Settings.world_path`.
- Produces (used by Task 9):
  - `OfsClient.get_world() -> Dictionary` (`name`, `pilot_position: Vector3`, `antennas: [{name, kind, aim: Vector3}]`, `objects: [{name, shape, center: Vector3, size: Vector3, radius, height, color: Color, rf_loss_db}]`, `emitters: [{name, position: Vector3, freq_mhz, power_mw}]`; `{}` before a world arrives) and `get_world_version() -> int`; the `start` dictionary takes `world_path`.
  - Settings: `world_path` (default `<repo>/worlds/flat.toml`, `OFS_WORLD`, `--world=`), `video_effects` (default true, `OFS_VIDEO_EFFECTS`, `--video-effects=`), `AppSettings.save_value(key, value, path := CONFIG_PATH) -> bool`.
  - `world.gd`: `build(world: Dictionary)` (replaces the field's objects and markers; `{}` clears them), `built_names() -> PackedStringArray`; node names are the world file's object names, `Pilot` (with `Aim_<antenna>` pointers for patches) and `Emitter_<name>`.
  - `app.gd`: `@onready var world: Node3D = $World`; it rebuilds the field when `get_world_version()` changes.

- [ ] **Step 1: Write the failing tests**

In `godot/tests/test_scene.gd`, replace:

```gdscript


func test_the_world_has_ground_pad_and_landmarks() -> void:
	var world := World.new()
	await add_to_tree(world)
	ok(world.get_node_or_null("Ground") is MeshInstance3D, "ground")
	ok(world.get_node_or_null("LaunchPad") != null, "launch pad")
	ok(world.get_node_or_null("Pylon1L") != null and world.get_node_or_null("Pylon8R") != null, "pylons")
	ok(world.get_node_or_null("Gate0Bar") != null and world.get_node_or_null("Gate2PostR") != null, "gates")
	ok(world.get_node_or_null("BuildingA") != null, "buildings")
	var environments := world.find_children("*", "WorldEnvironment", false, false)
	var suns := world.find_children("*", "DirectionalLight3D", false, false)
	eq(environments.size(), 1, "one environment")
	eq(suns.size(), 1, "one sun")
	var ground: MeshInstance3D = world.get_node("Ground")
	ok(ground.material_override is ShaderMaterial, "the ground is drawn by the grid shader")
	world.queue_free()

```

with:

```gdscript


## A world like the one `OfsClient.get_world()` returns (Godot's frame).
func _world_dict() -> Dictionary:
	return {
		"name": "Test field",
		"pilot_position": Vector3(2.0, 1.7, 3.0),
		"antennas": [
			{"name": "omni", "kind": "omni", "aim": Vector3.UP},
			{"name": "patch", "kind": "patch", "aim": Vector3(0.0, 0.17, -0.98).normalized()},
		],
		"objects": [
			{"name": "BuildingA", "shape": "box", "center": Vector3(-45.0, 7.0, -110.0), "size": Vector3(14.0, 14.0, 12.0),
				"radius": 0.0, "height": 0.0, "color": Color(0.62, 0.64, 0.66), "rf_loss_db": 25.0},
			{"name": "Pylon1L", "shape": "cylinder", "center": Vector3(-6.0, 1.5, -15.0), "size": Vector3.ZERO,
				"radius": 0.15, "height": 3.0, "color": Color(0.92, 0.92, 0.9), "rf_loss_db": 0.0},
		],
		"emitters": [{"name": "parked-quad", "position": Vector3(-60.0, 1.0, -30.0), "freq_mhz": 5695.0, "power_mw": 25.0}],
	}


func test_the_world_draws_sky_and_ground_and_builds_the_rest_from_the_world_file() -> void:
	var world := World.new()
	await add_to_tree(world)
	ok(world.get_node_or_null("Ground") is MeshInstance3D, "ground")
	var ground: MeshInstance3D = world.get_node("Ground")
	ok(ground.material_override is ShaderMaterial, "the ground is drawn by the grid shader")
	eq(world.find_children("*", "WorldEnvironment", false, false).size(), 1, "one environment")
	eq(world.find_children("*", "DirectionalLight3D", false, false).size(), 1, "one sun")
	eq(world.built_names(), PackedStringArray(), "no objects before the simulator sends a world")
	world.build(_world_dict())
	var building: MeshInstance3D = world.get_node("BuildingA")
	eq(building.position, Vector3(-45.0, 7.0, -110.0), "a box where the world file puts it")
	eq((building.mesh as BoxMesh).size, Vector3(14.0, 14.0, 12.0), "at its size")
	eq((building.material_override as StandardMaterial3D).albedo_color, Color(0.62, 0.64, 0.66), "in its colour")
	var pylon: MeshInstance3D = world.get_node("Pylon1L")
	near((pylon.mesh as CylinderMesh).height, 3.0, "a cylinder")
	near((pylon.mesh as CylinderMesh).top_radius, 0.15, "its radius")
	var pilot: Node3D = world.get_node("Pilot")
	eq(pilot.position, Vector3(2.0, 1.7, 3.0), "the pilot marker's head is at the goggles")
	ok(pilot.get_node_or_null("Aim_patch") != null and pilot.get_node_or_null("Aim_omni") == null, "a pointer for the patch only")
	var emitter: Node3D = world.get_node("Emitter_parked-quad")
	eq(emitter.position, Vector3(-60.0, 0.0, -30.0), "the emitter's post stands on the ground below it")
	ok((emitter.get_node("Label") as Label3D).text.contains("5695 MHz"), "labelled with its frequency")
	world.queue_free()


func test_a_new_world_replaces_the_old_one() -> void:
	var world := World.new()
	await add_to_tree(world)
	world.build(_world_dict())
	eq(world.built_names().size(), 4, "two objects, the pilot, one emitter")
	var smaller := _world_dict()
	smaller["objects"] = [smaller["objects"][1]]
	smaller["emitters"] = []
	world.build(smaller)
	eq(world.built_names(), PackedStringArray(["Pylon1L", "Pilot"]), "only the new world's nodes")
	ok(world.get_node_or_null("BuildingA") == null, "the old building is gone at once")
	world.build({})
	eq(world.built_names(), PackedStringArray(), "an empty world clears the field")
	ok(world.get_node_or_null("Ground") != null, "but keeps the ground")
	world.queue_free()

```

In `godot/tests/test_scene.gd`, replace:

```gdscript
	var world := World.new()
	await add_to_tree(world)
	eq(world.find_children("*", "CollisionObject3D", true, false).size(), 0, "nothing to collide with: the simulator knows only the ground")
	world.queue_free()
```

with:

```gdscript
	var world := World.new()
	await add_to_tree(world)
	world.build(_world_dict())
	eq(world.find_children("*", "CollisionObject3D", true, false).size(), 0, "nothing to collide with: the simulator knows only the ground")
	world.queue_free()
```

In `godot/tests/test_settings.gd`, replace:

```gdscript
	eq(s["server_addr"], "127.0.0.1:50051", "server address")
	eq(s["open_loop"], false, "Betaflight by default")
	ok(s["server_bin"].ends_with("ofs-sim") or s["server_bin"].ends_with("ofs-sim.exe"), "server binary %s" % s["server_bin"])

```

with:

```gdscript
	eq(s["server_addr"], "127.0.0.1:50051", "server address")
	eq(s["open_loop"], false, "Betaflight by default")
	ok(s["world_path"].begins_with(root) and s["world_path"].ends_with("flat.toml"), "world path %s" % s["world_path"])
	ok(FileAccess.file_exists(s["world_path"]), "the shipped world exists at %s" % s["world_path"])
	eq(s["video_effects"], true, "video effects on by default")
	ok(s["server_bin"].ends_with("ofs-sim") or s["server_bin"].ends_with("ofs-sim.exe"), "server binary %s" % s["server_bin"])

```

In `godot/tests/test_settings.gd`, replace:

```gdscript
	ok(not d.has("server_bin") and not d.has("env"), "attach only")
	eq(d["quad_path"], s["quad_path"], "quad path")
```

with:

```gdscript
	ok(not d.has("server_bin") and not d.has("env"), "attach only")
	eq(d["quad_path"], s["quad_path"], "quad path")


func test_the_world_and_the_video_effects_come_from_every_source() -> void:
	var s := AppSettings.resolve({"video_effects": false}, {"OFS_WORLD": "worlds/other.toml"}, PackedStringArray())
	eq(s["world_path"], "worlds/other.toml", "OFS_WORLD")
	eq(s["video_effects"], false, "the file turns the effects off")
	var t := AppSettings.resolve({}, {}, PackedStringArray(["--world=w.toml", "--video-effects=no"]))
	eq(t["world_path"], "w.toml", "--world")
	eq(t["video_effects"], false, "--video-effects=no")
	eq(AppSettings.to_client_dict(t)["world_path"], "w.toml", "the client gets the world")


func test_a_setting_saved_in_the_game_keeps_the_rest_of_the_file() -> void:
	var path := "user://test_ofs_client.cfg"
	DirAccess.remove_absolute(path)
	var file := ConfigFile.new()
	file.set_value("client", "seed", 7)
	file.save(path)
	ok(AppSettings.save_value("video_effects", false, path), "saved")
	var back := ConfigFile.new()
	eq(back.load(path), OK, "the file reads back")
	eq(back.get_value("client", "video_effects"), false, "the new value")
	eq(back.get_value("client", "seed"), 7, "the other values stay")
	DirAccess.remove_absolute(path)
```

In `godot/tests/test_extension.gd`, replace:

```gdscript
	eq(client.get_osd_version(), 0, "no OSD updates")
	eq(client.get_telemetry(), {}, "no telemetry, so no VTX keys")
	client.queue_free()

```

with:

```gdscript
	eq(client.get_osd_version(), 0, "no OSD updates")
	eq(client.get_telemetry(), {}, "no telemetry, so no VTX keys")
	client.queue_free()


func test_the_world_getters_are_inert_before_start() -> void:
	var client := await _client()
	eq(client.get_world(), {}, "no world")
	eq(client.get_world_version(), 0, "no world updates")
	client.queue_free()

```

- [ ] **Step 2: Run the tests to see them fail**

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh unit`
Expected: FAIL: `test_scene.gd` (no `build`, the objects are still hard-coded), `test_settings.gd` (no `world_path`, `video_effects`, `save_value`), `test_extension.gd` (no `get_world`).

- [ ] **Step 3: The world in the extension**

In `crates/ofs-godot/src/lib.rs`, replace:

```rust

    /// Starts connecting; returns at once. Returns an empty string when it started, otherwise why it did not
    /// ("already started", or which setting is invalid). Keys: `quad_path` (required), `server_addr`, `server_bin` (a program to start when nothing
    /// answers), `data_dir`, `log_file`, `env` (Dictionary of String to String for the started server), `seed`,
    /// `open_loop`, `overrun_policy` ("warn" or "slow"), `state_rate_hz`, `stick_rate_hz`.
    #[func]
    fn start(&mut self, settings: VarDictionary) -> GString {
```

with:

```rust

    /// Starts connecting; returns at once. Returns an empty string when it started, otherwise why it did not
    /// ("already started", or which setting is invalid). Keys: `quad_path` (required), `world_path` (empty or absent:
    /// the open field), `server_addr`, `server_bin` (a program to start when nothing answers), `data_dir`, `log_file`,
    /// `env` (Dictionary of String to String for the started server), `seed`, `open_loop`, `overrun_policy` ("warn" or
    /// "slow"), `state_rate_hz`, `stick_rate_hz`.
    #[func]
    fn start(&mut self, settings: VarDictionary) -> GString {
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        self.client.as_ref().map_or(0, |c| c.osd_version() as i64)
    }
}

```

with:

```rust
        self.client.as_ref().map_or(0, |c| c.osd_version() as i64)
    }

    /// The loaded session's world in Godot's frame, or an empty Dictionary before it arrives: `name`,
    /// `pilot_position` (Vector3), `antennas` (Array of {`name`, `kind`, `aim`: Vector3}), `objects` (Array of {`name`,
    /// `shape` ("box" or "cylinder"), `center`: Vector3, `size`: Vector3 (a box's), `radius`, `height` (a cylinder's),
    /// `color`: Color, `rf_loss_db`}) and `emitters` (Array of {`name`, `position`: Vector3, `freq_mhz`, `power_mw`}).
    #[func]
    fn get_world(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(world) = self.client.as_ref().and_then(|c| c.world()) else { return d };
        let mut antennas = VarArray::new();
        for a in &world.antennas {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(a.name.as_str()));
            e.set("kind", &GString::from(a.kind.as_str()));
            e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
            antennas.push(&e.to_variant());
        }
        let mut objects = VarArray::new();
        for o in &world.objects {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(o.name.as_str()));
            e.set("shape", &GString::from(o.shape.as_str()));
            e.set("center", vector3(o.center.x, o.center.y, o.center.z));
            e.set("size", vector3(o.size.x, o.size.y, o.size.z));
            e.set("radius", o.radius_m);
            e.set("height", o.height_m);
            e.set("color", Color::from_rgb(o.color[0] as f32, o.color[1] as f32, o.color[2] as f32));
            e.set("rf_loss_db", o.rf_loss_db);
            objects.push(&e.to_variant());
        }
        let mut emitters = VarArray::new();
        for x in &world.emitters {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(x.name.as_str()));
            e.set("position", vector3(x.position.x, x.position.y, x.position.z));
            e.set("freq_mhz", x.freq_mhz);
            e.set("power_mw", x.power_mw);
            emitters.push(&e.to_variant());
        }
        d.set("name", &GString::from(world.name.as_str()));
        d.set("pilot_position", vector3(world.pilot_position.x, world.pilot_position.y, world.pilot_position.z));
        d.set("antennas", &antennas);
        d.set("objects", &objects);
        d.set("emitters", &emitters);
        d
    }

    /// Counts world updates; when it changes, `get_world()` has a new world (or none, while a load is under way).
    #[func]
    fn get_world_version(&self) -> i64 {
        self.client.as_ref().map_or(0, |c| c.world_version() as i64)
    }
}

```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        }
    }
}

```

with:

```rust
        }
    }
}

fn vector3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x as f32, y as f32, z as f32)
}

```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
    let quad_path = string_key(d, "quad_path").ok_or("`quad_path` is required")?;
    let mut settings = Settings::new(quad_path);
    if let Some(addr) = string_key(d, "server_addr") {
        settings.server_addr = addr;
```

with:

```rust
    let quad_path = string_key(d, "quad_path").ok_or("`quad_path` is required")?;
    let mut settings = Settings::new(quad_path);
    if let Some(world) = string_key(d, "world_path") {
        settings.world_path = world;
    }
    if let Some(addr) = string_key(d, "server_addr") {
        settings.server_addr = addr;
```

- [ ] **Step 4: The settings**

In `godot/scripts/settings.gd`, replace:

```gdscript
	"OFS_OPEN_LOOP": "open_loop",
	"OFS_SITL_LAUNCH": "sitl_launch",
}
## Command-line flag -> setting (`--name=value`; `--open-loop` alone means true).
```

with:

```gdscript
	"OFS_OPEN_LOOP": "open_loop",
	"OFS_SITL_LAUNCH": "sitl_launch",
	"OFS_WORLD": "world_path",
	"OFS_VIDEO_EFFECTS": "video_effects",
}
## Command-line flag -> setting (`--name=value`; `--open-loop` alone means true).
```

In `godot/scripts/settings.gd`, replace:

```gdscript
	"--open-loop": "open_loop",
	"--sitl-launch": "sitl_launch",
}

```

with:

```gdscript
	"--open-loop": "open_loop",
	"--sitl-launch": "sitl_launch",
	"--world": "world_path",
	"--video-effects": "video_effects",
}

```

In `godot/scripts/settings.gd`, replace:

```gdscript
		"data_dir": root.path_join(".ofs-data"),
		"quad_path": root.path_join("quads").path_join("opendrone-5f-freestyle.toml"),
		"sitl_launch": "",
		"open_loop": false,
```

with:

```gdscript
		"data_dir": root.path_join(".ofs-data"),
		"quad_path": root.path_join("quads").path_join("opendrone-5f-freestyle.toml"),
		"world_path": root.path_join("worlds").path_join("flat.toml"),
		"video_effects": true,
		"sitl_launch": "",
		"open_loop": false,
```

In `godot/scripts/settings.gd`, replace:

```gdscript


## The dictionary `OfsClient.start` takes.
static func to_client_dict(s: Dictionary) -> Dictionary:
	var d := {
		"quad_path": s["quad_path"],
		"server_addr": s["server_addr"],
		"seed": s["seed"],
```

with:

```gdscript


## Stores one setting in the `[client]` section of user://ofs_client.cfg (for settings changed in the game, like
## "Video effects"), keeping the rest of the file. Returns false when the file cannot be written.
static func save_value(key: String, value, path := CONFIG_PATH) -> bool:
	var file := ConfigFile.new()
	file.load(path)  # a missing file starts empty
	file.set_value("client", key, value)
	return file.save(path) == OK


## The dictionary `OfsClient.start` takes.
static func to_client_dict(s: Dictionary) -> Dictionary:
	var d := {
		"quad_path": s["quad_path"],
		"world_path": s["world_path"],
		"server_addr": s["server_addr"],
		"seed": s["seed"],
```

- [ ] **Step 5: Build the field from the server's world**

Replace the whole of `godot/world/world.gd`:

```gdscript
extends Node3D
## The grey-box world. The sky, the sun and the ground plane with its metre grid are drawn here; the objects on the
## field (launch pad, pylons, gates, buildings), the pilot's spot and other transmitters come from the world file the
## simulator loaded (`OfsClient.get_world()`), so what you see is what the video link sees. Nothing here collides with
## the drone: the simulator's ground is a plane at height 0, and it knows the objects only for the video signal.
##
## Godot's frame: +Y up, forward (north, where the drone starts facing) is -Z, +X is right (east).

const GRID_SHADER := preload("res://world/grid.gdshader")

const PILOT_COLOR := Color(0.25, 0.55, 1.0)
const EMITTER_COLOR := Color(0.85, 0.25, 0.85)
## Where the pilot's eyes are below the goggle position, and how tall the marker is.
const PILOT_BODY_HEIGHT_M := 1.5

## The nodes `build` made, removed by the next `build`.
var _built: Array[Node] = []


func _ready() -> void:
	_add_environment()
	_add_ground()


## Replaces the field's objects and markers with those of `world` (the Dictionary `OfsClient.get_world()` returns;
## an empty one clears the field).
func build(world: Dictionary) -> void:
	for node in _built:
		remove_child(node)
		node.queue_free()
	_built.clear()
	for o in world.get("objects", []):
		match o.get("shape", ""):
			"box":
				_box(o["name"], o["center"], o["size"], o["color"])
			"cylinder":
				_cylinder(o["name"], o["center"], o["radius"], o["height"], o["color"])
	if world.has("pilot_position"):
		_add_pilot(world["pilot_position"], world.get("antennas", []))
	for e in world.get("emitters", []):
		_add_emitter(e)


## The names of the nodes the world file made (for the tests and the e2e).
func built_names() -> PackedStringArray:
	var names := PackedStringArray()
	for node in _built:
		names.append(node.name)
	return names


func _add_environment() -> void:
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.30, 0.48, 0.78)
	sky_material.sky_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_bottom_color = Color(0.40, 0.45, 0.40)
	var sky := Sky.new()
	sky.sky_material = sky_material
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.8
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	add_child(world_environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	sun.light_energy = 1.1
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 150.0
	add_child(sun)


func _add_ground() -> void:
	var plane := PlaneMesh.new()
	plane.size = Vector2(6000.0, 6000.0)
	var material := ShaderMaterial.new()
	material.shader = GRID_SHADER
	var ground := MeshInstance3D.new()
	ground.name = "Ground"
	ground.mesh = plane
	ground.material_override = material
	add_child(ground)


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.9
	return material


func _add_built(node: Node3D) -> void:
	add_child(node)
	_built.append(node)


func _mesh(node_name: String, mesh: Mesh, color: Color, position_: Vector3) -> MeshInstance3D:
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = position_
	return instance


func _box(node_name: String, center: Vector3, size: Vector3, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = size
	_add_built(_mesh(node_name, mesh, color, center))


func _cylinder(node_name: String, center: Vector3, radius: float, height: float, color: Color) -> void:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	_add_built(_mesh(node_name, mesh, color, center))


## A figure where the pilot stands (its head at the goggles), with a pointer along each patch antenna's aim.
func _add_pilot(goggles: Vector3, antennas: Array) -> void:
	var pilot := Node3D.new()
	pilot.name = "Pilot"
	pilot.position = goggles
	var body := CylinderMesh.new()
	body.top_radius = 0.18
	body.bottom_radius = 0.22
	body.height = PILOT_BODY_HEIGHT_M
	pilot.add_child(_mesh("Body", body, PILOT_COLOR, Vector3(0.0, -0.2 - PILOT_BODY_HEIGHT_M * 0.5, 0.0)))
	var head := SphereMesh.new()
	head.radius = 0.13
	head.height = 0.26
	pilot.add_child(_mesh("Head", head, PILOT_COLOR, Vector3.ZERO))
	for a in antennas:
		if a.get("kind", "") != "patch":
			continue
		var aim: Vector3 = a["aim"]
		var pointer := BoxMesh.new()
		pointer.size = Vector3(0.05, 0.05, 0.8)
		var arrow := _mesh("Aim_" + String(a["name"]), pointer, PILOT_COLOR.lightened(0.4), aim * 0.5)
		arrow.basis = Basis.looking_at(aim, Vector3.UP if absf(aim.y) < 0.99 else Vector3.FORWARD)
		pilot.add_child(arrow)
	_add_built(pilot)


## A post up to the transmitter, labelled with its frequency.
func _add_emitter(e: Dictionary) -> void:
	var at: Vector3 = e["position"]
	var marker := Node3D.new()
	marker.name = "Emitter_" + String(e["name"])
	marker.position = Vector3(at.x, 0.0, at.z)
	var post := CylinderMesh.new()
	post.top_radius = 0.05
	post.bottom_radius = 0.05
	post.height = maxf(at.y, 0.1)
	marker.add_child(_mesh("Post", post, EMITTER_COLOR, Vector3(0.0, post.height * 0.5, 0.0)))
	var label := Label3D.new()
	label.name = "Label"
	label.text = "%s  %d MHz" % [e["name"], int(e["freq_mhz"])]
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.position = Vector3(0.0, post.height + 0.4, 0.0)
	label.modulate = EMITTER_COLOR
	marker.add_child(label)
	_add_built(marker)
```

In `godot/scripts/app.gd`, replace:

```gdscript
@onready var osd: CanvasLayer = $Osd
@onready var hud: CanvasLayer = $Hud
```

with:

```gdscript
@onready var osd: CanvasLayer = $Osd
@onready var world: Node3D = $World
@onready var hud: CanvasLayer = $Hud
```

In `godot/scripts/app.gd`, replace:

```gdscript
var _osd_version := -1
```

with:

```gdscript
var _osd_version := -1
var _world_version := -1
```

In `godot/scripts/app.gd`, replace:

```gdscript
		osd.set_frame(client.get_osd())
	var telemetry: Dictionary = client.get_telemetry()
```

with:

```gdscript
		osd.set_frame(client.get_osd())
	var world_version: int = client.get_world_version()
	if world_version != _world_version:
		_world_version = world_version
		world.build(client.get_world())
	var telemetry: Dictionary = client.get_telemetry()
```

The client clears its world when a load starts and fetches the new one when it ends, so a reload rebuilds the field and a failed load leaves it empty. `build` removes the old nodes with `remove_child` before freeing them, so their names are free for the new ones at once.

- [ ] **Step 6: Run the tests to see them pass**

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh all`
Expected: PASS: every unit suite, the open-loop and error e2e (the Betaflight e2e skips without `OFS_SITL_LAUNCH`). The game now flies in `worlds/flat.toml`.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-godot/src/lib.rs godot/scripts/settings.gd godot/world/world.gd godot/scripts/app.gd godot/tests/test_scene.gd godot/tests/test_settings.gd godot/tests/test_extension.gd
git commit -m "feat(godot): the field is built from the simulator's world file; world and video-effects settings"
```

### Task 9: The analog picture in Godot

**Files:**
- Create: `godot/ui/video.gdshader`, `godot/ui/video.gd`, `godot/tests/test_video.gd` (+ the `.uid` files Godot makes for them)
- Modify: `godot/main.tscn`, `godot/scripts/app.gd`, `godot/ui/hud.gd`, `godot/ui/controls_menu.gd`, `godot/ui/osd.gd` (comment), `crates/ofs-godot/src/lib.rs` (the video telemetry keys)
- Test: `godot/tests/run_tests.gd`, `godot/tests/test_hud.gd`, `godot/tests/test_controls_menu.gd`, `godot/tests/e2e_open_loop.gd`, `godot/tests/e2e_betaflight.gd`, `godot/tests/shots.gd`

**Interfaces:**
- Consumes: Task 7's `Telemetry.video`; Task 8's settings (`video_effects`, `save_value`) and `app.world`.
- Produces:
  - Telemetry keys: `video_present`, `video_snr_db`, `video_interference_dbm`, `video_sync` ("locked", "unstable", "lost"; "" without a VTX), `video_noise`, `video_sparkles`, `video_chroma`, `video_antenna`, `video_rssi` (Dictionary of dBm by antenna name).
  - `video.gd` (CanvasLayer 3, node `Video`): `static uniforms_for(t: Dictionary) -> Dictionary` (`active`, `noise`, `sparkles`, `chroma`, `sync` 0/1/2), `static is_clean(u) -> bool`, `update_view(t, delta)`, `set_enabled(bool)` (chase view), `effects` (the setting), `is_active() -> bool`, `uniforms()`, `ROLL_SPEED`.
  - `hud.gd`: `video_text()` ("VID 18 dB  patch  -71 dBm", "  NO SYNC" when lost); toasts "Video lost - ..." and "Video back - ...".
  - `controls_menu.gd`: `signal video_effects_toggled(on: bool)`, `set_video_effects(on)` (no signal), `video_effects_checked()`.

- [ ] **Step 1: Write the failing tests**

Create `godot/tests/test_video.gd`:

```gdscript
extends "res://tests/testing.gd"
## The analog video layer's logic (headless has no renderer, so these check the amounts and when the layer draws, not
## pixels; tests/shots.gd draws it).

const Video = preload("res://ui/video.gd")


func _layer() -> CanvasLayer:
	var layer := Video.new()
	await add_to_tree(layer)
	return layer


func _telemetry(overrides := {}) -> Dictionary:
	var t := {"video_present": true, "video_snr_db": 30.0, "video_sync": "locked", "video_noise": 0.0, "video_sparkles": 0.0,
		"video_chroma": 1.0, "video_antenna": "omni", "video_rssi": {"omni": -60.0}}
	t.merge(overrides, true)
	return t


func test_the_amounts_follow_the_telemetry() -> void:
	var u := Video.uniforms_for(_telemetry({"video_noise": 0.4, "video_sparkles": 0.2, "video_chroma": 0.5, "video_sync": "unstable"}))
	eq(u, {"active": true, "noise": 0.4, "sparkles": 0.2, "chroma": 0.5, "sync": 1}, "unstable")
	eq(Video.uniforms_for(_telemetry({"video_sync": "lost"}))["sync"], 2, "lost")
	eq(Video.uniforms_for(_telemetry())["sync"], 0, "locked")
	eq(Video.uniforms_for(_telemetry({"video_noise": 7.0, "video_chroma": -1.0}))["noise"], 1.0, "clamped")
	eq(Video.uniforms_for({})["active"], false, "no telemetry: no link")
	eq(Video.uniforms_for(_telemetry({"video_present": false}))["active"], false, "a quad without a VTX")
	ok(Video.is_clean(Video.uniforms_for(_telemetry())), "a locked link without noise is clean")
	ok(not Video.is_clean(Video.uniforms_for(_telemetry({"video_chroma": 0.9}))), "fading colour is not")


func test_the_layer_draws_only_over_a_degraded_fpv_picture() -> void:
	var layer := await _layer()
	ok(not layer.is_active(), "nothing before the first telemetry")
	layer.update_view(_telemetry(), 0.016)
	ok(not layer.is_active(), "a clean link: pass-through, the layer stays off")
	layer.update_view(_telemetry({"video_noise": 0.5}), 0.016)
	ok(layer.is_active(), "grain: the layer draws")
	layer.set_enabled(false)
	ok(not layer.is_active(), "off in the chase view")
	layer.set_enabled(true)
	layer.effects = false
	ok(not layer.is_active(), "off with the Video effects setting off")
	layer.effects = true
	ok(layer.is_active(), "and back")
	layer.update_view(_telemetry({"video_present": false, "video_noise": 0.5}), 0.016)
	ok(not layer.is_active(), "off for a quad without a VTX")
	layer.queue_free()


func test_a_lost_picture_rolls_and_a_locked_one_does_not() -> void:
	var layer := await _layer()
	var material: ShaderMaterial = layer.get_node("VideoRect").material
	layer.update_view(_telemetry({"video_sync": "lost", "video_noise": 1.0}), 0.1)
	var first: float = material.get_shader_parameter("roll")
	layer.update_view(_telemetry({"video_sync": "lost", "video_noise": 1.0}), 0.1)
	var second: float = material.get_shader_parameter("roll")
	near(second - first, Video.ROLL_SPEED * 0.1, "rolls at its speed", 1e-5)
	near(material.get_shader_parameter("noise"), 1.0, "static")
	layer.update_view(_telemetry({"video_sync": "unstable", "video_noise": 0.7}), 0.1)
	near(material.get_shader_parameter("roll"), 0.0, "relocked: the roll stops")
	near(material.get_shader_parameter("tear"), 1.0, "unstable: tearing")
	layer.update_view(_telemetry({"video_noise": 0.3}), 0.1)
	near(material.get_shader_parameter("tear"), 0.0, "locked: no tearing")
	layer.queue_free()
```

In `godot/tests/run_tests.gd`, replace:

```gdscript
	"res://tests/test_scene.gd",
	"res://tests/test_osd.gd",
]

```

with:

```gdscript
	"res://tests/test_scene.gd",
	"res://tests/test_osd.gd",
	"res://tests/test_video.gd",
]

```

In `godot/tests/test_hud.gd`, replace:

```gdscript
	ok(true, "no errors")
	hud.queue_free()
```

with:

```gdscript
	ok(true, "no errors")
	hud.queue_free()


func test_the_video_line_shows_the_signal_and_its_antenna() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.video_text(), "", "no video link in the telemetry")
	var video := {"video_present": true, "video_snr_db": 18.4, "video_sync": "locked", "video_noise": 0.2,
		"video_antenna": "patch", "video_rssi": {"omni": -78.2, "patch": -71.4}}
	hud.update_view(_view({"telemetry": _telemetry(video)}))
	eq(hud.video_text(), "VID 18 dB  patch  -71 dBm", "the line")
	video["video_sync"] = "lost"
	video["video_snr_db"] = -2.6
	hud.update_view(_view({"telemetry": _telemetry(video)}))
	eq(hud.video_text(), "VID -3 dB  patch  -71 dBm  NO SYNC", "without sync")
	hud.update_view(_view({"telemetry": {}}))
	eq(hud.video_text(), "", "cleared with the telemetry")
	hud.queue_free()
```

In `godot/tests/test_controls_menu.gd`, replace:

```gdscript
	near(menu._rows["roll"]["live"].value, 0.5, "a closed screen does not update")
	menu.queue_free()
```

with:

```gdscript
	near(menu._rows["roll"]["live"].value, 0.5, "a closed screen does not update")
	menu.queue_free()


func test_the_video_effects_box_reports_changes_but_not_its_setup() -> void:
	var m := await _menu()
	var menu = m[0]
	var seen := []
	menu.video_effects_toggled.connect(func(on: bool) -> void: seen.append(on))
	menu.set_video_effects(false)
	ok(not menu.video_effects_checked(), "set from the settings")
	eq(seen, [], "without a signal")
	menu._video_effects.button_pressed = true
	eq(seen, [true], "a click is reported")
	menu.queue_free()
```

The end-to-end checks (the open-loop run shows the world and a locked link on the pad, then forces a lost link to check the layer; the Betaflight run checks the HUD line on a real session):

In `godot/tests/e2e_open_loop.gd`, replace:

```gdscript
	_check(t.get("vtx_present", false) and t.get("vtx_freq_mhz", 0) == 5658, "open loop: the VTX transmits R1: %s" % str(t.get("vtx_freq_mhz")))

	app.queue_free()
	await process_frame
```

with:

```gdscript
	_check(t.get("vtx_present", false) and t.get("vtx_freq_mhz", 0) == 5658, "open loop: the VTX transmits R1: %s" % str(t.get("vtx_freq_mhz")))

	# The world file and the video link.
	var world: Dictionary = app.client.get_world()
	_check(world.get("name", "") == "Flat field", "the shipped world is loaded: %s" % world.get("name", ""))
	var building = app.world.get_node_or_null("BuildingA")
	_check(building != null and building.position.is_equal_approx(Vector3(-45.0, 7.0, -110.0)), "building A stands where it always did")
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
	_check(t.get("video_present", false) and t.get("video_sync", "") == "locked", "the video link is locked on the pad: %s" % str(t.get("video_sync")))
	_check(app.hud.video_text().begins_with("VID "), "the HUD shows the video line: '%s'" % app.hud.video_text())
	_check(not app.video.is_active(), "a clean link draws nothing over the picture")
	app.set_process(false)  # the next frame would feed the real (clean) telemetry
	app.video.update_view({"video_present": true, "video_sync": "lost", "video_noise": 1.0, "video_sparkles": 1.0, "video_chroma": 0.0}, 0.016)
	_check(app.video.is_active(), "a lost link draws static")
	app.set_chase_view(true)
	_check(not app.video.is_active(), "the chase view is clean")
	app.set_chase_view(false)
	_check(app.video.is_active(), "and the FPV view degraded again")
	app.set_process(true)

	app.queue_free()
	await process_frame
```

In `godot/tests/e2e_betaflight.gd`, replace:

```gdscript
	_check(telemetry0.get("vtx_present", false) and telemetry0.get("vtx_freq_mhz", 0) == 5658, "the VTX is on R1 (5658 MHz): %s" % str(telemetry0.get("vtx_freq_mhz")))
	_check(app.hud.vtx_text().contains("5658"), "the HUD shows it: %s" % app.hud.vtx_text())

	app.sticks_override = _sticks(0.6, true)
```

with:

```gdscript
	_check(telemetry0.get("vtx_present", false) and telemetry0.get("vtx_freq_mhz", 0) == 5658, "the VTX is on R1 (5658 MHz): %s" % str(telemetry0.get("vtx_freq_mhz")))
	_check(app.hud.vtx_text().contains("5658"), "the HUD shows it: %s" % app.hud.vtx_text())
	_check(telemetry0.get("video_sync", "") == "locked" and app.hud.video_text().begins_with("VID "), "the goggles are locked on it: %s" % app.hud.video_text())
	_check(app.world.get_node_or_null("BuildingB") != null, "the world file's buildings are drawn")

	app.sticks_override = _sticks(0.6, true)
```

The screenshot script gains the three looks of the link, forced over the real picture:

In `godot/tests/shots.gd`, replace:

```gdscript
##   godot --path godot -s res://tests/shots.gd -- --out=C:/some/dir
## Flies the open-loop drone forward for a moment, then saves start_fpv.png, start_chase.png, osd_fpv.png, hop_fpv.png,
## hop_chase.png, help.png and controls.png.

func _initialize() -> void:
```

with:

```gdscript
##   godot --path godot -s res://tests/shots.gd -- --out=C:/some/dir
## Flies the open-loop drone forward for a moment, then saves start_fpv.png, start_chase.png, osd_fpv.png, hop_fpv.png,
## hop_chase.png, video_grain.png, video_unstable.png, video_lost.png, help.png and controls.png.

func _initialize() -> void:
```

In `godot/tests/shots.gd`, replace:

```gdscript
	await _save("hop_chase")
	app.set_chase_view(false)
	app.hud.set_help_visible(true)
	await _save("help")
```

with:

```gdscript
	await _save("hop_chase")
	app.set_chase_view(false)
	# The analog link's looks, forced (the drone stays near the pilot, where the real link is clean). `_process` would
	# overwrite them with the real telemetry, so it is paused while they are shot (after the HUD has caught up).
	await _frames(3)
	app.set_process(false)
	for look in [["video_grain", {"video_noise": 0.45, "video_sparkles": 0.1, "video_chroma": 1.0, "video_sync": "locked"}],
			["video_unstable", {"video_noise": 0.7, "video_sparkles": 0.6, "video_chroma": 0.3, "video_sync": "unstable"}],
			["video_lost", {"video_noise": 1.0, "video_sparkles": 1.0, "video_chroma": 0.0, "video_sync": "lost"}]]:
		var t: Dictionary = look[1]
		t["video_present"] = true
		for i in 10:
			app.video.update_view(t, 0.03)
			await process_frame
		await _save(look[0])
	app.set_process(true)
	app.hud.set_help_visible(true)
	await _save("help")
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh unit`
Expected: FAIL: `test_video.gd` cannot load `res://ui/video.gd`; `test_hud.gd` has no `video_text`; `test_controls_menu.gd` has no `video_effects_toggled`.

- [ ] **Step 3: The video telemetry keys in the extension**

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
    /// `vtx_present`, `vtx_band`, `vtx_channel`, `vtx_channel_name` (a String, empty without a VTX), `vtx_freq_mhz`,
    /// `vtx_power_mw` and `vtx_pit_mode`.
    #[func]
    fn get_telemetry(&self) -> VarDictionary {
```

with:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
    /// `vtx_present`, `vtx_band`, `vtx_channel`, `vtx_channel_name` (a String, empty without a VTX), `vtx_freq_mhz`,
    /// `vtx_power_mw` and `vtx_pit_mode`, and the video link's `video_present`, `video_snr_db`,
    /// `video_interference_dbm`, `video_sync` ("locked", "unstable", "lost"; empty without a VTX), `video_noise`,
    /// `video_sparkles`, `video_chroma`, `video_antenna` (the antenna in use) and `video_rssi` (a Dictionary of dBm by
    /// antenna name).
    #[func]
    fn get_telemetry(&self) -> VarDictionary {
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        d.set("vtx_power_mw", i64::from(t.vtx.power_mw));
        d.set("vtx_pit_mode", t.vtx.pit_mode);
        d.set("age_s", t.age_s);
        d
```

with:

```rust
        d.set("vtx_power_mw", i64::from(t.vtx.power_mw));
        d.set("vtx_pit_mode", t.vtx.pit_mode);
        let v = &t.video;
        d.set("video_present", v.present);
        d.set("video_snr_db", v.snr_db);
        d.set("video_interference_dbm", v.interference_dbm);
        d.set("video_sync", &GString::from(v.sync.as_str()));
        d.set("video_noise", v.noise);
        d.set("video_sparkles", v.sparkles);
        d.set("video_chroma", v.chroma);
        d.set("video_antenna", &GString::from(v.active_antenna.as_str()));
        let mut rssi = VarDictionary::new();
        for (name, dbm) in &v.rssi_dbm {
            rssi.set(&GString::from(name.as_str()), *dbm);
        }
        d.set("video_rssi", &rssi);
        d.set("age_s", t.age_s);
        d
```

- [ ] **Step 4: The shader and its layer**

Create `godot/ui/video.gdshader`:

```glsl
shader_type canvas_item;
// The analog 5.8 GHz video breaking up: grain, FM sparkles, colour loss, tearing and rolling, applied to everything
// drawn below this layer (the 3D view, the lens and Betaflight's OSD). Every amount comes from the simulator's link
// model; this shader only draws it. It never flashes the whole screen: static is fine grain around mid-grey.

uniform sampler2D screen_tex : hint_screen_texture, repeat_enable, filter_linear;
uniform float noise = 0.0;     // grain: 0 clean, 1 full static
uniform float sparkles = 0.0;  // density of FM threshold sparkles, 0 to 1
uniform float chroma = 1.0;    // colour saturation, 1 full, 0 black and white
uniform float tear = 0.0;      // 1 while the sync is unstable: line jitter and a drifting tear band
uniform float roll = 0.0;      // vertical picture offset (0 to 1) while the sync is lost
uniform float seed = 0.0;      // changes every rendered frame

// Analog lines the effects work on (a PAL field has about 288).
const float LINES = 288.0;
// Sparkle cells per line.
const float SPARKLE_CELLS = 96.0;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
}

void fragment() {
	vec2 uv = SCREEN_UV;
	float line = floor(uv.y * LINES);
	// Tearing: every line jitters sideways a little; inside a band drifting down the picture it shifts a lot.
	float band_at = fract(seed * 0.173);
	float band = 1.0 - smoothstep(0.0, 0.05, abs(uv.y - band_at));
	uv.x += tear * ((hash(vec2(line, 3.0)) - 0.5) * 0.008 + band * 0.05);
	// Rolling: the picture slides up through the frame.
	uv.y = fract(uv.y + roll);
	vec3 color = texture(screen_tex, uv).rgb;
	// Colour fades to black and white, bleeding sideways on the way.
	vec3 bled = texture(screen_tex, uv + vec2(0.006 * (1.0 - chroma), 0.0)).rgb;
	color = mix(color, vec3(bled.r, color.g, bled.b), 1.0 - chroma);
	float luma = dot(color, vec3(0.299, 0.587, 0.114));
	color = mix(vec3(luma), color, chroma);
	// Grain, then static: fine noise around mid-grey takes the picture over.
	float g = hash(floor(uv * vec2(1.0 / SCREEN_PIXEL_SIZE.x, LINES * 2.0)));
	color += (g - 0.5) * 0.5 * noise;
	color = mix(color, vec3(0.3 + 0.35 * g), smoothstep(0.6, 1.0, noise));
	// Sparkles: short white or black streaks along a line.
	float cell = floor(uv.x * SPARKLE_CELLS);
	if (hash(vec2(cell, line)) > 1.0 - sparkles * 0.03) {
		color = vec3(step(0.5, hash(vec2(line, cell + 11.0))));
	}
	COLOR = vec4(clamp(color, 0.0, 1.0), 1.0);
}
```

It never flashes the whole screen bright: static is fine grain around mid-grey and the roll is a smooth scroll.

Create `godot/ui/video.gd`:

```gdscript
extends CanvasLayer
## The analog video link's picture: a full-screen shader over the 3D view, the lens and Betaflight's OSD (layer 3,
## between the OSD on layer 2 and the HUD on layer 4), so the OSD breaks up with the picture as it does in real
## goggles. The simulator's link model decides how much grain, sparkles, colour loss, tearing and rolling there is;
## this layer only draws it. Off in the chase view, without a VTX, with the "Video effects" setting off, and while the
## link is clean.

const VIDEO_SHADER := preload("res://ui/video.gdshader")
## How fast a picture without sync rolls, in screen heights per second.
const ROLL_SPEED := 0.7

## The "Video effects" setting.
var effects := true:
	set(value):
		effects = value
		_refresh()

var _enabled := true
var _material := ShaderMaterial.new()
var _rect := ColorRect.new()
var _uniforms := uniforms_for({})
var _roll := 0.0


func _ready() -> void:
	_material.shader = VIDEO_SHADER
	_rect.name = "VideoRect"
	_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_rect.material = _material
	add_child(_rect)
	_refresh()


## The shader's amounts for a telemetry Dictionary (the one `OfsClient.get_telemetry()` returns): `active` is false
## without a video link; `sync` is 0 locked, 1 unstable, 2 lost.
static func uniforms_for(t: Dictionary) -> Dictionary:
	if not bool(t.get("video_present", false)):
		return {"active": false, "noise": 0.0, "sparkles": 0.0, "chroma": 1.0, "sync": 0}
	var sync: int = {"unstable": 1, "lost": 2}.get(String(t.get("video_sync", "")), 0)
	return {
		"active": true,
		"noise": clampf(float(t.get("video_noise", 0.0)), 0.0, 1.0),
		"sparkles": clampf(float(t.get("video_sparkles", 0.0)), 0.0, 1.0),
		"chroma": clampf(float(t.get("video_chroma", 1.0)), 0.0, 1.0),
		"sync": sync,
	}


## True when the amounts change nothing on screen (a locked, clean link).
static func is_clean(u: Dictionary) -> bool:
	return u["noise"] <= 0.0 and u["sparkles"] <= 0.0 and u["chroma"] >= 1.0 and u["sync"] == 0


## Feeds the layer one frame's telemetry.
func update_view(t: Dictionary, delta: float) -> void:
	_uniforms = uniforms_for(t)
	_roll = fposmod(_roll + ROLL_SPEED * delta, 1.0) if _uniforms["sync"] == 2 else 0.0
	_material.set_shader_parameter("noise", _uniforms["noise"])
	_material.set_shader_parameter("sparkles", _uniforms["sparkles"])
	_material.set_shader_parameter("chroma", _uniforms["chroma"])
	_material.set_shader_parameter("tear", 1.0 if _uniforms["sync"] == 1 else 0.0)
	_material.set_shader_parameter("roll", _roll)
	_material.set_shader_parameter("seed", randf())
	_refresh()


## Off in the chase view.
func set_enabled(enabled: bool) -> void:
	_enabled = enabled
	_refresh()


## True when the layer draws over the picture now.
func is_active() -> bool:
	return visible


## The amounts of the last `update_view` (for the tests).
func uniforms() -> Dictionary:
	return _uniforms


func _refresh() -> void:
	visible = _enabled and effects and _uniforms["active"] and not is_clean(_uniforms)
```

In `godot/main.tscn`, replace:

```ini
[ext_resource type="Script" path="res://ui/controls_menu.gd" id="8"]
[ext_resource type="Script" path="res://ui/osd.gd" id="9"]

[node name="Main" type="Node3D"]
```

with:

```ini
[ext_resource type="Script" path="res://ui/controls_menu.gd" id="8"]
[ext_resource type="Script" path="res://ui/osd.gd" id="9"]
[ext_resource type="Script" path="res://ui/video.gd" id="10"]

[node name="Main" type="Node3D"]
```

In `godot/main.tscn`, replace:

```ini
script = ExtResource("9")

[node name="Hud" type="CanvasLayer" parent="."]
layer = 4
```

with:

```ini
script = ExtResource("9")

[node name="Video" type="CanvasLayer" parent="."]
layer = 3
script = ExtResource("10")

[node name="Hud" type="CanvasLayer" parent="."]
layer = 4
```

In `godot/ui/osd.gd`, replace:

```gdscript
extends CanvasLayer
## Betaflight's OSD, drawn from the character grid the simulator decodes. It sits on layer 2: above the lens (layer 1,
## the camera optics) and below the HUD (layer 4). Layer 3 is reserved for the analog degradation of M3b, which must
## see the picture and this OSD together, as a real analog link does. Hidden in the chase view.

const FONT: Texture2D = preload("res://ui/osd_font.png")
```

with:

```gdscript
extends CanvasLayer
## Betaflight's OSD, drawn from the character grid the simulator decodes. It sits on layer 2: above the lens (layer 1,
## the camera optics) and below the analog video layer (3, ui/video.gd), which breaks up the picture and this OSD
## together as a real analog link does; the HUD is on layer 4. Hidden in the chase view.

const FONT: Texture2D = preload("res://ui/osd_font.png")
```

- [ ] **Step 5: The HUD line, the F2 box and the wiring**

In `godot/ui/hud.gd`, replace:

```gdscript
var _battery := Label.new()
var _vtx := Label.new()
var _flight := Label.new()
var _configurator := Label.new()
```

with:

```gdscript
var _battery := Label.new()
var _vtx := Label.new()
var _video := Label.new()
var _flight := Label.new()
var _configurator := Label.new()
```

In `godot/ui/hud.gd`, replace:

```gdscript
	_place(root, _battery, 1.0, 0.0, 14.0, 44.0, true)
	_place(root, _vtx, 1.0, 0.0, 14.0, 74.0, true)
	_place(root, _flight, 0.0, 1.0, 14.0, 14.0, false)
	_place(root, _configurator, 1.0, 1.0, 14.0, 14.0, true)
```

with:

```gdscript
	_place(root, _battery, 1.0, 0.0, 14.0, 44.0, true)
	_place(root, _vtx, 1.0, 0.0, 14.0, 74.0, true)
	_place(root, _video, 1.0, 0.0, 14.0, 104.0, true)
	_place(root, _flight, 0.0, 1.0, 14.0, 14.0, false)
	_place(root, _configurator, 1.0, 1.0, 14.0, 14.0, true)
```

In `godot/ui/hud.gd`, replace:

```gdscript
	_help.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.grow_vertical = Control.GROW_DIRECTION_BOTH
	for label in [_status, _sim, _link, _battery, _vtx, _flight, _configurator]:
		_style(label, 18)

```

with:

```gdscript
	_help.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.grow_vertical = Control.GROW_DIRECTION_BOTH
	for label in [_status, _sim, _link, _battery, _vtx, _video, _flight, _configurator]:
		_style(label, 18)

```

In `godot/ui/hud.gd`, replace:

```gdscript
func set_hud_visible(visible_now: bool) -> void:
	_shown = visible_now
	for node in [_status, _sim, _link, _battery, _vtx, _flight, _configurator, _sticks_view, _toasts]:
		node.visible = visible_now

```

with:

```gdscript
func set_hud_visible(visible_now: bool) -> void:
	_shown = visible_now
	for node in [_status, _sim, _link, _battery, _vtx, _video, _flight, _configurator, _sticks_view, _toasts]:
		node.visible = visible_now

```

In `godot/ui/hud.gd`, replace:

```gdscript


## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
## controls_status, configurator, quad, camera, radio_cut}
```

with:

```gdscript


func video_text() -> String:
	return _video.text


## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
## controls_status, configurator, quad, camera, radio_cut}
```

In `godot/ui/hud.gd`, replace:

```gdscript
		_battery.text = ""
		_vtx.text = ""
		_flight.text = ""
	else:
```

with:

```gdscript
		_battery.text = ""
		_vtx.text = ""
		_video.text = ""
		_flight.text = ""
	else:
```

In `godot/ui/hud.gd`, replace:

```gdscript
		else:
			_vtx.text = ""
		_flight.text = "ALT   %.1f m
SPEED %.1f m/s
```

with:

```gdscript
		else:
			_vtx.text = ""
		_update_video(t)
		_flight.text = "ALT   %.1f m
SPEED %.1f m/s
```

In `godot/ui/hud.gd`, replace:

```gdscript
	_configurator.text = "Betaflight Configurator: %s" % address if address != "" else ""
	_update_banner(view, t, phase)


```

with:

```gdscript
	_configurator.text = "Betaflight Configurator: %s" % address if address != "" else ""
	_update_banner(view, t, phase)


## "VID 18 dB  patch  -71 dBm": the goggles' signal, the antenna in use and its power; green when the picture is clean,
## yellow while it degrades, red without sync.
func _update_video(t: Dictionary) -> void:
	if not t.get("video_present", false):
		_video.text = ""
		return
	var antenna: String = t.get("video_antenna", "")
	var rssi: Dictionary = t.get("video_rssi", {})
	var sync: String = t.get("video_sync", "")
	_video.text = "VID %d dB  %s  %d dBm%s" % [
		roundi(t["video_snr_db"]), antenna, roundi(rssi.get(antenna, 0.0)), "  NO SYNC" if sync == "lost" else ""]
	var clean: bool = t.get("video_noise", 0.0) <= 0.0 and sync == "locked"
	_video.add_theme_color_override("font_color", GREEN if clean else (RED if sync == "lost" else YELLOW))


```

In `godot/ui/controls_menu.gd`, replace:

```gdscript
const BIND_THRESHOLD := 0.6

var controls = null  # Controls
var save_path := ""
```

with:

```gdscript
const BIND_THRESHOLD := 0.6

## The "Video effects" box changed (the game applies and saves it).
signal video_effects_toggled(on: bool)

var controls = null  # Controls
var save_path := ""
```

In `godot/ui/controls_menu.gd`, replace:

```gdscript
var _deadzone_label := Label.new()
var _hint := Label.new()
var _rows := {}  # channel -> {description, bind, invert, live}
var _listening := ""
```

with:

```gdscript
var _deadzone_label := Label.new()
var _hint := Label.new()
var _video_effects := CheckBox.new()
var _rows := {}  # channel -> {description, bind, invert, live}
var _listening := ""
```

In `godot/ui/controls_menu.gd`, replace:

```gdscript
	dz_row.add_child(_deadzone_label)

	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	box.add_child(_hint)
```

with:

```gdscript
	dz_row.add_child(_deadzone_label)

	_video_effects.text = "Video effects (analog breakup with distance and obstacles)"
	_video_effects.button_pressed = true
	_video_effects.toggled.connect(func(on: bool) -> void: video_effects_toggled.emit(on))
	box.add_child(_video_effects)

	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	box.add_child(_hint)
```

In `godot/ui/controls_menu.gd`, replace:

```gdscript
func is_open() -> bool:
	return visible


```

with:

```gdscript
func is_open() -> bool:
	return visible


## Shows the "Video effects" setting without emitting `video_effects_toggled`.
func set_video_effects(on: bool) -> void:
	_video_effects.set_pressed_no_signal(on)


func video_effects_checked() -> bool:
	return _video_effects.button_pressed


```

In `godot/scripts/app.gd`, replace:

```gdscript
@onready var lens: CanvasLayer = $Lens
@onready var osd: CanvasLayer = $Osd
@onready var world: Node3D = $World
@onready var hud: CanvasLayer = $Hud
```

with:

```gdscript
@onready var lens: CanvasLayer = $Lens
@onready var osd: CanvasLayer = $Osd
@onready var video: CanvasLayer = $Video
@onready var world: Node3D = $World
@onready var hud: CanvasLayer = $Hud
```

In `godot/scripts/app.gd`, replace:

```gdscript
	_load_controls()
	controls_menu.setup(controls, CONTROLS_PATH)
	if not ClassDB.class_exists("OfsClient"):
		hud.show_fatal("The OfsClient extension is not loaded.\nBuild it with `cargo build -p ofs-godot` (docs/dev-setup.md) and restart.")
```

with:

```gdscript
	_load_controls()
	controls_menu.setup(controls, CONTROLS_PATH)
	video.effects = settings["video_effects"]
	controls_menu.set_video_effects(settings["video_effects"])
	controls_menu.video_effects_toggled.connect(_on_video_effects_toggled)
	if not ClassDB.class_exists("OfsClient"):
		hud.show_fatal("The OfsClient extension is not loaded.\nBuild it with `cargo build -p ofs-godot` (docs/dev-setup.md) and restart.")
```

In `godot/scripts/app.gd`, replace:

```gdscript
	if telemetry.has("motor_cmd"):
		drone.set_motors(telemetry["motor_cmd"], delta)
	var detail: String = client.get_phase_detail()
	if client.get_phase() == "failed" and TIPS.has(phase_kind):
```

with:

```gdscript
	if telemetry.has("motor_cmd"):
		drone.set_motors(telemetry["motor_cmd"], delta)
	video.update_view(telemetry, delta)
	var detail: String = client.get_phase_detail()
	if client.get_phase() == "failed" and TIPS.has(phase_kind):
```

In `godot/scripts/app.gd`, replace:

```gdscript
	lens.set_enabled(not chase)
	osd.set_enabled(not chase)


```

with:

```gdscript
	lens.set_enabled(not chase)
	osd.set_enabled(not chase)
	video.set_enabled(not chase)


func _on_video_effects_toggled(on: bool) -> void:
	video.effects = on
	settings["video_effects"] = on
	if not AppSettings.save_value("video_effects", on):
		hud.add_toast("Could not save the Video effects setting to %s" % AppSettings.CONFIG_PATH, "warn")


```

In `godot/scripts/app.gd`, replace:

```gdscript
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
		"serial_overflow":
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
```

with:

```gdscript
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
		"video_lost":
			hud.add_toast("Video lost - %s" % message.trim_prefix("video lost: "), "warn")
		"video_restored":
			hud.add_toast("Video back - %s" % message.trim_prefix("video restored: "))
		"serial_overflow":
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh all`
Expected: PASS: every unit suite (273 checks with `test_video.gd`), the open-loop e2e ("building A stands where it always did", "the video link is locked on the pad", "a lost link draws static", "the chase view is clean") and the error e2e.

Run: `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf" OFS_SIM_BIN=<a firewall-allowed ofs-sim.exe> GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh e2e`
Expected: the Betaflight e2e passes too ("the goggles are locked on it: VID 54 dB  patch  -38 dBm").

Run: `godot --path godot -s res://tests/shots.gd -- --out=<a scratch dir>`
Expected: `video_grain.png` (grain and a few sparkles), `video_unstable.png` (no colour, sparkles, a tear band, the OSD broken up) and `video_lost.png` (static); the HUD stays sharp in all three. Look at them; do not commit them.

- [ ] **Step 7: Commit**

```bash
git add godot/ui/video.gdshader godot/ui/video.gdshader.uid godot/ui/video.gd godot/ui/video.gd.uid godot/tests/test_video.gd godot/tests/test_video.gd.uid godot/main.tscn godot/scripts/app.gd godot/ui/hud.gd godot/ui/controls_menu.gd godot/ui/osd.gd crates/ofs-godot/src/lib.rs godot/tests/run_tests.gd godot/tests/test_hud.gd godot/tests/test_controls_menu.gd godot/tests/e2e_open_loop.gd godot/tests/e2e_betaflight.gd godot/tests/shots.gd
git commit -m "feat(godot): the analog picture: grain, sparkles, colour loss, tearing, rolling; HUD video line and Video effects setting"
```

### Task 10: Docs, carried debt and the final check

**Files:**
- Create: `docs/research/video-link.md`, `docs/superpowers/m3b-carried-debt.md`
- Modify: `docs/dev-setup.md`, `README.md`

CI needs no change: the `core` and `windows` jobs run the workspace and the Python tests, the `godot` job runs the open-loop e2e (now with the world and the link), and the `sitl` job already runs `test_sitl_video.py` (with the two new live tests) and the Betaflight e2e.

- [ ] **Step 1: The model, for readers**

Create `docs/research/video-link.md`:

```markdown
# The analog video link model

How the simulator decides what the pilot's goggles see, from the VTX on the quad to the picture in the FPV view.
The code is `crates/ofs-video/src/propagation.rs` (the physics, pure functions) and `crates/ofs-video/src/link.rs`
(the receiver, a model on the bus). The design is `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md`.

Every constant below is an **estimate** for a typical 5.8 GHz analog setup, not a measurement. They sit in one place
each, named, so they can be replaced by measured values.

## Once per PAL field (50 Hz)

1. Read the quad's position and attitude, and the VTX's frequency, power and pit mode (`vtx.*`, published by the
   SmartAudio VTX of M3a). In pit mode the power is the quad file's `vtx.pit_power_mw` (default 0.1 mW).
2. For each goggle antenna in the world file, compute the received power:

   `P = Ptx + Gtx(direction) + Grx(direction) - FSPL - polarization loss - body shadow - obstruction`, with the
   ground bounce added by phase and a fading term on top.
3. Add the other emitters' power, reduced by the receiver's channel filter, to the noise floor; the SNR of each
   antenna is its signal over that.
4. Diversity picks the antenna with the best SNR (it changes only for one 2 dB better).
5. The receiver turns that SNR into a picture (grain, sparkles, colour) and a sync state.

## Propagation

| Effect | Model | Constant |
|---|---|---|
| Free-space path loss | `20 log10(d m) + 20 log10(f MHz) - 27.55`; d at least 1 m | 87.7 dB at 100 m, 5800 MHz |
| Omni antenna | dipole doughnut `gain + 20 log10(sin θ)` from its axis | floor 20 dB below the peak |
| Patch antenna | `cos^n` main lobe, n set so the gain is -3 dB at half the beamwidth | back lobe 20 dB below the peak |
| Polarization | same-hand circular 0 dB, opposite hands 20 dB, circular to linear 3 dB, linear to linear `-20 log10|cos Δ|` | cap 20 dB |
| Body shadow | up to 8 dB when the pilot is ahead of and below the quad (the frame, stack and battery are between the VTX antenna and the goggles) | `BODY_SHADOW_DB` = 8 |
| Obstruction | ITU-R P.526 single knife edge, `J(v) = 6.9 + 20 log10(√((v-0.1)²+1) + v - 0.1)` for v > -0.78, capped at the object's `rf_loss_db` | buildings 20-30 dB in `worlds/flat.toml` |
| Ground bounce | two rays: the direct one and one reflected off the ground (coefficient -1), added with their phase difference | — |
| Fading | Rician, an AR(1) complex Gaussian scatter correlated over λ/2 of quad movement | K = 10 dB with line of sight, minus the obstruction loss |

Notes:
- **Obstruction.** The knife edge is the deepest point of the straight path in the object (or the closest it
  passes), found by a golden-section search on the object's signed distance, which is exact for the convex shapes of
  the world file (boxes, cylinders) however thin the object or long the path. An object standing on the ground
  continues below it, so a signal diffracts over its top or around its sides, never underneath. Going behind a
  building fades the picture over a few metres instead of switching it off.
- **Ground bounce and polarization.** A reflection reverses the hand of a circular wave, so a circular receiver
  takes the reflected ray with the 20 dB cross-polarization loss: a ripple under 1.5 dB. With linear antennas the
  two rays are nearly equal and cancel into deep fades when flying low. This is why FPV uses circular antennas.
- **Fading.** A hovering quad sees a steady signal (no movement, no change); fast flight flickers.

## Interference

Each emitter of the world file goes through the same propagation (without fading), then the receiver's
adjacent-channel rejection by frequency offset: 0 dB on channel, 10 dB at 20 MHz, 25 dB at 40 MHz, 40 dB at 60 MHz
and beyond, linear in between. The next Raceband channel (37 MHz away) is rejected by 22.75 dB.

`SNR = P - 10 log10(10^(N/10) + Σ 10^(I/10))`, with the noise floor N = -93 dBm by default (thermal noise in about
20 MHz plus an 8 dB noise figure).

## The receiver

| SNR | Picture |
|---|---|
| ≥ 25 dB | clean |
| 25 → 12 dB | grain rises to half of full static |
| < 12 dB | sparkles (FM threshold clicks), full at 4 dB |
| < 8 dB | colour fades, gone at 5 dB |
| < 6 dB | sync unstable: tearing, line jitter |
| < 3 dB for 3 fields | sync lost: the picture rolls; static at 0 dB and below |
| ≥ 6 dB for 5 fields | a lost sync relocks |

## Link budget check

25 mW (14 dBm) with 2 dBi RHCP omnis at both ends, at their peak gain, with the body shadow, the ground bounce and
fading off, falls to 8 dB SNR at 580 m; 600 mW reaches about 3 km (a unit test pins both). In the shipped world
(diversity goggles: an omni and an 8 dBi patch), with the quad on the ground straight ahead of the pilot:

| VTX power | Clean to | Sync lost by |
|---|---|---|
| 25 mW | about 100 m | 2 km |
| 200 mW | about 300 m | 4 km |
| 600 mW | about 500 m | beyond 4 km |

("Clean" is SNR ≥ 25 dB, no grain at all; the picture stays easily flyable well beyond, with grain and then
sparkles.)

## Where it shows

- Bus signals: `video.snr_db`, `video.rssi_dbm.<antenna>`, `video.antenna`, `video.interference_dbm`,
  `video.noise`, `video.sparkles`, `video.chroma`, `video.sync`.
- The state stream: `State.video` (protocol 4), and the events `video_lost` and `video_restored`.
- The game: the HUD's `VID` line, and the shader of `godot/ui/video.gd` between Betaflight's OSD and the HUD, so the
  OSD breaks up with the picture.
```

- [ ] **Step 2: Developer docs and README**

In `docs/dev-setup.md`, replace:

```markdown
  3. Fly, then cut the radio (K, or switch the transmitter off): the HUD reports the link lost and Betaflight fails safe within a few seconds. R reloads the quad.
  4. Configurator: connect to the address the HUD shows (`tcp://127.0.0.1:5761`), change a value and Save (the Configuration tab's Save and Reboot reboots Betaflight; the HUD counts the restarts).
- **Visual check:** `godot --path godot -s res://tests/shots.gd -- --out=<dir>` opens a window briefly and saves screenshots of the start and a short hop in FPV and chase views plus the help and controls screens (`start_fpv.png`, `start_chase.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png`); review them by eye. Without `--out=` they go to the project's user data dir.

## OSD, VTX and battery telemetry (M3a)
```

with:

```markdown
  3. Fly, then cut the radio (K, or switch the transmitter off): the HUD reports the link lost and Betaflight fails safe within a few seconds. R reloads the quad.
  4. Configurator: connect to the address the HUD shows (`tcp://127.0.0.1:5761`), change a value and Save (the Configuration tab's Save and Reboot reboots Betaflight; the HUD counts the restarts).
- **Visual check:** `godot --path godot -s res://tests/shots.gd -- --out=<dir>` opens a window briefly and saves screenshots of the start and a short hop in FPV and chase views, the OSD, the three looks of the analog video link (`video_grain.png`, `video_unstable.png`, `video_lost.png`), and the help and controls screens (`start_fpv.png`, `start_chase.png`, `osd_fpv.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png`); review them by eye. Without `--out=` they go to the project's user data dir.

## OSD, VTX and battery telemetry (M3a)
```

In `docs/dev-setup.md`, replace:

```markdown
4. Change the same from Betaflight Configurator's VTX tab and Save.
5. Lower `battery.initial_soc` to 0.02 and see LOW BATTERY.

## Troubleshooting (Windows)
```

with:

```markdown
4. Change the same from Betaflight Configurator's VTX tab and Save.
5. Lower `battery.initial_soc` to 0.02 and see LOW BATTERY.

## The world file and the analog video link (M3b)

A session flies in a world: `LoadRequest.world_path` (Python `load(..., world=...)`, the game's `world_path` setting,
`OFS_WORLD`, `--world=`) names a world file, and without one the server uses a built-in open field (the pilot at home,
one omni, nothing else). The game loads `worlds/flat.toml` by default.

A world file (`schema_version = 1`, NED metres from home like the quad file, sizes `[north, east, height]`) has:
- `[pilot]` — where the goggles are and which way the pilot faces;
- `[receiver]` — the noise floor, diversity, and the goggle antennas (`omni` or `patch` with a beamwidth; gain,
  polarization, aim relative to the pilot's facing);
- `[[objects]]` — boxes and vertical cylinders with a colour and an `rf_loss_db` (0: transparent to the signal).
  The game draws them from the world the server loaded (`GetWorld`), so what you see is what the video link sees.
  Nothing collides with the drone (the simulator's ground is the plane d = 0);
- `[[emitters]]` — other transmitters (a frequency, or a band and channel, and a power), which interfere with
  neighbouring channels. The shipped world has one, on R2, next to the quad's default R1.

The quad file's `[vtx]` describes the VTX antenna (`[vtx.antenna]`: kind, gain, polarization, `mount_frd`) and the
pit-mode power (`pit_power_mw`). The VTX transmits from load on, also in open loop (it has no Betaflight to answer
then, so it stays on its power-up channel and power).

The link model runs once per PAL field (50 Hz): path loss, antenna patterns, polarization, the quad's own frame,
diffraction around objects, the ground bounce, fading and interference give each goggle antenna's signal; diversity
picks one; the SNR decides the picture (grain, sparkles, colour, tearing, rolling, static). The equations and every
constant are in `docs/research/video-link.md`. The results are in `State.video` (Python `state.video`), and
`video_lost` / `video_restored` events mark sync losses. In the game, the HUD's `VID` line shows the SNR, the antenna
in use and its signal, and the layer `godot/ui/video.gd` draws the breakup over the picture and Betaflight's OSD. The
**Video effects** box on the F2 screen (also `OFS_VIDEO_EFFECTS`, `--video-effects=`) turns the drawing off; it is
saved in `user://ofs_client.cfg`.

Manual check (with the game, real Betaflight optional):
1. Fly away from the pilot (the figure beside the launch pad) and watch grain, then sparkles, colour loss, tearing and
   static as the HUD's `VID` number drops; come back and the picture relocks.
2. Fly behind building B (the tall one, east of the course): the picture breaks up within a few metres.
3. With Betaflight: lower the VTX power in the OSD menu (25 mW) and the picture breaks up much closer; pit mode leaves
   a picture only next to the pilot.
4. Switch the VTX to R2 (the parked quad's channel) and the picture gets noisier; any channel far from R2 is clean.
5. Untick Video effects on the F2 screen: the picture is clean whatever the link does.

## Troubleshooting (Windows)
```

In `README.md`, replace:

```markdown
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M2b — the Godot pilot client.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
- A Godot pilot client flies the simulator from a game window, through the same radio link (see below).

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
```

with:

```markdown
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M3b — the analog video link.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
- A Godot pilot client flies the simulator from a game window, through the same radio link (see below).
- Betaflight's own OSD, a SmartAudio VTX and the battery reach the FPV view (M3a), and a 5.8 GHz analog link model
  breaks the picture up with distance, attitude, buildings and interference (M3b, `docs/research/video-link.md`).

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
```

In `README.md`, replace:

```markdown
Configurator port all behave as in the Python session. Betaflight's own OSD is drawn in the FPV view, the VTX is
controlled by Betaflight over SmartAudio and shown in the HUD, and the battery reaches Betaflight so its OSD shows
real voltage and warnings. A USB radio in joystick mode, a gamepad or the keyboard
flies it; the F2 screen sets the bindings, and they persist to `user://controls.json` (Godot's per-user data dir,
`%APPDATA%\Godot\app_userdata\Open FPV Sim\` on Windows, `~/.local/share/godot/app_userdata/Open FPV Sim/` on Linux).
```

with:

```markdown
Configurator port all behave as in the Python session. Betaflight's own OSD is drawn in the FPV view, the VTX is
controlled by Betaflight over SmartAudio and shown in the HUD, and the battery reaches Betaflight so its OSD shows
real voltage and warnings. The world (`worlds/flat.toml`: launch pad, gates, buildings, the pilot's spot and a parked
quad's VTX) is a data file the simulator loads; its analog video link degrades the picture and the OSD together, like
real goggles. A USB radio in joystick mode, a gamepad or the keyboard
flies it; the F2 screen sets the bindings, and they persist to `user://controls.json` (Godot's per-user data dir,
`%APPDATA%\Godot\app_userdata\Open FPV Sim\` on Windows, `~/.local/share/godot/app_userdata/Open FPV Sim/` on Linux).
```

In `README.md`, replace:

```markdown
Settings: built-in defaults < the `[client]` section of `user://ofs_client.cfg` < `OFS_*` environment variables <
command-line flags after `--`. By default the game starts `ofs-sim` itself (from `target/release`, else
`target/debug`, else `PATH`) on `127.0.0.1:50051` and loads `quads/opendrone-5f-freestyle.toml` in real time.
Overrides:

```

with:

```markdown
Settings: built-in defaults < the `[client]` section of `user://ofs_client.cfg` < `OFS_*` environment variables <
command-line flags after `--`. By default the game starts `ofs-sim` itself (from `target/release`, else
`target/debug`, else `PATH`) on `127.0.0.1:50051` and loads `quads/opendrone-5f-freestyle.toml` in
`worlds/flat.toml` in real time.
Overrides:

```

In `README.md`, replace:

```markdown
| open loop (no firmware) | `OFS_OPEN_LOOP` | `--open-loop` |
| SITL launch command | `OFS_SITL_LAUNCH` | `--sitl-launch=` |

    godot --path godot -- --open-loop              # fly the model without Betaflight
```

with:

```markdown
| open loop (no firmware) | `OFS_OPEN_LOOP` | `--open-loop` |
| SITL launch command | `OFS_SITL_LAUNCH` | `--sitl-launch=` |
| world file | `OFS_WORLD` | `--world=` |
| video effects (on/off) | `OFS_VIDEO_EFFECTS` | `--video-effects=` |

    godot --path godot -- --open-loop              # fly the model without Betaflight
```

- [ ] **Step 3: Carried debt**

Create `docs/superpowers/m3b-carried-debt.md`:

```markdown
# M3b carried debt

These items were deliberately left out of M3b, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **Collision with world objects.** The world file's boxes and cylinders are drawn and block the video signal, but the drone flies through them: the physics contact model still knows only the ground plane. The world file is the natural input for it.
- **ELRS on the shared propagation.** `ofs-video::propagation` is pure and frequency-agnostic; the ELRS link still uses the quad file's fixed RSSI, SNR and loss figures. Moving it onto the same geometry would make range and buildings cost control-link quality too.
- **The "VTX interference" fault (M4).** Emitters are static world data today; the fault would add, move or remove emitters at run time.
- **Non-flat terrain and rotated boxes.** The ground is the plane d = 0 (ground bounce, rooted objects) and boxes are axis-aligned.
- **Configurable receiver thresholds.** The picture thresholds (25, 12, 8, 6, 3 dB...) are named constants in `ofs-video::link`, not world-file fields.
- **Video latency, frame drops and DVR.** The link changes the picture's quality only.

## Modelling simplifications (estimates, see docs/research/video-link.md)
- One knife edge per object, losses of several objects added; no reflections off objects, only off the ground.
- The reflected ray is not obstructed separately: an object's loss applies to the combined direct and reflected signal.
- The body shadow is one smooth lobe (forward and down) of up to 8 dB, not a measured pattern of a real frame.
- Every constant is an estimate; none is measured.

## Rulings made while planning
- **The VTX transmits in open loop.** The spec's open-loop tests need a transmitting VTX, and a powered VTX transmits whether or not a flight controller talks to it, so an open-loop quad with `[vtx]` gets the VTX model (on its power-up channel and power) and the link. M3a's tests that expected no VTX in open loop were updated.
- **The obstruction is found by a search, not 32 samples.** A golden-section search on the convex shape's signed distance finds the deepest point exactly, so a thin post on a long path is not stepped over (the spec's risk table named this).
- **Objects standing on the ground are rooted below it.** Otherwise the nearest face of a building to a low path is its bottom face, and the knife edge measured under the building.
- **Emitter frequencies may be 5300 to 6000 MHz.** The spec said 5600 to 6000; Lowband (5362 to 5621 MHz) is one of the VTX's own bands, so it is accepted for emitters too.
- **The sync-loss test is at 4 km and 25 mW**, not 2 km: with the shipped diversity goggles (8 dBi patch), 25 mW at 2 km is still marginal rather than lost.
```

- [ ] **Step 4: The whole suite**

Run: `cargo test --workspace --locked`
Expected: PASS.

Run: `wsl_run cargo test --workspace --locked`
Expected: PASS on Linux.

Run: `wsl_run cargo test -p ofs-fc -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`
Expected: PASS (8 tests).

Run: `wsl_run cargo test -p ofs-video --test vtx_live --locked -- --ignored --test-threads=1`
Expected: PASS.

Run: `python -m pytest python/tests -q`
Expected: PASS (with `OFS_SITL_LAUNCH` set: the SITL tests too).

Run: `GODOT_BIN=<console exe> bash scripts/run-godot-tests.sh all`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add docs/research/video-link.md docs/superpowers/m3b-carried-debt.md docs/dev-setup.md README.md
git commit -m "docs: M3b, the analog video link and the world file"
```

- [ ] **Step 6: Hand the manual check to the user**

The user flies the game (with a controller; real Betaflight for steps 3 and 4) through the checklist under "The world file and the analog video link (M3b)" in `docs/dev-setup.md`: fly away and back, fly behind building B, lower the VTX power and try pit mode, switch to R2, untick Video effects. Report what they saw; the constants live in `ofs-video::link` and `ofs-video::propagation` if the feel needs tuning.
