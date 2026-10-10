# M3c: World Collision and ELRS on the Shared Propagation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The world file's objects become solid — the quad bounces off buildings and gates, lands on roofs and raises `COLLISION` events — and the ELRS control link's RSSI, SNR, LQ and loss come from the same geometry as the video link instead of fixed quad-file numbers.

**Architecture:** `Shape` (axis-aligned box, vertical cylinder) moves to a new `ofs_core::shape` module and gains an analytic `normal` and `bounds`; the shared RF code (propagation, Rician fader, diversity, body shadow) moves to a new `ofs-rf` crate that `ofs-video` and `ofs-radio` both depend on. `ofs-physics` gains two contact mechanisms against the world's shapes: the M1 spring-damper landing contact points now also act against objects, and new collision spheres (one per config or a generated default) resolve impulse contacts with position correction, restitution, Coulomb friction and a `COLLISION` event. `ofs-radio::elrs` is rewritten to compute per-packet RSSI per quad antenna through `ofs-rf` at 2440 MHz, pick the best antenna with 2 dB hysteresis, and lose packets by a logistic curve around the LoRa mode's sensitivity. Quad schema 4 replaces the fixed `[radio]` loss numbers with `tx_power_mw` and antennas and adds the optional `[collision]` section; the world file gains an optional `[handset]`. Protocol 5 carries SNR, active antenna, downlink LQ, the collision speed and the handset; the Godot HUD shows the link line, toasts collisions, and draws a handset marker.

**Tech Stack:** Rust 1.85+ (workspace: glam, rand, rand_chacha, rand_distr, serde/toml, tonic/prost), Python 3.10+ client (grpcio), Godot 4.7.2 + godot-rust.

**Spec:** `docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md` (read it first; this plan implements it). Predecessor: `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md` (merged as 40ac096). Parent: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`: no language or std features newer than 1.85 in any crate except `ofs-godot` (the msrv CI job runs `cargo check --workspace --all-targets --locked --exclude ofs-godot`).
- Python >= 3.10. Godot **4.7.2** only (GL Compatibility). License GPL-3.0-or-later.
- Determinism: same quad + world + seed + inputs in lockstep give identical output. The ELRS link draws only from `model_rng(seed, "radio.elrs")` and every packet draws the same count of numbers whatever the inputs; the contact resolver must be order-fixed (deepest first, stable ties).
- Protocol: `PROTOCOL_VERSION = 5` in `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` together (a test compares them). Quad schema moves 3 → **4** (`MIN_SCHEMA_VERSION` moves to 4 too: a schema-3 file is a load error). World schema stays **1**.
- Frames: world files use NED metres from home (up is a negative `d`), the quad file's body frame is FRD (x forward, y right, z down). Only `ofs-client` converts to Godot's frame; GDScript does no NED arithmetic.
- The ELRS link runs at the packet rate (50/150/250/500 Hz, all must divide `sim.base_hz`); the video link stays at 50 Hz.
- ELRS is 2.4 GHz only, `ELRS_FREQ_MHZ = 2440.0`; no frequency hopping, no 900 MHz.
- Every text file ends with a newline. Commit the `.uid` files Godot creates next to new `.gd` files. Never stage `.superpowers/`, `graphify-out/`, `shots/` or `build/`.
- Commit messages end with the trailer `Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>`.
- Never use `pkill -f` (use `pkill -x betaflight_SITL`); never run `wsl --shutdown`; do not touch Windows Firewall settings (ask the user).
- Task 2 and Task 6 change the dependency graph (`ofs-rf` is new; `ofs-video` loses `rand`/`rand_distr`; `ofs-radio` gains `glam`/`ofs-rf`; `ofs-sim` gains `ofs-rf`): run the first cargo command of each without `--locked` so `Cargo.lock` updates, and commit `Cargo.lock` with the task.
- Files that contain backslashes must be written with the editor tools (Write/Edit), not Bash heredocs (heredocs collapse `\\`).
- Every edit below is "In `file`, replace: ... with: ...": the old text appears exactly once in the file at that point of the plan. Apply the edits of a step in the order given.

## Rulings and facts verified in advance

Every code block below was written against the current tree (ab92d59) with these facts in hand; where the spec was silent or self-contradictory, the ruling is listed here and the change is one config value or one function if the human disagrees.

- **Ruling: a 50 km, 10 mW link is never up, so `LINK_DOWN` has no edge there.** The spec's "at 50 km with 10 mW the link goes down and `LINK_DOWN` is raised" splits into two pins: the 50 km run proves the link never comes up (Task 9, Rust and live), and the `LINK_DOWN` event is pinned on the up→down transition with the radio-link-loss fault (Task 9, Python). The server's existing edge detector cannot fire on a link that was never up.
- **Ruling: the config schema lands (Task 5) before the ELRS rewrite (Task 6).** The spec's order of work puts the ELRS link at step 3 and the schema at step 4, but the new `LinkParams` reads the new config fields — rewriting the link first would leave `ofs-sim` uncompilable at the task boundary. The ELRS unit tests still all land before any protocol or client work.
- **Ruling: the collision event needs a third bus signal.** The spec's `body.collision_speed` and `body.collision_object` carry the event's values, but `Session::events_for` detects events by comparing remembered state (how `fc_restarts` and `link_up` work today), so the body also publishes `body.collision_count`, a monotonic counter of raised events. The count never goes to the protocol; the two spec signals do (via `State.collision_speed_mps`) and the event message does.
- **Ruling: the event message is the toast text.** The server formats `HIT <ObjectName> <speed>.0 m/s` (e.g. `HIT BuildingA 7.2 m/s`) and `HARD LANDING <speed>.0 m/s` for the ground, exactly the strings the spec's Godot section shows; Python's event message therefore names the object and the speed, and Godot toasts the message verbatim.
- **Ruling: `RadioLink.active_antenna` is the antenna's name** (a string, like the video link's `active_antenna`), not an index: a client cannot map an index without the quad file.
- **Ruling: the handset transmits the uplink on its active antenna.** With two handset antennas, the active one is chosen by the same 2 dB-hysteresis `Diversity`, fed by the downlink RSSI it receives; the downlink RSSI/LQ/SNR reported in LINK_STATISTICS are measured at that active handset antenna. With one antenna everything degenerates to the spec's formulas (all tests use one or two quad antennas, one handset antenna unless testing diversity).
- **Ruling: the downlink runs one packet per uplink packet** (same rate, same sensitivity curve, `DOWNLINK_TX_POWER_MW = 100`), with its own fading processes (one per handset antenna) and its own 100-packet window. Lost downlink packets affect only the LINK_STATISTICS fields (CRF telemetry to the radio is M3d).
- **Ruling: `rf_mode` and the TX power index.** `rf_mode` is the CRSF rf_mode index of the packet rate (0 = 4 Hz, 1 = 50, 2 = 150, 3 = 250, 4 = 500). `uplink_tx_power` is the CRSF power index: 10 mW → 1, 25 → 6, 50 → 10, 100 → 13, 250 → 17, 500 → 20, 1000 → 23.
- **Ruling: the downlink bus signal is `radio.downlink_lq`** verbatim from the spec §5 (the uplink's existing signal stays `radio.lq_pct`; the inconsistency is the spec's, kept for reviewability).
- **Ruling: the open-loop collision test drops the quad onto BuildingA's roof.** `OpenLoopFc` is throttle-only (`crates/ofs-fc/src/open_loop.rs`), so no open-loop test can steer into a wall. The spec's "a quad driven into building A raises one `COLLISION` naming it and ends up outside it" is implemented as: initial pose 1.5 m above BuildingA's roof, free fall, one `COLLISION` naming `BuildingA`, resting on the roof, never inside it. The 30 m/s wall hit into a 12 cm post is pinned at the `RigidBody` unit level (Task 4).
- **Ruling: the generated body sphere reaches 1 cm past the shipped quad's legs.** The spec generates a 4 cm body sphere at the centre of mass and claims the spheres "sit above the landing contact points, so the quad rests on its legs" — but the shipped quad's legs are at FRD z = +0.03 and a 4 cm sphere reaches z = +0.04. Kept the spec's literal 4 cm (changing a spec number silently is worse): at rest the quad rests on its belly sphere 1 cm earlier than on its legs; the rest tests pin "stays still", not which surface touches. Flagged for the maintainer in `m3c-carried-debt.md` (fix = body sphere 2.5 cm, or legs at 0.05). The no-12-cm-gap claim holds either way (adjacent prop spheres leave a 3.35 cm gap; ring-to-body leaves 0.85 cm).
- **Ruling: sphere contacts use the shape's normal at the sphere centre**, the contact point is the sphere's surface point along that normal, and the impulse's lever arm runs from the centre of mass to that surface point — an off-centre hit turns the quad through the inertia tensor (spec §4 step 3).
- **Ruling: schema-3 files fail at the version check**, before serde runs, with a message naming every removed field and its replacement (the serde path would only say "unknown field `rssi_dbm`").
- **Fact: ` ofs-video` uses `rand`/`rand_distr` only for the Rician fader** (`crates/ofs-video/src/link.rs` lines 10-11): after the move `ofs-video` drops both and keeps `rand_chacha` (the `VideoLink` still owns the stream that feeds `Fader::next`).
- **Fact: the ELRS test rig parses the UART with `ofs_radio::crsf::Decoder`** (`Decoder::default().push(&wire.take(usize::MAX)) -> Vec<Frame>`, `Frame::RcChannels([u16; 16])`, `Frame::LinkStatistics`), and `LinkParams::ideal` has three in-crate test users — all rewritten in Task 6.
- **Fact: the shipped quad's numbers used throughout**: `prop.diameter_m = 0.127` (tip radius 6.35 cm), motors at FRD ±0.08, legs at z = +0.03, `base_hz = 8000`, packet rate 500 Hz, radio on UART2.
- **Fact: the open-loop vehicle tests build directly** (`ofs_sim::vehicle::build` with `fc_override: Some(FcKind::OpenLoop)` and a `WorldConfig`), and `Vehicle::digest()` hashes the whole bus — the existing `same_seed_and_inputs_give_identical_runs` test therefore covers the radio and collision signals the moment they are bus signals.

## How to run things (Windows host, Git Bash)

- Rust tests: `cargo test -p <crate> --locked` from the repository root; the whole workspace: `cargo test --workspace --locked`.
- Live SITL tests run in WSL (Windows Firewall blocks freshly built test executables that must receive SITL datagrams). Define once per shell:

```bash
wsl_run() { wsl.exe -d Ubuntu -e bash -lc "cd /mnt$(pwd) && export CARGO_TARGET_DIR=\$HOME/ofs/target OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf && $*"; }
```

  Example: `wsl_run cargo test -p ofs-fc --test sitl_live --locked -- --ignored --test-threads=1`. (`wsl.exe` prints a harmless `.wslconfig` warning.)
- Python tests run on Windows: `cargo build -p ofs-sim` then `python -m pytest python/tests -q`. The SITL ones need `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf"` and an `ofs-sim.exe` the firewall allows (see `docs/dev-setup.md`, "Troubleshooting (Windows)"; `OFS_SIM_BIN` points at it).
- Godot tests: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh [unit|e2e|all]` (it builds `ofs-sim` and `ofs-godot` into `target/` first; Godot loads the extension from `target/debug`, so keep `CARGO_TARGET_DIR` at its default for these).
- Regenerate the Python stubs after a proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`.

## Review Focus

Failure modes the spec implies but a straight reading of the tasks would not cover; each has a pinning test in the named task.

1. **Existing schema-3 quad files** (every user quad file written before this milestone): refused at load by the version check with a message naming what was removed (`radio.rssi_dbm`, `radio.snr_db`, the loss parameters) and what replaces them (`radio.tx_power_mw`, `radio.antennas`) — not a serde "unknown field" error. Test: Task 5 (`a_schema_3_quad_file_is_refused_with_the_field_changes`).
2. **The quad at rest** (on the pad, on a roof, after a bounce): no drift, no jitter above 1 mm over 10 s, nothing non-finite — restitution is 0 below 0.2 m/s and the legs carry the weight. Tests: Task 3 (`a_quad_rests_on_a_roof_for_ten_seconds`), Task 4 (`a_bouncing_sphere_settles_and_stays_put`), Task 9 (vehicle level).
3. **The open field and object-free worlds** (the default): the collision code is a no-op with zero objects, the ELRS link works with no obstacles and the default handset, nothing divides by an empty list. Tests: Task 4 (`contacts_with_no_objects_leave_the_body_alone`), Task 6 (`ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics`).
4. **The link at the sensitivity edge** (RSSI ≈ sensitivity, fading swinging it across): PER ≈ 0.5 gives intermittent packets, LQ between 0 and 100, SNR finite and negative-capable, one-antenna diversity never panics. Tests: Task 6 (`per_is_half_at_the_sensitivity`, `the_rssi_reaches_the_sensitivity_at_the_published_range`, `a_hovering_quad_in_the_open_keeps_lq_at_100`).
5. **A protocol-4 client against the protocol-5 server** (an old Python install): the handshake reply names the server's version, the client raises `ProtocolMismatch` naming both, and no field-level garbage is read. Tests: Task 7 (the version constants move together — `crates/ofs-proto/tests/messages.rs` asserts 5 on the Rust side and reads the Python constant's side of the contract; the existing gRPC handshake test re-runs against 5).

## File Structure

Create:
- `crates/ofs-core/src/shape.rs`, `crates/ofs-core/tests/shape.rs` — `Shape`, `Aabb`, `signed_distance`, `rooted`, `normal`, `bounds`.
- `crates/ofs-rf/` — `Cargo.toml`, `src/lib.rs`, `src/propagation.rs`, `src/fading.rs`, `tests/propagation.rs`, `tests/fading.rs`.
- `crates/ofs-physics/src/collision.rs`, `crates/ofs-physics/tests/collision.rs` — impulse contact of the collision spheres.
- `crates/ofs-radio/src/elrs.rs` (rewritten), `crates/ofs-radio/tests/elrs.rs` (rewritten).
- `python/tests/test_sitl_link.py`, `quads/opendrone-5f-freestyle-10mw-far.toml` (written by the live test into `tmp_path`, not committed).
- `docs/research/elrs-link.md`, `docs/research/collision.md`, `docs/superpowers/m3c-carried-debt.md`.

Modify: `crates/ofs-core/src/lib.rs`, `crates/ofs-video/{Cargo.toml,src/lib.rs,src/propagation.rs,src/link.rs}`, `crates/ofs-video/tests/{propagation,link}.rs` (moved/trimmed), `Cargo.toml`, `Cargo.lock`, `crates/ofs-physics/src/{lib.rs,rigid_body.rs}`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-config/src/{lib.rs,world.rs}`, `crates/ofs-config/tests/{config,world}.rs`, `quads/opendrone-5f-freestyle.toml`, `crates/ofs-radio/{Cargo.toml,src/lib.rs,src/names? no — ofs-core/src/names.rs}`, `crates/ofs-sim/{Cargo.toml,src/vehicle.rs,src/session.rs}`, `crates/ofs-sim/tests/{open_loop_vehicle,grpc}.rs`, `proto/ofs/v1/sim.proto`, `crates/ofs-proto/{src/lib.rs,tests/messages.rs}`, `python/ofs/{__init__.py,client.py}` and the regenerated `python/ofs/v1/*`, `python/tests/{conftest,test_client,test_realtime}.py`, `crates/ofs-client/src/{model,worker}.rs`, `crates/ofs-client/tests/{model,session}.rs`, `crates/ofs-godot/src/lib.rs`, `godot/{scripts/app.gd,world/world.gd,ui/hud.gd}`, `godot/tests/{test_hud,test_scene,e2e_open_loop}.gd`, `docs/dev-setup.md`, `README.md`.

---

### Task 1: `ofs-core::shape` — the shared solid, with a normal and bounds

**Files:**
- Create: `crates/ofs-core/src/shape.rs`, `crates/ofs-core/tests/shape.rs`
- Modify: `crates/ofs-core/src/lib.rs`, `crates/ofs-video/src/propagation.rs`, `crates/ofs-video/tests/propagation.rs`, `crates/ofs-video/tests/link.rs` (import line only), `crates/ofs-sim/src/vehicle.rs` (import line only)

**Interfaces:**
- Consumes: nothing new (glam `DVec3`).
- Produces (used by Tasks 2, 3, 4, 6), all in `ofs_core::shape`:
  - `pub enum Shape { Box { center: DVec3, half: DVec3 }, Cylinder { center: DVec3, radius: f64, half_height: f64 } }` with the existing `signed_distance(&self, p: DVec3) -> f64` and `rooted(self) -> Shape`, plus new:
  - `pub fn normal(&self, p: DVec3) -> DVec3` — the unit gradient of the signed distance (out of the solid; on an edge or rim the diagonal of the two faces; inside, towards the closest face).
  - `pub fn bounds(&self) -> Aabb`
  - `pub struct Aabb { pub min: DVec3, pub max: DVec3 }` with `pub fn grown(self, margin: f64) -> Aabb` and `pub fn contains(self, p: DVec3) -> bool`.
- `ofs_video::propagation` no longer exports `Shape`; importers use `ofs_core::shape::Shape`.

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-core/tests/shape.rs`:

```rust
use glam::DVec3;
use ofs_core::shape::{Aabb, Shape};

fn close_dir(v: DVec3, expected: DVec3, what: &str) {
    assert!((v - expected).length() < 1e-12, "{what}: expected {expected}, got {v}");
}

const BOX: Shape = Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(1.0, 2.0, 3.0) };
const CYL: Shape = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -2.0), radius: 1.0, half_height: 2.0 };

#[test]
fn a_box_face_normal_points_straight_out_of_that_face() {
    close_dir(BOX.normal(DVec3::new(11.5, 20.0, -5.0)), DVec3::X, "+x face");
    close_dir(BOX.normal(DVec3::new(8.0, 20.0, -5.0)), -DVec3::X, "-x face");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -1.0)), DVec3::Z, "bottom face (down is +z)");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -9.0)), -DVec3::Z, "top face");
    close_dir(BOX.normal(DVec3::new(10.0, 23.0, -5.0)), DVec3::Y, "+y face");
}

#[test]
fn a_box_edge_normal_is_the_diagonal_of_its_two_faces() {
    let n = BOX.normal(DVec3::new(11.5, 22.5, -5.0));
    close_dir(n, DVec3::new(1.0, 1.0, 0.0).normalize(), "+x+y edge");
}

#[test]
fn an_inside_box_point_normals_towards_the_closest_face() {
    close_dir(BOX.normal(DVec3::new(10.9, 20.0, -5.0)), DVec3::X, "closest face is +x");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -2.5)), DVec3::Z, "closest face is the bottom");
    // On a tie the lower axis (x, then y, then z) and the positive side win, so the result is stable.
    close_dir(BOX.normal(BOX.center()), DVec3::X, "dead centre: +x wins");
}

#[test]
fn a_cylinder_normals_out_of_its_side_caps_and_rim() {
    close_dir(CYL.normal(DVec3::new(2.0, 0.0, -2.0)), DVec3::X, "side");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, -5.0)), -DVec3::Z, "top cap (up is -z)");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, 1.0)), DVec3::Z, "bottom cap");
    close_dir(CYL.normal(DVec3::new(1.5, 0.0, -5.0)), DVec3::new(1.0, 0.0, -1.0).normalize(), "top rim");
    close_dir(CYL.normal(DVec3::new(-1.5, 0.0, 1.0)), DVec3::new(-1.0, 0.0, 1.0).normalize(), "bottom rim");
    close_dir(CYL.normal(DVec3::new(0.5, 0.0, -2.0)), DVec3::X, "inside, side is closest");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, -0.5)), DVec3::Z, "inside, bottom is closest");
}

#[test]
fn normals_are_unit_length_everywhere_around_a_shape() {
    for shape in [BOX, CYL] {
        let (lo, hi) = (shape.bounds().min, shape.bounds().max);
        for i in 0..20 {
            for j in 0..20 {
                let p = DVec3::new(
                    lo.x + (hi.x - lo.x) * i as f64 / 19.0,
                    lo.y + (hi.y - lo.y) * j as f64 / 19.0,
                    lo.z - 1.0,
                );
                let n = shape.normal(p);
                assert!((n.length() - 1.0).abs() < 1e-9, "normal at {p} is {n}");
                // The gradient points towards increasing signed distance: stepping along it never goes deeper in.
                let step = shape.signed_distance(p + n * 1e-6) - shape.signed_distance(p);
                assert!(step >= -1e-9, "gradient points inward at {p}");
            }
        }
    }
}

#[test]
fn bounds_contain_the_shape_and_their_surface() {
    for shape in [BOX, CYL] {
        let b = shape.bounds();
        assert!(b.contains(shape.bounds().min + DVec3::ONE * 1e-6));
        // Every axis sample of the shape's surface is inside the box.
        for t in 0..=10 {
            let f = t as f64 / 10.0;
            match shape {
                Shape::Box { center, half } => {
                    assert!(b.contains(center + DVec3::new(half.x, half.y * (f - 0.5), half.z * (f - 0.5))));
                }
                Shape::Cylinder { center, radius, half_height } => {
                    let a = f * 2.0 * std::f64::consts::PI;
                    assert!(b.contains(center + DVec3::new(radius * a.cos(), radius * a.sin(), half_height)));
                }
            }
        }
    }
    let b = BOX.bounds();
    assert!(!b.contains(DVec3::new(0.0, 0.0, 0.0)), "a far point is outside");
}

#[test]
fn grown_boxes_and_contains_behave() {
    let b = BOX.bounds();
    let big = b.grown(2.0);
    assert!((big.min.x - (b.min.x - 2.0)).abs() < 1e-12 && (big.max.z - (b.max.z + 2.0)).abs() < 1e-12);
    assert!(big.contains(BOX.center() + DVec3::new(2.5, 0.0, 0.0)));
    assert!(!b.contains(BOX.center() + DVec3::new(1.5, 0.0, 0.0)));
}

impl Shape {
    fn center(&self) -> DVec3 {
        match *self {
            Shape::Box { center, .. } | Shape::Cylinder { center, .. } => center,
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-core --test shape --locked`
Expected: FAIL to compile — `ofs_core::shape` does not exist.

- [ ] **Step 3: Move `Shape` into `ofs-core` and add `normal` and `bounds`**

Create `crates/ofs-core/src/shape.rs` — copy the `Shape` enum, `ROOT_DEPTH_M`, `ON_GROUND_M`, `rooted` and `signed_distance` **verbatim** from `crates/ofs-video/src/propagation.rs` (lines 148-196), then add:

```rust
//! Solids in the world, by their signed distance (negative inside): the video link's obstacles, the physics'
//! collidable objects and the ELRS link's obstructions all use them. Positions and directions are NED metres
//! (down is +z, the ground is the plane z = 0).
use glam::DVec3;
```

at the top (with the enum), and this inside the existing `impl Shape` block:

```rust
    /// The unit gradient of the signed distance at `p`: it points out of the solid. On a face it is the face's
    /// normal, on an edge or a rim the diagonal of its two faces, inside it points at the closest face (on a tie
    /// the lower axis wins, so the result is stable). Deep inside a degenerate point gives up.
    pub fn normal(&self, p: DVec3) -> DVec3 {
        match *self {
            Shape::Box { center, half } => {
                let d = p - center;
                let q = d.abs() - half;
                let outside = q.max(DVec3::ZERO);
                if outside != DVec3::ZERO {
                    return (d.signum() * outside).normalize();
                }
                let axis = if q.x >= q.y && q.x >= q.z { 0 } else if q.y >= q.z { 1 } else { 2 };
                let mut n = DVec3::ZERO;
                n[axis] = if d[axis] >= 0.0 { 1.0 } else { -1.0 };
                n.try_normalize().unwrap_or(DVec3::NEG_Z)
            }
            Shape::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let radial = DVec3::new(d.x, d.y, 0.0);
                let radial_len = radial.length();
                let r = radial_len - radius;
                let v = d.z.abs() - half_height;
                let cap = DVec3::new(0.0, 0.0, d.z.signum());
                if r > 0.0 && v > 0.0 {
                    return (radial / radial_len * r + cap * v).normalize();
                }
                if r > 0.0 {
                    return radial / radial_len;
                }
                if v > 0.0 {
                    return cap;
                }
                if r >= v && radial_len > 0.0 {
                    radial / radial_len
                } else {
                    cap.try_normalize().unwrap_or(DVec3::NEG_Z)
                }
            }
        }
    }

    /// The axis-aligned bounding box of the shape.
    pub fn bounds(&self) -> Aabb {
        match *self {
            Shape::Box { center, half } => Aabb { min: center - half, max: center + half },
            Shape::Cylinder { center, radius, half_height } => Aabb {
                min: DVec3::new(center.x - radius, center.y - radius, center.z - half_height),
                max: DVec3::new(center.x + radius, center.y + radius, center.z + half_height),
            },
        }
    }
}

/// An axis-aligned bounding box: the collision broad phase's unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    /// The box grown by `margin` on every side.
    pub fn grown(self, margin: f64) -> Aabb {
        Aabb { min: self.min - DVec3::splat(margin), max: self.max + DVec3::splat(margin) }
    }

    pub fn contains(self, p: DVec3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z <= self.max.z
    }
```

(The final `}` closes `impl Aabb`.) In `crates/ofs-core/src/lib.rs`, replace:

```rust
pub mod scheduler;
```

with:

```rust
pub mod scheduler;
pub mod shape;
```

- [ ] **Step 4: Point `ofs-video` and `ofs-sim` at the new home**

In `crates/ofs-video/src/propagation.rs`, delete the `Shape` enum, the `ROOT_DEPTH_M`/`ON_GROUND_M` consts, the whole `impl Shape` block, and the doc comment above the enum; replace the import block at the top:

```rust
use glam::DVec3;
```

with:

```rust
use glam::DVec3;
use ofs_core::shape::Shape;
```

In `crates/ofs-video/tests/propagation.rs`, replace:

```rust
use ofs_video::propagation::*;
```

with:

```rust
use ofs_core::shape::Shape;
use ofs_video::propagation::*;
```

In `crates/ofs-video/tests/link.rs`, replace:

```rust
use ofs_video::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

with:

```rust
use ofs_core::shape::Shape;
use ofs_video::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_video::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

with:

```rust
use ofs_core::shape::Shape;
use ofs_video::propagation::{Antenna, AntennaKind, Obstacle, Polarization};
```

- [ ] **Step 5: Run the moved tests to verify they pass**

Run: `cargo test -p ofs-core --locked && cargo test -p ofs-video --locked && cargo test -p ofs-sim --locked`
Expected: PASS — the propagation tests exercise `signed_distance`/`rooted` unchanged through the new import paths; the new `shape` tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-core crates/ofs-video crates/ofs-sim/src/vehicle.rs
git commit -m "feat(core): Shape moves to ofs-core::shape with an analytic normal and bounds

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 2: the `ofs-rf` crate — propagation, fading, diversity and body shadow move out of `ofs-video`

A pure move: every M3b test passes unchanged except import paths. `ofs-rf` depends only on `ofs-core` and the rand stack; `ofs-video` and (from Task 6) `ofs-radio` depend on `ofs-rf`.

**Files:**
- Create: `crates/ofs-rf/Cargo.toml`, `crates/ofs-rf/src/lib.rs`, `crates/ofs-rf/src/propagation.rs`, `crates/ofs-rf/src/fading.rs`, `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-rf/tests/fading.rs`
- Modify: `Cargo.toml` (workspace members + deps), `Cargo.lock`, `crates/ofs-video/Cargo.toml`, `crates/ofs-video/src/lib.rs`, `crates/ofs-video/src/link.rs`, `crates/ofs-video/tests/link.rs`, `crates/ofs-sim/Cargo.toml`, `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs` (import lines only)

**Interfaces:**
- Consumes: `ofs_core::shape::Shape` (Task 1).
- Produces (used by Tasks 6 and 7), all in `ofs_rf`:
  - `ofs_rf::propagation` — everything `ofs_video::propagation` had (minus `Shape`): `wavelength_m`, `fspl_db`, `mw_to_dbm`, `dbm_to_mw`, `power_sum_dbm`, `Polarization`, `AntennaKind`, `Antenna` (`gain_towards`, `field_direction`), `polarization_loss_db`, `knife_edge_loss_db`, `fresnel_v`, `Obstacle`, `deepest_point`, `obstruction_loss_db`, `Endpoint`, `PathGain`, `path_gain`, `obstruction_db`, `unobstructed_gain_db`, `adjacent_channel_rejection_db`, and the constants `SPEED_OF_LIGHT_MPS`, `PATTERN_FLOOR_DB`, `CROSS_POLARIZATION_DB`, `CIRCULAR_TO_LINEAR_DB`, `MIN_DISTANCE_M`.
  - `ofs_rf::fading` — `Fader` (now `pub`, same `next(&mut self, rng: &mut ChaCha8Rng, rho: f64, k_db: f64) -> f64`), `Diversity` (`choose(&mut self, db: &[f64]) -> usize` plus a new `pub fn active(&self) -> usize`), `body_shadow_db(att: DQuat, quad_pos: DVec3, other_pos: DVec3) -> f64`, constants `BODY_SHADOW_DB`, `LOS_K_DB`, `MIN_K_DB`, `DIVERSITY_HYSTERESIS_DB`.
- `ofs_video::link` keeps everything else (`SyncTracker`, `picture`, `snr_db`, `VideoSync`, the thresholds) and consumes `ofs_rf`.

- [ ] **Step 1: Create the crate with the moved code**

Create `crates/ofs-rf/Cargo.toml`:

```toml
[package]
name = "ofs-rf"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
glam.workspace = true
ofs-core.workspace = true
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

Create `crates/ofs-rf/src/lib.rs`:

```rust
//! Shared radio-frequency models: propagation, Rician fading, receiver diversity and body shadow. The analog
//! video link and the ELRS control link both build on them. No bus and no state below the fader level.
pub mod fading;
pub mod propagation;
```

Create `crates/ofs-rf/src/propagation.rs`: copy `crates/ofs-video/src/propagation.rs` **verbatim**, with two changes — the module doc comment becomes:

```rust
//! Radio propagation as pure functions: free-space path loss, antenna patterns, polarization, knife-edge
//! diffraction around obstacles and the two-ray ground bounce. Positions and directions are NED metres (down is
//! +z, the ground is the plane z = 0).
```

and the imports become:

```rust
use glam::DVec3;
use ofs_core::shape::Shape;
```

Create `crates/ofs-rf/src/fading.rs`:

```rust
//! Rician fading, receiver diversity and the quad's body shadow: the receiver-side effects both links share.
use glam::{DQuat, DVec3};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

/// The most the frame, battery and stack take from a signal when they sit between the quad's antenna and the
/// other end of the link.
pub const BODY_SHADOW_DB: f64 = 8.0;
/// The body direction (FRD, unit) the frame shadows most: forward and down, through the stack and the battery.
const SHADOW_DIRECTION: DVec3 = DVec3::new(0.6, 0.0, 0.8);
/// The receiver changes antenna only when another one is better by this much.
pub const DIVERSITY_HYSTERESIS_DB: f64 = 2.0;
/// Rician K-factor with a clear line of sight; obstruction lowers it dB for dB (towards Rayleigh fading).
pub const LOS_K_DB: f64 = 10.0;
/// Lowest K-factor (deep behind an obstacle: practically Rayleigh).
pub const MIN_K_DB: f64 = -20.0;

/// Rician fading of one antenna: an AR(1) complex Gaussian scatter (unit mean power) added to the line of sight.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fader {
    re: f64,
    im: f64,
}

impl Fader {
    /// The fade in dB for this packet or field. `rho` is the correlation with the previous one (1 = stood still).
    pub fn next(&mut self, rng: &mut ChaCha8Rng, rho: f64, k_db: f64) -> f64 {
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

/// Diversity: the antenna with the best signal, changed only for a [`DIVERSITY_HYSTERESIS_DB`] better one.
#[derive(Debug, Clone, Default)]
pub struct Diversity {
    active: usize,
}

impl Diversity {
    /// The antenna to use, given each antenna's level in dB (at least one).
    pub fn choose(&mut self, db: &[f64]) -> usize {
        self.active = self.active.min(db.len().saturating_sub(1));
        let best = (0..db.len()).max_by(|a, b| db[*a].total_cmp(&db[*b])).unwrap_or(0);
        if db[best] > db[self.active] + DIVERSITY_HYSTERESIS_DB {
            self.active = best;
        }
        self.active
    }

    /// The antenna in use (the handset transmits on the antenna it receives best).
    pub fn active(&self) -> usize {
        self.active
    }
}

/// How much of the frame, battery and stack lies between the quad's antenna and `other_pos`, as a loss in dB:
/// up to [`BODY_SHADOW_DB`] when the other end is forward and below the quad, nothing when it is behind or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, other_pos: DVec3) -> f64 {
    let Some(dir_world) = (other_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
}
```

- [ ] **Step 2: Move the propagation tests, move the fading tests**

Move `crates/ofs-video/tests/propagation.rs` to `crates/ofs-rf/tests/propagation.rs` unchanged (its imports already read `ofs_core::shape::Shape` + `ofs_video::propagation::*` — change the latter to `ofs_rf::propagation::*`).

Create `crates/ofs-rf/tests/fading.rs` with the two tests that leave `crates/ofs-video/tests/link.rs` (copy them out of that file):

```rust
use glam::{DQuat, DVec3};
use ofs_rf::fading::{body_shadow_db, Diversity, BODY_SHADOW_DB};

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

#[test]
fn diversity_switches_only_for_a_clearly_better_antenna() {
    let mut d = Diversity::default();
    assert_eq!(d.choose(&[10.0, 11.0]), 0, "1 dB better is not enough");
    assert_eq!(d.choose(&[10.0, 12.5]), 1, "2.5 dB better: switch");
    assert_eq!(d.choose(&[11.0, 10.0]), 1, "and stay while the other is only 1 dB better");
    assert_eq!(d.choose(&[13.0, 10.0]), 0, "back when it is 3 dB better");
    assert_eq!(d.choose(&[5.0]), 0, "a single antenna");
    assert_eq!(d.active(), 0);
}

#[test]
fn the_frame_shadows_the_antenna_when_the_other_end_is_ahead_and_below() {
    let level = DQuat::IDENTITY;
    let quad = DVec3::new(0.0, 0.0, -20.0);
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, 0.0)), BODY_SHADOW_DB, 1e-9, "straight below");
    close(body_shadow_db(level, quad, DVec3::new(-100.0, 0.0, -20.0)), 0.0, 1e-12, "behind, level");
    close(body_shadow_db(level, quad, DVec3::new(0.0, 0.0, -40.0)), 0.0, 1e-12, "above");
    let ahead = body_shadow_db(level, quad, DVec3::new(100.0, 0.0, -20.0));
    assert!(ahead > 4.0 && ahead < BODY_SHADOW_DB, "ahead, level: most of it ({ahead})");
    let turned = DQuat::from_rotation_z(std::f64::consts::PI);
    close(body_shadow_db(turned, quad, DVec3::new(100.0, 0.0, -20.0)), 0.0, 1e-9, "turned away: the antenna sees the other end");
}
```

In `crates/ofs-video/tests/link.rs`: delete the two moved tests, and replace the import line:

```rust
use ofs_video::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};
```

with:

```rust
use ofs_rf::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};
```

- [ ] **Step 3: Rewire `ofs-video` onto `ofs-rf`**

In `crates/ofs-video/src/lib.rs`, replace:

```rust
//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles.
pub mod link;
pub mod osd;
pub mod propagation;
pub mod smartaudio;
pub mod vtx;
```

with:

```rust
//! Video-side models: the SmartAudio VTX, Betaflight's OSD over MSP DisplayPort, and the 5.8 GHz analog link from
//! the VTX to the pilot's goggles (the RF maths itself lives in `ofs-rf`).
pub mod link;
pub mod osd;
pub mod smartaudio;
pub mod vtx;
```

Delete `crates/ofs-video/src/propagation.rs`.

In `crates/ofs-video/src/link.rs`, replace:

```rust
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

use crate::propagation::{
    adjacent_channel_rejection_db, mw_to_dbm, obstruction_db, power_sum_dbm, unobstructed_gain_db, wavelength_m, Antenna,
    Endpoint, Obstacle, PathGain,
};
```

with:

```rust
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{
    adjacent_channel_rejection_db, mw_to_dbm, obstruction_db, power_sum_dbm, unobstructed_gain_db, wavelength_m, Antenna,
    Endpoint, Obstacle, PathGain,
};
use rand_chacha::ChaCha8Rng;
```

Then, in the same file:
- delete the `BODY_SHADOW_DB`, `SHADOW_DIRECTION`, `DIVERSITY_HYSTERESIS_DB`, `LOS_K_DB` and `MIN_K_DB` consts (they moved; `BODY_SHADOW_DB` keeps its doc line in `ofs-rf`);
- delete the whole `Diversity` struct and `impl` (moved);
- delete the whole `Fader` struct and `impl` (moved);
- in `struct VideoLink`, the field `faders: Vec<Fader>` now refers to `ofs_rf::fading::Fader` — no textual change needed.

In `crates/ofs-video/Cargo.toml`, replace:

```toml
[dependencies]
glam.workspace = true
ofs-core.workspace = true
ofs-fc.workspace = true
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

with:

```toml
[dependencies]
glam.workspace = true
ofs-core.workspace = true
ofs-fc.workspace = true
ofs-rf.workspace = true
rand_chacha.workspace = true
```

In the workspace `Cargo.toml`, replace:

```toml
ofs-radio = { path = "crates/ofs-radio" }
```

with:

```toml
ofs-radio = { path = "crates/ofs-radio" }
ofs-rf = { path = "crates/ofs-rf" }
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_video::propagation::{Antenna, AntennaKind, Obstacle, Polarization};
```

with:

```rust
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization};
```

In `crates/ofs-sim/Cargo.toml`, in `[dependencies]`, replace:

```toml
ofs-radio.workspace = true
```

with:

```toml
ofs-radio.workspace = true
ofs-rf.workspace = true
```

(`crates/ofs-sim/src/session.rs` imports only `ofs_video::link::VideoSync` — no change. If `cargo check` reports any other `ofs_video::propagation` import anywhere — the Godot extension, the client — point it at `ofs_rf::propagation` the same way.)

- [ ] **Step 4: Run every test to verify the move is behaviour-neutral**

Run (first command without `--locked` so `Cargo.lock` gains `ofs-rf`):
`cargo test --workspace --locked`
Expected: PASS — the same test count as before the task (the propagation and fading tests moved crates; nothing else changed).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/ofs-rf crates/ofs-video crates/ofs-sim
git commit -m "feat(rf): the shared RF code (propagation, fading, diversity, body shadow) moves to a new ofs-rf crate

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 3: the landing contact points act against world objects

The M1 spring-damper and friction of `[ground]` now act on every `contact_points_frd_m` point against the ground plane **and** every world object: against an object, the depth is minus the shape's signed distance and the normal is the shape's `normal` there. This is how the quad lands on the pad or on a roof. (The impulse spheres of Task 4 are separate.)

**Files:**
- Modify: `crates/ofs-physics/src/rigid_body.rs`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-sim/src/vehicle.rs` (one field)

**Interfaces:**
- Consumes: `ofs_core::shape::{Aabb, Shape}` (Task 1).
- Produces (used by Tasks 4 and 6):
  - `ofs_physics::rigid_body::WorldObject { pub name: String, pub shape: Shape }`
  - `AirframeParams` gains `pub objects: Vec<WorldObject>` (empty = no objects, the behaviour until Task 6 wires the world in).

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-physics/tests/rigid_body.rs`, replace the helper:

```rust
fn params(mounts: Vec<MotorMount>, contacts: Vec<DVec3>) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts,
        contact_points_frd_m: contacts,
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
    }
}
```

with:

```rust
fn params(mounts: Vec<MotorMount>, contacts: Vec<DVec3>) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts,
        contact_points_frd_m: contacts,
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects: Vec::new(),
    }
}

/// A box occupying x in [0, 2], y in [-2, 2], z in [-1, 0]: a wall the quad's feet can press into.
fn wall() -> Vec<WorldObject> {
    vec![WorldObject { name: "wall".into(), shape: Shape::Box { center: DVec3::new(1.0, 0.0, -0.5), half: DVec3::new(1.0, 2.0, 0.5) } }]
}

/// A roof to land on: a box whose top face is the plane z = -1.
fn roof() -> Vec<WorldObject> {
    vec![WorldObject { name: "roof".into(), shape: Shape::Box { center: DVec3::new(0.0, 0.0, -1.5), half: DVec3::new(5.0, 5.0, 0.5) } }]
}

#[test]
fn a_foot_pressed_into_a_wall_pushes_back_along_its_normal() {
    // One foot 5 mm inside the wall's near face (x = 0), level with the wall (z in [-1, 0]); the spring-damper
    // pushes it out along -x.
    let mut s = sim(params(vec![], vec![DVec3::new(0.08, 0.0, 0.0)]), at(DVec3::new(-0.075, 0.0, -0.5)));
    s.step().unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    let push = 3000.0 * 0.005 / 0.65; // k * depth / mass, in m/s^2
    assert!((vel.x + push / 8000.0).abs() < push / 8000.0 * 0.2, "one tick of push: vel {vel}");
    assert!((vel.z - GRAVITY_MPS2 / 8000.0).abs() < 1e-9, "gravity still acts: vel {vel}");
}

#[test]
fn a_quad_lands_and_rests_on_a_roof() {
    let mut p = params(vec![], feet());
    p.objects = roof();
    let mut s = sim(p, at(DVec3::new(0.0, 0.0, -1.05)));
    s.run_for(3.0).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.length() < 1e-3, "settled: vel {vel}");
    let pos = vec3(&s, names::BODY_POS_NED);
    // The legs (0.03 below the CoM) carry it 2 mm into the surface (mg/k), never through it.
    let height = pos.z + 0.03;
    assert!(height > -1.0 && height < -0.995, "resting on the roof: legs at {height}, roof top at -1");
}

#[test]
fn a_quad_rests_on_a_roof_for_ten_seconds_without_drift_or_jitter() {
    let mut p = params(vec![], feet());
    p.objects = roof();
    let mut s = sim(p, at(DVec3::new(0.0, 0.0, -1.05)));
    let mut samples = Vec::new();
    for _ in 0..20 {
        s.run_for(0.5).unwrap();
        samples.push(vec3(&s, names::BODY_POS_NED));
    }
    let last = &samples[10..];
    for axis in 0..3 {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for p in last {
            min = min.min(p[axis]);
            max = max.max(p[axis]);
        }
        assert!(max - min < 1e-3, "axis {axis} jitters {} mm over the last 5 s", (max - min) * 1000.0);
    }
    let (first, end) = (last[0], last[last.len() - 1]);
    assert!((end - first).length() < 1e-3, "no drift over the last 5 s: {} -> {}", first, end);
}
```

At the top of the file, replace:

```rust
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
```

with:

```rust
use ofs_core::shape::Shape;
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody, WorldObject};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-physics --locked`
Expected: FAIL to compile — no field `objects` on `AirframeParams`, no type `WorldObject`.

- [ ] **Step 3: Add the objects to the body and extend the contact model**

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper ground contact.
use glam::{DQuat, DVec3};
```

with:

```rust
//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper contact against the
//! ground plane and the world's objects.
use glam::{DQuat, DVec3};
use ofs_core::shape::{Aabb, Shape};
```

After the `MotorMount` struct, add:

```rust
/// A world object the body can touch, by its name and its shape exactly as the world file gives it (not rooted).
#[derive(Debug, Clone)]
pub struct WorldObject {
    pub name: String,
    pub shape: Shape,
}
```

In `AirframeParams`, replace:

```rust
    pub contact_points_frd_m: Vec<DVec3>,
    pub ground: GroundParams,
}
```

with:

```rust
    pub contact_points_frd_m: Vec<DVec3>,
    pub ground: GroundParams,
    /// The world's objects, all of them collidable.
    pub objects: Vec<WorldObject>,
}
```

In `struct RigidBody`, replace:

```rust
pub struct RigidBody {
    p: AirframeParams,
    s: BodyState,
```

with:

```rust
pub struct RigidBody {
    p: AirframeParams,
    /// Each object's bounding box, computed once at build (the contact broad phase).
    bounds: Vec<Aabb>,
    s: BodyState,
```

In `RigidBody::new`, replace:

```rust
        let n = p.mounts.len();
        let body = Self {
```

with:

```rust
        let n = p.mounts.len();
        let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect();
        let body = Self {
```

and in the same struct literal, replace:

```rust
            p,
            s: initial,
        };
```

with:

```rust
            bounds,
            p,
            s: initial,
        };
```

Replace the whole `contact` method with:

```rust
    /// Total contact force and torque about the centre of mass, both in NED. The spring-damper and friction of
    /// `[ground]` act on every landing contact point against the ground plane and against every world object;
    /// against an object the depth is minus the signed distance and the normal is the shape's gradient there.
    fn contact(&self, s: &BodyState) -> (DVec3, DVec3) {
        let g = &self.p.ground;
        let mut force = DVec3::ZERO;
        let mut torque = DVec3::ZERO;
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            let depth = s.pos_ned_m.z + lever.z; // ground plane is z = 0, NED z points down
            if depth > 0.0 {
                let normal = (g.stiffness_npm * depth + g.damping_nspm * v.z).max(0.0);
                let v_t = DVec3::new(v.x, v.y, 0.0);
                let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
                let f = DVec3::new(0.0, 0.0, -normal) + friction;
                force += f;
                torque += lever.cross(f);
            }
            let point = s.pos_ned_m + lever;
            for (i, object) in self.p.objects.iter().enumerate() {
                if !self.bounds[i].contains(point) {
                    continue;
                }
                let sd = object.shape.signed_distance(point);
                if sd >= 0.0 {
                    continue;
                }
                let depth = -sd;
                let n = object.shape.normal(point);
                let v_n = v.dot(n);
                let mag = (g.stiffness_npm * depth - g.damping_nspm * v_n).max(0.0);
                let v_t = v - n * v_n;
                let friction = -v_t * (g.friction_coeff * mag / v_t.length().max(0.05));
                let f = n * mag + friction;
                force += f;
                torque += lever.cross(f);
            }
        }
        (force, torque)
    }
```

- [ ] **Step 4: Keep the vehicle compiling**

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
    };
```

with:

```rust
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
        objects: Vec::new(), // the world's objects are wired in with the collision milestone
    };
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p ofs-physics --locked && cargo test -p ofs-sim --locked`
Expected: PASS — the new contact tests pass, every existing physics and sim test is unchanged (empty objects list).

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-physics crates/ofs-sim/src/vehicle.rs
git commit -m "feat(physics): the landing contact points act against the world's objects (spring-damper, as against the ground)

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 4: collision spheres — impulse contact, position correction, friction, events

At the end of each physics step, every collision sphere that overlaps a world object or the ground raises an impulse contact: position correction along the deepest contact's normal, a normal impulse scaled by `restitution` (0 below 0.2 m/s inward), a Coulomb friction impulse capped at `friction_coeff` times the normal impulse, resolved deepest first in a fixed order. A sphere or contact point that starts touching at 1 m/s or more inward raises a collision event (one per touching spell, rearmed 20 ms after leaving contact).

**Files:**
- Create: `crates/ofs-physics/src/collision.rs`, `crates/ofs-physics/tests/collision.rs`
- Modify: `crates/ofs-physics/src/lib.rs`, `crates/ofs-physics/src/rigid_body.rs`, `crates/ofs-core/src/names.rs`, `crates/ofs-sim/src/vehicle.rs` (one field)

**Interfaces:**
- Consumes: `ofs_core::shape::{Aabb, Shape}`, Task 3's `WorldObject`/`AirframeParams.objects`.
- Produces (used by Tasks 6 and 7):
  - `ofs_physics::collision::CollisionParams { pub restitution: f64, pub friction_coeff: f64, pub spheres: Vec<(DVec3, f64)> }` (body-frame centre and radius), with `Default` (0.3, 0.5, empty).
  - `ofs_physics::collision::{RESTITUTION_SPEED_FLOOR_MPS (0.2), EVENT_MIN_SPEED_MPS (1.0), EVENT_REARM_S (0.020), GROUND_OBJECT_INDEX (-1)}`.
  - `ofs_physics::collision::TouchState::new(objects: usize) -> TouchState`
  - `ofs_physics::collision::resolve(p: &AirframeParams, bounds: &[Aabb], s: &mut BodyState, touch: &mut TouchState, time_s: f64) -> Vec<(i32, f64)>` — applies the contacts to `s` and returns the (object index, inward speed) of the events raised this step (−1 = ground).
  - `AirframeParams` gains `pub collision: CollisionParams`.
  - Bus signals `body.collision_speed` (inward speed of the last event, 0 before one), `body.collision_object` (its object index, −1 for the ground), `body.collision_count` (events raised so far; the edge detector).

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-physics/tests/collision.rs`:

```rust
use glam::{DQuat, DVec3};
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
use ofs_physics::collision::{CollisionParams, TouchState, GROUND_OBJECT_INDEX};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody, WorldObject};

/// A post 12 cm across, 3 m tall, standing at `north`, `east`.
fn post(north: f64, east: f64) -> Vec<WorldObject> {
    vec![WorldObject {
        name: "post".into(),
        shape: ofs_core::shape::Shape::Cylinder { center: DVec3::new(north, east, -1.5), radius: 0.06, half_height: 1.5 },
    }]
}

/// A roof to land on: a box whose top face is the plane z = -1.
fn roof() -> Vec<WorldObject> {
    vec![WorldObject {
        name: "roof".into(),
        shape: ofs_core::shape::Shape::Box { center: DVec3::new(0.0, 0.0, -1.5), half: DVec3::new(5.0, 5.0, 0.5) },
    }]
}

fn params(objects: Vec<WorldObject>, spheres: Vec<(DVec3, f64)>, restitution: f64) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts: vec![],
        contact_points_frd_m: vec![],
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects,
        collision: CollisionParams { restitution, friction_coeff: 0.5, spheres },
    }
}

fn state(pos: DVec3, vel: DVec3) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: vel, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO }
}

fn sim(p: AirframeParams, s: BodyState) -> Scheduler {
    let mut bus = Bus::new();
    let body = RigidBody::new(p, s, &mut bus);
    let mut sch = Scheduler::new(8000, bus);
    sch.add(Box::new(body));
    sch
}

fn vec3(s: &Scheduler, name: &str) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(name).unwrap())
}

fn scalar(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_vel(s: &mut Scheduler, vel: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_VEL_NED).unwrap();
    s.bus_mut().set(sig, vel);
}

/// The one sphere of these rigs sits at the centre of mass.
const BODY_SPHERE: DVec3 = DVec3::ZERO;
const R: f64 = 0.04;

#[test]
fn a_dropped_sphere_bounces_to_restitution_squared_of_its_height() {
    let mut s = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.5), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    // The fall takes about 0.45 s and the rise about 0.22 s; track the highest point (the lowest z).
    let mut apex = f64::MAX;
    for _ in 0..9600 {
        s.step().unwrap();
        apex = apex.min(vec3(&s, names::BODY_POS_NED).z);
    }
    // The CoM fell 1 m, bounces at e = 0.5, so the first apex is e^2 * 1 m (a sphere, so nothing rotates).
    assert!((apex + 0.25).abs() < 0.0125, "apex {apex}, expected -0.25 +- 5%");
}

#[test]
fn a_quad_at_30_mps_into_a_post_never_ends_up_past_it() {
    let mut s = sim(params(post(10.0, 0.0), vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(9.0, 0.0, -1.5), DVec3::new(30.0, 0.0, 0.0)));
    s.run_for(0.5).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    assert!(pos.x < 10.0, "the body stopped before the post's centre: {pos}");
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), 0.0, "the event names the post");
    assert!(scalar(&s, names::BODY_COLLISION_SPEED) > 25.0, "at impact speed {}", scalar(&s, names::BODY_COLLISION_SPEED));
}

#[test]
fn a_quad_rests_on_a_roof_with_spheres_and_legs_for_ten_seconds() {
    let mut p = params(roof(), vec![(BODY_SPHERE, R)], 0.3);
    p.contact_points_frd_m = vec![
        DVec3::new(-0.08, 0.08, 0.03),
        DVec3::new(0.08, 0.08, 0.03),
        DVec3::new(-0.08, -0.08, 0.03),
        DVec3::new(0.08, -0.08, 0.03),
    ];
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -1.05), DVec3::ZERO));
    let mut samples = Vec::new();
    for _ in 0..20 {
        s.run_for(0.5).unwrap();
        samples.push(vec3(&s, names::BODY_POS_NED));
    }
    let last = &samples[10..];
    for axis in 0..3 {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for p in last {
            min = min.min(p[axis]);
            max = max.max(p[axis]);
        }
        assert!(max - min < 1e-3, "axis {axis} jitters {} mm over the last 5 s", (max - min) * 1000.0);
    }
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.length() < 1e-2, "at rest: {vel}");
    // Above the roof's top face, never inside it.
    let pos = samples[samples.len() - 1];
    assert!(pos.z < -1.0 && pos.z > -1.2, "resting on the roof: {pos}");
}

#[test]
fn a_prop_sphere_clipping_a_post_starts_a_rotation() {
    // A sphere out on the left front arm, a post 5 cm off its path: an off-centre hit.
    let spheres = vec![(DVec3::new(0.08, -0.08, 0.0), 0.015), (DVec3::new(-0.08, 0.08, 0.0), 0.015), (BODY_SPHERE, R)];
    let mut p = params(post(0.3, -0.03), spheres, 0.3);
    p.contact_points_frd_m = vec![DVec3::new(0.0, 0.0, 0.03)];
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -1.5), DVec3::new(10.0, 0.0, 0.0)));
    let mut worst = 0.0;
    for _ in 0..4000 {
        s.step().unwrap();
        worst = worst.max(vec3(&s, names::BODY_RATE_FRD).length());
    }
    assert!(worst > 2.0, "the clip spun the quad: worst rate {worst} rad/s");
}

#[test]
fn friction_stops_a_sliding_sphere() {
    let mut s = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.0), state(DVec3::new(0.0, 0.0, -R), DVec3::new(1.0, 0.0, 0.0)));
    s.run_for(0.5).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.x.abs() < 0.01, "sliding stopped: vel {vel}");
    assert!(vec3(&s, names::BODY_POS_NED).x < 0.15, "it slid less than mu*g would allow");
}

#[test]
fn a_bounce_raises_one_event_per_touching_spell() {
    // e = 0.3 from 1 m: impacts at 4.4 and 1.3 m/s (both >= 1), the third at 0.4 m/s is silent.
    let mut s = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    s.run_for(3.0).unwrap();
    assert_eq!(scalar(&s, names::BODY_COLLISION_COUNT), 2.0, "two hard touches");
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), f64::from(GROUND_OBJECT_INDEX));
}

#[test]
fn a_new_touch_within_the_rearm_window_raises_nothing() {
    let p = params(vec![], vec![(BODY_SPHERE, R)], 0.0);
    let bounds = Vec::new();
    let mut touch = TouchState::new(0);
    // Touch hard, leave, touch hard again 10 ms later: still one event. After 20 ms free: the next touch raises.
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.0).len(), 1);
    let mut s = state(DVec3::new(0.0, 0.0, -R - 0.1), DVec3::ZERO);
    assert!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.001).is_empty(), "in the air: free");
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.011).len(), 0, "10 ms after leaving: rearmed not yet");
    let mut s = state(DVec3::new(0.0, 0.0, -R - 0.1), DVec3::ZERO);
    assert!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.012).is_empty());
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.05).len(), 1, "38 ms after leaving: rearmed");
}

#[test]
fn contacts_with_no_objects_leave_the_body_alone() {
    // A body far from any object behaves exactly as with an empty world.
    let far = vec![WorldObject {
        name: "far".into(),
        shape: ofs_core::shape::Shape::Box { center: DVec3::new(1000.0, 0.0, -5.0), half: DVec3::new(5.0, 5.0, 5.0) },
    }];
    let mut with = sim(params(far, vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    let mut without = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    with.run_for(1.0).unwrap();
    without.run_for(1.0).unwrap();
    assert_eq!(with.bus().digest(), without.bus().digest(), "the broad phase skipped the far object entirely");
}

#[test]
fn the_same_impacts_give_bit_identical_states() {
    let scripted = || {
        let mut s = sim(params(post(10.0, 0.0), vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(9.0, 0.0, -1.5), DVec3::new(30.0, 0.0, 0.0)));
        s.run_for(0.2).unwrap();
        s.bus().digest()
    };
    assert_eq!(scripted(), scripted());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-physics --test collision --locked`
Expected: FAIL to compile — `ofs_physics::collision` does not exist.

- [ ] **Step 3: Implement the impulse resolver**

Create `crates/ofs-physics/src/collision.rs`:

```rust
//! Impulse contact of the collision spheres against the world's objects and the ground plane, applied after
//! integration: the body is moved out along the deepest contact's normal, then normal and friction impulses are
//! resolved one contact at a time, deepest first, in a fixed order, so the result is deterministic. At 8 kHz the
//! quad moves a few millimetres per step at racing speed, so no sphere passes through a 12 cm post between steps.
use glam::DVec3;

use crate::rigid_body::{AirframeParams, BodyState};

/// Inward contact speeds below this reverse with restitution 0, so nothing jitters at rest.
pub const RESTITUTION_SPEED_FLOOR_MPS: f64 = 0.2;
/// A contact that starts touching at least this fast inward raises a collision event.
pub const EVENT_MIN_SPEED_MPS: f64 = 1.0;
/// A touching object raises no further event until it has been out of contact for this long.
pub const EVENT_REARM_S: f64 = 0.020;
/// The ground plane's index in a collision event (objects count from 0).
pub const GROUND_OBJECT_INDEX: i32 = -1;

/// The collision spheres of `[collision]`, in the body frame.
#[derive(Debug, Clone)]
pub struct CollisionParams {
    pub restitution: f64,
    pub friction_coeff: f64,
    /// Each sphere's centre (FRD) and radius.
    pub spheres: Vec<(DVec3, f64)>,
}

impl Default for CollisionParams {
    fn default() -> Self {
        Self { restitution: 0.3, friction_coeff: 0.5, spheres: Vec::new() }
    }
}

/// Per-object contact memory for the events: who is touching a sphere, and who went free when.
#[derive(Debug, Default)]
pub struct TouchState {
    touching: Vec<bool>,
    free_since: Vec<f64>,
}

impl TouchState {
    /// One slot per world object, then the ground.
    pub fn new(objects: usize) -> Self {
        Self { touching: vec![false; objects + 1], free_since: vec![-2.0 * EVENT_REARM_S; objects + 1] }
    }
}

/// One sphere's contact with one solid, in world coordinates.
struct Contact {
    /// Out of the solid, unit.
    normal: DVec3,
    /// The sphere's surface point along the normal (its deepest point into the solid).
    point: DVec3,
    penetration: f64,
    /// `GROUND_OBJECT_INDEX` for the ground, else the object's index.
    object: i32,
    /// The inward speed of the contact point when the contact was collected (for the events).
    inward: f64,
}

/// Applies the sphere contacts of one step to `s` and returns the (object index, inward speed) pairs of the
/// collision events raised this step, hardest last in the vector.
pub fn resolve(p: &AirframeParams, bounds: &[Aabb], s: &mut BodyState, touch: &mut TouchState, time_s: f64) -> Vec<(i32, f64)> {
    let mut contacts = collect(p, bounds, s);
    if contacts.is_empty() {
        release_everything(touch, time_s);
        return Vec::new();
    }
    let events = touch_events(&contacts, touch, time_s);
    // One order for everything: deepest first, ties by object index (the ground, -1, sorts first), then the
    // order the spheres were collected in (the sort is stable), so the result is deterministic.
    contacts.sort_by(|a, b| {
        b.penetration
            .total_cmp(&a.penetration)
            .then(a.object.cmp(&b.object))
    });
    // Position correction: the body leaves the deepest contact along its normal, by its penetration.
    let deepest = contacts.first().expect("checked non-empty");
    s.pos_ned_m += deepest.normal * deepest.penetration;
    let inv_mass = 1.0 / p.mass_kg;
    let inv_inertia = DVec3::new(1.0 / p.inertia_kgm2.x, 1.0 / p.inertia_kgm2.y, 1.0 / p.inertia_kgm2.z);
    for c in &contacts {
        let lever = c.point - s.pos_ned_m;
        let mut j = 0.0;
        let v_point = s.vel_ned_mps + (s.att * s.rate_frd_radps).cross(lever);
        let v_n = v_point.dot(c.normal);
        if v_n < 0.0 {
            let e = if -v_n < RESTITUTION_SPEED_FLOOR_MPS { 0.0 } else { p.collision.restitution };
            let rn = lever.cross(c.normal);
            // Effective mass along the normal: the impulse also turns the body through the inertia tensor.
            let kn = inv_mass + c.normal.dot((s.att * (inv_inertia * (s.att.inverse() * rn))).cross(lever));
            if kn > 0.0 {
                j = -(1.0 + e) * v_n / kn;
                apply(s, inv_mass, inv_inertia, lever, c.normal * j);
            }
        }
        // Coulomb friction, at most mu times the normal impulse, against the tangential velocity.
        let v_point = s.vel_ned_mps + (s.att * s.rate_frd_radps).cross(lever);
        let v_t = v_point - c.normal * v_point.dot(c.normal);
        let speed = v_t.length();
        if speed > 1e-9 && j > 0.0 {
            let t = v_t / speed;
            let rt = lever.cross(t);
            let kt = inv_mass + t.dot((s.att * (inv_inertia * (s.att.inverse() * rt))).cross(lever));
            if kt > 0.0 {
                let jt = (speed / kt).min(p.collision.friction_coeff * j);
                apply(s, inv_mass, inv_inertia, lever, -t * jt);
            }
        }
    }
    events
}

/// An impulse at a world point: linear velocity along the impulse, angular velocity through the inertia tensor.
fn apply(s: &mut BodyState, inv_mass: f64, inv_inertia: DVec3, lever: DVec3, impulse: DVec3) {
    s.vel_ned_mps += impulse * inv_mass;
    s.rate_frd_radps += s.att.inverse() * (inv_inertia * lever.cross(impulse));
}

/// Every sphere's contacts with every near object and with the ground.
fn collect(p: &AirframeParams, bounds: &[Aabb], s: &BodyState) -> Vec<Contact> {
    let mut contacts = Vec::new();
    let omega = s.att * s.rate_frd_radps;
    let quad_radius = p.collision.spheres.iter().map(|(c, r)| c.length() + r).fold(0.0, f64::max);
    for (centre_frd, radius) in &p.collision.spheres {
        let centre = s.pos_ned_m + s.att * *centre_frd;
        // The ground plane is the solid z >= 0: its signed distance is -z, its normal is up.
        let penetration = radius + centre.z;
        if penetration > 0.0 {
            let normal = DVec3::NEG_Z;
            let point = centre - normal * *radius;
            let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
            contacts.push(Contact { normal, point, penetration, object: GROUND_OBJECT_INDEX, inward });
        }
        for (i, object) in p.objects.iter().enumerate() {
            // Broad phase: the quad's centre inside the object's box, grown by the quad's bounding radius.
            if !bounds[i].grown(quad_radius).contains(s.pos_ned_m) {
                continue;
            }
            let sd = object.shape.signed_distance(centre);
            let penetration = radius - sd;
            if penetration <= 0.0 {
                continue;
            }
            let normal = object.shape.normal(centre);
            let point = centre - normal * *radius;
            let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
            contacts.push(Contact { normal, point, penetration, object: i as i32, inward });
        }
    }
    contacts
}

/// The events of the contacts against the per-object touch memory: one per touching spell that starts hard
/// enough, and only once the object has been out of contact for `EVENT_REARM_S`.
fn touch_events(contacts: &[Contact], touch: &mut TouchState, time_s: f64) -> Vec<(i32, f64)> {
    let mut events = Vec::new();
    let n = touch.touching.len();
    for idx in 0..n {
        let object = if idx + 1 == n { GROUND_OBJECT_INDEX } else { idx as i32 };
        let touching_now = contacts.iter().any(|c| c.object == object);
        if touching_now && !touch.touching[idx] {
            let inward = contacts.iter().filter(|c| c.object == object).map(|c| c.inward).fold(0.0, f64::max);
            if inward >= EVENT_MIN_SPEED_MPS && time_s - touch.free_since[idx] >= EVENT_REARM_S {
                events.push((object, inward));
            }
        }
        if !touching_now && touch.touching[idx] {
            touch.free_since[idx] = time_s;
        }
        touch.touching[idx] = touching_now;
    }
    events
}

fn release_everything(touch: &mut TouchState, time_s: f64) {
    for idx in 0..touch.touching.len() {
        if touch.touching[idx] {
            touch.free_since[idx] = time_s;
            touch.touching[idx] = false;
        }
    }
}
```

with this exact import block at the top:

```rust
use glam::DVec3;
use ofs_core::shape::Aabb;

use crate::rigid_body::{AirframeParams, BodyState};
```

In `crates/ofs-physics/src/lib.rs`, replace:

```rust
//! Flight physics: 6-DOF rigid body, propellers.
pub mod propeller;
pub mod rigid_body;
```

with:

```rust
//! Flight physics: 6-DOF rigid body, propellers, impulse collision against the world's objects.
pub mod collision;
pub mod propeller;
pub mod rigid_body;
```

- [ ] **Step 4: Wire the spheres, the events and the signals into the body**

All edits in this step are in `crates/ofs-physics/src/rigid_body.rs`. Replace:

```rust
use ofs_core::shape::{Aabb, Shape};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
```

with:

```rust
use crate::collision::{self, CollisionParams, TouchState};
use ofs_core::shape::{Aabb, Shape};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
```

Replace:

```rust
    /// The world's objects, all of them collidable.
    pub objects: Vec<WorldObject>,
}
```

with:

```rust
    /// The world's objects, all of them collidable.
    pub objects: Vec<WorldObject>,
    /// The collision spheres and their contact behaviour.
    pub collision: CollisionParams,
}
```

Replace:

```rust
    /// Each object's bounding box, computed once at build (the contact broad phase).
    bounds: Vec<Aabb>,
    s: BodyState,
```

with:

```rust
    /// Each object's bounding box, computed once at build (the contact broad phase).
    bounds: Vec<Aabb>,
    /// The quad's bounding radius (the largest sphere centre plus radius): the broad phase's margin.
    bounding_radius_m: f64,
    touch: TouchState,
    last_collision: Option<(i32, f64)>,
    collision_count: u64,
    s: BodyState,
```

Replace:

```rust
    thrust: Vec<Signal<f64>>,
    torque: Vec<Signal<f64>>,
    omega_dot: Vec<Signal<f64>>,
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    accel: Signal<DVec3>,
}
```

with:

```rust
    thrust: Vec<Signal<f64>>,
    torque: Vec<Signal<f64>>,
    omega_dot: Vec<Signal<f64>>,
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    accel: Signal<DVec3>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
    collision_count: Signal<f64>,
}
```

Replace:

```rust
        let n = p.mounts.len();
        let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect();
        let body = Self {
```

with:

```rust
        let n = p.mounts.len();
        let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect();
        let bounding_radius_m = p.collision.spheres.iter().map(|(c, r)| c.length() + r).fold(0.0, f64::max);
        let touch = TouchState::new(p.objects.len());
        let body = Self {
```

Replace:

```rust
            accel: bus.signal(names::BODY_ACCEL_NED),
            p,
            s: initial,
        };
```

with:

```rust
            accel: bus.signal(names::BODY_ACCEL_NED),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
            collision_count: bus.signal(names::BODY_COLLISION_COUNT),
            bounds,
            bounding_radius_m,
            touch,
            last_collision: None,
            collision_count: 0,
            p,
            s: initial,
        };
```

Replace:

```rust
    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
    }
```

with:

```rust
    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
        bus.set(self.collision_speed, self.last_collision.map_or(0.0, |(_, speed)| speed));
        bus.set(
            self.collision_object,
            self.last_collision.map_or(f64::from(collision::GROUND_OBJECT_INDEX), |(object, _)| f64::from(object)),
        );
        bus.set(self.collision_count, self.collision_count as f64);
    }
```

Replace:

```rust
        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        self.publish(bus, accel_ned);
        Ok(())
```

with:

```rust
        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        // The contacts apply after integration, before publishing, so a contact this tick is visible this tick;
        // the scheduler's per-step non-finite check catches any non-finite value afterwards.
        let events = collision::resolve(&self.p, &self.bounds, &mut self.s, &mut self.touch, ctx.time_s);
        if let Some((object, speed)) = events.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.last_collision = Some((*object, *speed));
            self.collision_count += 1;
        }
        self.publish(bus, accel_ned);
        Ok(())
```

In `crates/ofs-core/src/names.rs`, replace:

```rust
/// Kinematic acceleration (not specific force), NED.
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";
```

with:

```rust
/// Kinematic acceleration (not specific force), NED.
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";
/// Inward speed of the last collision event, m/s (0 before the first one).
pub const BODY_COLLISION_SPEED: &str = "body.collision_speed";
/// Object index of the last collision event; -1 is the ground plane.
pub const BODY_COLLISION_OBJECT: &str = "body.collision_object";
/// Collision events raised so far; the server detects the event's edge on this counter.
pub const BODY_COLLISION_COUNT: &str = "body.collision_count";
```

In `publish`, replace:

```rust
    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
    }
```

with:

```rust
    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
        bus.set(self.collision_speed, self.last_collision.map_or(0.0, |(_, speed)| speed));
        bus.set(self.collision_object, self.last_collision.map_or(f64::from(collision::GROUND_OBJECT_INDEX), |(object, _)| f64::from(object)));
        bus.set(self.collision_count, self.collision_count as f64);
    }
```

In `Model for RigidBody::step`, replace:

```rust
        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        self.publish(bus, accel_ned);
        Ok(())
```

with:

```rust
        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        let events = collision::resolve(&self.p, &self.bounds, &mut self.s, &mut self.touch, ctx.time_s);
        if let Some((object, speed)) = events.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.last_collision = Some((*object, *speed));
            self.collision_count += 1;
        }
        self.publish(bus, accel_ned);
        Ok(())
```

(The resolve runs after integration and before publishing, so a contact this tick is visible this tick. The scheduler's existing per-step non-finite check catches any non-finite signal afterwards, as the spec requires.)

In `crates/ofs-core/src/names.rs`, after the `BODY_ACCEL_NED` line, add:

```rust
/// Inward speed of the last collision event, m/s (0 before the first one).
pub const BODY_COLLISION_SPEED: &str = "body.collision_speed";
/// Object index of the last collision event; -1 is the ground plane.
pub const BODY_COLLISION_OBJECT: &str = "body.collision_object";
/// Collision events raised so far; the server detects the event's edge on this counter.
pub const BODY_COLLISION_COUNT: &str = "body.collision_count";
```

- [ ] **Step 5: Keep the vehicle compiling**

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        objects: Vec::new(), // the world's objects are wired in with the collision milestone
    };
```

with:

```rust
        objects: Vec::new(), // the world's objects are wired in with the collision milestone
        collision: Default::default(),
    };
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ofs-physics -p ofs-core -p ofs-sim --locked`
Expected: PASS — the resolver's analytic tests (bounce height, the 30 m/s post, the 10 s rests, the clip spin, friction, the event rearm, the broad phase, determinism) pass; existing tests unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-physics crates/ofs-core/src/names.rs crates/ofs-sim/src/vehicle.rs
git commit -m "feat(physics): collision spheres resolve impulse contacts against objects and the ground, raising collision events

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 5: quad schema 4 (`[radio]` reborn, `[collision]`), world `[handset]`

The schema moves 3 → 4: `radio.rssi_dbm`, `radio.snr_db` and the four loss parameters are removed; `radio.tx_power_mw` and the `[[radio.antennas]]` list replace them; the optional `[collision]` section arrives with generated default spheres. The world file (schema stays 1) gains an optional `[handset]`. Schema-3 files are refused at the version check with a message that names what changed.

**Files:**
- Modify: `crates/ofs-config/src/lib.rs`, `crates/ofs-config/src/world.rs`, `crates/ofs-config/tests/config.rs`, `crates/ofs-config/tests/world.rs`, `quads/opendrone-5f-freestyle.toml`

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Tasks 6 and 7):
  - `ofs_config::SCHEMA_VERSION == 4`, `ofs_config::MIN_SCHEMA_VERSION == 4`.
  - `RadioSection { kind, packet_rate_hz, uart, latency_packets, tx_power_mw: u32, link_stats_interval_packets, antennas: Vec<RadioAntennaSection> }`; `RadioAntennaSection { name: String, kind: AntennaKind, gain_dbi: f64, polarization: Polarization, mount_frd: [f64; 3] }` (serde defaults: omni, 2 dBi, linear, `[-0.5, 0.0, -1.0]`; `Default` has name `"antenna"`).
  - `QuadConfig.collision: Option<CollisionSection>`; `CollisionSection { restitution: f64, friction_coeff: f64, spheres_frd_m: Option<Vec<[f64; 4]>> }` (serde defaults 0.3 / 0.5).
  - `QuadConfig::collision_spheres(&self) -> Vec<[f64; 4]>` (the file's list or the generated default) and `QuadConfig::collision_contact(&self) -> (f64, f64)`.
  - `ofs_config::world::HandsetSection { position_ned_m: Option<[f64; 3]>, antennas: Vec<AntennaSection> }` with `Default` (one 2 dBi **linear** omni named `"handset"`, aims up) and `pub fn position(&self, pilot: &PilotSection) -> [f64; 3]` (0.5 m below the goggles when unset); `WorldConfig.handset: HandsetSection`.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-config/tests/config.rs`, add (with the file's existing imports; add `use std::path::Path;` if absent):

```rust
const SHIPPED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

/// The shipped quad with `edit` applied, loaded from a temporary file.
fn shipped_with(edit: impl FnOnce(&mut String)) -> Result<ofs_config::QuadConfig, ofs_config::ConfigError> {
    let mut text = std::fs::read_to_string(Path::new(SHIPPED)).unwrap();
    edit(&mut text);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quad.toml");
    std::fs::write(&path, &text).unwrap();
    ofs_config::load(&path)
}

#[test]
fn a_schema_3_quad_file_is_refused_with_the_field_changes() {
    let err = shipped_with(|t| *t = t.replace("schema_version = 4", "schema_version = 3")).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("schema_version 3"), "{message}");
    assert!(message.contains("radio.rssi_dbm"), "{message}");
    assert!(message.contains("radio.snr_db"), "{message}");
    assert!(message.contains("radio.loss_good"), "{message}");
    assert!(message.contains("radio.tx_power_mw"), "{message}");
    assert!(message.contains("radio.antennas"), "{message}");
    assert!(message.contains("[collision]"), "{message}");
}

#[test]
fn the_old_radio_fields_are_unknown_in_schema_4() {
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 250\nrssi_dbm = -50.0")).unwrap_err();
    assert!(err.to_string().contains("unknown field `rssi_dbm`"), "{}", err);
}

#[test]
fn packet_rate_must_be_an_elrs_lora_mode() {
    for rate in [10, 75, 100, 1000] {
        let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", &format!("packet_rate_hz = {rate}"))).unwrap_err();
        assert!(err.to_string().contains("radio.packet_rate_hz"), "{rate}: {err}");
    }
    assert!(shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "packet_rate_hz = 250")).is_ok());
}

#[test]
fn tx_power_must_be_a_real_handset_power() {
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 300")).unwrap_err();
    assert!(err.to_string().contains("radio.tx_power_mw"), "{err}");
    assert!(shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 10")).is_ok());
}

#[test]
fn radio_antennas_are_validated() {
    let three = "[[radio.antennas]]\nname = \"b\"\n\n[[radio.antennas]]\nname = \"c\"\n\n[[radio.antennas]]\nname = \"d\"\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{three}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas"), "{err}");
    let patch = "[[radio.antennas]]\nname = \"patchy\"\nkind = \"patch\"\nbeamwidth_deg = 90.0\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{patch}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas[0].kind"), "{err}");
    let flat = "[[radio.antennas]]\nname = \"flat\"\nmount_frd = [0.0, 0.0, 0.0]\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{flat}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas[0].mount_frd"), "{err}");
    let twin = "[[radio.antennas]]\nname = \"a\"\n\n[[radio.antennas]]\nname = \"a\"\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{twin}"))).unwrap_err();
    assert!(err.to_string().contains("used twice"), "{err}");
    // Two named dipoles are fine: receiver diversity.
    assert!(shipped_with(|t| {
        *t = t.replace("tx_power_mw = 250", "tx_power_mw = 250\n\n[[radio.antennas]]\nname = \"left\"\n\n[[radio.antennas]]\nname = \"right\"");
    })
    .is_ok());
}

#[test]
fn the_shipped_quad_gets_25_collision_spheres_with_no_gap_for_a_post() {
    let cfg = ofs_config::load(Path::new(SHIPPED)).unwrap();
    let spheres = cfg.collision_spheres();
    assert_eq!(spheres.len(), 25, "6 spheres per prop tip circle + one body sphere");
    let (restitution, friction) = cfg.collision_contact();
    assert_eq!((restitution, friction), (0.3, 0.5), "the spec's defaults");
    let tip = cfg.prop.diameter_m / 2.0;
    for m in &cfg.frame.motor_positions_frd_m {
        let mut ring: Vec<[f64; 4]> = spheres
            .iter()
            .filter(|s| (s[2] - m[2]).abs() < 1e-9 && (s[0] - m[0]).hypot(s[1] - m[1]) <= tip + 1e-9)
            .copied()
            .collect();
        ring.sort_by(|a, b| (a[0] - m[0]).atan2(a[1] - m[1]).total_cmp(&(b[0] - m[0]).atan2(b[1] - m[1])));
        assert_eq!(ring.len(), 6, "six spheres around each motor");
        for w in ring.windows(2) {
            let d = (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]);
            assert!((d - tip).abs() < 1e-6, "adjacent spheres are {d} apart, the 60 degree chord is {tip}");
            assert!(d - w[0][3] - w[1][3] < 0.12, "no 12 cm post passes between two prop spheres");
        }
    }
    let body = spheres.last().unwrap();
    assert_eq!((body[0], body[1], body[2], body[3]), (0.0, 0.0, 0.0, 0.04), "the body sphere sits at the centre of mass");
    for s in &spheres[..spheres.len() - 1] {
        let d = ((s[0] - body[0]).powi(2) + (s[1] - body[1]).powi(2) + (s[2] - body[2]).powi(2)).sqrt();
        assert!(d - s[3] - body[3] < 0.12, "no 12 cm post passes between a prop sphere and the body sphere");
    }
}

#[test]
fn a_collision_section_is_honoured() {
    let cfg = shipped_with(|t| {
        *t = t.replace(
            "tx_power_mw = 250",
            "tx_power_mw = 250\n\n[collision]\nrestitution = 0.7\nfriction_coeff = 0.2\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.05]]",
        )
    })
    .unwrap();
    assert_eq!(cfg.collision_spheres(), vec![[0.0, 0.0, 0.0, 0.05]]);
    assert_eq!(cfg.collision_contact(), (0.7, 0.2));
}

#[test]
fn collision_validation_problems_are_collected() {
    let err = shipped_with(|t| {
        *t = t.replace(
            "tx_power_mw = 250",
            "tx_power_mw = 250\n\n[collision]\nrestitution = 1.5\nfriction_coeff = -1.0\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.0], [nan, 0.0, 0.0, 0.02]]",
        )
    })
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("collision.restitution"), "{message}");
    assert!(message.contains("collision.friction_coeff"), "{message}");
    assert!(message.contains("collision.spheres_frd_m"), "{message}");
}
```

In `crates/ofs-config/tests/world.rs`, add:

```rust
fn written_world(text: &str) -> Result<WorldConfig, ConfigError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.toml");
    std::fs::write(&path, text).unwrap();
    ofs_config::world::load(&path)
}

#[test]
fn the_default_handset_sits_half_a_metre_below_the_goggles() {
    let world = WorldConfig::open_field();
    assert_eq!(world.handset.position(&world.pilot), [0.0, 0.0, -1.2]);
    let a = &world.handset.antennas[0];
    assert_eq!((a.name.as_str(), a.kind, a.polarization, a.gain_dbi), ("handset", AntennaKind::Omni, Polarization::Linear, 2.0));
    assert_eq!(a.aim_el(), 90.0, "the default dipole stands upright");
}

#[test]
fn a_world_file_may_place_and_aim_the_handset() {
    let world = written_world(
        r#"schema_version = 1
name = "handset field"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
facing_deg = 90.0
[receiver.antennas]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[handset]
position_ned_m = [-3.0, 2.0, -1.2]
[[handset.antennas]]
name = "left"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
aim_az_deg = -30.0
"#,
    )
    .unwrap();
    assert_eq!(world.handset.position(&world.pilot), [-3.0, 2.0, -1.2]);
    assert_eq!(world.handset.antennas.len(), 1);
    assert_eq!(world.handset.antennas[0].name, "left");
}

#[test]
fn handset_validation_problems_are_collected() {
    let err = written_world(
        r#"schema_version = 1
name = "bad handset"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
[receiver.antennas]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[handset]
position_ned_m = [-3.0, 2.0, 0.5]
[[handset.antennas]]
name = "same"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
[[handset.antennas]]
name = "same"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
"#,
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("handset.position_ned_m"), "{message}");
    assert!(message.contains("used twice"), "{message}");
}
```

(Adjust the helper names if `crates/ofs-config/tests/world.rs` already defines a file-writing helper — keep one.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-config --locked`
Expected: FAIL to compile — `HandsetSection`, `CollisionSection`, `collision_spheres` do not exist; the shipped quad is still schema 3 so the schema-3 test's premise also fails.

- [ ] **Step 3: Rewrite the radio section, add `[collision]`, bump the schema**

All edits in `crates/ofs-config/src/lib.rs`. Replace:

```rust
/// 3: adds the optional `[esc_telemetry]`, `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 3;
/// Oldest schema this build still reads (schema-2 files have no video sections).
pub const MIN_SCHEMA_VERSION: u32 = 2;
```

with:

```rust
/// 4: the `[radio]` section describes the real link (power, antennas) instead of fixed RSSI and loss figures, and
/// the optional `[collision]` section arrives (M3c). 3: the optional `[esc_telemetry]`, `[osd]` and `[vtx]`
/// sections (M3a). 2: the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 4;
/// Oldest schema this build still reads: only 4 (the radio section's meaning changed, so older files cannot be
/// mapped onto it).
pub const MIN_SCHEMA_VERSION: u32 = 4;
```

In `QuadConfig`, replace:

```rust
    pub radio: RadioSection,
    #[serde(default)]
    pub esc_telemetry: Option<EscTelemetrySection>,
```

with:

```rust
    pub radio: RadioSection,
    #[serde(default)]
    pub collision: Option<CollisionSection>,
    #[serde(default)]
    pub esc_telemetry: Option<EscTelemetrySection>,
```

Replace the whole `RadioSection` struct, its field docs, and the four loss/`rssi_dbm`/`snr_db` default functions (from `/// The pilot's radio link...` through `fn default_snr_db() -> f64 { ... }` inclusive) with:

```rust
/// The pilot's radio link. The receiver's CRSF output is wired to a Betaflight UART.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioSection {
    pub kind: RadioKind,
    /// The ExpressLRS 2.4 GHz LoRa modes: 50, 150, 250 or 500 packets per second.
    pub packet_rate_hz: u32,
    /// Betaflight UART number the receiver is wired to (1-based, as in the Configurator's Ports tab).
    pub uart: u8,
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
```

(`default_p_bad_to_good` goes with the removed loss fields — delete it too.)

After the `VtxAntennaSection`'s `default_pit_power_mw` function, add:

```rust
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
pub const PROP_SPHERE_RADIUS_M: f64 = 0.015;
/// The radius of the generated body sphere at the centre of mass.
pub const BODY_SPHERE_RADIUS_M: f64 = 0.04;
/// Generated spheres per prop tip circle.
pub const SPHERES_PER_PROP: usize = 6;
```

In `impl QuadConfig`, replace:

```rust
    /// Resolves a path written in the quad file relative to the quad file's directory.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        self.source_dir().join(rel)
    }
```

with:

```rust
    /// Resolves a path written in the quad file relative to the quad file's directory.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        self.source_dir().join(rel)
    }

    /// The collision spheres: the file's list, or the generated default — 6 spheres of radius 1.5 cm evenly
    /// spaced on each prop's tip circle (in the motor's plane), plus one body sphere of radius 4 cm at the
    /// centre of mass.
    pub fn collision_spheres(&self) -> Vec<[f64; 4]> {
        if let Some(spheres) = self.collision.as_ref().and_then(|c| c.spheres_frd_m.clone()) {
            return spheres;
        }
        let tip = self.prop.diameter_m / 2.0;
        let mut spheres = Vec::with_capacity(self.frame.motor_positions_frd_m.len() * SPHERES_PER_PROP + 1);
        for m in &self.frame.motor_positions_frd_m {
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
```

In `load`, replace:

```rust
        Some(v @ ..=1) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}); schema 2 adds the required [radio] section \
                 and the CRSF receiver lines in the quad's betaflight.diff, see quads/opendrone-5f-freestyle.toml and \
                 quads/opendrone-5f-freestyle.betaflight.diff"
            );
            return Err(parse_err(message));
        }
```

with:

```rust
        Some(v @ ..=3) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads schema {SCHEMA_VERSION}); schema 4 removes radio.rssi_dbm, radio.snr_db, \
                 radio.loss_good, radio.loss_bad, radio.p_good_to_bad and radio.p_bad_to_good, and adds radio.tx_power_mw, the \
                 [[radio.antennas]] list and the optional [collision] section, see quads/opendrone-5f-freestyle.toml"
            );
            return Err(parse_err(message));
        }
```

In `validate`, replace:

```rust
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
```

with:

```rust
        let r = &self.radio;
        c.check(
            matches!(r.packet_rate_hz, 50 | 150 | 250 | 500),
            "radio.packet_rate_hz",
            format!("must be one of the ExpressLRS 2.4 GHz LoRa modes: 50, 150, 250 or 500 (got {})", r.packet_rate_hz),
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
            c.check(r.antennas.iter().take(i).all(|other| other.name != a.name), &field("name"), "names must be unique");
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
```

- [ ] **Step 4: The world's `[handset]`**

All edits in `crates/ofs-config/src/world.rs`. After the `ReceiverSection` struct and its `default_noise_floor_dbm`/`default_true` functions, add:

```rust
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
```

In `WorldConfig`, replace:

```rust
    pub pilot: PilotSection,
    pub receiver: ReceiverSection,
```

with:

```rust
    pub pilot: PilotSection,
    pub receiver: ReceiverSection,
    /// The pilot's handset; the ELRS uplink transmits from here.
    #[serde(default)]
    pub handset: HandsetSection,
```

In `open_field()`, replace:

```rust
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
```

with:

```rust
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
```

In `validate`, replace:

```rust
        let mut names = HashSet::new();
        for (i, o) in self.objects.iter().enumerate() {
```

with:

```rust
        let h = &self.handset;
        let handset_at = h.position(&p);
        c.check(handset_at.iter().all(|v| v.is_finite()), "handset.position_ned_m", "values must be finite");
        c.check(
            handset_at[2] <= 0.0,
            "handset.position_ned_m",
            format!("the handset must not be below the ground (d = {} > 0)", handset_at[2]),
        );
        let mut handset_names = HashSet::new();
        for (i, a) in h.antennas.iter().enumerate() {
            let field = |f: &str| format!("handset.antennas[{i}].{f}");
            check_name(&mut c, &a.name, &field("name"), &mut handset_names);
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(finite(&[a.aim_az_deg, a.aim_el()]), &field("aim_az_deg"), "aims must be finite");
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
        }
        let mut names = HashSet::new();
        for (i, o) in self.objects.iter().enumerate() {
```

- [ ] **Step 5: The shipped quad moves to schema 4**

In `quads/opendrone-5f-freestyle.toml`, replace:

```toml
schema_version = 3
```

with:

```toml
schema_version = 4
```

and replace:

```toml
[radio]
# ExpressLRS, behavioural (level 1). The receiver's CRSF output is wired to Betaflight's UART2; the
# betaflight.diff selects the CRSF receiver there.
kind = "elrs"
packet_rate_hz = 500
uart = 2
latency_packets = 1
rssi_dbm = -50.0
snr_db = 10.0
```

with:

```toml
[radio]
# ExpressLRS 2.4 GHz. The receiver's CRSF output is wired to Betaflight's UART2; the betaflight.diff selects
# the CRSF receiver there. RSSI, SNR and packet loss come from the geometry of the world file (M3c); the
# default receiver antenna is a 2 dBi linear dipole leaning up and back (mount_frd = [-0.5, 0.0, -1.0]).
kind = "elrs"
packet_rate_hz = 500
uart = 2
latency_packets = 1
tx_power_mw = 250
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p ofs-config --locked && cargo test --workspace --locked`
Expected: PASS for `ofs-config`; the workspace run fails only where other crates still read the removed `RadioSection` fields (`crates/ofs-sim/src/vehicle.rs`, `crates/ofs-radio/tests/elrs.rs`) — that is Task 6's first step. Record the failing compile as the expected baseline.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-config quads/opendrone-5f-freestyle.toml
git commit -m "feat(config)!: quad schema 4 - the radio section describes the real link, [collision] arrives; the world file gains [handset]

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 6: the ELRS link on `ofs-rf`, wired to the world

`ofs-radio::elrs` is rewritten: per packet it computes each quad antenna's uplink RSSI from the geometry at 2440 MHz (antenna patterns, range, obstruction, polarization, ground bounce, fading, body shadow), picks the best antenna with 2 dB hysteresis, and loses packets by a logistic curve around the mode's sensitivity. The downlink runs the reversed path at 100 mW with its own fading and window. The vehicle wires in the world's handset, obstacles, collidable objects and the quad's `[collision]` spheres.

**Files:**
- Modify: `crates/ofs-radio/Cargo.toml`, `crates/ofs-radio/src/elrs.rs` (rewrite), `crates/ofs-radio/tests/elrs.rs` (rewrite), `crates/ofs-core/src/names.rs`, `crates/ofs-sim/src/vehicle.rs`

**Interfaces:**
- Consumes: `ofs_rf::{propagation, fading}` (Task 2); Task 5's `RadioSection`/`RadioAntennaSection`/`collision_spheres()`/`collision_contact()`; Task 5's `WorldConfig.handset`; Tasks 3/4's `WorldObject`/`CollisionParams`.
- Produces (used by Tasks 7, 8, 9):
  - `ofs_radio::elrs::{sensitivity_dbm(u32) -> Option<f64>, rf_mode_index(u32) -> Option<u8>, tx_power_index_mw(u32) -> u8, packet_error_rate(rssi_dbm: f64, sensitivity_dbm: f64) -> f64}`
  - `ofs_radio::elrs::{ELRS_FREQ_MHZ (2440.0), NOISE_FLOOR_DBM (-108.90), DOWNLINK_TX_POWER_MW (100.0), PER_SLOPE_DB (1.0), LQ_WINDOW, NO_SIGNAL_RSSI_DBM (-130.0), MODEL_NAME}`
  - `LinkParams { packet_rate_hz, latency_packets, tx_power_mw: f64, link_stats_interval_packets, handset_position: DVec3, handset_antennas: Vec<Antenna>, quad_antennas: Vec<Antenna>, obstacles: Vec<Obstacle>, fading: bool, ground_bounce: bool }` with `LinkParams::ideal(packet_rate_hz)`; `ElrsLink::new(params, rate_divisor, seed, uart, bus)` unchanged in shape.
  - Bus signals `radio.snr_db`, `radio.antenna`, `radio.downlink_lq` (names `RADIO_SNR`, `RADIO_ANTENNA`, `RADIO_DOWNLINK_LQ`); `radio.rssi_dbm` is the active antenna's RSSI as before.
  - `ofs_sim::vehicle::{world_objects(&WorldConfig) -> Vec<WorldObject>, world_obstacles(&WorldConfig) -> Vec<Obstacle>}` (both `pub`).

- [ ] **Step 1: Write the failing tests**

Replace the whole of `crates/ofs-radio/tests/elrs.rs` with:

```rust
use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{stick_ticks, Decoder, Frame};
use ofs_radio::elrs::{
    packet_error_rate, rf_mode_index, sensitivity_dbm, tx_power_index_mw, ElrsLink, LinkParams, NOISE_FLOOR_DBM,
    NO_SIGNAL_RSSI_DBM,
};
use ofs_rf::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let div = BASE_HZ / params.packet_rate_hz;
    let link = ElrsLink::new(params, div, seed, uart.clone(), &mut bus);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(link));
    (s, uart)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

fn frames(uart: &Wire) -> Vec<Frame> {
    Decoder::default().push(&uart.take(usize::MAX))
}

fn rc(frames: &[Frame]) -> Vec<[u16; 16]> {
    frames.iter().filter_map(|f| if let Frame::RcChannels(c) = f { Some(*c) } else { None }).collect()
}

fn statistics(frames: &[Frame]) -> Vec<ofs_radio::crsf::LinkStatistics> {
    frames.iter().filter_map(|f| if let Frame::LinkStatistics(s) = f { Some(*s) } else { None }).collect()
}

/// Steps `packets` radio packets at 500 Hz.
fn packets(s: &mut Scheduler, count: u32) {
    for _ in 0..count * (BASE_HZ / 500) {
        s.step().unwrap();
    }
}

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

#[test]
fn the_sensitivity_table_is_elrs_published() {
    assert_eq!(sensitivity_dbm(50), Some(-117.0));
    assert_eq!(sensitivity_dbm(150), Some(-112.0));
    assert_eq!(sensitivity_dbm(250), Some(-108.0));
    assert_eq!(sensitivity_dbm(500), Some(-105.0));
    assert_eq!(sensitivity_dbm(1000), None);
    assert_eq!(rf_mode_index(500), Some(4));
    assert_eq!(rf_mode_index(50), Some(1));
    assert_eq!(tx_power_index_mw(250), 17);
    assert_eq!(tx_power_index_mw(10), 1);
    assert_eq!(tx_power_index_mw(1000), 23);
}

#[test]
fn per_is_half_at_the_sensitivity_and_small_five_db_up() {
    let s = sensitivity_dbm(500).unwrap();
    close(packet_error_rate(s, s), 0.5, 1e-9, "at the sensitivity");
    assert!(packet_error_rate(s + 5.0, s) < 0.01, "5 dB up: {}", packet_error_rate(s + 5.0, s));
    assert!(packet_error_rate(s - 15.0, s) > 0.999_999, "15 dB down: effectively everything is lost");
}

#[test]
fn the_rssi_reaches_the_sensitivity_at_the_published_range() {
    // 250 mW and 10 mW, 2 dBi dipoles at peak gain and co-polarized, fading, bounce and shadow off.
    for (power_mw, range_m) in [(250.0, 43_600.0), (10.0, 8_700.0)] {
        let mut p = LinkParams::ideal(500);
        p.tx_power_mw = power_mw;
        let (mut s, _uart) = rig(p, 1);
        set_pos(&mut s, DVec3::new(range_m, 0.0, -1.7));
        packets(&mut s, 2);
        let rssi = get(&s, names::RADIO_RSSI);
        // +-5 % of range is +-0.42 dB; give it a little slack.
        close(rssi, sensitivity_dbm(500).unwrap(), 0.55, &format!("{power_mw} mW at {range_m} m"));
    }
}

#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    set_pos(&mut s, DVec3::new(0.0, 0.0, -1.7)); // level with the handset, 2 m away: a horizontal path
    s.run_for(1.0).unwrap();
    let f = frames(&uart);
    assert_eq!(rc(&f).len(), 500);
    assert_eq!(statistics(&f).len(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    let expected = 10.0 * 250f64.log10() + 4.0 - fspl_db(2.0, 2440.0);
    close(get(&s, names::RADIO_RSSI), expected, 0.1, "the bare budget at 2 m");
    close(get(&s, names::RADIO_SNR), expected - NOISE_FLOOR_DBM, 0.1, "SNR is RSSI minus the LoRa noise floor");
    let stats = statistics(&f);
    assert_eq!((stats[0].rf_mode, stats[0].uplink_tx_power, stats[0].active_antenna), (4, 17, 0));
    assert_eq!(stats[0].uplink_lq, 100);
    assert_eq!(stats[0].uplink_rssi_1, (-expected).round() as u8);
    assert_eq!(stats[0].uplink_rssi_2, stats[0].uplink_rssi_1, "one antenna: both fields report it");
}

#[test]
fn a_second_antenna_is_picked_when_it_is_clearly_better() {
    let mut p = LinkParams::ideal(500);
    p.quad_antennas.push(Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Rhcp, axis: DVec3::NEG_X });
    let (mut s, uart) = rig(p, 1);
    set_pos(&mut s, DVec3::new(20.0, 0.0, -1.7)); // the handset is due west of the quad: the patch's boresight
    s.run_for(1.0).unwrap();
    let stats = statistics(&frames(&uart));
    assert_eq!(stats[0].active_antenna, 1, "the 8 dBi patch aimed at the handset wins");
    assert_eq!(get(&s, names::RADIO_ANTENNA), 1.0);
    assert!(stats[0].uplink_rssi_2 + 4 <= stats[0].uplink_rssi_1, "the patch hears it at least 4 dB better: {stats:?}");
}

#[test]
fn a_dipole_crossed_to_the_handset_loses_20_db() {
    let (mut aligned, _) = rig(LinkParams::ideal(500), 1);
    set_pos(&mut aligned, DVec3::new(0.0, 0.0, -1.7)); // level with the handset: a horizontal path
    packets(&mut aligned, 2);
    let mut crossed_params = LinkParams::ideal(500);
    crossed_params.quad_antennas[0].axis = DVec3::Y; // broadside to the path, its field crossed with the handset's
    let (mut crossed, _) = rig(crossed_params, 1);
    set_pos(&mut crossed, DVec3::new(0.0, 0.0, -1.7));
    packets(&mut crossed, 2);
    let loss = get(&aligned, names::RADIO_RSSI) - get(&crossed, names::RADIO_RSSI);
    close(loss, 20.0, 0.1, "crossed linear dipoles");
}

#[test]
fn behind_a_building_the_rssi_drops_by_its_capped_knife_edge_loss() {
    let mut blocked_params = LinkParams::ideal(500);
    let shape = Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(10.0, 10.0, 10.0) };
    blocked_params.obstacles = vec![Obstacle { shape: shape.rooted(), rf_loss_db: 25.0 }];
    let (mut open, _) = rig(LinkParams::ideal(500), 1);
    let (mut blocked, _) = rig(blocked_params, 1);
    set_pos(&mut open, DVec3::new(100.0, 0.0, -1.7));
    set_pos(&mut blocked, DVec3::new(100.0, 0.0, -1.7));
    packets(&mut open, 2);
    packets(&mut blocked, 2);
    let drop = get(&open, names::RADIO_RSSI) - get(&blocked, names::RADIO_RSSI);
    assert!(drop > 24.0 && drop <= 25.0 + 1e-9, "the building costs its cap of 25 dB: {drop}");
}

#[test]
fn a_hovering_quad_in_the_open_keeps_lq_at_100() {
    let mut p = LinkParams::ideal(500);
    p.fading = true;
    p.ground_bounce = true;
    let (mut s, _uart) = rig(p, 7);
    set_pos(&mut s, DVec3::new(30.0, 0.0, -1.7));
    s.run_for(1.0).unwrap();
    assert_eq!(get(&s, names::RADIO_LQ), 100.0, "60 dB of margin survives any fade");
}

#[test]
fn the_same_seed_gives_the_same_packet_stream() {
    let scripted = |seed: u64| {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        p.ground_bounce = true;
        let (mut s, uart) = rig(p, seed);
        for north in [0.0, 500.0, 1000.0] {
            set_pos(&mut s, DVec3::new(north, 0.0, -1.7));
            s.run_for(0.2).unwrap();
        }
        uart.take(usize::MAX)
    };
    assert_eq!(scripted(11), scripted(11), "same seed: identical bytes");
    assert_ne!(scripted(11), scripted(12), "a different seed fades differently");
}

#[test]
fn the_loss_draws_do_not_depend_on_the_transmitter() {
    // At the sensitivity edge (PER near one half), the received/not pattern of the 250 packets after the
    // switch-on must be identical whether the handset transmitted from the start or mid-run: the draws never
    // depend on the inputs.
    let pattern = |tx_from: u64| {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        let (mut s, uart) = rig(p, 3);
        set_pos(&mut s, DVec3::new(42_000.0, 0.0, -1.7));
        let mut seen = Vec::new();
        for t in 0..8000 {
            set(&mut s, names::RADIO_TX_ENABLED, if t >= tx_from { 1.0 } else { 0.0 });
            s.step().unwrap();
            if t >= tx_from {
                seen.push(!Decoder::default().push(&uart.take(usize::MAX)).is_empty());
            }
        }
        seen
    };
    assert_eq!(pattern(0), pattern(4000));
}

#[test]
fn with_the_transmitter_off_the_receiver_goes_silent_and_lq_drains() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.2).unwrap(); // LQ 100
    set(&mut s, names::RADIO_TX_ENABLED, 0.0);
    s.run_for(0.25).unwrap(); // 100 packets of silence drain the window
    assert!(rc(&frames(&uart)).is_empty(), "no frames while the handset is off");
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    assert_eq!(get(&s, names::RADIO_RSSI), NO_SIGNAL_RSSI_DBM);
}

#[test]
fn the_radio_link_loss_fault_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.1).unwrap();
    let before = rc(&frames(&uart)).len();
    assert!(before > 0);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    s.run_for(0.1).unwrap();
    let after = rc(&frames(&uart)).len();
    assert_eq!(after, 0, "every packet lost while the fault is active");
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
}

#[test]
fn the_sticks_reach_the_receiver_after_latency_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
    set(&mut s, names::RC_ROLL, 0.5);
    s.run_for(0.012).unwrap(); // 6 packets at 500 Hz
    let seen = rc(&frames(&uart));
    assert_eq!(seen.len(), 6);
    assert_eq!(seen[0][0], stick_ticks(0.0), "the first frames carry the sticks from before the move");
    assert_eq!(seen.last().unwrap()[0], stick_ticks(0.5));
}

#[test]
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_YAW, 0.5), (names::RC_THROTTLE, 0.25)] {
        set(&mut s, name, v);
    }
    s.run_for(0.004).unwrap();
    let frame = rc(&frames(&uart)).pop().unwrap();
    assert_eq!((frame[0], frame[1], frame[2], frame[3]), (stick_ticks(0.2), stick_ticks(-0.2), ofs_radio::crsf::throttle_ticks(0.25), stick_ticks(0.5)));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-radio --locked`
Expected: FAIL to compile — `glam` and `ofs_rf` are not dependencies of `ofs-radio`, and the new functions do not exist.

- [ ] **Step 3: Rewrite `elrs.rs`**

In `crates/ofs-radio/Cargo.toml`, replace:

```toml
[dependencies]
ofs-core.workspace = true
rand.workspace = true
rand_chacha.workspace = true
```

with:

```toml
[dependencies]
glam.workspace = true
ofs-core.workspace = true
ofs-rf.workspace = true
rand.workspace = true
rand_chacha.workspace = true
```

Replace the whole of `crates/ofs-radio/src/elrs.rs` with:

```rust
//! ExpressLRS 2.4 GHz on the shared propagation model. Every packet the handset samples the sticks; the
//! packet's RSSI comes from the geometry — antenna patterns, range, obstruction, polarization, ground bounce,
//! fading, body shadow — and a logistic curve around the mode's sensitivity decides loss. The receiver writes
//! one CRSF RC frame per packet it receives (and LINK_STATISTICS every N received packets) to the flight
//! controller's UART; when packets stop it goes silent, as ExpressLRS does by default, and Betaflight's own
//! failsafe takes over. Deterministic: every packet draws the same count of numbers from the model's own
//! seeded stream, whatever the inputs.
use std::collections::VecDeque;

use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{mw_to_dbm, path_gain, wavelength_m, Antenna, AntennaKind, Endpoint, Obstacle, Polarization};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::crsf::{self, LinkStatistics, CHANNEL_COUNT};

pub const MODEL_NAME: &str = "radio.elrs";
/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;
/// RSSI published while no packet is heard.
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;
/// The middle of the 2.4 GHz band; frequency hopping is not modelled.
pub const ELRS_FREQ_MHZ: f64 = 2440.0;
/// LoRa bandwidth 812.5 kHz, noise figure 6 dB (estimated): `-174 + 10*log10(812_500) + 6`.
pub const NOISE_FLOOR_DBM: f64 = -174.0 + 10.0 * 812_500f64.log10() + 6.0;
/// The receiver transmits the downlink at this power (estimated).
pub const DOWNLINK_TX_POWER_MW: f64 = 100.0;
/// The logistic PER curve's width in dB (an estimate).
pub const PER_SLOPE_DB: f64 = 1.0;

/// The RSSI at which half the packets decode, by packet rate: ExpressLRS's published sensitivities for its
/// 2.4 GHz LoRa modes.
pub fn sensitivity_dbm(packet_rate_hz: u32) -> Option<f64> {
    match packet_rate_hz {
        50 => Some(-117.0),
        150 => Some(-112.0),
        250 => Some(-108.0),
        500 => Some(-105.0),
        _ => None,
    }
}

/// The CRSF rf_mode index of a packet rate (the CRSF table: 0 = 4 Hz, 1 = 50, 2 = 150, 3 = 250, 4 = 500).
pub fn rf_mode_index(packet_rate_hz: u32) -> Option<u8> {
    match packet_rate_hz {
        50 => Some(1),
        150 => Some(2),
        250 => Some(3),
        500 => Some(4),
        _ => None,
    }
}

/// The CRSF power index of a TX power in mW (the CRSF power table).
pub fn tx_power_index_mw(power_mw: u32) -> u8 {
    match power_mw {
        0..=9 => 0,
        10..=24 => 1,
        25..=49 => 6,
        50..=99 => 10,
        100..=249 => 13,
        250..=499 => 17,
        500..=999 => 20,
        _ => 23,
    }
}

/// The packet error rate at `rssi_dbm` for a mode whose sensitivity is `sensitivity_dbm`: a logistic curve,
/// 50 % at the sensitivity (the slope is an estimate).
pub fn packet_error_rate(rssi_dbm: f64, sensitivity_dbm: f64) -> f64 {
    1.0 / (1.0 + ((rssi_dbm - sensitivity_dbm) / PER_SLOPE_DB).exp())
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// The handset's TX power.
    pub tx_power_mw: f64,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    /// Where the handset is (NED) and its antennas (axes already in the world frame).
    pub handset_position: DVec3,
    pub handset_antennas: Vec<Antenna>,
    /// The receiver's antennas on the quad (axes in the body frame, FRD).
    pub quad_antennas: Vec<Antenna>,
    pub obstacles: Vec<Obstacle>,
    /// Fading and the ground bounce; tests that check the bare link budget turn them off.
    pub fading: bool,
    pub ground_bounce: bool,
}

impl LinkParams {
    /// A strong link at `packet_rate_hz`: one packet of latency, both ends a vertical 2 dBi linear dipole, the
    /// handset 2 m from the quad's home and level with the pad (a horizontal path: the dipoles are broadside),
    /// no obstacles, no fading, no bounce.
    pub fn ideal(packet_rate_hz: u32) -> Self {
        let dipole = Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Linear, axis: DVec3::NEG_Z };
        Self {
            packet_rate_hz,
            latency_packets: 1,
            tx_power_mw: 250.0,
            link_stats_interval_packets: 50,
            handset_position: DVec3::new(-2.0, 0.0, -1.7),
            handset_antennas: vec![dipole],
            quad_antennas: vec![dipole],
            obstacles: Vec::new(),
            fading: false,
            ground_bounce: false,
        }
    }
}

struct Inputs {
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    fault_loss: Signal<f64>,
}

struct Outputs {
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    snr: Signal<f64>,
    antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
}

pub struct ElrsLink {
    params: LinkParams,
    div: u32,
    rng: ChaCha8Rng,
    uart: Wire,
    inputs: Inputs,
    outputs: Outputs,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    history: VecDeque<bool>,
    downlink_history: VecDeque<bool>,
    pipeline: VecDeque<[u16; CHANNEL_COUNT]>,
    since_stats: u32,
    quad_diversity: Diversity,
    handset_diversity: Diversity,
    quad_faders: Vec<Fader>,
    handset_faders: Vec<Fader>,
    last_pos: Option<DVec3>,
}

impl ElrsLink {
    /// `uart` receives the receiver's CRSF output (the flight controller's UART RX).
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus) -> Self {
        assert!(!params.quad_antennas.is_empty() && !params.handset_antennas.is_empty(), "both ends need at least one antenna");
        assert!(sensitivity_dbm(params.packet_rate_hz).is_some(), "packet_rate_hz must be one of 50, 150, 250, 500");
        let inputs = Inputs {
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            fault_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
        };
        let outputs = Outputs {
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
        };
        let mut rng = model_rng(seed, MODEL_NAME);
        let mut quad_faders = vec![Fader::default(); params.quad_antennas.len()];
        let mut handset_faders = vec![Fader::default(); params.handset_antennas.len()];
        for f in quad_faders.iter_mut().chain(handset_faders.iter_mut()) {
            f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first packet
        }
        Self {
            params,
            div: rate_divisor,
            rng,
            uart,
            inputs,
            outputs,
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            history: VecDeque::with_capacity(LQ_WINDOW + 1),
            downlink_history: VecDeque::with_capacity(LQ_WINDOW + 1),
            pipeline: VecDeque::new(),
            since_stats: 0,
            quad_diversity: Diversity::default(),
            handset_diversity: Diversity::default(),
            quad_faders,
            handset_faders,
            last_pos: None,
        }
    }

    /// Handset: sticks to channels in Betaflight's default AETR order, then AUX1..4, the rest centred.
    fn sample(&self, bus: &Bus) -> [u16; CHANNEL_COUNT] {
        let i = &self.inputs;
        let mut ch = [crsf::stick_ticks(0.0); CHANNEL_COUNT];
        ch[0] = crsf::stick_ticks(bus.get(i.roll));
        ch[1] = crsf::stick_ticks(bus.get(i.pitch));
        ch[2] = crsf::throttle_ticks(bus.get(i.throttle));
        ch[3] = crsf::stick_ticks(bus.get(i.yaw));
        for (slot, s) in ch[4..].iter_mut().zip(&i.aux) {
            *slot = crsf::stick_ticks(bus.get(*s));
        }
        ch
    }

    fn lq_of(history: &VecDeque<bool>) -> f64 {
        if history.is_empty() {
            return 0.0;
        }
        100.0 * history.iter().filter(|r| **r).count() as f64 / history.len() as f64
    }
}

impl Model for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Handset: the pipeline delays samples by `latency_packets` packets.
        let sample = self.sample(bus);
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(sample, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(sample);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let sensitivity = sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction");
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;

        // Draw order is fixed: every packet draws the same count of numbers whatever the inputs — the uplink
        // fades (two normals per quad antenna), the uplink loss draw, the downlink fades, the downlink draw.
        let rho = (-moved / (wavelength_m(ELRS_FREQ_MHZ) * 0.5)).exp();
        let shadow = body_shadow_db(att, pos, self.params.handset_position);
        let mut rssi = vec![NO_SIGNAL_RSSI_DBM; self.params.quad_antennas.len()];
        for j in 0..self.params.quad_antennas.len() {
            let configured = self.params.quad_antennas[j];
            let rx_ant = Antenna { axis: (att * configured.axis).normalize(), ..configured };
            let tx_ant = self.params.handset_antennas[self.handset_diversity.active().min(self.params.handset_antennas.len() - 1)];
            let path = path_gain(
                &Endpoint { position: self.params.handset_position, antenna: tx_ant },
                &Endpoint { position: pos, antenna: rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.quad_faders[j].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            rssi[j] = (mw_to_dbm(self.params.tx_power_mw) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let active = self.quad_diversity.choose(&rssi);
        let draw: f64 = self.rng.gen();
        let received = transmitting && draw >= packet_error_rate(rssi[active], sensitivity);

        // Downlink: the reversed path of the active antenna, the receiver transmitting at DOWNLINK_TX_POWER_MW,
        // received on the handset's best antenna.
        let configured = self.params.quad_antennas[active];
        let tx_ant = Antenna { axis: (att * configured.axis).normalize(), ..configured };
        let mut downlink = vec![NO_SIGNAL_RSSI_DBM; self.params.handset_antennas.len()];
        for a in 0..self.params.handset_antennas.len() {
            let rx_ant = self.params.handset_antennas[a];
            let path = path_gain(
                &Endpoint { position: pos, antenna: tx_ant },
                &Endpoint { position: self.params.handset_position, antenna: rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.handset_faders[a].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            downlink[a] = (mw_to_dbm(DOWNLINK_TX_POWER_MW) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let downlink_active = self.handset_diversity.choose(&downlink);
        let downlink_draw: f64 = self.rng.gen();
        let downlink_received = transmitting && downlink_draw >= packet_error_rate(downlink[downlink_active], sensitivity);

        // Link quality over the last 100 packets, per direction; silence on loss; Betaflight fails safe.
        self.history.push_back(received);
        if self.history.len() > LQ_WINDOW {
            self.history.pop_front();
        }
        self.downlink_history.push_back(downlink_received);
        if self.downlink_history.len() > LQ_WINDOW {
            self.downlink_history.pop_front();
        }
        let lq = Self::lq_of(&self.history);
        let downlink_lq = Self::lq_of(&self.downlink_history);
        let link_up = lq > 0.0;
        let snr = rssi[active] - NOISE_FLOOR_DBM;

        // Receiver: one RC frame per received packet, link statistics every N received packets.
        if received {
            self.uart.write(&crsf::rc_channels_frame(&channels));
            self.since_stats += 1;
            if self.since_stats >= self.params.link_stats_interval_packets {
                self.since_stats = 0;
                let dbm_field = |dbm: f64| (-dbm).round().clamp(0.0, 255.0) as u8;
                self.uart.write(&crsf::link_statistics_frame(&LinkStatistics {
                    uplink_rssi_1: dbm_field(rssi[0]),
                    uplink_rssi_2: rssi.get(1).map_or(dbm_field(rssi[0]), |v| dbm_field(*v)),
                    uplink_lq: lq.round().clamp(0.0, 100.0) as u8,
                    uplink_snr: snr.round().clamp(-128.0, 127.0) as i8,
                    active_antenna: active as u8,
                    rf_mode: rf_mode_index(self.params.packet_rate_hz).unwrap_or(0),
                    uplink_tx_power: tx_power_index_mw(self.params.tx_power_mw as u32),
                    downlink_rssi: dbm_field(downlink[downlink_active]),
                    downlink_lq: downlink_lq.round().clamp(0.0, 100.0) as u8,
                    downlink_snr: (downlink[downlink_active] - NOISE_FLOOR_DBM).round().clamp(-128.0, 127.0) as i8,
                }));
            }
        }
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, if link_up { rssi[active] } else { NO_SIGNAL_RSSI_DBM });
        bus.set(self.outputs.snr, if link_up { snr } else { NO_SIGNAL_RSSI_DBM - NOISE_FLOOR_DBM });
        bus.set(self.outputs.antenna, active as f64);
        bus.set(self.outputs.downlink_lq, downlink_lq);
        Ok(())
    }
}
```

In `crates/ofs-core/src/names.rs`, replace:

```rust
/// Uplink RSSI as the receiver reports it.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
```

with:

```rust
/// Uplink RSSI as the receiver reports it, at the antenna in use.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
/// Uplink SNR in dB at the antenna in use (LoRa decodes below the noise, so it may be negative).
pub const RADIO_SNR: &str = "radio.snr_db";
/// Index (into the quad file's radio antennas) of the antenna in use.
pub const RADIO_ANTENNA: &str = "radio.antenna";
/// Downlink link quality: percent of the last 100 packets the handset received.
pub const RADIO_DOWNLINK_LQ: &str = "radio.downlink_lq";
```

- [ ] **Step 4: Wire the vehicle**

All edits in `crates/ofs-sim/src/vehicle.rs`. Replace:

```rust
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
```

with:

```rust
use ofs_physics::collision::CollisionParams;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody, WorldObject};
```

After the `aim_ned` helper, add:

```rust
/// The world's objects as collidable shapes, exactly as the file gives them (not rooted).
pub fn world_objects(world: &WorldConfig) -> Vec<WorldObject> {
    world.objects.iter().map(|o| WorldObject { name: o.name.clone(), shape: object_shape(o) }).collect()
}

/// The world's objects as RF obstacles: those that take signal, rooted so nothing diffracts underneath them.
pub fn world_obstacles(world: &WorldConfig) -> Vec<Obstacle> {
    world
        .objects
        .iter()
        .filter(|o| o.rf_loss_db > 0.0)
        .map(|o| Obstacle { shape: object_shape(o).rooted(), rf_loss_db: o.rf_loss_db })
        .collect()
}

fn object_shape(o: &world_cfg::ObjectSection) -> Shape {
    let center = v3(o.center_ned_m);
    match o.shape {
        world_cfg::Shape::Box => {
            let s = o.size_m.unwrap_or_default();
            Shape::Box { center, half: DVec3::new(s[0], s[1], s[2]) * 0.5 }
        }
        world_cfg::Shape::Cylinder => {
            Shape::Cylinder { center, radius: o.radius_m.unwrap_or_default(), half_height: o.height_m.unwrap_or_default() * 0.5 }
        }
    }
}
```

In `link_params`, replace the obstacle construction:

```rust
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
```

with:

```rust
    let obstacles = world_obstacles(world);
```

In `build`, replace:

```rust
    let f = &cfg.frame;
    let airframe = AirframeParams {
```

with:

```rust
    let f = &cfg.frame;
    let (restitution, friction_coeff) = cfg.collision_contact();
    let airframe = AirframeParams {
```

and replace:

```rust
        objects: Vec::new(), // the world's objects are wired in with the collision milestone
        collision: Default::default(),
    };
```

with:

```rust
        objects: world_objects(&opts.world),
        collision: CollisionParams {
            restitution,
            friction_coeff,
            spheres: cfg.collision_spheres().into_iter().map(|s| (DVec3::new(s[0], s[1], s[2]), s[3])).collect(),
        },
    };
```

Replace the radio link construction:

```rust
    let r = &cfg.radio;
    let link = LinkParams {
        packet_rate_hz: r.packet_rate_hz,
        latency_packets: r.latency_packets,
        loss_good: r.loss_good,
        loss_bad: r.loss_bad,
        p_good_to_bad: r.p_good_to_bad,
        p_bad_to_good: r.p_bad_to_good,
        rssi_dbm: r.rssi_dbm,
        snr_db: r.snr_db,
        link_stats_interval_packets: r.link_stats_interval_packets,
        rf_mode: r.rf_mode,
        tx_power: r.tx_power,
    };
```

with:

```rust
    let r = &cfg.radio;
    let facing = opts.world.pilot.facing_deg;
    let handset = &opts.world.handset;
    let link = LinkParams {
        packet_rate_hz: r.packet_rate_hz,
        latency_packets: r.latency_packets,
        tx_power_mw: f64::from(r.tx_power_mw),
        link_stats_interval_packets: r.link_stats_interval_packets,
        handset_position: v3(handset.position(&opts.world.pilot)),
        handset_antennas: handset
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, a.beamwidth_deg),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el()),
            })
            .collect(),
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
        obstacles: world_obstacles(&opts.world),
        fading: true,
        ground_bounce: true,
    };
```

- [ ] **Step 5: Run the tests to verify they pass**

Run (first command without `--locked` so `Cargo.lock` updates):
`cargo test --workspace --locked`
Expected: PASS — the new ELRS tests pass; the video, config, physics and sim tests are unchanged or updated import-paths only. The old radio tests are gone (replaced).

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/ofs-radio crates/ofs-core/src/names.rs crates/ofs-sim
git commit -m "feat(radio)!: the ELRS link computes RSSI, SNR and loss from the shared propagation at 2440 MHz; the vehicle wires in the world's objects and handset

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 7: protocol 5 — SNR, active antenna, downlink, collision speed, handset; server events; Python

All clients move together: `PROTOCOL_VERSION = 5`. `RadioLink` gains `snr_db`, `active_antenna` and `downlink_lq_pct`; `State` gains `collision_speed_mps`; `EVENT_KIND_COLLISION = 13` is raised with the spec's toast text; `World` carries the handset. The Python client exposes all of it.

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`, `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs`, `python/ofs/client.py`, `python/ofs/__init__.py`, the regenerated `python/ofs/v1/sim_pb2.py`/`sim_pb2_grpc.py`/`sim_pb2.pyi`, `python/tests/test_client.py`

**Interfaces:**
- Consumes: Task 6's bus signals (`radio.snr_db`, `radio.antenna`, `radio.downlink_lq`, `body.collision_speed/object/count`).
- Produces (used by Tasks 8 and 9):
  - `PROTOCOL_VERSION == 5` on both sides.
  - `VehicleState` gains `collision_speed_mps: f64`, `collision_object: i32`, `collision_event_count: u64`; `RadioState` gains `snr_db: f64`, `active_antenna: String`, `downlink_lq_pct: f64` (and loses `Copy` — it now carries a `String`, like `VideoInfo`).
  - `Session::events_for` raises `EVENT_KIND_COLLISION` on a `collision_event_count` change with the message `HIT <object> <speed:.1> m/s` or `HARD LANDING <speed:.1> m/s`.
  - Python: `RadioLink(tx_enabled, link_up, lq_pct, rssi_dbm, snr_db, active_antenna, downlink_lq_pct)`, `State.collision_speed_mps`, `Handset(position_ned_m, antennas)`, `World.handset`.

- [ ] **Step 1: The protocol files**

In `proto/ofs/v1/sim.proto`, replace the header comment's first line:

```proto
// Simulator control, protocol version 4: lockstep and real-time sessions, state and event streams, the
```

with:

```proto
// Simulator control, protocol version 5: protocol 4 plus the radio link's SNR, active antenna and downlink,
// the collision speed, and the world's handset (M3c). Still: lockstep and real-time sessions, state and event
// streams, the
```

Replace:

```proto
message RadioLink {
  bool tx_enabled = 1;  // a pilot or script is connected
  bool link_up = 2;
  double lq_pct = 3;
  double rssi_dbm = 4;
}
```

with:

```proto
message RadioLink {
  bool tx_enabled = 1;  // a pilot or script is connected
  bool link_up = 2;
  double lq_pct = 3;
  double rssi_dbm = 4;         // at the antenna in use
  double snr_db = 5;           // may be negative: LoRa decodes below the noise
  string active_antenna = 6;   // the quad file's antenna name
  double downlink_lq_pct = 7;  // what the handset receives
}
```

Replace:

```proto
  uint64 serial_dropped_bytes = 15;  // Betaflight UART output dropped because a consumer fell behind
  VideoLink video = 16;
}
```

with:

```proto
  uint64 serial_dropped_bytes = 15;  // Betaflight UART output dropped because a consumer fell behind
  VideoLink video = 16;
  double collision_speed_mps = 17;   // the last collision's inward speed (0 until one happens)
}
```

Replace:

```proto
  EVENT_KIND_VIDEO_LOST = 11;      // the goggles lost sync
  EVENT_KIND_VIDEO_RESTORED = 12;  // and found it again
}
```

with:

```proto
  EVENT_KIND_VIDEO_LOST = 11;      // the goggles lost sync
  EVENT_KIND_VIDEO_RESTORED = 12;  // and found it again
  EVENT_KIND_COLLISION = 13;       // the quad hit something: the message names the object and the speed
}
```

Replace:

```proto
message World {
  string name = 1;
  Vec3 pilot_position_ned_m = 2;
  double pilot_facing_deg = 3;  // degrees clockwise from north
  repeated ReceiverAntenna antennas = 4;
  repeated WorldObject objects = 5;
  repeated Emitter emitters = 6;
}
```

with:

```proto
// The pilot's handset: where it is and what transmits from it (the ELRS uplink).
message Handset {
  Vec3 position_ned_m = 1;
  repeated ReceiverAntenna antennas = 2;
}

message World {
  string name = 1;
  Vec3 pilot_position_ned_m = 2;
  double pilot_facing_deg = 3;  // degrees clockwise from north
  repeated ReceiverAntenna antennas = 4;
  repeated WorldObject objects = 5;
  repeated Emitter emitters = 6;
  Handset handset = 7;
}
```

In `crates/ofs-proto/src/lib.rs`, replace:

```rust
//! Protocol 4 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
```

with:

```rust
//! Protocol 5 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
```

and replace:

```rust
pub const PROTOCOL_VERSION: u32 = 4;
```

with:

```rust
pub const PROTOCOL_VERSION: u32 = 5;
```

In `crates/ofs-proto/tests/messages.rs`, replace:

```rust
    assert_eq!(PROTOCOL_VERSION, 4);
```

with:

```rust
    assert_eq!(PROTOCOL_VERSION, 5);
```

- [ ] **Step 2: The server side**

All edits in `crates/ofs-sim/src/vehicle.rs`. Replace:

```rust
/// What the radio receiver reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioState {
    /// The transmitter is on (a pilot or script is connected).
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
}
```

with:

```rust
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
```

In `VehicleState`, replace:

```rust
    pub serial_dropped_bytes: u64,
    pub video: VideoInfo,
}
```

with:

```rust
    pub serial_dropped_bytes: u64,
    pub video: VideoInfo,
    /// The inward speed of the last collision event (0 until one happens).
    pub collision_speed_mps: f64,
    /// The object the last collision event hit (-1 = the ground).
    pub collision_object: i32,
    /// Collision events raised so far (the server detects the event's edge on this).
    pub collision_event_count: u64,
}
```

In `Handles`, after `serial_dropped: Signal<f64>,` add:

```rust
    radio_snr: Signal<f64>,
    radio_antenna: Signal<f64>,
    radio_downlink_lq: Signal<f64>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
    collision_count: Signal<f64>,
```

and in `Handles::register`, after `serial_dropped: bus.signal(names::FC_SERIAL_DROPPED),` add:

```rust
            radio_snr: bus.signal(names::RADIO_SNR),
            radio_antenna: bus.signal(names::RADIO_ANTENNA),
            radio_downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
            collision_count: bus.signal(names::BODY_COLLISION_COUNT),
```

In `pub struct Vehicle`, replace:

```rust
pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
    osd: Option<OsdHandle>,
    world: WorldConfig,
}
```

with:

```rust
pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
    osd: Option<OsdHandle>,
    world: WorldConfig,
    /// The quad file's radio antenna names, by index (the state's `active_antenna`).
    radio_antenna_names: Vec<String>,
}
```

In `build`, replace:

```rust
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd, world: opts.world.clone() };
```

with:

```rust
    let radio_antenna_names = cfg.radio.antennas.iter().map(|a| a.name.clone()).collect();
    let mut vehicle =
        Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl, osd, world: opts.world.clone(), radio_antenna_names };
```

In `Vehicle::state`, replace:

```rust
            radio: RadioState {
                tx_enabled: b.get(h.tx_enabled) > 0.5,
                link_up: b.get(h.link_up) > 0.5,
                lq_pct: b.get(h.lq),
                rssi_dbm: b.get(h.rssi),
            },
```

with:

```rust
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
```

and replace:

```rust
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
            video: h.video.read(b),
        }
    }
```

with:

```rust
            serial_dropped_bytes: b.get(h.serial_dropped) as u64,
            video: h.video.read(b),
            collision_speed_mps: b.get(h.collision_speed),
            collision_object: b.get(h.collision_object) as i32,
            collision_event_count: b.get(h.collision_count) as u64,
        }
    }
```

All edits in `crates/ofs-sim/src/session.rs`. In `pub struct Session`, replace:

```rust
    last_link_up: bool,
    last_restarts: u32,
```

with:

```rust
    last_link_up: bool,
    last_collision_count: u64,
    last_restarts: u32,
```

In `Session::new`, replace:

```rust
            last_link_up: false,
            last_restarts: 0,
```

with:

```rust
            last_link_up: false,
            last_collision_count: 0,
            last_restarts: 0,
```

In `events_for`, replace:

```rust
        if s.radio.link_up != self.last_link_up {
            self.last_link_up = s.radio.link_up;
            let (kind, message) =
                if s.radio.link_up { (pb::EventKind::LinkUp, "radio link up") } else { (pb::EventKind::LinkDown, "radio link lost") };
            out.push(event(s.time_s, kind, message));
        }
```

with:

```rust
        if s.radio.link_up != self.last_link_up {
            self.last_link_up = s.radio.link_up;
            let (kind, message) =
                if s.radio.link_up { (pb::EventKind::LinkUp, "radio link up") } else { (pb::EventKind::LinkDown, "radio link lost") };
            out.push(event(s.time_s, kind, message));
        }
        if s.collision_event_count != self.last_collision_count {
            self.last_collision_count = s.collision_event_count;
            let message = if s.collision_object < 0 {
                format!("HARD LANDING {:.1} m/s", s.collision_speed_mps)
            } else {
                let name = self
                    .vehicle
                    .world()
                    .objects
                    .get(s.collision_object.max(0) as usize)
                    .map_or("unknown", |o| o.name.as_str());
                format!("HIT {name} {:.1} m/s", s.collision_speed_mps)
            };
            out.push(event(s.time_s, pb::EventKind::Collision, message));
        }
```

In `state_msg`, replace:

```rust
            radio: Some(pb::RadioLink {
                tx_enabled: s.radio.tx_enabled,
                link_up: s.radio.link_up,
                lq_pct: s.radio.lq_pct,
                rssi_dbm: s.radio.rssi_dbm,
            }),
```

with:

```rust
            radio: Some(pb::RadioLink {
                tx_enabled: s.radio.tx_enabled,
                link_up: s.radio.link_up,
                lq_pct: s.radio.lq_pct,
                rssi_dbm: s.radio.rssi_dbm,
                snr_db: s.radio.snr_db,
                active_antenna: s.radio.active_antenna.clone(),
                downlink_lq_pct: s.radio.downlink_lq_pct,
            }),
```

and replace:

```rust
            serial_dropped_bytes: s.serial_dropped_bytes,
            video: Some(video_msg(&s.video)),
        }
    }
```

with:

```rust
            serial_dropped_bytes: s.serial_dropped_bytes,
            video: Some(video_msg(&s.video)),
            collision_speed_mps: s.collision_speed_mps,
        }
    }
```

For the handset in `world_msg`, replace the receiver-antenna mapping:

```rust
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
```

with:

```rust
        antennas: w.receiver.antennas.iter().map(antenna_msg).collect(),
```

then, next to `polarization_name`, add:

```rust
fn antenna_msg(a: &world_cfg::AntennaSection) -> pb::ReceiverAntenna {
    pb::ReceiverAntenna {
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
    }
}
```

and in `world_msg`, replace:

```rust
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

with:

```rust
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
        handset: Some(pb::Handset {
            position_ned_m: vec3_of(w.handset.position(&w.pilot)),
            antennas: w.handset.antennas.iter().map(antenna_msg).collect(),
        }),
    }
}
```

Add a test next to the file's existing `events_for` tests (reuse their way of building a session and a state; the state literal below is complete in case none exists):

```rust
    #[test]
    fn a_new_collision_count_raises_one_collision_event_naming_the_object() {
        let world = ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap();
        let building_a = world.objects.iter().position(|o| o.name == "BuildingA").expect("the flat world has BuildingA") as i32;
        let cfg = ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap();
        let opts = crate::vehicle::BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-session-collision-test"),
            fc_override: Some(ofs_config::FcKind::OpenLoop),
            world,
        };
        let mut session = Session::new(crate::vehicle::build(&cfg, &opts).unwrap(), RunMode::Lockstep, OverrunPolicy::Warn, true);
        let state = crate::vehicle::VehicleState {
            time_s: 1.0,
            pos_ned_m: glam::DVec3::ZERO,
            vel_ned_mps: glam::DVec3::ZERO,
            att: glam::DQuat::IDENTITY,
            rate_frd_radps: glam::DVec3::ZERO,
            battery_voltage_v: 25.0,
            battery_current_a: 0.0,
            motor_rpm: vec![0.0; 4],
            motor_cmd: vec![0.0; 4],
            radio: crate::vehicle::RadioState {
                tx_enabled: true,
                link_up: true,
                lq_pct: 100.0,
                rssi_dbm: -60.0,
                snr_db: 49.0,
                active_antenna: "antenna".into(),
                downlink_lq_pct: 100.0,
            },
            fc_restarts: 0,
            vtx: crate::vehicle::VtxInfo { present: false, band: 0, channel: 0, freq_mhz: 0, power_mw: 0, pit_mode: false },
            serial_dropped_bytes: 0,
            video: crate::vehicle::VideoInfo::default(),
            collision_speed_mps: 7.2,
            collision_object: building_a,
            collision_event_count: 1,
        };
        let events = session.events_for(&state);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].kind, pb::EventKind::Collision as i32);
        assert_eq!(events[0].message, "HIT BuildingA 7.2 m/s");
        // The same count again raises nothing: the edge is the counter.
        assert!(session.events_for(&state).is_empty());
        // A negative object index is the ground.
        let mut ground = state;
        ground.collision_object = -1;
        ground.collision_event_count = 2;
        let events = session.events_for(&ground);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "HARD LANDING 7.2 m/s");
    }
```

- [ ] **Step 3: The Python client**

In `python/ofs/client.py`, replace:

```python
PROTOCOL_VERSION = 4
```

with:

```python
PROTOCOL_VERSION = 5
```

Replace:

```python
@dataclass(frozen=True)
class RadioLink:
    tx_enabled: bool = False
    link_up: bool = False
    lq_pct: float = 0.0
    rssi_dbm: float = 0.0
```

with:

```python
@dataclass(frozen=True)
class RadioLink:
    tx_enabled: bool = False
    link_up: bool = False
    lq_pct: float = 0.0
    rssi_dbm: float = 0.0  # at the antenna in use
    snr_db: float = 0.0  # may be negative: LoRa decodes below the noise
    active_antenna: str = ""
    downlink_lq_pct: float = 0.0  # what the handset receives
```

After the `Emitter` dataclass, add:

```python
@dataclass(frozen=True)
class Handset:
    """The pilot's handset: where it is and what transmits from it (the ELRS uplink)."""
    position_ned_m: tuple
    antennas: tuple
```

Replace:

```python
@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters."""
    name: str
    pilot_position_ned_m: tuple
    pilot_facing_deg: float
    antennas: tuple
    objects: tuple
    emitters: tuple
```

with:

```python
@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters."""
    name: str
    pilot_position_ned_m: tuple
    pilot_facing_deg: float
    antennas: tuple
    objects: tuple
    emitters: tuple
    handset: Handset = field(default_factory=lambda: Handset((), ()))
```

In `State`, replace:

```python
    serial_dropped_bytes: int = 0
    video: VideoLink = field(default_factory=VideoLink)
```

with:

```python
    serial_dropped_bytes: int = 0
    video: VideoLink = field(default_factory=VideoLink)
    collision_speed_mps: float = 0.0  # the last collision's inward speed (0 until one happens)
```

Replace:

```python
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
    )
```

with:

```python
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
        handset=Handset(
            position_ned_m=_v(m.handset.position_ned_m),
            antennas=tuple(ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg,
                                           a.aim_el_deg) for a in m.handset.antennas),
        ) if m.HasField("handset") else Handset((), ()),
    )
```

Replace:

```python
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm),
```

with:

```python
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm,
                        m.radio.snr_db, m.radio.active_antenna, m.radio.downlink_lq_pct),
```

and in `_state`, replace:

```python
        video=_video(m.video),
    )
```

with:

```python
        video=_video(m.video),
        collision_speed_mps=m.collision_speed_mps,
    )
```

In `python/ofs/__init__.py`, replace:

```python
from .client import (Emitter, Event, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World, WorldObject,
```

with:

```python
from .client import (Emitter, Event, Handset, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World,
                     WorldObject,
```

(keep the rest of that import list and the `__all__` list in step: add `"Handset"` after `"Event",` in `__all__`.)

Then regenerate the stubs:

```bash
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
```

- [ ] **Step 4: The Python tests**

In `python/tests/test_client.py`, add:

```python
def test_the_radio_reports_snr_antenna_and_downlink(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.radio.link_up and s.radio.lq_pct == 100.0, s.radio
    assert s.radio.snr_db > 20.0, f"1.7 m from the handset: strong ({s.radio.snr_db})"
    assert s.radio.active_antenna == "antenna", s.radio
    assert s.radio.downlink_lq_pct == 100.0, s.radio


def test_the_state_starts_without_a_collision(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.collision_speed_mps == 0.0


def test_the_world_includes_the_handset(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True, world_path=WORLD)
    w = sim.get_world()
    assert w.handset.position_ned_m == (-3.0, 2.0, -1.2), "0.5 m below the goggles by default"
    assert w.handset.antennas[0].name == "handset"
    assert w.handset.antennas[0].polarization == "linear"


def test_a_collision_event_names_the_object(sim, tmp_path):
    import pathlib
    text = pathlib.Path(QUAD).read_text().replace(
        "position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [110.0, -45.0, -15.5]")
    quad = tmp_path / "onto-building-a.toml"
    quad.write_text(text)
    sim.load(str(quad), seed=1, open_loop_fc=True, world_path=WORLD)
    s = sim.run(5.0)
    collisions = [e for e in sim.events() if e.kind == "collision"]
    assert len(collisions) == 1, collisions
    assert "BuildingA" in collisions[0].message and "m/s" in collisions[0].message, collisions[0].message
    assert s.collision_speed_mps > 3.0, "the fall onto the roof was hard"
```

(`WORLD` is already defined in `python/tests/conftest.py`; import it the way the file's other tests do.)

- [ ] **Step 5: Run everything**

Run: `cargo build -p ofs-sim --locked && cargo test --workspace --locked && python -m pytest python/tests -q`
Expected: PASS — Rust and Python agree on protocol 5; the collision and handset tests pass. (The SITL-marked Python tests skip without `OFS_SITL_LAUNCH`.)

- [ ] **Step 6: Commit**

```bash
git add proto crates/ofs-proto crates/ofs-sim python
git commit -m "feat(proto)!: protocol 5 - radio SNR, active antenna and downlink, collision events with speed, the world's handset

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 8: `ofs-client` and Godot — the HUD link line, the collision toast, the handset marker

**Files:**
- Modify: `crates/ofs-client/src/model.rs`, `crates/ofs-client/tests/model.rs` (struct literals), `crates/ofs-godot/src/lib.rs`, `godot/ui/hud.gd`, `godot/scripts/app.gd`, `godot/world/world.gd`, `godot/tests/test_hud.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 7's protocol 5 (`RadioLink.snr_db/active_antenna/downlink_lq_pct`, `State.collision_speed_mps`, `World.handset`, `EVENT_KIND_COLLISION`).
- Produces (used by Task 9's e2e and the game):
  - `ofs_client::model::Telemetry` gains `radio_snr_db: f64`, `radio_antenna: String`, `collision_speed_mps: f64`.
  - `ofs_client::model::World` gains `handset_position: DVec3` (Godot frame) and `handset_antennas: Vec<WorldAntenna>`.
  - `EventKind::Collision` → `"collision"`.
  - Godot telemetry keys `radio_snr_db`, `radio_antenna`, `collision_speed_mps`; world keys `handset_position`, `handset_antennas`.
  - `Hud.link_text() -> String`; the world builds a `Handset` node; a `collision` event toasts its message.

- [ ] **Step 1: The Rust client model**

In `crates/ofs-client/src/model.rs`, replace:

```rust
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
    /// The session is paced to the wall clock right now (false while paused).
    pub running: bool,
```

with:

```rust
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
    /// At the antenna in use; may be negative (LoRa decodes below the noise).
    pub radio_snr_db: f64,
    /// The quad file's name of the antenna in use.
    pub radio_antenna: String,
    /// What the handset receives.
    pub downlink_lq_pct: f64,
    /// The inward speed of the last collision (0 until one happens).
    pub collision_speed_mps: f64,
    /// The session is paced to the wall clock right now (false while paused).
    pub running: bool,
```

Replace:

```rust
            tx_enabled: radio.tx_enabled,
            link_up: radio.link_up,
            lq_pct: radio.lq_pct,
            rssi_dbm: radio.rssi_dbm,
            running: s.running,
```

with:

```rust
            tx_enabled: radio.tx_enabled,
            link_up: radio.link_up,
            lq_pct: radio.lq_pct,
            rssi_dbm: radio.rssi_dbm,
            radio_snr_db: radio.snr_db,
            radio_antenna: radio.active_antenna.clone(),
            downlink_lq_pct: radio.downlink_lq_pct,
            collision_speed_mps: s.collision_speed_mps,
            running: s.running,
```

In `pub struct World`, replace:

```rust
    pub antennas: Vec<WorldAntenna>,
```

with:

```rust
    pub antennas: Vec<WorldAntenna>,
    /// The pilot's handset (in Godot's frame) and its antennas' aims.
    pub handset_position: DVec3,
    pub handset_antennas: Vec<WorldAntenna>,
```

(the struct has `objects` and `emitters` between `antennas` and the closing brace; the new fields go directly after `antennas`.) In `World::from_pb`, replace:

```rust
            antennas: w
                .antennas
                .into_iter()
                .map(|a| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind })
                .collect(),
```

with:

```rust
            antennas: w
                .antennas
                .into_iter()
                .map(|a| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind })
                .collect(),
            handset_position: vec_to_godot(ned(w.handset.as_ref().map_or(None, |h| h.position_ned_m))),
            handset_antennas: w
                .handset
                .map(|h| {
                    h.antennas
                        .into_iter()
                        .map(|a| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind })
                })
                .unwrap_or_default()
                .collect(),
```

In `EventKind`, replace:

```rust
    VideoLost,
    VideoRestored,
    Unknown,
```

with:

```rust
    VideoLost,
    VideoRestored,
    Collision,
    Unknown,
```

and replace:

```rust
            EventKind::VideoLost => "video_lost",
            EventKind::VideoRestored => "video_restored",
            EventKind::Unknown => "unknown",
```

with:

```rust
            EventKind::VideoLost => "video_lost",
            EventKind::VideoRestored => "video_restored",
            EventKind::Collision => "collision",
            EventKind::Unknown => "unknown",
```

(If `EventKind::from_pb`-style mapping exists — an `i32 -> EventKind` match — add `EventKind::Collision` to it the same way; `cargo check` names the spot.)

In `crates/ofs-client/tests/model.rs` (and anywhere else a test constructs `Telemetry` or `World` literally), add to every `Telemetry` literal `radio_snr_db: 0.0, radio_antenna: String::new(), downlink_lq_pct: 0.0, collision_speed_mps: 0.0,` and to every `World` literal `handset_position: DVec3::ZERO, handset_antennas: Vec::new(),` — `cargo check -p ofs-client` names each site.

- [ ] **Step 2: The Godot extension**

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
```

with:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `radio_snr_db`, `radio_antenna`, `downlink_lq_pct`,
    /// `collision_speed_mps`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
```

Replace:

```rust
        d.set("rssi_dbm", t.rssi_dbm);
        d.set("running", t.running);
```

with:

```rust
        d.set("rssi_dbm", t.rssi_dbm);
        d.set("radio_snr_db", t.radio_snr_db);
        d.set("radio_antenna", &GString::from(t.radio_antenna.as_str()));
        d.set("downlink_lq_pct", t.downlink_lq_pct);
        d.set("collision_speed_mps", t.collision_speed_mps);
        d.set("running", t.running);
```

In `get_world`, replace:

```rust
        d.set("name", &GString::from(world.name.as_str()));
        d.set("pilot_position", vector3(world.pilot_position.x, world.pilot_position.y, world.pilot_position.z));
        d.set("antennas", &antennas);
        d.set("objects", &objects);
        d.set("emitters", &emitters);
        d
    }
```

with:

```rust
        let mut handset_antennas = VarArray::new();
        for a in &world.handset_antennas {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(a.name.as_str()));
            e.set("kind", &GString::from(a.kind.as_str()));
            e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
            handset_antennas.push(&e.to_variant());
        }
        d.set("name", &GString::from(world.name.as_str()));
        d.set("pilot_position", vector3(world.pilot_position.x, world.pilot_position.y, world.pilot_position.z));
        d.set("antennas", &antennas);
        d.set("handset_position", vector3(world.handset_position.x, world.handset_position.y, world.handset_position.z));
        d.set("handset_antennas", &handset_antennas);
        d.set("objects", &objects);
        d.set("emitters", &emitters);
        d
    }
```

- [ ] **Step 3: The HUD link line and the collision toast**

In `godot/ui/hud.gd`, replace:

```gdscript
		var up: bool = t["link_up"] and t["tx_enabled"]
		_link.text = "LINK %s   LQ %d %%   %d dBm" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"]]
```

with:

```gdscript
		var up: bool = t["link_up"] and t["tx_enabled"]
		_link.text = "LINK %s   LQ %d %%   %d dBm   SNR %d" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"], roundi(t.get("radio_snr_db", 0.0))]
```

and, next to the other read-only accessors (after `video_text()`), add:

```gdscript
## The radio link line (for the tests).
func link_text() -> String:
	return _link.text
```

In `godot/scripts/app.gd`, replace:

```gdscript
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
```

with:

```gdscript
		"vtx_changed":
			hud.add_toast("VTX: %s" % message)
		"collision":
			hud.add_toast(message, "warn")
```

- [ ] **Step 4: The handset marker**

In `godot/world/world.gd`, replace:

```gdscript
	if world.has("pilot_position"):
		_add_pilot(world["pilot_position"], world.get("antennas", []))
```

with:

```gdscript
	if world.has("pilot_position"):
		_add_pilot(world["pilot_position"], world.get("antennas", []))
	if world.has("handset_position"):
		_add_handset(world["handset_position"])
```

and, after `_add_pilot`, add:

```gdscript
## The pilot's handset: a small dark transmitter on the ground beside the pilot, 0.5 m below the goggles.
func _add_handset(position: Vector3) -> void:
	var handset := Node3D.new()
	handset.name = "Handset"
	handset.position = position
	var body := BoxMesh.new()
	body.size = Vector3(0.07, 0.14, 0.025)
	handset.add_child(_mesh("Body", body, Color(0.16, 0.16, 0.2), Vector3.ZERO))
	_add_built(handset)
```

- [ ] **Step 5: The Godot tests**

In `godot/tests/test_hud.gd`, replace:

```gdscript
	var t := {
		"time_s": 12.3, "altitude_m": 4.5, "speed_mps": 6.7, "climb_mps": -0.4, "battery_voltage_v": 24.1,
		"battery_current_a": 12.0, "motor_cmd": PackedFloat32Array([0.3, 0.3, 0.3, 0.3]), "motors_spinning": true,
		"tx_enabled": true, "link_up": true, "lq_pct": 100.0, "rssi_dbm": -50.0, "running": true, "overruns": 0,
		"fc_restarts": 0, "age_s": 0.01,
	}
```

with:

```gdscript
	var t := {
		"time_s": 12.3, "altitude_m": 4.5, "speed_mps": 6.7, "climb_mps": -0.4, "battery_voltage_v": 24.1,
		"battery_current_a": 12.0, "motor_cmd": PackedFloat32Array([0.3, 0.3, 0.3, 0.3]), "motors_spinning": true,
		"tx_enabled": true, "link_up": true, "lq_pct": 100.0, "rssi_dbm": -50.0, "radio_snr_db": 49.0,
		"radio_antenna": "antenna", "downlink_lq_pct": 100.0, "collision_speed_mps": 0.0, "running": true,
		"overruns": 0, "fc_restarts": 0, "age_s": 0.01,
	}
```

and add:

```gdscript
func test_the_link_line_carries_the_snr() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.link_text(), "LINK UP   LQ 100 %   -50 dBm   SNR 49", "the link line")
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.link_text().begins_with("LINK DOWN"), "down: '%s'" % hud.link_text())
	hud.queue_free()
```

In `godot/tests/e2e_open_loop.gd`, replace:

```gdscript
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
```

with:

```gdscript
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
	_check(app.world.get_node_or_null("Handset") != null, "the pilot's handset is marked beside him")
```

and, at the end of `_run()`, replace:

```gdscript
	_check(not app.video.is_active(), "a clean link draws nothing over the picture")
```

with:

```gdscript
	_check(not app.video.is_active(), "a clean link draws nothing over the picture")

	# A hard touchdown raises the collision toast (the quad falls back onto the launch pad or the ground).
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.9, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var up: bool = await _wait_for(func(): return app.drone.position.y > 2.0, 20.0)
	_check(up, "climbs for the landing test: y = %.2f" % app.drone.position.y)
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var hit: bool = await _wait_for(func():
		var toasts := " ".join(app.hud.toast_texts())
		return toasts.contains("HIT LaunchPad") or toasts.contains("HARD LANDING"), 20.0)
	_check(hit, "the fall raises a collision toast: %s" % str(app.hud.toast_texts()))
```

- [ ] **Step 6: Run everything**

Run: `cargo test --workspace --locked && GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh all`
Expected: PASS — the client model tests (after the literal fixes) and the Godot unit and e2e suites pass, the e2e now ends with a collision toast.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-client crates/ofs-godot godot
git commit -m "feat(client): the HUD link line carries SNR, collisions toast, the pilot's handset is marked

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 9: end-to-end tests — collision and link-down in the simulator, the live chain to Betaflight

**Files:**
- Modify: `crates/ofs-sim/tests/open_loop_vehicle.rs`, `python/tests/test_client.py`
- Create: `python/tests/test_sitl_link.py`

**Interfaces:**
- Consumes: everything before (the vehicle, protocol 5, the Python client).
- Produces: the spec's open-loop and live test evidence; no new APIs.

- [ ] **Step 1: The open-loop vehicle tests**

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, after the existing `vehicle()` helper, add:

```rust
fn flat_world() -> WorldConfig {
    ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap()
}

fn vehicle_in(cfg: &ofs_config::QuadConfig, world: WorldConfig) -> Vehicle {
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    build(cfg, &opts).unwrap()
}

#[test]
fn a_quad_dropped_onto_building_a_raises_one_collision_and_rests_on_the_roof() {
    // Open loop cannot steer, so the quad falls 1.5 m onto BuildingA's roof (its top is the plane z = -14).
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.initial.position_ned_m = [110.0, -45.0, -15.5];
    let building_a = flat_world().objects.iter().position(|o| o.name == "BuildingA").unwrap() as i32;
    let mut v = vehicle_in(&cfg, flat_world());
    v.run_for(5.0).unwrap();
    let s = v.state();
    assert_eq!(s.collision_object, building_a, "the event names BuildingA");
    assert!(s.collision_speed_mps > 3.0, "the fall was hard: {}", s.collision_speed_mps);
    assert!(s.vel_ned_mps.length() < 1e-2, "at rest: {}", s.vel_ned_mps);
    assert!(s.pos_ned_m.z < -14.0 && s.pos_ned_m.z > -14.2, "on the roof, never inside it: {}", s.pos_ned_m.z);
}

#[test]
fn fifty_kilometres_out_at_ten_milliwatts_the_link_never_comes_up() {
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.radio.tx_power_mw = 10;
    cfg.initial.position_ned_m = [50_000.0, 0.0, -0.03];
    let mut v = vehicle_in(&cfg, WorldConfig::open_field());
    v.run_for(1.0).unwrap();
    let s = v.state();
    assert!(!s.radio.link_up, "no packet survives 15 dB under the sensitivity");
    assert_eq!(s.radio.lq_pct, 0.0);
}
```

- [ ] **Step 2: The open-loop Python tests**

In `python/tests/test_client.py`, add:

```python
def test_cutting_the_radio_raises_link_down(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True)
    s = sim.run(0.5)
    assert s.radio.link_up, s.radio
    sim.inject(ofs.faults.RadioLinkLoss())
    s = sim.run(0.5)
    assert not s.radio.link_up
    assert "link_down" in [e.kind for e in sim.events()], [e.kind for e in sim.events()]


def test_two_lockstep_runs_give_identical_body_and_radio_signals(sim):
    runs = []
    for _ in range(2):
        sim.load(QUAD, seed=5, open_loop_fc=True)
        s = sim.run(0.25)
        runs.append((s.time_s, tuple(s.motor_rpm), s.position_ned_m, s.radio))
        sim.unload()
    assert runs[0] == runs[1], "lockstep is deterministic, radio included"
```

(`import ofs` is already at the top of the file; `sim.unload()` is the client's existing method.)

- [ ] **Step 3: The live SITL test**

Create `python/tests/test_sitl_link.py`:

```python
"""The ELRS link end to end against real Betaflight (M3c): arms over the geometric link at home; 50 km out at
10 mW the receiver is silent, Betaflight fails safe and the OSD's LQ cell reads 0. Needs OFS_SITL_LAUNCH."""
import os
import pathlib
import re

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)


def test_at_home_the_quad_arms_over_the_geometric_link(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)  # boot and gyro calibration
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(1.0)
    assert min(s.motor_cmd) > 0.0, f"never armed: {s.motor_cmd}"
    assert s.radio.link_up and s.radio.lq_pct == 100.0, s.radio


def test_fifty_km_out_at_ten_milliwatts_betaflight_fails_safe(sim, tmp_path):
    text = pathlib.Path(QUAD).read_text()
    text = text.replace("tx_power_mw = 250", "tx_power_mw = 10")
    text = text.replace("position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [50000.0, 0.0, -0.03]")
    quad = tmp_path / "far-10mw.toml"
    quad.write_text(text)
    sim.load(str(quad), seed=1)
    s = sim.run(4.0)  # boot and calibration, with the receiver silent the whole time
    assert not s.radio.link_up, s.radio
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(2.0)
    assert max(s.motor_cmd) == 0.0, f"Betaflight armed without a receiver: {s.motor_cmd}"
    # The shipped diff places the LQ element on the OSD (osd_link_quality_pos): it reads 0 with no link.
    texts = set()
    for _ in range(5):
        osd = sim.get_osd()
        if osd.present:
            texts.add(osd.text)
        sim.run(0.2)
    assert texts, "the OSD never appeared"
    assert any(re.search(r"LQ\s*0\b", t) for t in texts), "\n\n".join(sorted(texts))
```

- [ ] **Step 4: Run everything**

Run: `cargo build -p ofs-sim --locked && cargo test -p ofs-sim --locked && python -m pytest python/tests -q`
Expected: PASS (the SITL live tests skip without `OFS_SITL_LAUNCH`). Then, for the live ones:

```bash
wsl_run python -m pytest python/tests/test_sitl_link.py python/tests/test_sitl_radio.py -q
```

  (`OFS_SITL_LAUNCH` set per "How to run things"; test_sitl_radio re-runs to prove the shipped quad still arms and fails safe as before.)
Expected: PASS — arms at home; at 50 km with 10 mW Betaflight never arms, the OSD LQ cell reads 0.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-sim/tests python/tests
git commit -m "test(sim): collision onto BuildingA, the 50 km silent link, lockstep determinism, and the live 10 mW failsafe

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 10: docs, carried debt, CI

**Files:**
- Create: `docs/research/elrs-link.md`, `docs/research/collision.md`, `docs/superpowers/m3c-carried-debt.md`
- Modify: `docs/dev-setup.md`, `README.md`

- [ ] **Step 1: `docs/research/elrs-link.md`**

Write the research note (same style as `docs/research/video-link.md`: sections, formulas, and a provenance column of "measured / ExpressLRS published / estimated"). Required content:

- **The budget per packet and per antenna**: `RSSI = Ptx + Gtx + Grx − FSPL(2440 MHz) − polarization − body shadow − obstruction`, combined with the two-ray ground bounce and the Rician fade, citing `ofs-rf::propagation`/`ofs_rf::fading` and the video-link note for the shared parts.
- **Constants table** (value, provenance): frequency 2440 MHz (band middle, hopping not modelled — estimate); LoRa bandwidth 812.5 kHz (ExpressLRS 2.4 GHz), noise figure 6 dB (estimate) → noise floor `−174 + 10·log10(812 500) + 6 = −108.9 dBm`; sensitivities 50 Hz −117, 150 Hz −112, 250 Hz −108, 500 Hz −105 dBm (ExpressLRS published); PER slope 1 dB (estimate); downlink TX 100 mW (estimate); correlation length λ/2 = 6.1 cm (physics); K-factor 10 dB clear, lowered dB for dB by obstruction (shared with the video link).
- **The worked range check**: 250 mW at 500 Hz with 2 dBi dipoles reaches −105 dBm at 43.6 km; 10 mW at 8.7 km — show the arithmetic (FSPL = 133 dB and 119 dB respectively).
- **The protocol mappings**: rf_mode 1/2/3/4 for 50/150/250/500 Hz and the CRSF power index table rows used (10 → 1, 25 → 6, 50 → 10, 100 → 13, 250 → 17, 500 → 20, 1000 → 23), marked "CRSF specification".
- **Diversity**: 2 dB hysteresis at the receiver; the handset transmits on the antenna it receives best (modelling simplification).

- [ ] **Step 2: `docs/research/collision.md`**

Write the education-audience note. Required content:

- **Why impulse contact and not a penalty spring** (the spec's rejected alternative): a spring stiff enough to stop 30 m/s within a 12 cm post needs a step far below 8 kHz to stay stable, while an impulse bounds the velocity change directly; with the numbers, both ways.
- **The contact model**: sphere vs signed distance (penetration `radius − sd`), the surface point along the shape's normal, position correction of the deepest contact, the normal impulse `j = −(1+e)·v_n/k` with `k = 1/m + n·((I⁻¹(r×n))×r)`, restitution 0 below 0.2 m/s, Coulomb friction capped at `μ·j`, deepest first in a fixed order (determinism).
- **The landing contact points**: the M1 spring-damper now also against objects; why both mechanisms (weight must rest stably; hits must not sink through).
- **The collision spheres**: the generated default (6 × 1.5 cm per prop tip, one 4 cm body sphere), the no-12-cm-gap arithmetic (adjacent spheres 3.35 cm apart, ring-to-body 0.85 cm).
- **Events**: 1 m/s threshold, the 20 ms rearm, and why a gentle landing raises nothing.
- **Constants table**: restitution 0.3 and friction 0.5 (defaults, estimates, user-tunable), the 0.2 m/s floor and the 1 m/s event threshold (estimates).

- [ ] **Step 3: `docs/superpowers/m3c-carried-debt.md`**

Create the file with the M3b debt doc's structure (Deferred features / Modelling simplifications / Rulings made while planning). It must list at least:

- **The generated body sphere reaches 1 cm past the shipped quad's legs** (4 cm vs 3 cm): the quad rests on its belly sphere; fix by a 2.5 cm body sphere or 5 cm legs, after a manual flight check.
- **The handset transmits on its active antenna** (its best downlink antenna): real dual-antenna handsets alternate; a per-packet schedule would be closer.
- **MSP_STATUS failsafe flags are not decoded** in Python: the live test observes the failsafe as "never arms + OSD LQ 0"; a decoded flag would be stricter.
- **Open-loop sessions cannot steer**: wall-hit behaviour is only unit-tested (30 m/s into a post) and e2e-tested via falls; a scripted-FC test could fly a circuit.
- **A 50 km/10 mW link is never up**, so `LINK_DOWN` has no edge to detect there: the spec's "LINK_DOWN is raised" is pinned on the up→down transition (the fault test) instead.
- The spec's own out-of-scope list (900 MHz, dynamic power, frequency hopping, Wi-Fi interference, damage, non-flat terrain, rotated boxes, frequency-dependent `rf_loss_db`, quad-to-object collisions beyond the quad) and anything surfaced while building.

- [ ] **Step 4: `docs/dev-setup.md` and `README.md`**

In `docs/dev-setup.md`, in the quad-file section, add a paragraph: quad files are **schema 4**; a schema-3 file is refused with a message naming the removed fields (`radio.rssi_dbm`, `radio.snr_db`, the loss parameters) and the replacements (`radio.tx_power_mw`, `[[radio.antennas]]`, `[collision]`); point at `quads/opendrone-5f-freestyle.toml` as the example. In the manual-flight-check section (create it after the existing checks if there is none), add:

```markdown
### M3c manual checks

1. Hit a wall: fly at BuildingA in the Flat field — the HUD toasts `HIT BuildingA ... m/s` and the quad bounces.
2. Clip a gate post: brush Gate0PostL at speed — the quad tumbles, one toast.
3. Land on a roof: settle on BuildingC — the quad rests, no jitter, no toast below 1 m/s.
4. Watch the link: with a quad file at `tx_power_mw = 10`, fly low behind BuildingB — the HUD's SNR falls,
   LQ decays, and Betaflight fails safe when the window drains.
```

In `README.md`, extend the feature list with one line: world collision (bounce off buildings and gates, land on roofs, `COLLISION` events) and a geometric ELRS link (RSSI/SNR/LQ from the world's geometry, Betaflight failsafe on RX loss).

- [ ] **Step 5: CI check**

Run: `cargo check --workspace --all-targets --locked --exclude ofs-godot`
Expected: PASS — `ofs-rf` is a workspace member, so the msrv job covers it with no new CI jobs; confirm no workflow file mentions crates individually (grep `.github/` for `ofs-video` — if a job names crates explicitly, add `ofs-rf` beside them).

- [ ] **Step 6: Commit**

```bash
git add docs README.md
git commit -m "docs: M3c research notes (elrs-link, collision), dev-setup schema 4 and manual checks, carried debt

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

## Coverage of the spec's test list

| Spec §7 test | Where |
|---|---|
| `normal` is the unit gradient on faces, edges, rim, caps; `bounds` contain | Task 1 |
| Bounce to restitution², 30 m/s post, rest on pad/roof, clip spin, friction stop, event once + 20 ms, bit-identical | Task 4 (Task 3 for the roof rest, Task 9 for the vehicle-level rest) |
| PER curve, sensitivity table, range check, cross-polarization, knife-edge behind building B, diversity hysteresis, hover LQ 100, LINK_STATISTICS, same seed | Task 6 (diversity hysteresis in Task 2's moved test + Task 6's antenna pick) |
| Schema 3 refused, 25 spheres/no 12 cm gap, validation collected, handset defaults + open field | Task 5 |
| BuildingA collision event + outside, 50 km link down + LINK_DOWN, lockstep identical, GetWorld handset | Tasks 7 and 9 (see the Rulings: the roof drop instead of steering; LINK_DOWN pinned on the up→down edge) |
| Live: arms at home; 10 mW at 50 km fails safe, OSD LQ 0 | Task 9 |
| HUD link text, collision toast, handset marker, e2e collision toast | Task 8 |
| No new CI jobs | Task 10 |

