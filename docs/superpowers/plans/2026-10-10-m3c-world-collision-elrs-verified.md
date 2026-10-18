# M3c: World Collision and ELRS on the Shared Propagation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The drone collides with the world file's objects (and lands on them), and the ExpressLRS control link's RSSI, SNR and packet loss come from the same propagation model as the video link, so range, buildings, antenna orientation and the ground bounce cost control-link quality.

**Architecture:** The radio maths of M3b move from `ofs-video` into a new `ofs-rf` crate (propagation, Rician fading, diversity, the frame's shadow) and the world's solids into `ofs-core::shape`. The rigid body (`ofs-physics`) gains impulse contact for collision spheres on the prop tips and the body, keeps the M1 spring-damper for its feet (now against objects too) and counts collisions on the bus. The ELRS model (`ofs-radio`) computes each packet's RSSI from the handset (a new world-file `[handset]`) to the quad's dipoles and loses packets by the packet rate's sensitivity. Quad files move to schema 4, the protocol to 5 (radio SNR/antenna/downlink, a `Collision` message and event, the handset in `World`); the Rust, Python and Godot clients follow.

**Tech Stack:** Rust 1.85+ (workspace: glam, rand_chacha, rand_distr, serde/toml, tonic/prost), Python 3.10+ client (grpcio), Godot 4.7.2 + godot-rust.

**Spec:** `docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md` (read it first; this plan implements it, with the rulings below where they differ). Predecessor: `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md` (merged as 40ac096). Parent: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`: no language or std features newer than 1.85 in any crate except `ofs-godot` (the msrv CI job runs `cargo check --workspace --all-targets --locked --exclude ofs-godot`).
- Python >= 3.10. Godot **4.7.2** only (GL Compatibility). License GPL-3.0-or-later.
- Determinism: same quad + world + seed + inputs in lockstep give identical output. The ELRS link's fading and loss draws come only from `model_rng(seed, "radio.elrs")` and every packet draws the same count of numbers whatever the inputs; contacts are resolved in a fixed order.
- Protocol: `PROTOCOL_VERSION = 5` in `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` together (a test compares them). Quad files: `schema_version = 4` only (schema 3 is refused with a message naming the changes). World files stay `schema_version = 1` (`[handset]` is optional).
- Frames: world files use the quad file's frame, NED metres from home (up is a negative `d`). Only `ofs-client` converts to Godot's frame (`frames::vec_to_godot`); GDScript does no NED arithmetic.
- The ELRS model runs once per packet: `radio.packet_rate_hz` must be one of 50, 150, 250, 500 and divide `sim.base_hz`. `radio.tx_power_mw` is one of 10, 25, 50, 100, 250, 500, 1000.
- Every text file ends with a newline. Never stage `.superpowers/`, `graphify-out/`, `shots/` or `build/`.
- Commit messages end with the trailer `Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>`.
- Never use `pkill -f` (use `pkill -x betaflight_SITL`); never run `wsl --shutdown`; do not touch Windows Firewall settings (ask the user).
- This plan downloads nothing.
- Tasks 1 and 3 change the dependency graph (a new crate; `ofs-radio` gains `glam` and `ofs-rf`): run their first cargo command without `--locked` so `Cargo.lock` updates, and commit `Cargo.lock` with the task.
- Files that contain backslashes must be written with the editor tools (Write/Edit), not Bash heredocs (heredocs collapse `\\`).
- Every edit below is "In `file`, replace: ... with: ...": the old text appears exactly once in the file at that point of the plan. Apply the edits of a step in the order given. "Create `file` with:" gives a new file's whole content.

## Rulings and facts verified in advance

Every task of this plan was written as code in a scratch copy of the repository (at ab92d59) and run there, including against real Betaflight SITL, before the plan was written; the code blocks below are that code. A script then replayed the plan, task by task, onto a fresh copy and checked that each task reproduces the tested tree exactly and that its tests pass. Observed, not assumed:

- **Results of the verified tree.** Windows: 383 Rust tests (workspace) and clippy with `-D warnings` clean; 38 Python tests (13 of them against Betaflight SITL); 296 Godot unit checks, the open-loop and errors end-to-end runs and the game flying real Betaflight. Linux (WSL): the workspace plus the live Rust SITL tests (`sitl_live`, `vtx_live`).
- **The ELRS figures are ExpressLRS's own.** From its source (master 15c78990e7, `src/src/common.cpp`, `src/include/common.h`, `src/lib/POWERMGNT/POWERMGNT.cpp`): SX1280 sensitivities -105/-108/-112/-115 dBm at 500/250/150/50 Hz, `rf_Mode` = the rate enum (29/27/24/21), `uplink_TX_Power` = Crossfire's power index (10 mW 1, 25 mW 2, 50 mW 8, 100 mW 3, 250 mW 7, 500 mW 4, 1000 mW 5). **Ruling:** the 50 Hz sensitivity is -115 dBm (the spec said -117).
- **Link numbers in the shipped world** (no fading, quad level, the handset beside the pad): at 250 mW the link holds 300 m behind building B at 5 m altitude (-99 dBm, 0.3 % loss) and fails there at 1.5 m (-108 dBm, 96 % loss, the ground bounce); at 10 mW it fails 150 m behind B. The bare budget (dipoles at peak, no bounce, no fading): 250 mW at 500 Hz reaches its sensitivity at 43.6 km, 10 mW at 8.7 km.
- **Betaflight SITL ignores LINK_STATISTICS.** SITL compiles `USE_CRSF_LINK_STATISTICS` only together with CRSF telemetry, which it leaves out (M0, `docs/research/sitl-interface.md` §5), so its OSD link-quality cell reads `0` whatever the link does (seen live). **Ruling:** the live test checks what Betaflight does see: near the pilot it arms; 50 km out at 10 mW no RC frame arrives and it will not arm (seen live: motors stay at 0, the OSD says `DISARMED`). The OSD link-quality element is carried to M3d.
- **Ruling: the body sphere is 2.5 cm** (spec: 4 cm). A 4 cm sphere at the centre reaches below the 3 cm feet, so the quad rested on it instead of its legs (seen in `a_quad_rests_still_on_a_roof_and_on_the_launch_pad`). The default spheres leave no gap a 12 cm post fits through.
- **Ruling: collision shapes standing on the ground are rooted below it** (`Shape::rooted`, as the radio obstacles are). Without it a quad starting inside a building near its bottom was pushed down into the ground and stayed wedged (`a_quad_that_starts_inside_a_building_is_pushed_out_and_stays_finite`).
- **Ruling: the launch pad becomes a 5 mm mat** (it was 2 cm). The quad starts on the ground at home, so its feet start inside the (rooted) pad and step up onto it; 5 mm keeps that gentle and below the collision-event speed.
- **Ruling: collisions travel as a `Collision` message** (`count`, `speed_mps`, `object`) in `State.collision` (spec: one `State.collision_speed_mps`), and the bus gains `body.collisions` so several collisions between two samples make one event that says how many ("ground at 3.1 m/s (2 collisions)"). `object` is empty before the first collision.
- **A hard landing is several collisions.** From 3 m (7.6 m/s) the legs compress past the prop tips, which strike, and the quad bounces back at 1.8 m/s: a second collision (seen live in the scratch run; `a_hard_drop_strikes_the_props_and_bounces_but_ends_on_the_roof`). The single-collision tests drop from 15 cm.
- **Ruling: open-loop sessions cannot move the quad 50 km**, so the "link down at 50 km" tests start there: the link never comes up (no `link_down` event, `link_up` false, RSSI -130 dBm).
- **The open-loop e2e drone falls back onto the launch pad** it climbed from (toast "Hit LaunchPad at 21.3 m/s" in the scratch run), so the e2e checks for the pad, not for a ground "hard landing".
- **Ruling: 150 Hz does not divide 8 kHz**, so the shipped quad cannot use it; the config refuses it with the divide message.
- **Two flaky tests predate M3c; do not chase them in this plan.** (1) The SITL Python tests sometimes fail with `FirmwareCrashed: Betaflight SITL could not bind its ports (a stale SITL is probably still running ...)`: `SitlProcess::launch` runs `pkill -x betaflight_SITL` and starts the next SITL without waiting for the old one to exit (`crates/ofs-fc/src/sitl/process.rs`). M3c adds two SITL tests, so it shows up more often (2 of 6 full runs in the scratch check, each time a different test; rerun the suite). (2) `runner::tests::a_panicking_step_poisons_the_session_and_keeps_the_runner_alive` can fail on a loaded machine (an overrun event arrives before the panic's event); it passed 5 runs in a row when rerun alone.

## How to run things (Windows host, Git Bash)

- Rust tests: `cargo test -p <crate> --locked` from the repository root; the whole workspace: `cargo test --workspace --locked`; lints: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Live SITL tests run in WSL (Windows Firewall blocks freshly built test executables that must receive SITL datagrams). Define once per shell:

```bash
wsl_run() { MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu -e bash -lc "cd /mnt$(pwd) && export CARGO_TARGET_DIR=\$HOME/ofs/target OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf && $*"; }
```

  Example: `wsl_run ~/.cargo/bin/cargo test --locked -p ofs-fc -p ofs-sim -p ofs-video --test sitl_live --test vtx_live -- --ignored --test-threads=1`.
- Python tests run on Windows: `cargo build -p ofs-sim` then `python -m pytest python/tests -q`. The SITL ones need `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf"` and an `ofs-sim.exe` the firewall allows (see `docs/dev-setup.md`, "Troubleshooting (Windows)"; `OFS_SIM_BIN` points at it).
- Godot tests: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh [unit|e2e|all]` (it builds `ofs-sim` and `ofs-godot` into `target/` first).
- Regenerate the Python stubs after a proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`.

## Review Focus

Failure modes the spec implies but a straight reading of the tasks would not cover; each has a pinning test in the named task.

1. **A quad file still on schema 3** (or with the removed `[radio]` fields): one load error naming every removed and added field, nothing loaded. Tests: Task 4 (`a_schema_3_quad_is_refused_with_what_changed`, `the_schema_1_hint_reads_as_one_sentence`).
2. **A quad that starts inside an object** (a bad start position, a world edited under a parked quad): pushed out through a side or the top, never wedged in the ground, every value finite. Test: Task 2 (`a_quad_that_starts_inside_a_building_is_pushed_out_and_stays_finite`).
3. **Thin objects at racing speed** (a 12 cm gate post at 30 m/s, at every lateral offset): no sphere ever more than 2 cm inside. Test: Task 2 (`a_quad_at_30_mps_never_passes_through_a_12_cm_post`).
4. **Resting on objects** (the pad, a roof) for a long time: no drift, no jitter, no phantom collision events. Tests: Task 2 (`a_quad_rests_still_on_a_roof_and_on_the_launch_pad`), Task 4 (`nothing_has_been_hit_at_the_start`).
5. **The quad at odd places for the radio** (at the handset, inside a building, below the ground in a crash, 1000 km out): every link value finite (the simulator stops on any non-finite signal). Test: Task 3 (`odd_positions_give_finite_values`).

## File Structure

Create:
- `crates/ofs-core/src/shape.rs` — the world's solids (`Shape`), signed distance, rooting, normals, bounds.
- `crates/ofs-rf/` (new crate) — `propagation.rs` (moved from `ofs-video`), `fading.rs`, `diversity.rs`, `shadow.rs`, and their tests.
- `crates/ofs-physics/src/collision.rs` — contacts, the broad phase, collision events; `crates/ofs-physics/tests/collision.rs`.
- `python/tests/test_sitl_link.py`.
- `docs/research/elrs-link.md`, `docs/research/collision.md`, `docs/superpowers/m3c-carried-debt.md`.

Modify: `Cargo.toml`, `Cargo.lock`, `crates/ofs-core/src/{lib,names}.rs`, `crates/ofs-video/{Cargo.toml,src/lib.rs,src/link.rs,tests/link.rs}`, `crates/ofs-physics/src/{lib,rigid_body}.rs`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-radio/{Cargo.toml,src/elrs.rs,tests/elrs.rs}`, `crates/ofs-config/src/{lib,world}.rs`, `crates/ofs-config/tests/{config,world}.rs`, `quads/opendrone-5f-freestyle.toml`, `worlds/flat.toml`, `crates/ofs-sim/{Cargo.toml,src/vehicle.rs,src/session.rs,tests/open_loop_vehicle.rs,tests/grpc.rs}`, `proto/ofs/v1/sim.proto`, `crates/ofs-proto/{src/lib.rs,tests/messages.rs}`, `python/ofs/{client.py,__init__.py}` and the regenerated `python/ofs/v1/*`, `python/tests/test_client.py`, `crates/ofs-client/{src/model.rs,tests/model.rs}`, `crates/ofs-godot/src/lib.rs`, `godot/{scripts/app.gd,ui/hud.gd,world/world.gd}`, `godot/tests/{test_hud,test_scene,e2e_open_loop}.gd`, `README.md`, `docs/dev-setup.md`, `docs/research/video-link.md`, `docs/superpowers/{m3a,m3b}-carried-debt.md`.

---

### Task 1: Shared RF code in `ofs-rf`, shapes in `ofs-core`

A behaviour-neutral move: `ofs-video::propagation` becomes `ofs-rf::propagation` (without `Shape`, which moves to `ofs-core::shape` and gains `normal` and `bounds`), and the Rician fader, the diversity switch and the frame's shadow leave `ofs-video::link` for `ofs-rf::{fading, diversity, shadow}`. Every M3b test keeps passing; the diversity and shadow tests move with their code. The fader's draws are unchanged, so lockstep runs stay bit-identical.

**Files:**
- Move: `crates/ofs-video/src/propagation.rs` -> `crates/ofs-rf/src/propagation.rs`
- Move: `crates/ofs-video/tests/propagation.rs` -> `crates/ofs-rf/tests/propagation.rs`
- Create: `crates/ofs-core/tests/shape.rs`
- Create: `crates/ofs-rf/tests/receiver.rs`
- Create: `crates/ofs-core/src/shape.rs`
- Create: `crates/ofs-rf/Cargo.toml`
- Create: `crates/ofs-rf/src/diversity.rs`
- Create: `crates/ofs-rf/src/fading.rs`
- Create: `crates/ofs-rf/src/lib.rs`
- Create: `crates/ofs-rf/src/shadow.rs`
- Modify: `crates/ofs-video/tests/link.rs`
- Modify: `Cargo.toml`
- Modify: `crates/ofs-core/src/lib.rs`
- Modify: `crates/ofs-sim/Cargo.toml`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Modify: `crates/ofs-video/Cargo.toml`
- Modify: `crates/ofs-video/src/lib.rs`
- Modify: `crates/ofs-video/src/link.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: nothing new.
- Produces: `ofs_core::shape::Shape` (`Box { center, half }`, `Cylinder { center, radius, half_height }`) with `rooted(self) -> Shape`, `signed_distance(&self, DVec3) -> f64`, `normal(&self, DVec3) -> DVec3` (unit, outward), `bounds(&self) -> (DVec3, DVec3)`; `ofs_rf::propagation::*` (unchanged API, re-exports `Shape`); `ofs_rf::fading::{Fader, LOS_K_DB, MIN_K_DB, correlation(moved_m, wavelength_m) -> f64}` with `Fader::started(&mut ChaCha8Rng) -> Fader` and `Fader::next(&mut self, rng, rho, k_db) -> f64`; `ofs_rf::diversity::{Diversity, DIVERSITY_HYSTERESIS_DB}` with `choose(&mut self, &[f64]) -> usize`; `ofs_rf::shadow::{body_shadow_db(att, quad_pos, other_pos) -> f64, BODY_SHADOW_DB}`.

- [ ] **Step 1: Move the files**

Move with git:

```bash
mkdir -p crates/ofs-rf/src crates/ofs-rf/tests
git mv crates/ofs-video/src/propagation.rs crates/ofs-rf/src/propagation.rs
git mv crates/ofs-video/tests/propagation.rs crates/ofs-rf/tests/propagation.rs
```

- [ ] **Step 2: Write the failing tests**

Create `crates/ofs-core/tests/shape.rs` with:

```rust
use glam::DVec3;
use ofs_core::shape::Shape;

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
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

/// The signed distance's gradient by central differences.
fn numeric_normal(shape: &Shape, p: DVec3) -> DVec3 {
    let h = 1e-6;
    let d = |v: DVec3| shape.signed_distance(p + v * h) - shape.signed_distance(p - v * h);
    DVec3::new(d(DVec3::X), d(DVec3::Y), d(DVec3::Z)).normalize()
}

#[test]
fn the_normal_is_the_unit_gradient_of_the_signed_distance() {
    let b = Shape::Box { center: DVec3::new(1.0, 2.0, -3.0), half: DVec3::new(2.0, 1.0, 3.0) };
    let c = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -1.5), radius: 0.15, half_height: 1.5 };
    let points = [
        (b, DVec3::new(5.0, 2.1, -2.0), "beside a box face"),
        (b, DVec3::new(4.0, 4.0, -3.0), "off a vertical box edge"),
        (b, DVec3::new(4.0, 4.0, -7.0), "off a box corner"),
        (b, DVec3::new(2.5, 2.2, -3.1), "inside a box, nearest the north face"),
        (b, DVec3::new(1.1, 2.0, -5.5), "inside a box, nearest the top"),
        (c, DVec3::new(0.3, 0.4, -1.0), "beside a cylinder"),
        (c, DVec3::new(0.05, 0.0, -3.5), "above a cylinder"),
        (c, DVec3::new(0.5, 0.0, -3.5), "off a cylinder's rim"),
        (c, DVec3::new(0.0, 0.1, -1.0), "inside a cylinder, nearest the side"),
        (c, DVec3::new(0.0, 0.01, -2.95), "inside a cylinder, nearest the top"),
    ];
    for (shape, p, what) in points {
        let n = shape.normal(p);
        close(n.length(), 1.0, 1e-12, what);
        assert!(n.distance(numeric_normal(&shape, p)) < 1e-6, "{what}: {n} vs {}", numeric_normal(&shape, p));
    }
}

#[test]
fn the_normal_has_a_direction_at_the_centre_and_on_the_axis() {
    let b = Shape::Box { center: DVec3::ZERO, half: DVec3::ONE };
    assert_eq!(b.normal(DVec3::ZERO), DVec3::X, "a tie goes to north");
    let c = Shape::Cylinder { center: DVec3::ZERO, radius: 1.0, half_height: 5.0 };
    assert_eq!(c.normal(DVec3::ZERO), DVec3::X, "on the axis, the side nearest");
    assert_eq!(c.normal(DVec3::new(0.0, 0.0, -10.0)), DVec3::NEG_Z, "straight above: up");
}

#[test]
fn the_bounds_contain_the_shape() {
    let b = Shape::Box { center: DVec3::new(1.0, 2.0, -3.0), half: DVec3::new(2.0, 1.0, 3.0) };
    assert_eq!(b.bounds(), (DVec3::new(-1.0, 1.0, -6.0), DVec3::new(3.0, 3.0, 0.0)));
    let c = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -1.5), radius: 0.15, half_height: 1.5 };
    assert_eq!(c.bounds(), (DVec3::new(-0.15, -0.15, -3.0), DVec3::new(0.15, 0.15, 0.0)));
}
```

In `crates/ofs-rf/tests/propagation.rs`, replace:

```rust
use ofs_video::propagation::*;
```

with:

```rust
use ofs_rf::propagation::*;
```

In `crates/ofs-rf/tests/propagation.rs`, replace:

```rust
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
```

with:

```rust
    close(knife_edge_loss_db(2.4), 20.6, 0.1, "v = 2.4");
```

Create `crates/ofs-rf/tests/receiver.rs` with:

```rust
use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_rf::diversity::Diversity;
use ofs_rf::fading::{correlation, Fader, LOS_K_DB};
use ofs_rf::shadow::{body_shadow_db, BODY_SHADOW_DB};

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

#[test]
fn a_fader_that_does_not_move_keeps_its_fade_and_one_that_moves_far_forgets_it() {
    let mut rng = model_rng(7, "test");
    let mut f = Fader::started(&mut rng);
    let first = f.next(&mut rng, 1.0, LOS_K_DB);
    for _ in 0..10 {
        close(f.next(&mut rng, 1.0, LOS_K_DB), first, 1e-12, "standing still");
    }
    close(correlation(0.0, 0.123), 1.0, 1e-12, "no move: full correlation");
    assert!(correlation(1.0, 0.123) < 1e-6, "a metre is many correlation lengths");
}
```

In `crates/ofs-video/tests/link.rs`, replace:

```rust
use ofs_video::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

with:

```rust
use ofs_rf::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

In `crates/ofs-video/tests/link.rs`, replace:

```rust
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
```

with:

```rust
    assert_eq!(VideoSync::from_signal(0.0), VideoSync::Locked);
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p ofs-core -p ofs-rf -p ofs-video`
Expected: FAIL: `ofs-rf` is not a workspace package yet (or, once it is, unresolved imports `ofs_core::shape`, `ofs_rf::fading`).

- [ ] **Step 4: Implement**

In `Cargo.toml`, replace:

```toml
ofs-radio = { path = "crates/ofs-radio" }
```

with:

```toml
ofs-radio = { path = "crates/ofs-radio" }
ofs-rf = { path = "crates/ofs-rf" }
```

In `crates/ofs-core/src/lib.rs`, replace:

```rust
pub mod scheduler;
```

with:

```rust
pub mod scheduler;
pub mod shape;
```

Create `crates/ofs-core/src/shape.rs` with:

```rust
//! Solids in the world (the world file's objects), by their signed distance: the video and control links use them
//! for line of sight, the rigid body for collisions. Positions are NED metres (down is +z, the ground is the plane
//! z = 0).
use glam::DVec3;

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

/// The sign of `x`, with 0 counted as positive (so a point on a symmetry plane still gets a direction).
fn sign(x: f64) -> f64 {
    if x < 0.0 {
        -1.0
    } else {
        1.0
    }
}

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

    /// The outward unit normal at `p`: the direction in which the signed distance grows fastest. Outside, it
    /// points from the nearest surface point to `p`; inside, out through the nearest face (ties go to north, then
    /// east, then down, for a box; to the side, for a cylinder).
    pub fn normal(&self, p: DVec3) -> DVec3 {
        match *self {
            Shape::Box { center, half } => {
                let d = p - center;
                let q = d.abs() - half;
                let s = DVec3::new(sign(d.x), sign(d.y), sign(d.z));
                let outside = q.max(DVec3::ZERO);
                if outside.length_squared() > 0.0 {
                    return (outside * s).normalize();
                }
                if q.x >= q.y && q.x >= q.z {
                    DVec3::new(s.x, 0.0, 0.0)
                } else if q.y >= q.z {
                    DVec3::new(0.0, s.y, 0.0)
                } else {
                    DVec3::new(0.0, 0.0, s.z)
                }
            }
            Shape::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let r = (d.x * d.x + d.y * d.y).sqrt();
                let side = if r > 0.0 { DVec3::new(d.x / r, d.y / r, 0.0) } else { DVec3::X };
                let end = DVec3::new(0.0, 0.0, sign(d.z));
                let radial = r - radius;
                let vertical = d.z.abs() - half_height;
                match (radial > 0.0, vertical > 0.0) {
                    (true, true) => (side * radial + end * vertical).normalize(),
                    (true, false) => side,
                    (false, true) => end,
                    (false, false) if radial >= vertical => side,
                    (false, false) => end,
                }
            }
        }
    }

    /// The smallest axis-aligned box around the shape, as its lowest and highest corners.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        match *self {
            Shape::Box { center, half } => (center - half, center + half),
            Shape::Cylinder { center, radius, half_height } => {
                let half = DVec3::new(radius, radius, half_height);
                (center - half, center + half)
            }
        }
    }
}
```

Create `crates/ofs-rf/Cargo.toml` with:

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
rand_chacha.workspace = true
rand_distr.workspace = true
```

Create `crates/ofs-rf/src/diversity.rs` with:

```rust
//! Receiver antenna diversity.

/// The receiver changes antenna only when another one is better by this much.
pub const DIVERSITY_HYSTERESIS_DB: f64 = 2.0;

/// Diversity: the antenna with the best value (SNR or RSSI), changed only for a 2 dB better one.
#[derive(Debug, Clone, Default)]
pub struct Diversity {
    active: usize,
}

impl Diversity {
    /// The antenna to use, given each antenna's value in dB (at least one).
    pub fn choose(&mut self, db: &[f64]) -> usize {
        self.active = self.active.min(db.len().saturating_sub(1));
        let best = (0..db.len()).max_by(|a, b| db[*a].total_cmp(&db[*b])).unwrap_or(0);
        if db[best] > db[self.active] + DIVERSITY_HYSTERESIS_DB {
            self.active = best;
        }
        self.active
    }
}
```

Create `crates/ofs-rf/src/fading.rs` with:

```rust
//! Rician fading: the line of sight plus a scatter that changes as the transmitter moves.
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

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
    /// The fade in dB for this sample. `rho` is the correlation with the previous sample (1 = nothing moved); every
    /// call draws two numbers from `rng`, whatever the inputs.
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

    /// A fader with a fresh scatter state, as for the first sample.
    pub fn started(rng: &mut ChaCha8Rng) -> Fader {
        let mut f = Fader::default();
        f.next(rng, 0.0, LOS_K_DB);
        f
    }
}

/// The correlation of the scatter between two samples `moved_m` apart, for a correlation length of half a
/// wavelength.
pub fn correlation(moved_m: f64, wavelength_m: f64) -> f64 {
    (-moved_m / (wavelength_m * 0.5)).exp()
}
```

Create `crates/ofs-rf/src/lib.rs` with:

```rust
//! Radio frequency maths shared by the links: propagation (path loss, antennas, polarization, obstruction, ground
//! bounce), Rician fading, receiver diversity and the shadow of the quad's own frame. The analog video link
//! (`ofs-video`) and the ExpressLRS control link (`ofs-radio`) build on it.
pub mod diversity;
pub mod fading;
pub mod propagation;
pub mod shadow;
```

In `crates/ofs-rf/src/propagation.rs`, replace:

```rust
//! diffraction around obstacles and the two-ray ground bounce. No bus and no state, so the video link uses them
//! today and the ELRS link can adopt them later. Positions and directions are NED metres (down is +z, the ground
//! is the plane z = 0).
use glam::DVec3;
```

with:

```rust
//! diffraction around obstacles and the two-ray ground bounce. No bus and no state: the video link and the ELRS link
//! both use them. Positions and directions are NED metres (down is +z, the ground is the plane z = 0).
use glam::DVec3;
pub use ofs_core::shape::Shape;
```

In `crates/ofs-rf/src/propagation.rs`, replace:

```rust

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

```

with:

```rust

```

Create `crates/ofs-rf/src/shadow.rs` with:

```rust
//! The shadow of the quad's own frame, battery and stack on its antennas.
use glam::{DQuat, DVec3};

/// The most the frame, battery and stack take from a signal when they sit between the quad's antenna and the
/// other end.
pub const BODY_SHADOW_DB: f64 = 8.0;
/// The body direction (FRD, unit) the frame shadows most: forward and down, through the stack and the battery.
const SHADOW_DIRECTION: DVec3 = DVec3::new(0.6, 0.0, 0.8);

/// How much of the frame, battery and stack lies between the quad's antenna and the other end of the path, as a
/// loss in dB: up to `BODY_SHADOW_DB` when the other end is forward and below the quad, nothing when it is behind
/// or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, other_pos: DVec3) -> f64 {
    let Some(dir_world) = (other_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
}
```

In `crates/ofs-sim/Cargo.toml`, replace:

```toml
ofs-video.workspace = true
```

with:

```toml
ofs-video.workspace = true
ofs-rf.workspace = true
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_video::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

with:

```rust
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

In `crates/ofs-video/Cargo.toml`, replace:

```toml
rand.workspace = true
rand_chacha.workspace = true
rand_distr.workspace = true
```

with:

```toml
ofs-rf.workspace = true
rand_chacha.workspace = true
```

In `crates/ofs-video/src/lib.rs`, replace:

```rust
pub mod osd;
pub mod propagation;
```

with:

```rust
pub mod osd;
```

In `crates/ofs-video/src/link.rs`, replace:

```rust
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

use crate::propagation::{
```

with:

```rust
use ofs_rf::diversity::Diversity;
use ofs_rf::fading::{correlation, Fader, LOS_K_DB};
use ofs_rf::propagation::{
```

In `crates/ofs-video/src/link.rs`, replace:

```rust
};

```

with:

```rust
};
use ofs_rf::shadow::body_shadow_db;
use rand_chacha::ChaCha8Rng;

```

In `crates/ofs-video/src/link.rs`, replace:

```rust
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

```

with:

```rust
pub const NO_SIGNAL_DBM: f64 = -150.0;
```

In `crates/ofs-video/src/link.rs`, replace:

```rust

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

```

with:

```rust

```

In `crates/ofs-video/src/link.rs`, replace:

```rust
    signal_dbm - power_sum_dbm([noise_floor_dbm, interference_dbm])
}

/// How much of the frame, battery and stack lies between the VTX antenna and the receiver, as a loss in dB:
/// up to `BODY_SHADOW_DB` when the receiver is forward and below the quad, nothing when it is behind or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, receiver_pos: DVec3) -> f64 {
    let Some(dir_world) = (receiver_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
```

with:

```rust
    signal_dbm - power_sum_dbm([noise_floor_dbm, interference_dbm])
```

In `crates/ofs-video/src/link.rs`, replace:

```rust

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

```

with:

```rust

```

In `crates/ofs-video/src/link.rs`, replace:

```rust
            .map(|_| {
                let mut f = Fader::default();
                f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first field
                f
            })
```

with:

```rust
            .map(|_| Fader::started(&mut rng)) // a fresh scatter state for the first field
```

In `crates/ofs-video/src/link.rs`, replace:

```rust
            let rho = (-moved / (wavelength_m(freq_mhz) * 0.5)).exp();
```

with:

```rust
            let rho = correlation(moved, wavelength_m(freq_mhz));
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p ofs-core -p ofs-rf -p ofs-video -p ofs-sim`
Expected: the first run without `--locked` (it adds `ofs-rf` to `Cargo.lock`); all pass, including the 3 new shape tests and the fader test

Run: `cargo test --workspace --locked`
Expected: PASS (344 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock Cargo.toml crates/ofs-core/src/lib.rs crates/ofs-core/src/shape.rs crates/ofs-core/tests/shape.rs crates/ofs-rf/Cargo.toml crates/ofs-rf/src/diversity.rs crates/ofs-rf/src/fading.rs crates/ofs-rf/src/lib.rs crates/ofs-rf/src/propagation.rs crates/ofs-rf/src/shadow.rs crates/ofs-rf/tests/propagation.rs crates/ofs-rf/tests/receiver.rs crates/ofs-sim/Cargo.toml crates/ofs-sim/src/vehicle.rs crates/ofs-video/Cargo.toml crates/ofs-video/src/lib.rs crates/ofs-video/src/link.rs crates/ofs-video/tests/link.rs
git commit -F - <<'EOF'
refactor: shared RF code moves to the new ofs-rf crate, shapes to ofs-core::shape (with normals and bounds)

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 2: Collision with the world (`ofs-physics`)

The rigid body takes the world's solids. Its landing contact points push against the ground and every object with the `[ground]` spring-damper; new collision spheres take impulses (restitution, Coulomb friction, through the inertia tensor) against objects and the ground after each step's integration; a tracker turns contacts into collision events on the bus. Objects standing on the ground are rooted below it. The vehicle passes no objects and no spheres yet (Task 4 wires them).

**Files:**
- Create: `crates/ofs-physics/tests/collision.rs`
- Create: `crates/ofs-physics/src/collision.rs`
- Modify: `crates/ofs-physics/tests/rigid_body.rs`
- Modify: `crates/ofs-core/src/names.rs`
- Modify: `crates/ofs-physics/src/lib.rs`
- Modify: `crates/ofs-physics/src/rigid_body.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`

**Interfaces:**
- Consumes: Task 1's `ofs_core::shape::Shape` (`rooted`, `signed_distance`, `normal`, `bounds`).
- Produces: `ofs_physics::collision::{Sphere { center_frd_m, radius_m }, CollisionParams { spheres, restitution, friction_coeff }, CollisionParams::none(), World::new(Vec<Shape>), World::near(p, reach), Contact, sphere_contacts, Tracker::new(objects), Tracker::update(time_s, &[(i32, f64)]) -> Vec<(i32, f64)>, EVENT_SPEED_MPS = 1.0, REARM_S = 0.02, RESTING_SPEED_MPS = 0.2, GROUND = -1}`; `AirframeParams.collision: CollisionParams`; `RigidBody::new(params, initial, objects: Vec<Shape>, bus)`; bus signals `names::BODY_COLLISIONS` ("body.collisions", a count), `names::BODY_COLLISION_SPEED` ("body.collision_speed_mps"), `names::BODY_COLLISION_OBJECT` ("body.collision_object": the object's index, -1 the ground).

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-physics/tests/collision.rs` with:

```rust
use glam::{DQuat, DVec3};
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler};
use ofs_physics::collision::{CollisionParams, Sphere, Tracker, World, GROUND};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody};

const A: f64 = 0.08;
const PROP_RADIUS: f64 = 0.0635;

fn feet() -> Vec<DVec3> {
    vec![DVec3::new(-A, A, 0.03), DVec3::new(A, A, 0.03), DVec3::new(-A, -A, 0.03), DVec3::new(A, -A, 0.03)]
}

/// The shipped quad's default spheres: 6 of 1.5 cm on each prop's tip circle, one of 2.5 cm at the centre.
fn quad_spheres() -> Vec<Sphere> {
    let mut spheres = Vec::new();
    for motor in [DVec3::new(-A, A, 0.0), DVec3::new(A, A, 0.0), DVec3::new(-A, -A, 0.0), DVec3::new(A, -A, 0.0)] {
        for k in 0..6 {
            let angle = k as f64 * std::f64::consts::PI / 3.0;
            let offset = DVec3::new(angle.cos(), angle.sin(), 0.0) * PROP_RADIUS;
            spheres.push(Sphere { center_frd_m: motor + offset, radius_m: 0.015 });
        }
    }
    spheres.push(Sphere { center_frd_m: DVec3::ZERO, radius_m: 0.025 });
    spheres
}

fn params(contacts: Vec<DVec3>, spheres: Vec<Sphere>, restitution: f64) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts: Vec::new(),
        contact_points_frd_m: contacts,
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        collision: CollisionParams { spheres, restitution, friction_coeff: 0.5 },
    }
}

fn moving(pos: DVec3, vel: DVec3) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: vel, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO }
}

fn sim(p: AirframeParams, s: BodyState, objects: Vec<Shape>) -> Scheduler {
    let mut bus = Bus::new();
    let body = RigidBody::new(p, s, objects, &mut bus);
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

fn pos(s: &Scheduler) -> DVec3 {
    vec3(s, names::BODY_POS_NED)
}

/// A 10 m building, 20 m on a side, its roof at d = -10.
fn building() -> Shape {
    Shape::Box { center: DVec3::new(20.0, 0.0, -5.0), half: DVec3::new(10.0, 10.0, 5.0) }
}

/// A gate post: 12 cm square, 2.5 m tall, 2 m north of home.
fn post(north: f64) -> Shape {
    Shape::Box { center: DVec3::new(north, 0.0, -1.25), half: DVec3::new(0.06, 0.06, 1.25) }
}

#[test]
fn a_sphere_dropped_on_a_box_bounces_to_restitution_squared_of_its_height() {
    let restitution = 0.5;
    let ball = vec![Sphere { center_frd_m: DVec3::ZERO, radius_m: 0.1 }];
    // The sphere's bottom 1 m above the roof.
    let mut s = sim(params(Vec::new(), ball, restitution), moving(DVec3::new(20.0, 0.0, -11.1), DVec3::ZERO), vec![building()]);
    s.run_for(0.5).unwrap();
    let mut apex = f64::INFINITY;
    let mut went_up = false;
    for _ in 0..800 {
        s.run_for(0.001).unwrap();
        let p = pos(&s);
        went_up |= vec3(&s, names::BODY_VEL_NED).z < 0.0;
        if went_up {
            apex = apex.min(p.z);
        }
    }
    let height = -10.1 - apex;
    let expected = restitution * restitution;
    assert!((height - expected).abs() < 0.05 * expected, "bounced {height} m, expected {expected} m");
}

#[test]
fn a_quad_at_30_mps_never_passes_through_a_12_cm_post() {
    for lateral in [-0.2, -0.15, -0.1, -0.06, -0.03, 0.0, 0.03, 0.06, 0.1, 0.15, 0.2] {
        let start = moving(DVec3::new(0.0, lateral, -1.5), DVec3::new(30.0, 0.0, 0.0));
        let mut s = sim(params(feet(), quad_spheres(), 0.3), start, vec![post(2.0)]);
        let spheres = quad_spheres();
        let mut deepest: f64 = 0.0;
        for _ in 0..1600 {
            s.run_for(1.0 / 8000.0).unwrap();
            let att = s.bus().get(s.bus().lookup::<DQuat>(names::BODY_ATT).unwrap());
            for sphere in &spheres {
                let c = pos(&s) + att * sphere.center_frd_m;
                deepest = deepest.max(sphere.radius_m - post(2.0).signed_distance(c));
            }
        }
        assert!(deepest < 0.02, "offset {lateral}: a sphere went {deepest} m into the post");
        if lateral == 0.0 {
            assert!(pos(&s).x < 2.0, "a head-on hit bounces back, ended at {}", pos(&s));
        }
    }
}

#[test]
fn a_quad_rests_still_on_a_roof_and_on_the_launch_pad() {
    let pad = Shape::Box { center: DVec3::new(0.0, 0.0, -0.01), half: DVec3::new(1.5, 1.5, 0.01) };
    for (what, start, objects) in [
        ("roof", DVec3::new(20.0, 0.0, -10.08), vec![building()]),
        ("pad", DVec3::new(0.0, 0.0, -0.1), vec![pad]),
    ] {
        let mut s = sim(params(feet(), quad_spheres(), 0.3), moving(start, DVec3::ZERO), objects);
        s.run_for(1.0).unwrap();
        let settled = pos(&s);
        let (mut low, mut high) = (settled, settled);
        for _ in 0..900 {
            s.run_for(0.01).unwrap();
            low = low.min(pos(&s));
            high = high.max(pos(&s));
        }
        assert!((high - low).max_element() < 1e-3, "{what}: moved {} m while resting", (high - low).max_element());
        let feet_depth = settled.z + 0.03 - if what == "roof" { -10.0 } else { -0.02 };
        assert!(feet_depth.abs() < 2e-3, "{what}: the feet rest on the top ({feet_depth} m in)");
        assert_eq!(scalar(&s, names::BODY_COLLISIONS), 0.0, "{what}: a gentle landing is no collision");
    }
}

#[test]
fn a_prop_clipping_a_post_turns_the_quad() {
    // The post passes the right-hand props' tips (east of the motors, which sit 8 cm out).
    let start = moving(DVec3::new(0.0, -0.17, -1.5), DVec3::new(10.0, 0.0, 0.0));
    let mut s = sim(params(feet(), quad_spheres(), 0.3), start, vec![post(2.0)]);
    s.run_for(0.4).unwrap();
    let rate = vec3(&s, names::BODY_RATE_FRD);
    assert!(rate.z.abs() > 1.0, "a clipped prop yaws the quad: rate {rate}");
    assert_eq!(scalar(&s, names::BODY_COLLISIONS), 1.0);
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), 0.0, "the post is object 0");
}

#[test]
fn friction_stops_a_sliding_body() {
    let skids = [(-0.1, -0.1), (-0.1, 0.1), (0.1, -0.1), (0.1, 0.1)]
        .map(|(x, y)| Sphere { center_frd_m: DVec3::new(x, y, 0.0), radius_m: 0.02 })
        .to_vec();
    let mut s = sim(params(Vec::new(), skids, 0.3), moving(DVec3::new(0.0, 0.0, -0.02), DVec3::new(2.0, 0.0, 0.0)), Vec::new());
    s.run_for(1.0).unwrap();
    let v = vec3(&s, names::BODY_VEL_NED);
    assert!(v.length() < 0.01, "still sliding at {v}");
    let travelled = pos(&s).x;
    let expected = 2.0 * 2.0 / (2.0 * 0.5 * 9.80665);
    assert!((travelled - expected).abs() < 0.1 * expected, "slid {travelled} m, Coulomb friction gives {expected} m");
}

#[test]
fn a_hard_landing_is_a_ground_collision_and_a_gentle_one_is_not() {
    for (speed, events) in [(3.0, 1.0), (0.3, 0.0)] {
        let mut s = sim(params(feet(), quad_spheres(), 0.3), moving(DVec3::new(0.0, 0.0, -0.05), DVec3::new(0.0, 0.0, speed)), Vec::new());
        s.run_for(1.0).unwrap();
        assert_eq!(scalar(&s, names::BODY_COLLISIONS), events, "landing at {speed} m/s");
        if events > 0.0 {
            assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), GROUND as f64);
            let hit = scalar(&s, names::BODY_COLLISION_SPEED);
            assert!((hit - speed).abs() < 0.1, "impact speed {hit}");
        }
    }
}

#[test]
fn flying_into_a_building_is_one_collision_and_the_quad_stays_outside() {
    let start = moving(DVec3::new(0.0, 0.0, -5.0), DVec3::new(15.0, 0.0, 0.0));
    let mut s = sim(params(feet(), quad_spheres(), 0.3), start, vec![post(-50.0), building()]);
    s.run_for(0.8).unwrap(); // the wall is 0.63 s away; the fall to the ground afterwards takes another second
    assert_eq!(scalar(&s, names::BODY_COLLISIONS), 1.0);
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), 1.0, "the building is object 1");
    assert!((scalar(&s, names::BODY_COLLISION_SPEED) - 15.0).abs() < 0.5);
    assert!(building().signed_distance(pos(&s)) > 0.0, "outside the building: {}", pos(&s));
}

#[test]
fn the_same_crash_twice_gives_identical_states() {
    let run = || {
        let mut start = moving(DVec3::new(0.0, 0.3, -5.0), DVec3::new(12.0, 1.0, -2.0));
        start.rate_frd_radps = DVec3::new(3.0, -2.0, 1.0);
        let mut s = sim(params(feet(), quad_spheres(), 0.3), start, vec![building(), post(5.0)]);
        s.run_for(3.0).unwrap();
        (pos(&s), vec3(&s, names::BODY_VEL_NED), vec3(&s, names::BODY_RATE_FRD))
    };
    assert_eq!(run(), run());
}

#[test]
fn the_tracker_raises_one_event_per_contact_and_rearms_after_20_ms() {
    let dt = 1.0 / 8000.0;
    let mut t = Tracker::new(2);
    assert_eq!(t.update(0.0, &[(1, 2.0), (1, 3.0)]), vec![(1, 3.0)], "the fastest of one step's touches");
    assert!(t.update(dt, &[(1, 3.0)]).is_empty(), "still touching");
    assert!(t.update(0.01, &[(1, 3.0)]).is_empty(), "10 ms apart is the same contact");
    assert_eq!(t.update(0.04, &[(1, 3.0)]), vec![(1, 3.0)], "30 ms apart is a new one");
    assert!(t.update(1.0, &[(0, 0.5)]).is_empty(), "too slow");
    assert!(t.update(1.0 + dt, &[(0, 5.0)]).is_empty(), "a slow contact that speeds up is the same contact");
    assert_eq!(t.update(2.0, &[(GROUND, 1.0), (0, 1.5)]), vec![(GROUND, 1.0), (0, 1.5)], "in target order");
}

#[test]
fn the_broad_phase_finds_only_nearby_objects() {
    let world = World::new(vec![post(2.0), building(), post(-50.0)]);
    let near: Vec<usize> = world.near(DVec3::new(1.9, 0.0, -1.0), 0.2).map(|(i, _)| i).collect();
    assert_eq!(near, vec![0]);
    let near: Vec<usize> = world.near(DVec3::new(9.95, 0.0, -1.0), 0.2).map(|(i, _)| i).collect();
    assert_eq!(near, vec![1], "within reach of the building's face");
    assert_eq!(world.near(DVec3::new(-20.0, 0.0, -1.0), 0.2).count(), 0);
}

#[test]
fn a_quad_that_starts_inside_a_building_is_pushed_out_and_stays_finite() {
    let mut s = sim(params(feet(), quad_spheres(), 0.3), moving(DVec3::new(20.0, 3.0, -4.0), DVec3::ZERO), vec![building()]);
    s.run_for(2.0).unwrap();
    let p = pos(&s);
    let v = vec3(&s, names::BODY_VEL_NED);
    assert!(p.is_finite() && v.is_finite(), "{p} {v}");
    assert!(building().signed_distance(p) > 0.0, "outside the building: {p}");
}
```

In `crates/ofs-physics/tests/rigid_body.rs`, replace:

```rust
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
```

with:

```rust
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
use ofs_physics::collision::CollisionParams;
```

In `crates/ofs-physics/tests/rigid_body.rs`, replace:

```rust
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
```

with:

```rust
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        collision: CollisionParams::none(),
```

In `crates/ofs-physics/tests/rigid_body.rs`, replace:

```rust
    let body = RigidBody::new(p, s, &mut bus);
```

with:

```rust
    let body = RigidBody::new(p, s, Vec::new(), &mut bus);
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-physics --locked`
Expected: FAIL: unresolved import `ofs_physics::collision`; `RigidBody::new` takes 3 arguments.

- [ ] **Step 3: Implement**

In `crates/ofs-core/src/names.rs`, replace:

```rust
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";
```

with:

```rust
pub const BODY_ACCEL_NED: &str = "body.accel_ned_mps2";
/// Collisions so far: a collision sphere or a landing contact point starting to touch a world object or the ground
/// at 1 m/s or more.
pub const BODY_COLLISIONS: &str = "body.collisions";
/// Inward speed of the last collision, m/s.
pub const BODY_COLLISION_SPEED: &str = "body.collision_speed_mps";
/// What the last collision hit: the world object's index, or -1 for the ground.
pub const BODY_COLLISION_OBJECT: &str = "body.collision_object";
```

Create `crates/ofs-physics/src/collision.rs` with:

```rust
//! Contact with the world: the world file's objects (boxes, vertical cylinders) and the ground plane. The rigid body
//! uses two contact models (docs/research/collision.md):
//! - its landing contact points push against the ground and the objects with the ground's spring-damper, so the
//!   quad stands on its legs, on the pad or on a roof;
//! - its collision spheres (on the prop tips and around the body) take impulses when they hit something: pushed out,
//!   their inward speed reversed and scaled by the restitution, slowed by Coulomb friction, through the inertia
//!   tensor so an off-centre hit turns the quad.
//!
//! Positions are NED metres (down is +z, the ground is the plane z = 0).
use glam::DVec3;
use ofs_core::shape::Shape;

/// A contact that starts at this inward speed or faster is a collision event.
pub const EVENT_SPEED_MPS: f64 = 1.0;
/// A target must be out of contact this long before touching it again is a new collision.
pub const REARM_S: f64 = 0.02;
/// Below this inward speed a sphere does not bounce (restitution 0), so nothing jitters at rest.
pub const RESTING_SPEED_MPS: f64 = 0.2;
/// The target index of the ground plane (world objects count from 0).
pub const GROUND: i32 = -1;

/// A collision sphere on the airframe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub center_frd_m: DVec3,
    pub radius_m: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollisionParams {
    pub spheres: Vec<Sphere>,
    /// Share of the inward speed a sphere keeps after a hit, 0 to 1.
    pub restitution: f64,
    /// Coulomb friction of a sphere's hit.
    pub friction_coeff: f64,
}

impl CollisionParams {
    /// No collision spheres: only the landing contact points touch anything.
    pub fn none() -> Self {
        Self { spheres: Vec::new(), restitution: 0.0, friction_coeff: 0.0 }
    }
}

/// The world's objects, with their bounds kept for the broad phase.
#[derive(Debug, Clone, Default)]
pub struct World {
    shapes: Vec<Shape>,
    bounds: Vec<(DVec3, DVec3)>,
}

impl World {
    /// Objects standing on the ground are rooted below it (`Shape::rooted`), so a body inside one is pushed out
    /// through its sides or top, never down into the ground.
    pub fn new(shapes: Vec<Shape>) -> Self {
        let shapes: Vec<Shape> = shapes.into_iter().map(Shape::rooted).collect();
        let bounds = shapes.iter().map(Shape::bounds).collect();
        Self { shapes, bounds }
    }

    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// The objects whose bounds, grown by `reach`, contain `p`, with their indices, in index order.
    pub fn near(&self, p: DVec3, reach: f64) -> impl Iterator<Item = (usize, &Shape)> + '_ {
        let grow = DVec3::splat(reach);
        self.shapes
            .iter()
            .zip(&self.bounds)
            .enumerate()
            .filter(move |(_, (_, (lo, hi)))| p.cmpge(*lo - grow).all() && p.cmple(*hi + grow).all())
            .map(|(i, (shape, _))| (i, shape))
    }
}

/// A point of the body inside something: `normal` points out of the target, `depth` is how far in the point is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub target: i32,
    pub point: DVec3,
    pub normal: DVec3,
    pub depth: f64,
}

/// Where a sphere centred at `center` (world) touches the ground or the objects near it, in target order: the
/// ground first, then the objects by index.
pub fn sphere_contacts(world: &World, center: DVec3, radius: f64, reach: f64, out: &mut Vec<Contact>) {
    let ground_depth = center.z + radius;
    if ground_depth > 0.0 {
        out.push(Contact { target: GROUND, point: center + DVec3::Z * radius, normal: DVec3::NEG_Z, depth: ground_depth });
    }
    for (i, shape) in world.near(center, reach) {
        let distance = shape.signed_distance(center);
        if distance < radius {
            let normal = shape.normal(center);
            out.push(Contact { target: i as i32, point: center - normal * radius, normal, depth: radius - distance });
        }
    }
}

/// Collision events from the contacts of each step: a target becomes a collision when it starts being touched (it
/// was not touched for `REARM_S`) at `EVENT_SPEED_MPS` or faster.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    /// When each target (index 0 the ground, then the objects) was last touched.
    last_touch_s: Vec<Option<f64>>,
}

impl Tracker {
    pub fn new(objects: usize) -> Self {
        Self { last_touch_s: vec![None; objects + 1] }
    }

    /// The step at `time_s` touched these targets with these inward speeds (several entries per target allowed).
    /// Returns the collisions, as (target, speed), in target order.
    pub fn update(&mut self, time_s: f64, touches: &[(i32, f64)]) -> Vec<(i32, f64)> {
        let mut fastest: Vec<Option<f64>> = vec![None; self.last_touch_s.len()];
        for &(target, speed) in touches {
            let slot = &mut fastest[(target + 1) as usize];
            *slot = Some(slot.map_or(speed, |s: f64| s.max(speed)));
        }
        let mut events = Vec::new();
        for (slot, (last, speed)) in self.last_touch_s.iter_mut().zip(fastest).enumerate() {
            let Some(speed) = speed else { continue };
            let new = last.is_none_or(|t| time_s - t > REARM_S);
            if new && speed >= EVENT_SPEED_MPS {
                events.push((slot as i32 - 1, speed));
            }
            *last = Some(time_s);
        }
        events
    }
}
```

In `crates/ofs-physics/src/lib.rs`, replace:

```rust
//! Flight physics: 6-DOF rigid body, propellers.
```

with:

```rust
//! Flight physics: 6-DOF rigid body, propellers, contact with the world.
pub mod collision;
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper ground contact.
use glam::{DQuat, DVec3};
use ofs_core::consts::{AIR_DENSITY_KGPM3, GRAVITY_MPS2};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};
```

with:

```rust
//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper contact of the landing
//! points with the ground and the world's objects, impulse contact of the collision spheres (see `collision`).
use glam::{DQuat, DVec3};
use ofs_core::consts::{AIR_DENSITY_KGPM3, GRAVITY_MPS2};
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

use crate::collision::{sphere_contacts, CollisionParams, Contact, Tracker, World, GROUND, RESTING_SPEED_MPS};
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
    pub ground: GroundParams,
```

with:

```rust
    /// The spring-damper and friction of the landing contact points, against the ground and the world's objects.
    pub ground: GroundParams,
    pub collision: CollisionParams,
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
    s: BodyState,
```

with:

```rust
    s: BodyState,
    world: World,
    /// The farthest any contact point or sphere surface reaches from the centre of mass.
    reach: f64,
    tracker: Tracker,
    collisions: u64,
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
    accel: Signal<DVec3>,
```

with:

```rust
    accel: Signal<DVec3>,
    collision_count: Signal<f64>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
    pub fn new(p: AirframeParams, initial: BodyState, bus: &mut Bus) -> Self {
        let n = p.mounts.len();
        let body = Self {
```

with:

```rust
    /// `objects` are the world's solids as the world file gives them (those standing on the ground are rooted here);
    /// collision events name them by their index in this list.
    pub fn new(p: AirframeParams, initial: BodyState, objects: Vec<Shape>, bus: &mut Bus) -> Self {
        let n = p.mounts.len();
        let reach = p
            .contact_points_frd_m
            .iter()
            .map(|c| c.length())
            .chain(p.collision.spheres.iter().map(|s| s.center_frd_m.length() + s.radius_m))
            .fold(0.0, f64::max);
        let world = World::new(objects);
        let body = Self {
            tracker: Tracker::new(world.len()),
            world,
            reach,
            collisions: 0,
            collision_count: bus.signal(names::BODY_COLLISIONS),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
        body.publish(bus, DVec3::ZERO);
```

with:

```rust
        body.publish(bus, DVec3::ZERO);
        bus.set(body.collision_object, GROUND as f64);
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
    /// Total contact force and torque about the centre of mass, both in NED.
    fn contact(&self, s: &BodyState) -> (DVec3, DVec3) {
```

with:

```rust
    /// Total force and torque of the landing contact points about the centre of mass, both in NED. Every point
    /// inside the ground or an object adds to `touches` (target, inward speed).
    fn contact(&self, s: &BodyState, touches: &mut Vec<(i32, f64)>) -> (DVec3, DVec3) {
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            let depth = s.pos_ned_m.z + lever.z; // ground plane is z = 0, NED z points down
            if depth <= 0.0 {
                continue;
            }
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            let normal = (g.stiffness_npm * depth + g.damping_nspm * v.z).max(0.0);
            let v_t = DVec3::new(v.x, v.y, 0.0);
            let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
            let f = DVec3::new(0.0, 0.0, -normal) + friction;
            force += f;
            torque += lever.cross(f);
        }
        (force, torque)
```

with:

```rust
        let mut contacts = Vec::new();
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            contacts.clear();
            sphere_contacts(&self.world, s.pos_ned_m + lever, 0.0, 0.0, &mut contacts);
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            for contact in &contacts {
                let v_n = v.dot(contact.normal); // positive: moving out
                touches.push((contact.target, (-v_n).max(0.0)));
                let normal = (g.stiffness_npm * contact.depth - g.damping_nspm * v_n).max(0.0);
                let v_t = v - contact.normal * v_n;
                let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
                let f = contact.normal * normal + friction;
                force += f;
                torque += lever.cross(f);
            }
        }
        (force, torque)
    }

    /// The inverse world-frame inertia applied to `v` (NED).
    fn inv_inertia(&self, att: DQuat, v: DVec3) -> DVec3 {
        att * ((att.inverse() * v) / self.p.inertia_kgm2)
    }

    /// Resolves the collision spheres' contacts on the integrated state `s`, deepest first: pushes the body out
    /// along the deepest contact's normal, then applies a normal impulse (restitution) and a Coulomb friction
    /// impulse at each contact point. Every contact adds to `touches` (target, inward speed).
    fn resolve_spheres(&self, s: &mut BodyState, touches: &mut Vec<(i32, f64)>) {
        if self.p.collision.spheres.is_empty() {
            return;
        }
        let mut contacts: Vec<Contact> = Vec::new();
        for sphere in &self.p.collision.spheres {
            let center = s.pos_ned_m + s.att * sphere.center_frd_m;
            sphere_contacts(&self.world, center, sphere.radius_m, self.reach, &mut contacts);
        }
        if contacts.is_empty() {
            return;
        }
        // Deepest first; equal depths keep their (sphere, target) order, so the result is deterministic.
        contacts.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        let push = contacts[0].normal * contacts[0].depth;
        s.pos_ned_m += push;
        let m = self.p.mass_kg;
        let c = &self.p.collision;
        let mut w = s.att * s.rate_frd_radps;
        for contact in &contacts {
            let r = contact.point + push - s.pos_ned_m;
            let n = contact.normal;
            let v_n = (s.vel_ned_mps + w.cross(r)).dot(n);
            touches.push((contact.target, (-v_n).max(0.0)));
            if v_n >= 0.0 {
                continue;
            }
            let restitution = if -v_n < RESTING_SPEED_MPS { 0.0 } else { c.restitution };
            let k_n = 1.0 / m + n.dot(self.inv_inertia(s.att, r.cross(n)).cross(r));
            let j_n = -(1.0 + restitution) * v_n / k_n;
            s.vel_ned_mps += n * (j_n / m);
            w += self.inv_inertia(s.att, r.cross(n * j_n));
            let v_point = s.vel_ned_mps + w.cross(r);
            let v_t = v_point - n * v_point.dot(n);
            let speed_t = v_t.length();
            if speed_t > 1e-9 {
                let t = v_t / speed_t;
                let k_t = 1.0 / m + t.dot(self.inv_inertia(s.att, r.cross(t)).cross(r));
                let j_t = (speed_t / k_t).min(c.friction_coeff * j_n);
                s.vel_ned_mps -= t * (j_t / m);
                w -= self.inv_inertia(s.att, r.cross(t * j_t));
            }
        }
        s.rate_frd_radps = s.att.inverse() * w;
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
        let (contact_force, contact_torque) = self.contact(&s);
```

with:

```rust
        let mut touches = Vec::new();
        let (contact_force, contact_torque) = self.contact(&s, &mut touches);
```

In `crates/ofs-physics/src/rigid_body.rs`, replace:

```rust
        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        self.publish(bus, accel_ned);
```

with:

```rust
        let mut next = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        self.resolve_spheres(&mut next, &mut touches);
        // The impulses change the velocity within the step: the accelerometer feels them as a spike.
        let accel_ned = accel_ned + (next.vel_ned_mps - vel) / dt;
        self.s = next;
        self.publish(bus, accel_ned);
        for (target, speed) in self.tracker.update(ctx.time_s, &touches) {
            self.collisions += 1;
            bus.set(self.collision_count, self.collisions as f64);
            bus.set(self.collision_speed, speed);
            bus.set(self.collision_object, target as f64);
        }
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_physics::propeller::{PropParams, Propeller};
```

with:

```rust
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::collision::CollisionParams;
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        },
    };
```

with:

```rust
        },
        collision: CollisionParams::none(),
    };
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    models.push(Box::new(RigidBody::new(airframe, initial, &mut bus)));
```

with:

```rust
    models.push(Box::new(RigidBody::new(airframe, initial, Vec::new(), &mut bus)));
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ofs-physics --locked`
Expected: PASS (the 11 collision tests and the 9 existing rigid-body tests)

Run: `cargo test --workspace --locked`
Expected: PASS (355 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-core/src/names.rs crates/ofs-physics/src/collision.rs crates/ofs-physics/src/lib.rs crates/ofs-physics/src/rigid_body.rs crates/ofs-physics/tests/collision.rs crates/ofs-physics/tests/rigid_body.rs crates/ofs-sim/src/vehicle.rs
git commit -F - <<'EOF'
feat(physics): collision with the world's objects: impulse contact of collision spheres, landing points against objects, collision events

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 3: The ELRS link on the shared propagation (`ofs-radio`)

`ElrsLink` drops its Gilbert-Elliott channel and fixed RSSI/SNR. Per packet it computes the path from the handset's antenna to each quad antenna on 2440 MHz with `ofs-rf` (path loss, dipole patterns, polarization, the frame's shadow, obstruction, the ground bounce, Rician fading), picks antennas with diversity, and loses the packet with a logistic packet error rate around the packet rate's SX1280 sensitivity. LINK_STATISTICS carry the real values (per-antenna RSSI, LQ, SNR, active antenna, ExpressLRS's `rf_mode`, the CRSF power index, the downlink). The vehicle builds the link for the built-in open field until Task 4 reads the quad and world files.

**Files:**
- Modify: `crates/ofs-radio/tests/elrs.rs`
- Modify: `crates/ofs-sim/tests/open_loop_vehicle.rs`
- Modify: `crates/ofs-core/src/names.rs`
- Modify: `crates/ofs-radio/Cargo.toml`
- Modify: `crates/ofs-radio/src/elrs.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: Task 1's `ofs_rf::{propagation, fading, diversity, shadow}`.
- Produces: `ofs_radio::elrs::{PacketRate { hz, sensitivity_dbm, rf_mode }, PACKET_RATES, packet_rate(hz) -> Option<PacketRate>, crsf_power_index(mw: u32) -> u8, packet_error_rate(rssi_dbm, sensitivity_dbm) -> f64, noise_floor_dbm() -> f64, dipole(axis) -> Antenna, Handset { position, antennas }, LinkParams { rate, latency_packets, link_stats_interval_packets, tx_power_mw: u32, handset, antennas, obstacles, fading, ground_bounce }, LinkParams::open_field(rate), path_gains(&LinkParams, quad_pos, att) -> Vec<Vec<f64>>, FREQ_MHZ, RX_TX_POWER_MW, NO_SIGNAL_RSSI_DBM}`; bus signals `names::RADIO_SNR` ("radio.snr_db"), `names::RADIO_ANTENNA` ("radio.antenna"), `names::RADIO_DOWNLINK_LQ` ("radio.downlink_lq_pct").

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{Decoder, Frame};
use ofs_radio::elrs::{ElrsLink, LinkParams, NO_SIGNAL_RSSI_DBM};
```

with:

```rust
use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{Decoder, Frame, LinkStatistics};
use ofs_radio::elrs::{
    crsf_power_index, dipole, noise_floor_dbm, packet_error_rate, packet_rate, path_gains, ElrsLink, LinkParams, PacketRate,
    NO_SIGNAL_RSSI_DBM, PACKET_RATES,
};
use ofs_rf::propagation::{obstruction_db, Obstacle, Shape};
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let div = BASE_HZ / params.packet_rate_hz;
```

with:

```rust
fn rate(hz: u32) -> PacketRate {
    packet_rate(hz).unwrap()
}

/// The open field without fading and the ground bounce: the bare link budget.
fn bare(hz: u32) -> LinkParams {
    LinkParams { fading: false, ground_bounce: false, ..LinkParams::open_field(rate(hz)) }
}

/// Both dipoles upright (the quad's along FRD -z), so a level path meets both at their peak, co-polarized.
fn upright(mut p: LinkParams) -> LinkParams {
    p.antennas = vec![dipole(DVec3::NEG_Z)];
    p
}

fn rig(params: LinkParams, seed: u64, pos: DVec3) -> (Scheduler, Wire) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let div = BASE_HZ / params.rate.hz;
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
    bus.set(tx, 1.0);
```

with:

```rust
    bus.set(tx, 1.0);
    let p = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(p, pos);
    let a = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(a, DQuat::IDENTITY);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
    s.bus_mut().set(sig, v);
```

with:

```rust
    s.bus_mut().set(sig, v);
}

fn place(s: &mut Scheduler, pos: DVec3, att: DQuat) {
    let p = s.bus_mut().signal::<DVec3>(names::BODY_POS_NED);
    s.bus_mut().set(p, pos);
    let a = s.bus_mut().signal::<DQuat>(names::BODY_ATT);
    s.bus_mut().set(a, att);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust

/// Steps one radio packet (16 base ticks at 500 Hz).
```

with:

```rust

fn stats(frames: &[Frame]) -> Vec<LinkStatistics> {
    frames.iter().filter_map(|f| if let Frame::LinkStatistics(s) = f { Some(*s) } else { None }).collect()
}

/// Steps one radio packet (16 base ticks at 500 Hz).
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
```

with:

```rust
/// The received power at the quad's first antenna, without fading, for `params` and a quad at `pos`.
fn rssi_at(params: &LinkParams, pos: DVec3, att: DQuat) -> f64 {
    10.0 * f64::from(params.tx_power_mw).log10() + path_gains(params, pos, att)[0][0]
}

/// How far north (level with the handset) the bare RSSI falls to `target_dbm`.
fn range_m(params: &LinkParams, target_dbm: f64) -> f64 {
    let h = params.handset.position;
    let (mut lo, mut hi): (f64, f64) = (10.0, 1e6);
    for _ in 0..100 {
        let mid = (lo * hi).sqrt();
        if rssi_at(params, h + DVec3::new(mid, 0.0, 0.0), DQuat::IDENTITY) > target_dbm {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

const NEAR: DVec3 = DVec3::new(20.0, 0.0, -3.0);

#[test]
fn a_quad_near_the_pilot_hears_every_packet_with_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::open_field(rate(500)), 1, NEAR);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
    assert_eq!(f.iter().filter(|f| matches!(f, Frame::LinkStatistics(_))).count(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    assert_eq!(get(&s, names::RADIO_RSSI), -50.0);
    let Some(Frame::LinkStatistics(stats)) = f.iter().find(|f| matches!(f, Frame::LinkStatistics(_))) else { unreachable!() };
    assert_eq!((stats.uplink_rssi_1, stats.uplink_lq, stats.uplink_snr), (50, 100, 10));
```

with:

```rust
    let st = stats(&f);
    assert_eq!(st.len(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_DOWNLINK_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    let rssi = get(&s, names::RADIO_RSSI);
    assert!((-75.0..-30.0).contains(&rssi), "RSSI {rssi} dBm 20 m out at 250 mW");
    assert!((get(&s, names::RADIO_SNR) - (rssi - noise_floor_dbm())).abs() < 1e-9);
    let last = st[st.len() - 1];
    assert_eq!((last.uplink_lq, last.rf_mode, last.uplink_tx_power, last.active_antenna), (100, 29, 7, 0));
    assert_eq!(last.uplink_rssi_1, last.uplink_rssi_2, "one antenna reports twice");
    assert!(last.uplink_snr > 30, "SNR {}", last.uplink_snr);
    assert!(last.downlink_rssi > last.uplink_rssi_1, "the receiver's 100 mW arrive weaker than the handset's 250 mW");
}

#[test]
fn the_packet_error_rate_is_half_at_the_sensitivity_and_falls_steeply_above_it() {
    assert!((packet_error_rate(-105.0, -105.0) - 0.5).abs() < 1e-12);
    assert!(packet_error_rate(-100.0, -105.0) < 0.01);
    assert!(packet_error_rate(-110.0, -105.0) > 0.99);
}

#[test]
fn packet_rates_carry_expresslrs_sensitivities_and_rf_modes() {
    let table: Vec<(u32, f64, u8)> = PACKET_RATES.iter().map(|r| (r.hz, r.sensitivity_dbm, r.rf_mode)).collect();
    assert_eq!(table, vec![(50, -115.0, 21), (150, -112.0, 24), (250, -108.0, 27), (500, -105.0, 29)]);
    assert!(packet_rate(333).is_none());
    let indices: Vec<u8> = [10, 25, 50, 100, 250, 500, 1000].map(crsf_power_index).to_vec();
    assert_eq!(indices, vec![1, 2, 8, 3, 7, 4, 5]);
    assert!((noise_floor_dbm() - -108.9).abs() < 0.05);
}

#[test]
fn two_hundred_fifty_milliwatts_at_500_hz_reach_43_6_km_and_ten_reach_8_7_km() {
    let p = upright(bare(500));
    let range = range_m(&p, -105.0);
    assert!((range - 43_600.0).abs() < 0.05 * 43_600.0, "250 mW: {range} m");
    let weak = LinkParams { tx_power_mw: 10, ..p };
    let range = range_m(&weak, -105.0);
    assert!((range - 8_700.0).abs() < 0.05 * 8_700.0, "10 mW: {range} m");
}

#[test]
fn a_dipole_rolled_across_the_handsets_loses_20_db() {
    let p = upright(bare(500));
    let pos = p.handset.position + DVec3::new(100.0, 0.0, 0.0);
    let level = rssi_at(&p, pos, DQuat::IDENTITY);
    let rolled = rssi_at(&p, pos, DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2));
    assert!((level - rolled - 20.0).abs() < 1e-6, "level {level}, rolled {rolled}");
}

#[test]
fn a_building_between_costs_its_knife_edge_loss() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(50.0, 0.0, -11.0), half: DVec3::new(5.0, 5.0, 11.0) }.rooted(), rf_loss_db: 30.0 };
    let open = upright(bare(500));
    let walled = LinkParams { obstacles: vec![building], ..open.clone() };
    let pos = DVec3::new(100.0, 0.0, -3.0);
    let loss = rssi_at(&open, pos, DQuat::IDENTITY) - rssi_at(&walled, pos, DQuat::IDENTITY);
    let expected = obstruction_db(&[building], open.handset.position, pos, 2440.0);
    assert!(expected > 15.0 && expected <= 30.0, "knife edge {expected} dB");
    assert!((loss - expected).abs() < 1e-9, "lost {loss} dB, knife edge {expected} dB");
}

#[test]
fn a_hovering_quad_in_the_open_field_keeps_lq_100() {
    let (mut s, _uart) = rig(LinkParams::open_field(rate(500)), 3, DVec3::new(150.0, 40.0, -2.0));
    for _ in 0..10 {
        s.run_for(1.0).unwrap();
        assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    }
}

#[test]
fn far_away_at_10_mw_the_link_goes_down() {
    let p = LinkParams { tx_power_mw: 10, ..LinkParams::open_field(rate(500)) };
    let (mut s, uart) = rig(p, 1, DVec3::new(50_000.0, 0.0, -50.0));
    s.run_for(1.0).unwrap();
    assert!(rc(&frames(&uart)).is_empty());
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    assert_eq!(get(&s, names::RADIO_RSSI), NO_SIGNAL_RSSI_DBM);
}

#[test]
fn diversity_moves_to_the_antenna_that_stays_upright() {
    let mut p = upright(bare(500));
    p.antennas.push(dipole(DVec3::Y)); // a second dipole across the frame
    let (mut s, uart) = rig(p, 1, DVec3::new(300.0, 0.0, -1.2));
    s.run_for(0.5).unwrap();
    assert_eq!(get(&s, names::RADIO_ANTENNA), 0.0, "level: the upright dipole");
    place(&mut s, DVec3::new(300.0, 0.0, -1.2), DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2));
    uart.take(usize::MAX);
    s.run_for(0.5).unwrap();
    assert_eq!(get(&s, names::RADIO_ANTENNA), 1.0, "rolled 90 degrees: the other one is upright now");
    let last = *stats(&frames(&uart)).last().unwrap();
    assert_eq!(last.active_antenna, 1);
    assert!(last.uplink_rssi_2 + 15 < last.uplink_rssi_1, "antenna 2 is the strong one: {last:?}");
}

#[test]
fn the_downlink_is_weaker_than_the_uplink_at_the_edge() {
    let p = upright(bare(500));
    // 3 dB above the uplink's sensitivity: the receiver's 100 mW are 4 dB weaker, 1 dB below it.
    let edge = range_m(&p, -102.0);
    let (mut s, _uart) = rig(p.clone(), 1, p.handset.position + DVec3::new(edge, 0.0, 0.0));
    s.run_for(2.0).unwrap();
    let (up, down) = (get(&s, names::RADIO_LQ), get(&s, names::RADIO_DOWNLINK_LQ));
    assert!(up >= 85.0 && down < up - 20.0, "uplink {up} %, downlink {down} %");
}

#[test]
fn losses_at_the_sensitivity_are_seeded() {
    let p = upright(bare(500));
    let edge = p.handset.position + DVec3::new(range_m(&p, -105.0), 0.0, 0.0);
    let run = |seed| {
        let (mut s, uart) = rig(p.clone(), seed, edge);
        s.run_for(2.0).unwrap();
        (uart.take(usize::MAX), get(&s, names::RADIO_LQ))
    };
    let (a, lq) = run(1);
    assert!((30.0..=70.0).contains(&lq), "LQ {lq} at the sensitivity");
    assert_eq!(a, run(1).0, "same seed, same bytes");
    assert_ne!(a, run(2).0, "different seed, different losses");
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
```

with:

```rust
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::open_field(rate(500)), 1, NEAR);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
fn transmitter_off_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
```

with:

```rust
fn transmitter_off_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::open_field(rate(500)), 1, NEAR);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
```

with:

```rust
    let (mut s, uart) = rig(LinkParams::open_field(rate(500)), 1, NEAR);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
fn random_loss_lowers_lq_and_is_seeded() {
    let lossy = LinkParams { loss_good: 0.3, ..LinkParams::ideal(500) };
    let run = |seed| {
        let (mut s, uart) = rig(lossy.clone(), seed);
        s.run_for(2.0).unwrap();
        (uart.take(usize::MAX), get(&s, names::RADIO_LQ))
    };
    let (a, lq) = run(1);
    assert!((55.0..=85.0).contains(&lq), "LQ {lq}");
    let received = rc(&Decoder::default().push(&a)).len();
    assert!((600..=800).contains(&received), "{received} of 1000 packets");
    assert_eq!(a, run(1).0, "same seed, same bytes");
    assert_ne!(a, run(2).0, "different seed, different losses");
}

#[test]
fn a_burst_that_never_ends_loses_everything() {
    let stuck_bad = LinkParams { loss_bad: 1.0, p_good_to_bad: 1.0, p_bad_to_good: 0.0, ..LinkParams::ideal(500) };
    let (mut s, uart) = rig(stuck_bad, 1);
    s.run_for(0.5).unwrap();
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
}

#[test]
fn latency_delays_stick_changes_by_whole_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
```

with:

```rust
fn latency_delays_stick_changes_by_whole_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::open_field(rate(500)) }, 1, NEAR);
```

In `crates/ofs-radio/tests/elrs.rs`, replace:

```rust
    assert_eq!(roll, vec![992, 992, 992, 992, 992, 1792, 1792, 1792]);
}
```

with:

```rust
    assert_eq!(roll, vec![992, 992, 992, 992, 992, 1792, 1792, 1792]);
}

#[test]
fn odd_positions_give_finite_values() {
    let building = Obstacle { shape: Shape::Box { center: DVec3::new(50.0, 0.0, -11.0), half: DVec3::new(5.0, 5.0, 11.0) }.rooted(), rf_loss_db: 30.0 };
    let p = LinkParams { obstacles: vec![building], ..LinkParams::open_field(rate(500)) };
    let handset = p.handset.position;
    for pos in [handset, DVec3::new(50.0, 0.0, -5.0), DVec3::new(10.0, 0.0, 0.5), DVec3::new(1e6, 0.0, -10.0)] {
        let (mut s, _uart) = rig(p.clone(), 1, pos);
        s.run_for(0.2).unwrap();
        for name in [names::RADIO_RSSI, names::RADIO_SNR, names::RADIO_LQ, names::RADIO_DOWNLINK_LQ] {
            assert!(get(&s, name).is_finite(), "{name} at {pos}");
        }
    }
}
```

In `crates/ofs-sim/tests/open_loop_vehicle.rs`, replace:

```rust
    assert_eq!(r.rssi_dbm, -50.0, "the quad file's radio.rssi_dbm");
```

with:

```rust
    assert!((-90.0..-20.0).contains(&r.rssi_dbm), "the handset beside the quad: {} dBm", r.rssi_dbm);
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-radio`
Expected: FAIL: unresolved imports (`packet_rate`, `path_gains`, `PacketRate`, ...); the first cargo command runs without `--locked` (it adds `glam` and `ofs-rf` to `ofs-radio` in `Cargo.lock`).

- [ ] **Step 3: Implement**

In `crates/ofs-core/src/names.rs`, replace:

```rust
/// Uplink RSSI as the receiver reports it.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
```

with:

```rust
/// Uplink RSSI at the receiver's active antenna.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
/// Uplink SNR at the receiver's active antenna (LoRa decodes below the noise: it may be negative).
pub const RADIO_SNR: &str = "radio.snr_db";
/// The receiver's active antenna (index into the quad file's `[[radio.antennas]]`).
pub const RADIO_ANTENNA: &str = "radio.antenna";
/// Downlink (telemetry) link quality: percent of the last 100 packets received by the handset.
pub const RADIO_DOWNLINK_LQ: &str = "radio.downlink_lq_pct";
```

In `crates/ofs-radio/Cargo.toml`, replace:

```toml
ofs-core.workspace = true
```

with:

```toml
glam.workspace = true
ofs-core.workspace = true
ofs-rf.workspace = true
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
//! ExpressLRS link, behavioural (fidelity level 1). Every packet the handset samples the sticks; the packet
//! crosses a channel with Gilbert-Elliott burst loss; the receiver writes one CRSF RC frame per packet it
//! receives (and LINK_STATISTICS every N received packets) to the flight controller's UART. When packets
//! stop, the receiver goes silent, as ExpressLRS does by default, and Betaflight's own failsafe takes over.
use std::collections::VecDeque;

use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
```

with:

```rust
//! ExpressLRS 2.4 GHz link, behavioural (fidelity level 1). Every packet the handset samples the sticks; the
//! packet crosses the radio path from the handset to the quad's antennas (`ofs-rf`: path loss, antenna patterns,
//! polarization, the frame's shadow, obstruction by the world's objects, the ground bounce and Rician fading); the
//! receiver loses it with a probability that rises steeply as the RSSI falls to the sensitivity of the packet
//! rate. Each received packet becomes one CRSF RC frame (and LINK_STATISTICS every N received packets) on the
//! flight controller's UART. When packets stop, the receiver goes silent, as ExpressLRS does by default, and
//! Betaflight's own failsafe takes over.
//!
//! Every constant is listed with its provenance in docs/research/elrs-link.md.
use std::collections::VecDeque;

use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
use ofs_rf::diversity::Diversity;
use ofs_rf::fading::{correlation, Fader, LOS_K_DB};
use ofs_rf::propagation::{mw_to_dbm, obstruction_db, unobstructed_gain_db, wavelength_m, Antenna, AntennaKind, Endpoint, Obstacle, Polarization};
use ofs_rf::shadow::body_shadow_db;
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;
```

with:

```rust
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;
/// The middle of the 2.4 GHz ISM band; frequency hopping is not modelled.
pub const FREQ_MHZ: f64 = 2440.0;
/// Thermal noise in the LoRa bandwidth (812.5 kHz) plus a 6 dB noise figure (estimated): about -108.9 dBm.
pub fn noise_floor_dbm() -> f64 {
    -174.0 + 10.0 * 812_500f64.log10() + 6.0
}
/// How steeply the packet error rate rises around the sensitivity, in dB (estimated).
pub const PER_SLOPE_DB: f64 = 1.0;
/// The receiver's own transmit power for telemetry, the downlink (estimated).
pub const RX_TX_POWER_MW: f64 = 100.0;
/// The handset's power settings, in mW.
pub const TX_POWERS_MW: [u32; 7] = [10, 25, 50, 100, 250, 500, 1000];

/// An ExpressLRS 2.4 GHz LoRa packet rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PacketRate {
    pub hz: u32,
    /// The SX1280 receiver's sensitivity at this rate (ExpressLRS `RxSensitivity`).
    pub sensitivity_dbm: f64,
    /// ExpressLRS's rate enum, which it sends as LINK_STATISTICS `rf_mode`.
    pub rf_mode: u8,
}

/// The 2.4 GHz LoRa rates (ExpressLRS master 15c78990e7, `src/src/common.cpp` and `src/include/common.h`).
pub const PACKET_RATES: [PacketRate; 4] = [
    PacketRate { hz: 50, sensitivity_dbm: -115.0, rf_mode: 21 },
    PacketRate { hz: 150, sensitivity_dbm: -112.0, rf_mode: 24 },
    PacketRate { hz: 250, sensitivity_dbm: -108.0, rf_mode: 27 },
    PacketRate { hz: 500, sensitivity_dbm: -105.0, rf_mode: 29 },
];

pub fn packet_rate(hz: u32) -> Option<PacketRate> {
    PACKET_RATES.iter().copied().find(|r| r.hz == hz)
}

/// Crossfire's power index for a power in mW, as ExpressLRS sends it in LINK_STATISTICS (`powerToCrsfPower`).
pub fn crsf_power_index(mw: u32) -> u8 {
    match mw {
        10 => 1,
        25 => 2,
        100 => 3,
        500 => 4,
        1000 => 5,
        2000 => 6,
        250 => 7,
        50 => 8,
        _ => 0,
    }
}

/// The chance that a packet received at `rssi_dbm` is lost: 50 % at the sensitivity, a logistic curve around it.
pub fn packet_error_rate(rssi_dbm: f64, sensitivity_dbm: f64) -> f64 {
    1.0 / (1.0 + ((rssi_dbm - sensitivity_dbm) / PER_SLOPE_DB).exp())
}

/// A 2 dBi dipole, linear polarization, along `axis`.
pub fn dipole(axis: DVec3) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Linear, axis: axis.normalize() }
}

/// The pilot's radio.
#[derive(Debug, Clone, PartialEq)]
pub struct Handset {
    pub position: DVec3,
    /// Axes in NED.
    pub antennas: Vec<Antenna>,
}
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// Loss probability per packet in the good and the bad (burst) channel state.
    pub loss_good: f64,
    pub loss_bad: f64,
    /// Per-packet probability of entering and of leaving the bad state.
    pub p_good_to_bad: f64,
    pub p_bad_to_good: f64,
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    pub rf_mode: u8,
    pub tx_power: u8,
```

with:

```rust
    pub rate: PacketRate,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    pub tx_power_mw: u32,
    pub handset: Handset,
    /// The receiver's antennas on the quad, axes in the body frame (FRD); two means diversity.
    pub antennas: Vec<Antenna>,
    pub obstacles: Vec<Obstacle>,
    /// Fading and the ground bounce; tests that check the bare link budget turn them off.
    pub fading: bool,
    pub ground_bounce: bool,
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    /// A perfect link at `packet_rate_hz`: no loss, one packet of latency.
    pub fn ideal(packet_rate_hz: u32) -> Self {
        Self {
            packet_rate_hz,
            latency_packets: 1,
            loss_good: 0.0,
            loss_bad: 0.0,
            p_good_to_bad: 0.0,
            p_bad_to_good: 1.0,
            rssi_dbm: -50.0,
            snr_db: 10.0,
            link_stats_interval_packets: 50,
            rf_mode: 0,
            tx_power: 0,
        }
    }
```

with:

```rust
    /// The built-in open field: the handset at home 1.2 m up with a vertical dipole, 250 mW, one dipole on the quad
    /// leaning up and back, no objects.
    pub fn open_field(rate: PacketRate) -> Self {
        Self {
            rate,
            latency_packets: 1,
            link_stats_interval_packets: 50,
            tx_power_mw: 250,
            handset: Handset { position: DVec3::new(0.0, 0.0, -1.2), antennas: vec![dipole(DVec3::NEG_Z)] },
            antennas: vec![dipole(DVec3::new(-0.5, 0.0, -1.0))],
            obstacles: Vec::new(),
            fading: true,
            ground_bounce: true,
        }
    }
}

/// The path gain, without fading, from each handset antenna (rows) to each quad antenna (columns), in dB: antenna
/// gains, path loss, polarization, ground bounce, obstruction and the frame's shadow. Paths are reciprocal, so the
/// same numbers serve the downlink.
pub fn path_gains(params: &LinkParams, quad_pos: DVec3, att: DQuat) -> Vec<Vec<f64>> {
    let h = &params.handset;
    let obstruction = obstruction_db(&params.obstacles, h.position, quad_pos, FREQ_MHZ);
    let shadow = body_shadow_db(att, quad_pos, h.position);
    h.antennas
        .iter()
        .map(|tx| {
            let tx = Endpoint { position: h.position, antenna: *tx };
            params
                .antennas
                .iter()
                .map(|rx| {
                    let rx = Endpoint { position: quad_pos, antenna: Antenna { axis: (att * rx.axis).normalize(), ..*rx } };
                    unobstructed_gain_db(&tx, &rx, FREQ_MHZ, params.ground_bounce) - obstruction - shadow
                })
                .collect()
        })
        .collect()
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    fault_loss: Signal<f64>,
```

with:

```rust
    fault_loss: Signal<f64>,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    rssi: Signal<f64>,
```

with:

```rust
    rssi: Signal<f64>,
    snr: Signal<f64>,
    antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
}

/// The last 100 packets, received or not.
#[derive(Debug, Default)]
struct Window(VecDeque<bool>);

impl Window {
    fn push(&mut self, received: bool) -> f64 {
        self.0.push_back(received);
        if self.0.len() > LQ_WINDOW {
            self.0.pop_front();
        }
        100.0 * self.0.iter().filter(|r| **r).count() as f64 / self.0.len() as f64
    }
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    bad: bool,
    history: VecDeque<bool>,
```

with:

```rust
    /// One per quad antenna (uplink), then one for the downlink.
    faders: Vec<Fader>,
    last_pos: Option<DVec3>,
    handset_diversity: Diversity,
    diversity: Diversity,
    uplink: Window,
    downlink: Window,
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus) -> Self {
```

with:

```rust
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus) -> Self {
        assert!(!params.antennas.is_empty() && !params.handset.antennas.is_empty(), "both ends need an antenna");
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
            fault_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
```

with:

```rust
            fault_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
            rssi: bus.signal(names::RADIO_RSSI),
        };
```

with:

```rust
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
        };
        bus.set(outputs.rssi, NO_SIGNAL_RSSI_DBM);
        bus.set(outputs.snr, NO_SIGNAL_RSSI_DBM - noise_floor_dbm());
        let mut rng = model_rng(seed, MODEL_NAME);
        let faders = (0..=params.antennas.len()).map(|_| Fader::started(&mut rng)).collect();
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
            rng: model_rng(seed, MODEL_NAME),
```

with:

```rust
            rng,
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
            bad: false,
            history: VecDeque::with_capacity(LQ_WINDOW + 1),
```

with:

```rust
            faders,
            last_pos: None,
            handset_diversity: Diversity::default(),
            diversity: Diversity::default(),
            uplink: Window::default(),
            downlink: Window::default(),
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust

    fn link_quality(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        let received = self.history.iter().filter(|r| **r).count();
        100.0 * received as f64 / self.history.len() as f64
    }

    fn statistics(&self, lq: f64) -> LinkStatistics {
        let rssi = (-self.params.rssi_dbm).round().clamp(0.0, 255.0) as u8;
        let snr = self.params.snr_db.round().clamp(-128.0, 127.0) as i8;
        let lq = lq.round().clamp(0.0, 100.0) as u8;
        LinkStatistics {
            uplink_rssi_1: rssi,
            uplink_rssi_2: rssi,
            uplink_lq: lq,
            uplink_snr: snr,
            active_antenna: 0,
            rf_mode: self.params.rf_mode,
            uplink_tx_power: self.params.tx_power,
            downlink_rssi: rssi,
            downlink_lq: lq,
            downlink_snr: snr,
        }
    }
```

with:

```rust
}

/// A power in dBm as CRSF carries it: -dBm in a byte (60 means -60 dBm).
fn crsf_rssi(dbm: f64) -> u8 {
    (-dbm).round().clamp(0.0, 255.0) as u8
}

fn crsf_snr(db: f64) -> i8 {
    db.round().clamp(-128.0, 127.0) as i8
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
        // Channel: both numbers are drawn every packet, so the random stream never depends on the inputs.
        let switch: f64 = self.rng.gen();
        let draw: f64 = self.rng.gen();
        self.bad = if self.bad { switch >= self.params.p_bad_to_good } else { switch < self.params.p_good_to_bad };
        let loss = if self.bad { self.params.loss_bad } else { self.params.loss_good };
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;
        let received = transmitting && draw >= loss;

        self.history.push_back(received);
        if self.history.len() > LQ_WINDOW {
            self.history.pop_front();
        }
        let lq = self.link_quality();
        let link_up = lq > 0.0;
```

with:

```rust
        // The path. The handset transmits on its antenna with the best path to the quad, the receiver listens on
        // its antenna with the best RSSI (each switching only for a 2 dB better one).
        let pos = bus.get(self.inputs.pos);
        let att = bus.get(self.inputs.att);
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let gains = path_gains(&self.params, pos, att);
        let best_per_handset: Vec<f64> = gains.iter().map(|row| row.iter().copied().fold(f64::NEG_INFINITY, f64::max)).collect();
        let tx = self.handset_diversity.choose(&best_per_handset);
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let rho = correlation(moved, wavelength_m(FREQ_MHZ));
        let k_db = LOS_K_DB - obstruction_db(&self.params.obstacles, self.params.handset.position, pos, FREQ_MHZ);
        // Every packet draws the same numbers whatever the inputs: two per fader, then the two loss draws.
        let fades: Vec<f64> = self
            .faders
            .iter_mut()
            .map(|f| {
                let fade = f.next(&mut self.rng, rho, k_db);
                if self.params.fading {
                    fade
                } else {
                    0.0
                }
            })
            .collect();
        let up_draw: f64 = self.rng.gen();
        let down_draw: f64 = self.rng.gen();

        let tx_dbm = mw_to_dbm(f64::from(self.params.tx_power_mw));
        let rssi: Vec<f64> = gains[tx].iter().zip(&fades).map(|(g, f)| (tx_dbm + g + f).max(NO_SIGNAL_RSSI_DBM)).collect();
        let active = self.diversity.choose(&rssi);
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;
        let sensitivity = self.params.rate.sensitivity_dbm;
        let received = transmitting && up_draw >= packet_error_rate(rssi[active], sensitivity);
        let lq = self.uplink.push(received);
        let link_up = lq > 0.0;

        // Downlink (telemetry from the receiver): the same path back, its own fading.
        let down_rssi = (mw_to_dbm(RX_TX_POWER_MW) + gains[tx][active] + fades[self.params.antennas.len()]).max(NO_SIGNAL_RSSI_DBM);
        let down_received = received && down_draw >= packet_error_rate(down_rssi, sensitivity);
        let downlink_lq = self.downlink.push(down_received);
```

In `crates/ofs-radio/src/elrs.rs`, replace:

```rust
                self.uart.write(&crsf::link_statistics_frame(&self.statistics(lq)));
            }
        }
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, if link_up { self.params.rssi_dbm } else { NO_SIGNAL_RSSI_DBM });
```

with:

```rust
                let stats = LinkStatistics {
                    uplink_rssi_1: crsf_rssi(rssi[0]),
                    uplink_rssi_2: crsf_rssi(*rssi.get(1).unwrap_or(&rssi[0])),
                    uplink_lq: lq.round().clamp(0.0, 100.0) as u8,
                    uplink_snr: crsf_snr(rssi[active] - noise_floor_dbm()),
                    active_antenna: active as u8,
                    rf_mode: self.params.rate.rf_mode,
                    uplink_tx_power: crsf_power_index(self.params.tx_power_mw),
                    downlink_rssi: crsf_rssi(down_rssi),
                    downlink_lq: downlink_lq.round().clamp(0.0, 100.0) as u8,
                    downlink_snr: crsf_snr(down_rssi - noise_floor_dbm()),
                };
                self.uart.write(&crsf::link_statistics_frame(&stats));
            }
        }
        let shown = if link_up { rssi[active] } else { NO_SIGNAL_RSSI_DBM };
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, shown);
        bus.set(self.outputs.snr, shown - noise_floor_dbm());
        bus.set(self.outputs.antenna, active as f64);
        bus.set(self.outputs.downlink_lq, downlink_lq);
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_radio::elrs::{ElrsLink, LinkParams};
```

with:

```rust
use ofs_radio::elrs::{packet_rate, ElrsLink, LinkParams};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
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
```

with:

```rust
    let rate = packet_rate(r.packet_rate_hz)
        .ok_or_else(|| SimError::InvalidArgument(format!("radio.packet_rate_hz {} is not an ExpressLRS 2.4 GHz LoRa rate", r.packet_rate_hz)))?;
    let link = LinkParams {
        latency_packets: r.latency_packets,
        link_stats_interval_packets: r.link_stats_interval_packets,
        ..LinkParams::open_field(rate)
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ofs-radio --locked`
Expected: PASS (16 ELRS tests and the CRSF tests)

Run: `cargo test --workspace --locked`
Expected: PASS (364 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/ofs-core/src/names.rs crates/ofs-radio/Cargo.toml crates/ofs-radio/src/elrs.rs crates/ofs-radio/tests/elrs.rs crates/ofs-sim/src/vehicle.rs crates/ofs-sim/tests/open_loop_vehicle.rs
git commit -F - <<'EOF'
feat(radio): the ExpressLRS link on the shared propagation: RSSI, SNR and loss from geometry, packet-rate sensitivity, diversity, downlink

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 4: Quad schema 4, the world's handset, and the vehicle wired to both

The quad file moves to schema 4 (`[radio]`: `tx_power_mw`, `[[radio.antennas]]`; the fixed figures removed; `[collision]` with default spheres generated from the motors and props); a schema-3 file is refused with a message naming the changes. The world file gains an optional `[handset]` (the shipped world places it), and its launch pad becomes a 5 mm mat. The vehicle passes the world's objects and the spheres to the rigid body, builds the ELRS link from `[radio]` and the world, and reports the radio's new values and the collisions in `VehicleState`.

**Files:**
- Modify: `crates/ofs-config/tests/config.rs`
- Modify: `crates/ofs-config/tests/world.rs`
- Modify: `crates/ofs-config/src/lib.rs`
- Modify: `crates/ofs-config/src/world.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Modify: `quads/opendrone-5f-freestyle.toml`
- Modify: `worlds/flat.toml`

**Interfaces:**
- Consumes: Task 2's `RigidBody::new(.., objects, ..)`, `CollisionParams`, `Sphere`, `GROUND`, the `body.collision*` signals; Task 3's `LinkParams`, `Handset`, `packet_rate`, the `radio.*` signals.
- Produces: `ofs_config::{SCHEMA_VERSION = 4, RadioSection { .., tx_power_mw, antennas: Vec<RadioAntennaSection> }, RadioAntennaSection { name, kind, gain_dbi, polarization, mount_frd }, CollisionSection { restitution, friction_coeff, spheres_frd_m: Option<Vec<[f64; 4]>> }, QuadConfig.collision, QuadConfig::collision_spheres() -> Vec<[f64; 4]>, RADIO_PACKET_RATES_HZ, RADIO_TX_POWERS_MW}`; `ofs_config::world::{HandsetSection { position_ned_m: Option<[f64; 3]>, antennas }, HandsetAntennaSection { name, kind, gain_dbi, polarization, aim_az_deg, aim_el_deg }, WorldConfig.handset, WorldConfig::handset_position() -> [f64; 3], HANDSET_BELOW_GOGGLES_M}`; `ofs_sim::vehicle::{world_shapes(&WorldConfig) -> Vec<Shape>, rf_obstacles(&WorldConfig) -> Vec<Obstacle>, elrs_params(&RadioSection, &WorldConfig) -> Result<LinkParams, SimError>, CollisionInfo { count, speed_mps, object }, RadioState { .., snr_db, active_antenna: String, downlink_lq_pct }, VehicleState.collision}`.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    assert_eq!(cfg.schema_version, 3);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart), (500, 2));
```

with:

```rust
    assert_eq!(cfg.schema_version, 4);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart, cfg.radio.tx_power_mw), (500, 2, 250));
    let antennas: Vec<(&str, [f64; 3])> = cfg.radio.antennas.iter().map(|a| (a.name.as_str(), a.mount_frd)).collect();
    assert_eq!(antennas, vec![("rx", [-0.5, 0.0, -1.0])]);
    assert_eq!(cfg.radio.antennas[0].polarization, ofs_config::Polarization::Linear);
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 3", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
```

with:

```rust
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 4", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    assert!(err.to_string().contains("[radio]") && err.to_string().contains("betaflight.diff"), "{err}");
```

with:

```rust
    assert!(err.to_string().contains("[radio]") && err.to_string().contains("betaflight.diff"), "{err}");
}

#[test]
fn a_schema_3_quad_is_refused_with_what_changed() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 4", "schema_version = 3");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err().to_string();
    assert!(err.contains("schema_version 3"), "{err}");
    for named in ["radio.rssi_dbm", "radio.snr_db", "radio.loss_good", "radio.tx_power_mw", "[[radio.antennas]]", "[collision]"] {
        assert!(err.contains(named), "{named} missing from: {err}");
    }
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
        .replace("rssi_dbm = -50.0", "rssi_dbm = -50.0\nloss_good = 1.5");
```

with:

```rust
        .replace("tx_power_mw = 250", "tx_power_mw = 300");
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    for field in ["radio.packet_rate_hz", "radio.uart", "radio.loss_good"] {
```

with:

```rust
    for field in ["radio.packet_rate_hz", "radio.uart", "radio.tx_power_mw"] {
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust

fn schema_2_text() -> String {
    let text = quad_text().replace("schema_version = 3", "schema_version = 2");
    text.split("\n[esc_telemetry]").next().unwrap().to_string() + "\n"
}

```

with:

```rust

```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    assert_eq!(cfg.schema_version, 3);
```

with:

```rust
    assert_eq!(cfg.schema_version, 4);
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
fn schema_2_files_still_load_without_the_video_sections() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &schema_2_text())).unwrap();
    assert_eq!(cfg.schema_version, 2);
    assert!(cfg.esc_telemetry.is_none() && cfg.osd.is_none() && cfg.vtx.is_none());
}

#[test]
fn video_sections_need_schema_3() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 3", "schema_version = 2");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version = 3"), "{err}");
```

with:

```rust
fn a_quad_without_video_sections_loads() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().split("\n[esc_telemetry]").next().unwrap().to_string() + "\n";
    let cfg = load(&write_quad(dir.path(), &text)).unwrap();
    assert!(cfg.esc_telemetry.is_none() && cfg.osd.is_none() && cfg.vtx.is_none());
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    let text = quad_text().replace("schema_version = 3", "schema_version = 1");
```

with:

```rust
    let text = quad_text().replace("schema_version = 4", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err().to_string();
    assert!(!err.contains("  "), "run-on spaces in: {err}");
    let text = quad_text().replace("schema_version = 4", "schema_version = 3");
```

In `crates/ofs-config/tests/config.rs`, replace:

```rust
    assert!(!err.contains("  "), "run-on spaces in: {err}");
}
```

with:

```rust
    assert!(!err.contains("  "), "run-on spaces in: {err}");
}

const SECOND_ANTENNA: &str = "\n[[radio.antennas]]\nname = \"rx2\"\nmount_frd = [0.0, 1.0, 0.0]\n";

/// The quad text with `extra` added at the end of its first `[[radio.antennas]]` table.
fn with_radio_antenna(extra: &str) -> String {
    quad_text().replace("mount_frd = [-0.5, 0.0, -1.0]\n\n# How the quad hits", &format!("mount_frd = [-0.5, 0.0, -1.0]\n{extra}\n# How the quad hits"))
}

#[test]
fn a_second_receiver_antenna_makes_diversity() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &with_radio_antenna(SECOND_ANTENNA))).unwrap();
    let names: Vec<&str> = cfg.radio.antennas.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["rx", "rx2"]);
    assert_eq!(cfg.radio.antennas[1].gain_dbi, 2.0, "defaults: a 2 dBi linear dipole");
    assert_eq!(cfg.radio.antennas[1].polarization, ofs_config::Polarization::Linear);
}

#[test]
fn radio_antenna_problems_are_reported_together() {
    let bad = SECOND_ANTENNA.replace("rx2", "rx").replace("[0.0, 1.0, 0.0]", "[0.0, 0.0, 0.0]") + "kind = \"patch\"\nbeamwidth_deg = 60.0\n";
    let fields = invalid_fields(&with_radio_antenna(&bad.replace("beamwidth_deg = 60.0\n", "")));
    for field in ["radio.antennas[1].name", "radio.antennas[1].mount_frd", "radio.antennas[1].kind"] {
        assert!(fields.iter().any(|f| f == field), "{field} missing from {fields:?}");
    }
    let three = with_radio_antenna(&(SECOND_ANTENNA.to_string() + &SECOND_ANTENNA.replace("rx2", "rx3")));
    assert!(invalid_fields(&three).iter().any(|f| f == "radio.antennas"), "three antennas");
}

#[test]
fn a_packet_rate_must_be_an_expresslrs_rate_that_divides_the_base_rate() {
    let fields = invalid_fields(&quad_text().replace("packet_rate_hz = 500", "packet_rate_hz = 150"));
    assert_eq!(fields, vec!["radio.packet_rate_hz"], "150 Hz does not divide 8 kHz");
    let fields = invalid_fields(&quad_text().replace("packet_rate_hz = 500", "packet_rate_hz = 1000"));
    assert_eq!(fields, vec!["radio.packet_rate_hz"], "1000 Hz is not a LoRa rate");
}

#[test]
fn the_default_collision_spheres_ring_the_props_and_stay_above_the_feet() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &quad_text())).unwrap();
    assert_eq!((cfg.collision.restitution, cfg.collision.friction_coeff), (0.3, 0.5));
    let spheres = cfg.collision_spheres();
    assert_eq!(spheres.len(), 4 * 6 + 1);
    let tip = cfg.prop.diameter_m / 2.0;
    for (i, motor) in cfg.frame.motor_positions_frd_m.iter().enumerate() {
        for s in &spheres[i * 6..i * 6 + 6] {
            let r = ((s[0] - motor[0]).powi(2) + (s[1] - motor[1]).powi(2)).sqrt();
            assert!((r - tip).abs() < 1e-12 && s[2] == motor[2] && s[3] == 0.015, "{s:?} on motor {i}");
        }
    }
    assert_eq!(spheres[24], [0.0, 0.0, 0.0, 0.025]);
    let lowest_sphere = spheres.iter().map(|s| s[2] + s[3]).fold(f64::MIN, f64::max);
    let feet = cfg.frame.contact_points_frd_m.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    assert!(lowest_sphere < feet, "the quad stands on its feet: spheres reach {lowest_sphere} m down, feet {feet} m");
}

#[test]
fn explicit_collision_spheres_replace_the_default_ones() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("friction_coeff = 0.5\n", "friction_coeff = 0.5\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.1]]\n");
    let cfg = load(&write_quad(dir.path(), &text)).unwrap();
    assert_eq!(cfg.collision_spheres(), vec![[0.0, 0.0, 0.0, 0.1]]);
}

#[test]
fn collision_problems_are_reported_together() {
    let text = quad_text()
        .replace("restitution = 0.3", "restitution = 1.5")
        .replace("friction_coeff = 0.5\n", "friction_coeff = -1.0\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.0], [nan, 0.0, 0.0, 0.1]]\n");
    let fields = invalid_fields(&text);
    for field in ["collision.restitution", "collision.friction_coeff", "collision.spheres_frd_m"] {
        assert!(fields.iter().any(|f| f == field), "{field} missing from {fields:?}");
    }
}

#[test]
fn a_quad_without_a_collision_section_gets_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let start = quad_text().find("# How the quad hits").unwrap();
    let end = quad_text().find("[esc_telemetry]").unwrap();
    let text = quad_text()[..start].to_string() + &quad_text()[end..];
    let cfg = load(&write_quad(dir.path(), &text)).unwrap();
    assert_eq!((cfg.collision.restitution, cfg.collision.friction_coeff), (0.3, 0.5));
    assert_eq!(cfg.collision_spheres().len(), 25);
}
```

In `crates/ofs-config/tests/world.rs`, replace:

```rust
        .replace("shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nsize_m = [3.0, 3.0, 0.02]", "shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nradius_m = 1.0");
```

with:

```rust
        .replace("shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.0025]\nsize_m = [3.0, 3.0, 0.005]", "shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.0025]\nradius_m = 1.0");
```

In `crates/ofs-config/tests/world.rs`, replace:

```rust
    assert!(!found.iter().any(|p| p.contains("below the ground")), "a NaN height is not below the ground: {found:#?}");
}
```

with:

```rust
    assert!(!found.iter().any(|p| p.contains("below the ground")), "a NaN height is not below the ground: {found:#?}");
}

#[test]
fn the_shipped_world_has_a_handset_below_the_goggles() {
    let w = world::load(Path::new(WORLD)).unwrap();
    assert_eq!(w.handset_position(), [-3.0, 2.0, -1.2]);
    let a = &w.handset.antennas[0];
    assert_eq!((a.name.as_str(), a.kind, a.polarization, a.aim_el_deg), ("tx", AntennaKind::Omni, Polarization::Linear, 90.0));
}

#[test]
fn the_handset_defaults_to_the_pilot_half_a_metre_lower() {
    let start = world_text().find("# The pilot's radio").unwrap();
    let end = world_text().find("# Another quad's VTX").unwrap();
    let text = world_text()[..start].to_string() + &world_text()[end..];
    let dir = tempfile::tempdir().unwrap();
    let w = world::load(&write(dir.path(), &text)).unwrap();
    assert_eq!(w.handset_position(), [-3.0, 2.0, -1.2]);
    assert_eq!(w.handset.antennas.len(), 1);
    assert_eq!(w.handset.antennas[0].gain_dbi, 2.0);
    assert_eq!(WorldConfig::open_field().handset_position(), [0.0, 0.0, -1.2]);
}

#[test]
fn handset_problems_are_reported_together() {
    let text = world_text()
        .replace("position_ned_m = [-3.0, 2.0, -1.2]", "position_ned_m = [-3.0, 2.0, 1.0]")
        .replace("aim_el_deg = 90.0\n", "aim_el_deg = 90.0\n\n[[handset.antennas]]\nname = \"tx\"\nkind = \"patch\"\n");
    let found = problems(&text);
    for field in ["handset.position_ned_m", "handset.antennas[1].name", "handset.antennas[1].kind"] {
        assert!(found.iter().any(|p| p.starts_with(field)), "{field} missing from {found:?}");
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-config -p ofs-sim --locked`
Expected: FAIL: no field `tx_power_mw`/`antennas` on `RadioSection`, no `collision_spheres`, no `handset_position`, no `elrs_params`.

- [ ] **Step 3: Implement**

In `crates/ofs-config/src/lib.rs`, replace:

```rust
/// 3: adds the optional `[esc_telemetry]`, `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 3;
/// Oldest schema this build still reads (schema-2 files have no video sections).
pub const MIN_SCHEMA_VERSION: u32 = 2;
```

with:

```rust
/// 4: the ELRS link comes from the geometry: `[radio]` trades its fixed RSSI, SNR and loss figures for `tx_power_mw`
/// and `[[radio.antennas]]`; adds the optional `[collision]` section (M3c). 3: adds the optional `[esc_telemetry]`,
/// `[osd]` and `[vtx]` sections (M3a). 2: the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 4;
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
    pub radio: RadioSection,
```

with:

```rust
    pub radio: RadioSection,
    #[serde(default)]
    pub collision: CollisionSection,
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
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
```

with:

```rust
    #[serde(default = "default_link_stats_interval_packets")]
    pub link_stats_interval_packets: u32,
    /// The handset's power setting: 10, 25, 50, 100, 250, 500 or 1000 mW.
    #[serde(default = "default_tx_power_mw")]
    pub tx_power_mw: u32,
    /// The receiver's antennas on the quad: one, or two for diversity.
    #[serde(default = "default_radio_antennas")]
    pub antennas: Vec<RadioAntennaSection>,
}

/// The ExpressLRS 2.4 GHz LoRa packet rates the link models.
pub const RADIO_PACKET_RATES_HZ: [u32; 4] = [50, 150, 250, 500];
/// The handset power settings, in mW.
pub const RADIO_TX_POWERS_MW: [u32; 7] = [10, 25, 50, 100, 250, 500, 1000];

/// A receiver antenna on the quad: a dipole (`omni`), its axis fixed in the body frame.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioAntennaSection {
    pub name: String,
    #[serde(default = "default_vtx_antenna_kind")]
    pub kind: AntennaKind,
    #[serde(default = "default_vtx_antenna_gain_dbi")]
    pub gain_dbi: f64,
    #[serde(default = "default_linear")]
    pub polarization: Polarization,
    /// The dipole's axis in the body frame: FRD, x forward, y right, z down.
    #[serde(default = "default_vtx_mount_frd")]
    pub mount_frd: [f64; 3],
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust

fn default_p_bad_to_good() -> f64 {
    1.0
}

fn default_rssi_dbm() -> f64 {
    -50.0
}

fn default_snr_db() -> f64 {
    10.0
}

```

with:

```rust

```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
    50
}

```

with:

```rust
    50
}

fn default_tx_power_mw() -> u32 {
    250
}

fn default_linear() -> Polarization {
    Polarization::Linear
}

/// One 2 dBi dipole leaning up and back, as on a typical 5" quad.
fn default_radio_antennas() -> Vec<RadioAntennaSection> {
    vec![RadioAntennaSection {
        name: "rx".into(),
        kind: AntennaKind::Omni,
        gain_dbi: 2.0,
        polarization: Polarization::Linear,
        mount_frd: default_vtx_mount_frd(),
    }]
}

/// How the quad hits the world's objects (and the ground) with its collision spheres. The landing contact points
/// (`frame.contact_points_frd_m`) use `[ground]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionSection {
    /// Share of the inward speed a sphere keeps after a hit, 0 to 1.
    #[serde(default = "default_restitution")]
    pub restitution: f64,
    #[serde(default = "default_collision_friction")]
    pub friction_coeff: f64,
    /// `[x, y, z, radius]` in the body frame (FRD). Default: see `QuadConfig::collision_spheres`.
    #[serde(default)]
    pub spheres_frd_m: Option<Vec<[f64; 4]>>,
}

impl Default for CollisionSection {
    fn default() -> Self {
        Self { restitution: default_restitution(), friction_coeff: default_collision_friction(), spheres_frd_m: None }
    }
}

fn default_restitution() -> f64 {
    0.3
}

fn default_collision_friction() -> f64 {
    0.5
}

/// The default collision spheres: this many on each prop's tip circle...
pub const PROP_TIP_SPHERES: usize = 6;
/// ...of this radius...
pub const PROP_TIP_SPHERE_RADIUS_M: f64 = 0.015;
/// ...and one at the centre of mass of this radius (small enough to stay above the landing contact points).
pub const BODY_SPHERE_RADIUS_M: f64 = 0.025;

```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
        Some(v) if v >= i64::from(MIN_SCHEMA_VERSION) && v <= i64::from(SCHEMA_VERSION) => {}
        Some(v @ ..=1) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION}); schema 2 adds the required [radio] section \
                 and the CRSF receiver lines in the quad's betaflight.diff, see quads/opendrone-5f-freestyle.toml and \
```

with:

```rust
        Some(v) if v == i64::from(SCHEMA_VERSION) => {}
        Some(v @ ..=3) => {
            let message = format!(
                "unsupported schema_version {v} (this build reads {SCHEMA_VERSION}); schema 4 computes the ELRS link from the \
                 geometry, so radio.rssi_dbm, radio.snr_db, radio.loss_good, radio.loss_bad, radio.p_good_to_bad, \
                 radio.p_bad_to_good, radio.rf_mode and radio.tx_power are gone and radio.tx_power_mw and [[radio.antennas]] \
                 replace them, and it adds the optional [collision] section; schema 2 added the required [radio] section and \
                 the CRSF receiver lines in the quad's betaflight.diff; see quads/opendrone-5f-freestyle.toml and \
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {MIN_SCHEMA_VERSION} to {SCHEMA_VERSION})"))),
```

with:

```rust
        Some(v) => return Err(parse_err(format!("unsupported schema_version {v} (this build reads {SCHEMA_VERSION})"))),
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
        self.source_dir().join(rel)
```

with:

```rust
        self.source_dir().join(rel)
    }

    /// The collision spheres, `[x, y, z, radius]` in FRD: `collision.spheres_frd_m`, or by default
    /// `PROP_TIP_SPHERES` spheres evenly spaced on each prop's tip circle (centred on the motor, in its plane) and
    /// one at the centre of mass.
    pub fn collision_spheres(&self) -> Vec<[f64; 4]> {
        if let Some(spheres) = &self.collision.spheres_frd_m {
            return spheres.clone();
        }
        let tip = self.prop.diameter_m * 0.5;
        let mut spheres = Vec::new();
        for m in &self.frame.motor_positions_frd_m {
            for k in 0..PROP_TIP_SPHERES {
                let angle = k as f64 * std::f64::consts::TAU / PROP_TIP_SPHERES as f64;
                spheres.push([m[0] + tip * angle.cos(), m[1] + tip * angle.sin(), m[2], PROP_TIP_SPHERE_RADIUS_M]);
            }
        }
        spheres.push([0.0, 0.0, 0.0, BODY_SPHERE_RADIUS_M]);
        spheres
```

In `crates/ofs-config/src/lib.rs`, replace:

```rust
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
```

with:

```rust
        c.check(
            RADIO_PACKET_RATES_HZ.contains(&r.packet_rate_hz),
            "radio.packet_rate_hz",
            format!("must be one of {RADIO_PACKET_RATES_HZ:?} (the ExpressLRS 2.4 GHz LoRa rates; got {})", r.packet_rate_hz),
        );
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
        c.check(r.link_stats_interval_packets > 0, "radio.link_stats_interval_packets", "must be > 0");
        c.check(
            RADIO_TX_POWERS_MW.contains(&r.tx_power_mw),
            "radio.tx_power_mw",
            format!("must be one of {RADIO_TX_POWERS_MW:?} (got {})", r.tx_power_mw),
        );
        c.check((1..=2).contains(&r.antennas.len()), "radio.antennas", format!("needs one or two antennas (got {})", r.antennas.len()));
        let mut antenna_names = std::collections::HashSet::new();
        for (i, a) in r.antennas.iter().enumerate() {
            let field = |f: &str| format!("radio.antennas[{i}].{f}");
            world::check_name(&mut c, &a.name, &field("name"), &mut antenna_names);
            c.check(a.kind == AntennaKind::Omni, &field("kind"), "a receiver antenna is a dipole: \"omni\"");
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(
                a.mount_frd.iter().all(|x| x.is_finite()) && a.mount_frd.iter().any(|x| *x != 0.0),
                &field("mount_frd"),
                "must be a finite, non-zero direction",
            );
        }
        let k = &self.collision;
        c.check((0.0..=1.0).contains(&k.restitution), "collision.restitution", format!("must be in [0, 1] (got {})", k.restitution));
        c.non_negative(k.friction_coeff, "collision.friction_coeff");
        if let Some(spheres) = &k.spheres_frd_m {
            c.finite(spheres.iter().flatten(), "collision.spheres_frd_m");
            c.check(spheres.iter().all(|s| s[3] > 0.0), "collision.spheres_frd_m", "every radius (the 4th value) must be > 0");
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
//! World description files (TOML): where the pilot stands, the goggles' antennas, the objects on the field and other
//! transmitters. The simulator uses them for the video link; the Godot client draws the objects. Positions are NED
//! metres from home, like the quad file's (up is a negative `d`); sizes are `[north, east, height]`.
```

with:

```rust
//! World description files (TOML): where the pilot stands, the goggles' antennas, the pilot's radio, the objects on
//! the field and other transmitters. The simulator uses them for the video and control links and for collisions; the
//! Godot client draws the objects. Positions are NED metres from home, like the quad file's (up is a negative `d`);
//! sizes are `[north, east, height]`.
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
    pub receiver: ReceiverSection,
```

with:

```rust
    pub receiver: ReceiverSection,
    #[serde(default)]
    pub handset: HandsetSection,
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
        })
    }
}
```

with:

```rust
        })
    }
}

/// The pilot's radio (the ELRS handset).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsetSection {
    /// Default: the pilot's position, `HANDSET_BELOW_GOGGLES_M` lower.
    #[serde(default)]
    pub position_ned_m: Option<[f64; 3]>,
    /// One, or two for diversity. Default: one upright 2 dBi dipole.
    #[serde(default = "default_handset_antennas")]
    pub antennas: Vec<HandsetAntennaSection>,
}

impl Default for HandsetSection {
    fn default() -> Self {
        Self { position_ned_m: None, antennas: default_handset_antennas() }
    }
}

/// How far below the goggles the handset is held by default.
pub const HANDSET_BELOW_GOGGLES_M: f64 = 0.5;

/// A handset antenna: a dipole (`omni`) aimed relative to the pilot's facing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsetAntennaSection {
    pub name: String,
    #[serde(default = "default_omni")]
    pub kind: AntennaKind,
    #[serde(default = "default_dipole_gain_dbi")]
    pub gain_dbi: f64,
    #[serde(default = "default_linear")]
    pub polarization: Polarization,
    /// The dipole's axis relative to the pilot's facing: azimuth clockwise, elevation up (default upright).
    #[serde(default)]
    pub aim_az_deg: f64,
    #[serde(default = "default_upright_deg")]
    pub aim_el_deg: f64,
}

fn default_handset_antennas() -> Vec<HandsetAntennaSection> {
    vec![HandsetAntennaSection {
        name: "tx".into(),
        kind: AntennaKind::Omni,
        gain_dbi: default_dipole_gain_dbi(),
        polarization: Polarization::Linear,
        aim_az_deg: 0.0,
        aim_el_deg: default_upright_deg(),
    }]
}

fn default_omni() -> AntennaKind {
    AntennaKind::Omni
}

fn default_dipole_gain_dbi() -> f64 {
    2.0
}

fn default_linear() -> Polarization {
    Polarization::Linear
}

fn default_upright_deg() -> f64 {
    90.0
}
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
            },
```

with:

```rust
            },
            handset: HandsetSection::default(),
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
        }
    }
```

with:

```rust
        }
    }

    /// Where the handset is: `handset.position_ned_m`, or `HANDSET_BELOW_GOGGLES_M` below the pilot's goggles.
    pub fn handset_position(&self) -> [f64; 3] {
        let p = self.pilot.position_ned_m;
        self.handset.position_ned_m.unwrap_or([p[0], p[1], p[2] + HANDSET_BELOW_GOGGLES_M])
    }
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
```

with:

```rust
            check_beamwidth(&mut c, a.kind, a.beamwidth_deg, &field("beamwidth_deg"));
        }
        let h = &self.handset;
        let handset = self.handset_position();
        c.check(finite(&handset), "handset.position_ned_m", "values must be finite");
        c.check(handset[2].is_nan() || handset[2] <= 0.0, "handset.position_ned_m", format!("the handset must not be below the ground (d = {} > 0)", handset[2]));
        c.check((1..=2).contains(&h.antennas.len()), "handset.antennas", format!("needs one or two antennas (got {})", h.antennas.len()));
        let mut names = HashSet::new();
        for (i, a) in h.antennas.iter().enumerate() {
            let field = |f: &str| format!("handset.antennas[{i}].{f}");
            check_name(&mut c, &a.name, &field("name"), &mut names);
            c.check(a.kind == AntennaKind::Omni, &field("kind"), "a handset antenna is a dipole: \"omni\"");
            c.check(a.gain_dbi.is_finite(), &field("gain_dbi"), "must be finite");
            c.check(finite(&[a.aim_az_deg, a.aim_el_deg]), &field("aim_az_deg"), "aims must be finite");
```

In `crates/ofs-config/src/world.rs`, replace:

```rust
fn check_name(c: &mut Checker, name: &str, field: &str, seen: &mut HashSet<String>) {
```

with:

```rust
pub(crate) fn check_name(c: &mut Checker, name: &str, field: &str, seen: &mut HashSet<String>) {
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_config::{FcKind, QuadConfig, VtxSection};
```

with:

```rust
use ofs_config::{FcKind, QuadConfig, RadioSection, VtxSection};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::collision::CollisionParams;
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_radio::elrs::{packet_rate, ElrsLink, LinkParams};
```

with:

```rust
use ofs_physics::collision::{CollisionParams, Sphere, GROUND};
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_radio::elrs::{packet_rate, ElrsLink, Handset, LinkParams};
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization, Shape};
```

with:

```rust
use ofs_video::osd::{OsdFrame, OsdHandle, OsdModel};
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
/// What the radio receiver reports.
#[derive(Debug, Clone, Copy, PartialEq)]
```

with:

```rust
/// What the radio receiver reports.
#[derive(Debug, Clone, PartialEq)]
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    pub rssi_dbm: f64,
```

with:

```rust
    /// At the active antenna; `NO_SIGNAL_RSSI_DBM` while the link is down.
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// The receiver antenna in use (its name in the quad file).
    pub active_antenna: String,
    /// Telemetry back to the handset.
    pub downlink_lq_pct: f64,
}

/// The quad's collisions with the world's objects and the ground.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollisionInfo {
    /// Collisions so far.
    pub count: u64,
    /// The last one's inward speed (0 before the first).
    pub speed_mps: f64,
    /// What the last one hit: a world object's name, or "ground" ("" before the first).
    pub object: String,
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    pub video: VideoInfo,
```

with:

```rust
    pub video: VideoInfo,
    pub collision: CollisionInfo,
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    rssi: Signal<f64>,
```

with:

```rust
    rssi: Signal<f64>,
    snr: Signal<f64>,
    radio_antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
    /// The quad's receiver antennas, by index.
    radio_antennas: Vec<String>,
    collisions: Signal<f64>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
    /// The world's objects, by index.
    objects: Vec<String>,
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    fn register(bus: &mut Bus, motors: usize, world: &WorldConfig) -> Self {
```

with:

```rust
    fn register(bus: &mut Bus, motors: usize, radio_antennas: Vec<String>, world: &WorldConfig) -> Self {
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
            rssi: bus.signal(names::RADIO_RSSI),
```

with:

```rust
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            radio_antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
            radio_antennas,
            collisions: bus.signal(names::BODY_COLLISIONS),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
            objects: world.objects.iter().map(|o| o.name.clone()).collect(),
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust

/// The video link's view of the world file and the quad's VTX antenna.
```

with:

```rust

/// The world file's objects as solids, in the file's order (for collisions; not rooted).
pub fn world_shapes(world: &WorldConfig) -> Vec<Shape> {
    world
        .objects
        .iter()
        .map(|o| {
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
        })
        .collect()
}

/// The objects that take signal (`rf_loss_db > 0`), rooted in the ground.
pub fn rf_obstacles(world: &WorldConfig) -> Vec<Obstacle> {
    world_shapes(world)
        .into_iter()
        .zip(&world.objects)
        .filter(|(_, o)| o.rf_loss_db > 0.0)
        .map(|(shape, o)| Obstacle { shape: shape.rooted(), rf_loss_db: o.rf_loss_db })
        .collect()
}

/// The ELRS link's view of the quad file's `[radio]` and the world file's handset and objects.
pub fn elrs_params(radio: &RadioSection, world: &WorldConfig) -> Result<LinkParams, SimError> {
    let rate = packet_rate(radio.packet_rate_hz)
        .ok_or_else(|| SimError::InvalidArgument(format!("radio.packet_rate_hz {} is not an ExpressLRS 2.4 GHz LoRa rate", radio.packet_rate_hz)))?;
    let facing = world.pilot.facing_deg;
    let handset = Handset {
        position: v3(world.handset_position()),
        antennas: world
            .handset
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, None),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el_deg),
            })
            .collect(),
    };
    let antennas = radio
        .antennas
        .iter()
        .map(|a| Antenna {
            kind: antenna_kind(a.kind, None),
            gain_dbi: a.gain_dbi,
            polarization: polarization(a.polarization),
            axis: v3(a.mount_frd).normalize(),
        })
        .collect();
    Ok(LinkParams {
        rate,
        latency_packets: radio.latency_packets,
        link_stats_interval_packets: radio.link_stats_interval_packets,
        tx_power_mw: radio.tx_power_mw,
        handset,
        antennas,
        obstacles: rf_obstacles(world),
        fading: true,
        ground_bounce: true,
    })
}

/// The video link's view of the world file and the quad's VTX antenna.
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

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
    let obstacles = rf_obstacles(world);
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        collision: CollisionParams::none(),
```

with:

```rust
        collision: CollisionParams {
            spheres: cfg
                .collision_spheres()
                .iter()
                .map(|s| Sphere { center_frd_m: DVec3::new(s[0], s[1], s[2]), radius_m: s[3] })
                .collect(),
            restitution: cfg.collision.restitution,
            friction_coeff: cfg.collision.friction_coeff,
        },
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    models.push(Box::new(RigidBody::new(airframe, initial, Vec::new(), &mut bus)));
```

with:

```rust
    models.push(Box::new(RigidBody::new(airframe, initial, world_shapes(&opts.world), &mut bus)));
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    let rate = packet_rate(r.packet_rate_hz)
        .ok_or_else(|| SimError::InvalidArgument(format!("radio.packet_rate_hz {} is not an ExpressLRS 2.4 GHz LoRa rate", r.packet_rate_hz)))?;
    let link = LinkParams {
        latency_packets: r.latency_packets,
        link_stats_interval_packets: r.link_stats_interval_packets,
        ..LinkParams::open_field(rate)
    };
```

with:

```rust
    let link = elrs_params(r, &opts.world)?;
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
    let h = Handles::register(&mut bus, n, &opts.world);
```

with:

```rust
    let h = Handles::register(&mut bus, n, cfg.radio.antennas.iter().map(|a| a.name.clone()).collect(), &opts.world);
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
                rssi_dbm: b.get(h.rssi),
```

with:

```rust
                rssi_dbm: b.get(h.rssi),
                snr_db: b.get(h.snr),
                active_antenna: h.radio_antennas.get(b.get(h.radio_antenna) as usize).cloned().unwrap_or_default(),
                downlink_lq_pct: b.get(h.downlink_lq),
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
            video: h.video.read(b),
```

with:

```rust
            video: h.video.read(b),
            collision: CollisionInfo {
                count: b.get(h.collisions) as u64,
                speed_mps: b.get(h.collision_speed),
                object: match b.get(h.collision_object) as i32 {
                    _ if b.get(h.collisions) == 0.0 => String::new(),
                    GROUND => "ground".into(),
                    i => h.objects.get(i as usize).cloned().unwrap_or_default(),
                },
            },
```

In `crates/ofs-sim/src/vehicle.rs`, replace:

```rust
        assert!(open.snr_db - shadowed.snr_db > 15.0, "open {} dB, behind the building {} dB", open.snr_db, shadowed.snr_db);
    }
}
```

with:

```rust
        assert!(open.snr_db - shadowed.snr_db > 15.0, "open {} dB, behind the building {} dB", open.snr_db, shadowed.snr_db);
    }

    /// The shipped quad with its feet `height` metres above building A's roof (the roof is at d = -14).
    fn above_building_a(height: f64) -> QuadConfig {
        let mut cfg = shipped_quad();
        cfg.initial.position_ned_m = [110.0, -45.0, -14.03 - height];
        cfg
    }

    #[test]
    fn a_quad_dropped_on_building_a_lands_on_its_roof_as_one_collision() {
        // 15 cm: the legs alone take the 1.7 m/s landing; no prop touches the roof.
        let mut vehicle = open_loop(&above_building_a(0.15), flat_world());
        vehicle.run_for(2.0).unwrap();
        let s = vehicle.state();
        assert_eq!(s.collision.count, 1, "{:?}", s.collision);
        assert_eq!(s.collision.object, "BuildingA");
        let fall = (2.0 * 9.80665 * 0.15_f64).sqrt();
        assert!((s.collision.speed_mps - fall).abs() < 0.05, "hit at {} m/s, fell at {fall}", s.collision.speed_mps);
        assert!((s.pos_ned_m.z - -14.03).abs() < 2e-3, "standing on the roof: {}", s.pos_ned_m);
        assert!(s.vel_ned_mps.length() < 0.01);
    }

    #[test]
    fn a_hard_drop_strikes_the_props_and_bounces_but_ends_on_the_roof() {
        // 3 m: 7.6 m/s compresses the legs past the prop tips, which strike the roof; the quad bounces.
        let mut vehicle = open_loop(&above_building_a(3.0), flat_world());
        vehicle.run_for(3.0).unwrap();
        let s = vehicle.state();
        assert!(s.collision.count >= 2, "the landing and the bounce: {:?}", s.collision);
        assert_eq!(s.collision.object, "BuildingA");
        assert!(-s.pos_ned_m.z > 14.0 && -s.pos_ned_m.z < 14.2, "on the roof: {}", s.pos_ned_m);
    }

    #[test]
    fn a_crash_replays_identically() {
        let run = || {
            let mut vehicle = open_loop(&above_building_a(3.0), flat_world());
            vehicle.run_for(1.5).unwrap();
            vehicle.digest()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn nothing_has_been_hit_at_the_start() {
        let mut vehicle = open_loop(&shipped_quad(), flat_world());
        vehicle.run_for(1.0).unwrap();
        let c = vehicle.state().collision;
        assert_eq!((c.count, c.speed_mps), (0, 0.0), "stepping up onto the 5 mm launch pad at home is no collision");
    }

    #[test]
    fn the_elrs_link_takes_the_handset_and_buildings_from_the_world_file() {
        let world = flat_world();
        let params = elrs_params(&shipped_quad().radio, &world).unwrap();
        assert_eq!(params.handset.position, DVec3::new(-3.0, 2.0, -1.2));
        assert_eq!(params.handset.antennas.len(), 1);
        assert!((params.handset.antennas[0].axis - DVec3::NEG_Z).length() < 1e-12, "upright");
        assert_eq!(params.handset.antennas[0].polarization, Polarization::Linear);
        assert_eq!(params.antennas.len(), 1);
        assert!((params.antennas[0].axis - DVec3::new(-0.5, 0.0, -1.0).normalize()).length() < 1e-12);
        assert_eq!(params.obstacles.len(), 4, "the buildings");
        assert_eq!((params.rate.hz, params.tx_power_mw), (500, 250));
        assert!(params.fading && params.ground_bounce);
        let open = elrs_params(&shipped_quad().radio, &WorldConfig::open_field()).unwrap();
        assert_eq!(open.handset.position, DVec3::new(0.0, 0.0, -1.2));
        assert!(open.obstacles.is_empty());
    }

    #[test]
    fn fifty_kilometres_out_at_10_mw_the_link_is_down() {
        let mut cfg = shipped_quad();
        cfg.initial.position_ned_m = [50_000.0, 0.0, -100.0];
        cfg.radio.tx_power_mw = 10;
        let mut vehicle = open_loop(&cfg, flat_world());
        vehicle.set_transmitter(true);
        vehicle.run_for(1.0).unwrap();
        let r = vehicle.state().radio;
        assert!(!r.link_up && r.lq_pct == 0.0, "{r:?}");
        assert_eq!(r.rssi_dbm, ofs_radio::elrs::NO_SIGNAL_RSSI_DBM);
    }

    #[test]
    fn near_the_pilot_the_link_reports_its_antenna_and_downlink() {
        let mut vehicle = open_loop(&shipped_quad(), flat_world());
        vehicle.set_transmitter(true);
        vehicle.run_for(0.5).unwrap();
        let r = vehicle.state().radio;
        assert!(r.link_up && r.lq_pct == 100.0 && r.downlink_lq_pct == 100.0, "{r:?}");
        assert_eq!(r.active_antenna, "rx");
        assert!(r.snr_db > 20.0, "{r:?}");
    }
}
```

In `quads/opendrone-5f-freestyle.toml`, replace:

```toml
schema_version = 3
```

with:

```toml
schema_version = 4
```

In `quads/opendrone-5f-freestyle.toml`, replace:

```toml
rssi_dbm = -50.0
snr_db = 10.0
```

with:

```toml
# The handset's power setting (ExpressLRS: 10, 25, 50, 100, 250, 500 or 1000 mW). The handset itself, where the pilot
# holds it and its antenna, is in the world file ([handset]); RSSI, SNR and packet loss come from the geometry between
# the two (docs/research/elrs-link.md).
tx_power_mw = 250

# The receiver's antenna: a 2 dBi dipole on the back of the frame, leaning back. A second one makes diversity.
[[radio.antennas]]
name = "rx"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
mount_frd = [-0.5, 0.0, -1.0]

# How the quad hits the world's objects and the ground: spheres on the prop tips and around the body take impulses
# (docs/research/collision.md); the landing contact points ([frame] contact_points_frd_m) use [ground]. Without
# spheres_frd_m, 6 spheres of 1.5 cm on each prop's tip circle and one of 2.5 cm at the centre follow from the motor
# positions and prop.diameter_m.
[collision]
restitution = 0.3
friction_coeff = 0.5
```

In `worlds/flat.toml`, replace:

```toml
# The simulator uses it for the video link (line of sight, diffraction around the buildings, interference); the Godot
# client draws the objects from it. Nothing here collides with the drone: the simulator's ground is the plane d = 0.
```

with:

```toml
# The simulator uses it for the video and control links (line of sight, diffraction around the buildings,
# interference) and for collisions: the drone hits every object here; the Godot client draws them from it.
```

In `worlds/flat.toml`, replace:

```toml

# Another quad's VTX, left on at the west edge of the field on R2, next to the default R1.
```

with:

```toml

# The pilot's radio (the ELRS handset), held at chest height below the goggles, its dipole upright.
[handset]
position_ned_m = [-3.0, 2.0, -1.2]

[[handset.antennas]]
name = "tx"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
aim_el_deg = 90.0

# Another quad's VTX, left on at the west edge of the field on R2, next to the default R1.
```

In `worlds/flat.toml`, replace:

```toml
power_mw = 25.0

[[objects]]
```

with:

```toml
power_mw = 25.0

# A 5 mm mat. The quad starts on the ground at home, 5 mm into it, and its feet step up onto it gently.
[[objects]]
```

In `worlds/flat.toml`, replace:

```toml
center_ned_m = [0.0, 0.0, -0.01]
size_m = [3.0, 3.0, 0.02]
```

with:

```toml
center_ned_m = [0.0, 0.0, -0.0025]
size_m = [3.0, 3.0, 0.005]
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ofs-config -p ofs-sim --locked`
Expected: PASS

Run: `cargo test --workspace --locked`
Expected: PASS (381 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-config/src/lib.rs crates/ofs-config/src/world.rs crates/ofs-config/tests/config.rs crates/ofs-config/tests/world.rs crates/ofs-sim/src/vehicle.rs quads/opendrone-5f-freestyle.toml worlds/flat.toml
git commit -F - <<'EOF'
feat(config): quad schema 4 ([radio] tx_power_mw and antennas, [collision]), the world's handset; the vehicle collides with the world and flies the ELRS link from it

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 5: Protocol 5: the server and the Python client

`RadioLink` gains `snr_db`, `active_antenna`, `downlink_lq_pct`; a `Collision` message rides in `State.collision`; `EVENT_KIND_COLLISION = 13` is raised once per state sample with new collisions; `World` carries the `Handset`. The Rust client gets the minimum to keep compiling (the new event kind); Task 6 does the rest. The Python client gains `RadioLink`'s fields, `Collision`, `Handset`, and the live SITL test of the link.

**Files:**
- Create: `python/tests/test_sitl_link.py`
- Modify: `crates/ofs-client/tests/model.rs`
- Modify: `crates/ofs-proto/tests/messages.rs`
- Modify: `crates/ofs-sim/tests/grpc.rs`
- Modify: `python/tests/test_client.py`
- Modify: `crates/ofs-client/src/model.rs`
- Modify: `crates/ofs-proto/src/lib.rs`
- Modify: `crates/ofs-sim/src/session.rs`
- Modify: `proto/ofs/v1/sim.proto`
- Modify: `python/ofs/__init__.py`
- Modify: `python/ofs/client.py`
- Modify: `python/ofs/v1/sim_pb2.py`
- Modify: `python/ofs/v1/sim_pb2.pyi`

**Interfaces:**
- Consumes: Task 4's `VehicleState.collision`, `RadioState`'s new fields, `WorldConfig::handset_position`.
- Produces: protocol 5 messages `pb::RadioLink { .., snr_db, active_antenna, downlink_lq_pct }`, `pb::Collision { count, speed_mps, object }`, `pb::State.collision`, `pb::EventKind::Collision`, `pb::Handset { position_ned_m, antennas }`, `pb::World.handset`; `ofs_client::EventKind::Collision` ("collision"); Python `ofs.RadioLink(.., snr_db, active_antenna, downlink_lq_pct)`, `ofs.Collision(count, speed_mps, object)`, `State.collision`, `ofs.Handset(position_ned_m, antennas)`, `World.handset`, event kind `"collision"`.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-client/tests/model.rs`, replace:

```rust
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0 }),
```

with:

```rust
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0, ..Default::default() }),
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
        emitters: vec![pb::Emitter { name: "parked-quad".into(), position_ned_m: v(30.0, -60.0, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
```

with:

```rust
        emitters: vec![pb::Emitter { name: "parked-quad".into(), position_ned_m: v(30.0, -60.0, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
        handset: None,
```

In `crates/ofs-proto/tests/messages.rs`, replace:

```rust
    assert_eq!(PROTOCOL_VERSION, 4);
```

with:

```rust
    assert_eq!(PROTOCOL_VERSION, 5);
```

In `crates/ofs-sim/tests/grpc.rs`, replace:

```rust
    assert!(video.snr_db > 40.0, "{video:?}");
```

with:

```rust
    assert!(video.snr_db > 40.0, "{video:?}");
    let handset = world.handset.unwrap();
    assert_eq!(handset.position_ned_m.unwrap().z, -1.2);
    assert_eq!(handset.antennas[0].name, "tx");
    let radio = s.radio.unwrap();
    assert_eq!((radio.active_antenna.as_str(), radio.lq_pct, radio.downlink_lq_pct), ("rx", 100.0, 100.0));
    assert!(radio.snr_db > 20.0, "{radio:?}");
    assert_eq!(s.collision.unwrap().count, 0, "standing at home");
```

In `crates/ofs-sim/tests/grpc.rs`, replace:

```rust
        if !radio.unwrap().tx_enabled && !radio.unwrap().link_up {
```

with:

```rust
        let r = radio.as_ref().unwrap();
        if !r.tx_enabled && !r.link_up {
```

In `crates/ofs-sim/tests/grpc.rs`, replace:

```rust
    drop(tx);
}
```

with:

```rust
    drop(tx);
}

/// The shipped quad (and its diff) copied into `dir`, its start position replaced by `initial`.
fn quad_at(dir: &std::path::Path, initial: &str) -> String {
    let text = std::fs::read_to_string(QUAD).unwrap().replace("position_ned_m = [0.0, 0.0, -0.03]", &format!("position_ned_m = {initial}"));
    let quads = std::path::Path::new(QUAD).parent().unwrap();
    std::fs::copy(quads.join("opendrone-5f-freestyle.betaflight.diff"), dir.join("opendrone-5f-freestyle.betaflight.diff")).unwrap();
    let path = dir.join("quad.toml");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn a_quad_dropped_on_a_building_raises_a_collision_event_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let quad = quad_at(dir.path(), "[110.0, -45.0, -14.18]"); // feet 15 cm above building A's roof
    let mut c = start().await;
    c.load(LoadRequest { quad_path: quad, world_path: WORLD.into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
    let mut watch = c.watch(Empty {}).await.unwrap().into_inner();
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    let collision = s.collision.unwrap();
    assert_eq!((collision.count, collision.object.as_str()), (1, "BuildingA"));
    loop {
        let e = tokio::time::timeout(Duration::from_secs(5), watch.next()).await.unwrap().unwrap().unwrap();
        if e.kind() == EventKind::Collision {
            assert_eq!(e.message, "BuildingA at 1.7 m/s");
            break;
        }
    }
}
```

In `python/tests/test_client.py`, replace:

```python
    video = sim.run(0.5).video
```

with:

```python
    assert world.handset.position_ned_m == (-3.0, 2.0, -1.2)
    assert [(a.name, a.kind, a.polarization) for a in world.handset.antennas] == [("tx", "omni", "linear")]
    state = sim.run(0.5)
    radio = state.radio
    assert (radio.active_antenna, radio.lq_pct, radio.downlink_lq_pct) == ("rx", 100.0, 100.0), radio
    assert radio.snr_db > 20.0 and radio.rssi_dbm > -90.0, radio
    assert state.collision == ofs.Collision(), "nothing hit at home"
    video = state.video
```

In `python/tests/test_client.py`, replace:

```python
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_RESTORED)).kind == "video_restored"
```

with:

```python
    assert _event(pb.Event(kind=pb.EVENT_KIND_VIDEO_RESTORED)).kind == "video_restored"
    assert _event(pb.Event(kind=pb.EVENT_KIND_COLLISION)).kind == "collision"


def test_radio_collision_and_world_messages_convert():
    from ofs.client import _state, _world
    from ofs.v1 import sim_pb2 as pb

    s = _state(pb.State(radio=pb.RadioLink(link_up=True, snr_db=-3.5, active_antenna="rx2", downlink_lq_pct=80.0),
                        collision=pb.Collision(count=2, speed_mps=4.5, object="Pylon1L")))
    assert (s.radio.snr_db, s.radio.active_antenna, s.radio.downlink_lq_pct) == (-3.5, "rx2", 80.0)
    assert s.collision == ofs.Collision(2, 4.5, "Pylon1L")
    assert _world(pb.World(name="old")).handset is None, "a world without a handset message"
    w = _world(pb.World(handset=pb.Handset(position_ned_m=pb.Vec3(z=-1.2), antennas=[pb.ReceiverAntenna(name="tx")])))
    assert w.handset.position_ned_m == (0.0, 0.0, -1.2) and w.handset.antennas[0].name == "tx"


def _quad_at(tmp_path, initial):
    """The shipped quad (and its diff) copied to `tmp_path`, its start position replaced by `initial`."""
    import pathlib
    import shutil

    quads = pathlib.Path(QUAD).parent
    shutil.copy(quads / "opendrone-5f-freestyle.betaflight.diff", tmp_path)
    text = pathlib.Path(QUAD).read_text().replace("position_ned_m = [0.0, 0.0, -0.03]", f"position_ned_m = {initial}")
    path = tmp_path / "quad.toml"
    path.write_text(text)
    return str(path)


def test_a_quad_dropped_on_a_building_hits_it(sim, tmp_path):
    sim.load(_quad_at(tmp_path, "[110.0, -45.0, -14.18]"), open_loop_fc=True, world=WORLD)
    state = sim.run(1.0)
    assert state.collision == ofs.Collision(1, state.collision.speed_mps, "BuildingA"), state.collision
    assert abs(state.collision.speed_mps - 1.7) < 0.05
    assert abs(state.altitude_m - 14.03) < 0.01, "standing on the roof"
    import time

    deadline = time.monotonic() + 3.0
    hits = []
    while time.monotonic() < deadline and not hits:
        hits = [e for e in sim.events() if e.kind == "collision"]
        time.sleep(0.05)
    assert [e.message for e in hits] == ["BuildingA at 1.7 m/s"]


def test_far_away_at_10_mw_the_link_is_down(sim, tmp_path):
    path = _quad_at(tmp_path, "[50000.0, 0.0, -100.0]")
    import pathlib

    pathlib.Path(path).write_text(pathlib.Path(path).read_text().replace("tx_power_mw = 250", "tx_power_mw = 10"))
    sim.load(path, open_loop_fc=True, world=WORLD)
    radio = sim.run(0.5).radio
    assert not radio.link_up and radio.lq_pct == 0.0 and radio.rssi_dbm == -130.0, radio
```

Create `python/tests/test_sitl_link.py` with:

```python
"""The ExpressLRS link on the propagation model, through the API against Betaflight SITL (spec
docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md). Needs OFS_SITL_LAUNCH."""
import os
import pathlib
import shutil

import pytest

from conftest import QUAD, WORLD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)


def far_quad(tmp_path):
    """The shipped quad 50 km north of the pilot, its handset at 10 mW: 15 dB below the receiver's sensitivity."""
    shutil.copy(pathlib.Path(QUAD).parent / "opendrone-5f-freestyle.betaflight.diff", tmp_path)
    text = (pathlib.Path(QUAD).read_text()
            .replace("position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [50000.0, 0.0, -0.03]")
            .replace("tx_power_mw = 250", "tx_power_mw = 10"))
    path = tmp_path / "far.toml"
    path.write_text(text)
    return str(path)


def arm(sim):
    sim.run(5.0)  # boot and gyro calibration
    sim.set_sticks(aux=ARM_AND_ANGLE)
    return sim.run(1.5)


def test_near_the_pilot_the_link_is_full_and_betaflight_arms(sim):
    sim.load(QUAD, seed=1, world=WORLD)
    s = arm(sim)
    assert s.radio.link_up and s.radio.lq_pct == 100.0 and s.radio.active_antenna == "rx", s.radio
    assert min(s.motor_cmd) > 0.0, f"never armed: {s.motor_cmd}"


def test_far_away_at_10_mw_betaflight_hears_no_receiver_and_will_not_arm(sim, tmp_path):
    sim.load(far_quad(tmp_path), seed=1, world=WORLD)
    s = arm(sim)
    assert not s.radio.link_up and s.radio.lq_pct == 0.0 and s.radio.rssi_dbm == -130.0, s.radio
    assert max(s.motor_cmd) == 0.0, f"armed without a receiver: {s.motor_cmd}"
    assert "DISARMED" in sim.get_osd().text
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-proto -p ofs-sim --locked`
Expected: FAIL: `PROTOCOL_VERSION` is 4; no `pb::Collision`, no `handset` on `pb::World`.

- [ ] **Step 3: Implement**

In `crates/ofs-client/src/model.rs`, replace:

```rust
        let radio = s.radio.unwrap_or_default();
```

with:

```rust
        let radio = s.radio.clone().unwrap_or_default();
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
    VideoRestored,
```

with:

```rust
    VideoRestored,
    Collision,
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            EventKind::VideoRestored => "video_restored",
```

with:

```rust
            EventKind::VideoRestored => "video_restored",
            EventKind::Collision => "collision",
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            Ok(pb::EventKind::VideoRestored) => EventKind::VideoRestored,
```

with:

```rust
            Ok(pb::EventKind::VideoRestored) => EventKind::VideoRestored,
            Ok(pb::EventKind::Collision) => EventKind::Collision,
```

In `crates/ofs-proto/src/lib.rs`, replace:

```rust
//! Protocol 4 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
```

with:

```rust
//! Protocol 5 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
```

In `crates/ofs-proto/src/lib.rs`, replace:

```rust
pub const PROTOCOL_VERSION: u32 = 4;
```

with:

```rust
pub const PROTOCOL_VERSION: u32 = 5;
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
use crate::vehicle::{VideoInfo, Vehicle, VehicleState, VtxInfo};
```

with:

```rust
use crate::vehicle::{CollisionInfo, VideoInfo, Vehicle, VehicleState, VtxInfo};
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
    last_video_lost: Option<bool>,
```

with:

```rust
    last_video_lost: Option<bool>,
    last_collisions: u64,
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
    format!("{channel} {} MHz {} mW{}", v.freq_mhz, v.power_mw, if v.pit_mode { " (pit mode)" } else { "" })
```

with:

```rust
    format!("{channel} {} MHz {} mW{}", v.freq_mhz, v.power_mw, if v.pit_mode { " (pit mode)" } else { "" })
}

/// "BuildingA at 7.2 m/s", or with several since the last sample "ground at 3.1 m/s (2 collisions)".
pub(crate) fn collision_message(c: &CollisionInfo, new: u64) -> String {
    let more = if new > 1 { format!(" ({new} collisions)") } else { String::new() };
    format!("{} at {:.1} m/s{more}", c.object, c.speed_mps)
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
/// The world as the protocol carries it. Emitters given by band and channel carry their frequency.
```

with:

```rust
fn antenna_kind_name(k: AntennaKind) -> &'static str {
    match k {
        AntennaKind::Omni => "omni",
        AntennaKind::Patch => "patch",
    }
}

/// The world as the protocol carries it. Emitters given by band and channel carry their frequency; the handset is
/// where it is held (its default position filled in).
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
                kind: match a.kind {
                    AntennaKind::Omni => "omni",
                    AntennaKind::Patch => "patch",
                }
                .into(),
```

with:

```rust
                kind: antenna_kind_name(a.kind).into(),
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            .collect(),
    }
```

with:

```rust
            .collect(),
        handset: Some(pb::Handset {
            position_ned_m: vec3_of(w.handset_position()),
            antennas: w
                .handset
                .antennas
                .iter()
                .map(|a| pb::ReceiverAntenna {
                    name: a.name.clone(),
                    kind: antenna_kind_name(a.kind).into(),
                    gain_dbi: a.gain_dbi,
                    beamwidth_deg: 0.0,
                    polarization: polarization_name(a.polarization).into(),
                    aim_az_deg: a.aim_az_deg,
                    aim_el_deg: a.aim_el_deg,
                })
                .collect(),
        }),
    }
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            last_video_lost: None,
```

with:

```rust
            last_video_lost: None,
            last_collisions: 0,
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
    /// VTX changes, dropped UART bytes, video sync lost or regained.
```

with:

```rust
    /// VTX changes, dropped UART bytes, video sync lost or regained, collisions.
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
        }
        out
```

with:

```rust
        }
        if s.collision.count != self.last_collisions {
            let new = s.collision.count.saturating_sub(self.last_collisions);
            self.last_collisions = s.collision.count;
            out.push(event(s.time_s, pb::EventKind::Collision, collision_message(&s.collision, new)));
        }
        out
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
                rssi_dbm: s.radio.rssi_dbm,
```

with:

```rust
                rssi_dbm: s.radio.rssi_dbm,
                snr_db: s.radio.snr_db,
                active_antenna: s.radio.active_antenna.clone(),
                downlink_lq_pct: s.radio.downlink_lq_pct,
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
            video: Some(video_msg(&s.video)),
```

with:

```rust
            video: Some(video_msg(&s.video)),
            collision: Some(pb::Collision {
                count: s.collision.count,
                speed_mps: s.collision.speed_mps,
                object: s.collision.object.clone(),
            }),
```

In `crates/ofs-sim/src/session.rs`, replace:

```rust
        assert_eq!((msg.emitters[0].name.as_str(), msg.emitters[0].freq_mhz), ("parked-quad", 5695.0), "R2, from band and channel");
```

with:

```rust
        assert_eq!((msg.emitters[0].name.as_str(), msg.emitters[0].freq_mhz), ("parked-quad", 5695.0), "R2, from band and channel");
        let handset = msg.handset.as_ref().unwrap();
        assert_eq!(handset.position_ned_m, Some(pb::Vec3 { x: -3.0, y: 2.0, z: -1.2 }));
        let tx = &handset.antennas[0];
        assert_eq!((tx.name.as_str(), tx.kind.as_str(), tx.polarization.as_str(), tx.aim_el_deg), ("tx", "omni", "linear", 90.0));
        let open = world_msg(&WorldConfig::open_field());
        assert_eq!(open.handset.unwrap().position_ned_m, Some(pb::Vec3 { x: 0.0, y: 0.0, z: -1.2 }), "the default: below the goggles");
    }

    #[test]
    fn collisions_become_events_naming_what_was_hit() {
        let mut session = testing::open_loop_session(RunMode::Lockstep);
        let mut s = session.vehicle.state();
        let collisions = |events: Vec<pb::Event>| events.into_iter().filter(|e| e.kind == pb::EventKind::Collision as i32).collect::<Vec<_>>();
        assert!(collisions(session.events_for(&s)).is_empty(), "no collision yet");
        s.collision = CollisionInfo { count: 1, speed_mps: 7.25, object: "BuildingA".into() };
        let hit = collisions(session.events_for(&s));
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].message, "BuildingA at 7.2 m/s");
        assert!(collisions(session.events_for(&s)).is_empty(), "the same collision is reported once");
        s.collision = CollisionInfo { count: 3, speed_mps: 3.06, object: "ground".into() };
        let more = collisions(session.events_for(&s));
        assert_eq!(more.len(), 1, "one event per sample");
        assert_eq!(more[0].message, "ground at 3.1 m/s (2 collisions)");
        let msg = session.state_msg();
        assert_eq!(msg.collision, Some(pb::Collision { count: 0, speed_mps: 0.0, object: String::new() }), "the vehicle's own state: nothing hit yet");
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  double rssi_dbm = 4;
```

with:

```protobuf
  double rssi_dbm = 4;          // at the active antenna; -130 while the link is down
  double snr_db = 5;            // LoRa decodes below the noise: it may be negative
  string active_antenna = 6;    // the quad's receiver antenna in use (its name in the quad file)
  double downlink_lq_pct = 7;   // telemetry back to the handset
}

// The quad's collisions with the world's objects and the ground.
message Collision {
  uint64 count = 1;      // so far
  double speed_mps = 2;  // the last one's inward speed (0 before the first)
  string object = 3;     // what it hit: a world object's name, or "ground"
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  VideoLink video = 16;
```

with:

```protobuf
  VideoLink video = 16;
  Collision collision = 17;
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  EVENT_KIND_VIDEO_RESTORED = 12;  // and found it again
```

with:

```protobuf
  EVENT_KIND_VIDEO_RESTORED = 12;  // and found it again
  EVENT_KIND_COLLISION = 13;       // the quad hit an object or the ground at 1 m/s or more
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf

message World {
```

with:

```protobuf

// The pilot's radio (the ELRS handset); its antennas are dipoles ("omni"), aimed like the goggles' antennas.
message Handset {
  Vec3 position_ned_m = 1;
  repeated ReceiverAntenna antennas = 2;
}

message World {
```

In `proto/ofs/v1/sim.proto`, replace:

```protobuf
  repeated Emitter emitters = 6;
```

with:

```protobuf
  repeated Emitter emitters = 6;
  Handset handset = 7;
```

In `python/ofs/__init__.py`, replace:

```python
from .client import (Emitter, Event, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx, World, WorldObject,
                     connect, launch)
```

with:

```python
from .client import (Collision, Emitter, Event, Handset, Osd, RadioLink, ReceiverAntenna, Sim, State, VideoLink, Vtx,
                     World, WorldObject, connect, launch)
```

In `python/ofs/__init__.py`, replace:

```python
    "Sim", "State", "RadioLink", "Event", "Osd", "Vtx", "VideoLink", "World", "WorldObject", "ReceiverAntenna", "Emitter",
```

with:

```python
    "Sim", "State", "RadioLink", "Collision", "Event", "Osd", "Vtx", "VideoLink", "World", "WorldObject", "ReceiverAntenna",
    "Emitter", "Handset",
```

In `python/ofs/client.py`, replace:

```python
PROTOCOL_VERSION = 4
```

with:

```python
PROTOCOL_VERSION = 5
```

In `python/ofs/client.py`, replace:

```python
    rssi_dbm: float = 0.0
```

with:

```python
    rssi_dbm: float = 0.0  # at the active antenna; -130 while the link is down
    snr_db: float = 0.0  # LoRa decodes below the noise: it may be negative
    active_antenna: str = ""  # the quad's receiver antenna in use (its name in the quad file)
    downlink_lq_pct: float = 0.0  # telemetry back to the handset


@dataclass(frozen=True)
class Collision:
    """The quad's collisions with the world's objects and the ground."""
    count: int = 0  # so far
    speed_mps: float = 0.0  # the last one's inward speed
    object: str = ""  # what the last one hit: a world object's name, or "ground"
```

In `python/ofs/client.py`, replace:

```python
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters."""
```

with:

```python
class Handset:
    """The pilot's radio (the ELRS handset): where it is held and its antennas (dipoles, `ReceiverAntenna` values)."""
    position_ned_m: tuple
    antennas: tuple


@dataclass(frozen=True)
class World:
    """The field a session flies in: where the pilot stands, the goggles' antennas, the objects, other transmitters,
    the pilot's radio."""
```

In `python/ofs/client.py`, replace:

```python
    emitters: tuple
```

with:

```python
    emitters: tuple
    handset: Handset | None = None
```

In `python/ofs/client.py`, replace:

```python
    video: VideoLink = field(default_factory=VideoLink)
```

with:

```python
    video: VideoLink = field(default_factory=VideoLink)
    collision: Collision = field(default_factory=Collision)
```

In `python/ofs/client.py`, replace:

```python
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended", "vtx_changed", "video_lost";
```

with:

```python
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended", "vtx_changed", "video_lost", "collision";
```

In `python/ofs/client.py`, replace:

```python

def _world(m) -> World:
```

with:

```python

def _antenna(a) -> ReceiverAntenna:
    return ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg, a.aim_el_deg)


def _world(m) -> World:
```

In `python/ofs/client.py`, replace:

```python
        antennas=tuple(ReceiverAntenna(a.name, a.kind, a.gain_dbi, a.beamwidth_deg, a.polarization, a.aim_az_deg,
                                       a.aim_el_deg) for a in m.antennas),
```

with:

```python
        antennas=tuple(_antenna(a) for a in m.antennas),
```

In `python/ofs/client.py`, replace:

```python
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
```

with:

```python
        emitters=tuple(Emitter(e.name, _v(e.position_ned_m), e.freq_mhz, e.power_mw) for e in m.emitters),
        handset=Handset(_v(m.handset.position_ned_m), tuple(_antenna(a) for a in m.handset.antennas))
        if m.HasField("handset") else None,
```

In `python/ofs/client.py`, replace:

```python
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm),
```

with:

```python
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm, m.radio.snr_db,
                        m.radio.active_antenna, m.radio.downlink_lq_pct),
```

In `python/ofs/client.py`, replace:

```python
        video=_video(m.video),
```

with:

```python
        video=_video(m.video),
        collision=Collision(m.collision.count, m.collision.speed_mps, m.collision.object),
```

- [ ] **Step 4: Regenerate the Python stubs**

Regenerate the Python stubs:

```bash
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test --workspace --locked`
Expected: PASS (383 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

Run: `cargo build -p ofs-sim && python -m pytest python/tests -q`
Expected: PASS (25 passed, the SITL ones skipped without `OFS_SITL_LAUNCH`)

Run: `OFS_SIM_BIN=<a firewall-allowed ofs-sim.exe> OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf" python -m pytest python/tests -q`
Expected: PASS (38 passed, including `test_sitl_link.py`)

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-client/src/model.rs crates/ofs-client/tests/model.rs crates/ofs-proto/src/lib.rs crates/ofs-proto/tests/messages.rs crates/ofs-sim/src/session.rs crates/ofs-sim/tests/grpc.rs proto/ofs/v1/sim.proto python/ofs/__init__.py python/ofs/client.py python/ofs/v1/sim_pb2.py python/ofs/v1/sim_pb2.pyi python/tests/test_client.py python/tests/test_sitl_link.py
git commit -F - <<'EOF'
feat(proto): protocol 5: the radio's SNR, antenna and downlink, collisions and their event, the world's handset; server and Python client

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 6: The Rust client and the Godot extension

`Telemetry` gains the radio's SNR, antenna and downlink and the collision count, speed and object; `World` gains the handset in Godot's frame. `OfsClient.get_telemetry()` and `get_world()` expose them.

**Files:**
- Modify: `crates/ofs-client/tests/model.rs`
- Modify: `crates/ofs-client/src/model.rs`
- Modify: `crates/ofs-godot/src/lib.rs`

**Interfaces:**
- Consumes: Task 5's protocol 5 messages.
- Produces: `ofs_client::{Telemetry { .., snr_db, radio_antenna, downlink_lq_pct, collisions, collision_speed_mps, collision_object }, WorldHandset { position, antennas }, World.handset: Option<WorldHandset>}`; Godot telemetry keys `snr_db`, `radio_antenna`, `downlink_lq_pct`, `collisions`, `collision_speed_mps`, `collision_object`; world key `handset` ({`position`: Vector3, `antennas`: Array of {`name`, `kind`, `aim`}}, empty without one).

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-client/tests/model.rs`, replace:

```rust
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0, ..Default::default() }),
```

with:

```rust
        radio: Some(pb::RadioLink {
            tx_enabled: true,
            link_up: true,
            lq_pct: 98.0,
            rssi_dbm: -50.0,
            snr_db: 58.9,
            active_antenna: "rx".into(),
            downlink_lq_pct: 97.0,
        }),
        collision: Some(pb::Collision { count: 2, speed_mps: 6.5, object: "BuildingA".into() }),
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
    assert_eq!((t.overruns, t.fc_restarts), (3, 1));
```

with:

```rust
    assert_eq!((t.overruns, t.fc_restarts), (3, 1));
    assert_eq!((t.snr_db, t.radio_antenna.as_str(), t.downlink_lq_pct), (58.9, "rx", 97.0));
    assert_eq!((t.collisions, t.collision_speed_mps, t.collision_object.as_str()), (2, 6.5, "BuildingA"));
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
    assert_eq!(Event::from_pb(pb::Event { kind: 999, ..Default::default() }).kind, EventKind::Unknown);
```

with:

```rust
    assert_eq!(Event::from_pb(pb::Event { kind: 999, ..Default::default() }).kind, EventKind::Unknown);
    let hit = Event::from_pb(pb::Event { kind: pb::EventKind::Collision as i32, message: "BuildingA at 7.2 m/s".into(), ..Default::default() });
    assert_eq!((hit.kind, hit.kind.as_str()), (EventKind::Collision, "collision"));
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
        handset: None,
```

with:

```rust
        handset: Some(pb::Handset {
            position_ned_m: v(-3.0, 2.0, -1.2),
            antennas: vec![pb::ReceiverAntenna { name: "tx".into(), kind: "omni".into(), aim_el_deg: 90.0, ..Default::default() }],
        }),
```

In `crates/ofs-client/tests/model.rs`, replace:

```rust
    assert_eq!(w.emitters[0].position, DVec3::new(-60.0, 1.0, -30.0));
```

with:

```rust
    assert_eq!(w.emitters[0].position, DVec3::new(-60.0, 1.0, -30.0));
    let handset = w.handset.as_ref().unwrap();
    assert_eq!(handset.position, DVec3::new(2.0, 1.2, 3.0), "east, up, south");
    assert!((handset.antennas[0].aim - DVec3::Y).length() < 1e-12, "the dipole stands up");
    assert!(World::from_pb(pb::World::default()).handset.is_none(), "a server without a handset");
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p ofs-client --locked`
Expected: FAIL: no field `snr_db`, `collisions`, `handset`.

- [ ] **Step 3: Implement**

In `crates/ofs-client/src/model.rs`, replace:

```rust

/// The world the session flies in, converted to Godot's frame (+Y up, north is -Z).
```

with:

```rust

/// The pilot's radio, in Godot's frame; its antennas' `aim` is the dipole's axis.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldHandset {
    pub position: DVec3,
    pub antennas: Vec<WorldAntenna>,
}

/// The world the session flies in, converted to Godot's frame (+Y up, north is -Z).
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
    pub emitters: Vec<WorldEmitter>,
```

with:

```rust
    pub emitters: Vec<WorldEmitter>,
    /// None from a server that sends no handset.
    pub handset: Option<WorldHandset>,
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
        let facing = w.pilot_facing_deg;
```

with:

```rust
        let facing = w.pilot_facing_deg;
        let antenna = |a: pb::ReceiverAntenna| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind };
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            antennas: w
                .antennas
                .into_iter()
                .map(|a| WorldAntenna { aim: vec_to_godot(aim_ned(facing + a.aim_az_deg, a.aim_el_deg)), name: a.name, kind: a.kind })
                .collect(),
```

with:

```rust
            handset: w.handset.map(|h| WorldHandset {
                position: vec_to_godot(ned(h.position_ned_m)),
                antennas: h.antennas.into_iter().map(antenna).collect(),
            }),
            antennas: w.antennas.into_iter().map(antenna).collect(),
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
    pub rssi_dbm: f64,
```

with:

```rust
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// The quad's receiver antenna in use.
    pub radio_antenna: String,
    pub downlink_lq_pct: f64,
    /// Collisions so far, and the last one's speed and what it hit (a world object's name or "ground").
    pub collisions: u64,
    pub collision_speed_mps: f64,
    pub collision_object: String,
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
        let radio = s.radio.clone().unwrap_or_default();
```

with:

```rust
        let radio = s.radio.clone().unwrap_or_default();
        let collision = s.collision.clone().unwrap_or_default();
```

In `crates/ofs-client/src/model.rs`, replace:

```rust
            rssi_dbm: radio.rssi_dbm,
```

with:

```rust
            rssi_dbm: radio.rssi_dbm,
            snr_db: radio.snr_db,
            radio_antenna: radio.active_antenna,
            downlink_lq_pct: radio.downlink_lq_pct,
            collisions: collision.count,
            collision_speed_mps: collision.speed_mps,
            collision_object: collision.object,
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
use ofs_client::{Client, Command, LaunchSpec, OverrunPolicy, Settings, Sticks, Update};
```

with:

```rust
use ofs_client::{Client, Command, LaunchSpec, OverrunPolicy, Settings, Sticks, Update, WorldAntenna};
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
```

with:

```rust
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `snr_db`, `radio_antenna` (the receiver antenna in use),
    /// `downlink_lq_pct`, `collisions` (so far), `collision_speed_mps` and `collision_object` (the last one's; a
    /// world object's name or "ground"), `running`, `overruns`, `fc_restarts`, `age_s`, and the VTX's
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        d.set("rssi_dbm", t.rssi_dbm);
```

with:

```rust
        d.set("rssi_dbm", t.rssi_dbm);
        d.set("snr_db", t.snr_db);
        d.set("radio_antenna", &GString::from(t.radio_antenna.as_str()));
        d.set("downlink_lq_pct", t.downlink_lq_pct);
        d.set("collisions", t.collisions as i64);
        d.set("collision_speed_mps", t.collision_speed_mps);
        d.set("collision_object", &GString::from(t.collision_object.as_str()));
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
    /// `color`: Color, `rf_loss_db`}) and `emitters` (Array of {`name`, `position`: Vector3, `freq_mhz`, `power_mw`}).
```

with:

```rust
    /// `color`: Color, `rf_loss_db`}), `emitters` (Array of {`name`, `position`: Vector3, `freq_mhz`, `power_mw`}) and
    /// `handset` ({`position`: Vector3, `antennas`: Array like `antennas`}; empty from a server that sends none).
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        let mut antennas = VarArray::new();
        for a in &world.antennas {
            let mut e = VarDictionary::new();
            e.set("name", &GString::from(a.name.as_str()));
            e.set("kind", &GString::from(a.kind.as_str()));
            e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
            antennas.push(&e.to_variant());
```

with:

```rust
        let antenna_array = |list: &[WorldAntenna]| {
            let mut antennas = VarArray::new();
            for a in list {
                let mut e = VarDictionary::new();
                e.set("name", &GString::from(a.name.as_str()));
                e.set("kind", &GString::from(a.kind.as_str()));
                e.set("aim", vector3(a.aim.x, a.aim.y, a.aim.z));
                antennas.push(&e.to_variant());
            }
            antennas
        };
        let antennas = antenna_array(&world.antennas);
        let mut handset = VarDictionary::new();
        if let Some(h) = &world.handset {
            handset.set("position", vector3(h.position.x, h.position.y, h.position.z));
            handset.set("antennas", &antenna_array(&h.antennas));
```

In `crates/ofs-godot/src/lib.rs`, replace:

```rust
        d.set("emitters", &emitters);
```

with:

```rust
        d.set("emitters", &emitters);
        d.set("handset", &handset);
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p ofs-client --locked`
Expected: PASS

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-client/src/model.rs crates/ofs-client/tests/model.rs crates/ofs-godot/src/lib.rs
git commit -F - <<'EOF'
feat(client): the Rust client and the Godot extension carry the radio's SNR, antenna and downlink, collisions and the world's handset

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 7: The game: the link line, collision toasts, the handset marker

The HUD's `LINK` line shows the SNR; a `collision` event raises a toast ("Hit BuildingA at 7.2 m/s", "Hard landing at 3.1 m/s"); the chase view marks the pilot's radio. The open-loop end-to-end run cuts the throttle high up and sees the drone hit the launch pad.

**Files:**
- Modify: `godot/tests/e2e_open_loop.gd`
- Modify: `godot/tests/test_hud.gd`
- Modify: `godot/tests/test_scene.gd`
- Modify: `godot/scripts/app.gd`
- Modify: `godot/ui/hud.gd`
- Modify: `godot/world/world.gd`

**Interfaces:**
- Consumes: Task 6's telemetry keys and world `handset`.
- Produces: `Hud.link_text() -> String`, `Hud.collision_text(message: String) -> String` (static), the world builder's `Handset` node (`Antenna_<name>` children).

- [ ] **Step 1: Write the failing tests**

In `godot/tests/e2e_open_loop.gd`, replace:

```gdscript
	_check(app.hud.hud_visible(), "and shows it again")
```

with:

```gdscript
	_check(app.hud.hud_visible(), "and shows it again")
	_check(app.hud.link_text().begins_with("LINK UP") and app.hud.link_text().contains("SNR"), "the HUD link line: '%s'" % app.hud.link_text())

	# Cut the throttle high up: the drone falls back onto the launch pad it climbed from.
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var landed: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("Hit LaunchPad"), 30.0)
	_check(landed, "falling back raises a collision toast: %s" % str(app.hud.toast_texts()))
	var hit: Dictionary = app.client.get_telemetry()
	_check(hit.get("collisions", 0) >= 1 and hit.get("collision_object", "") == "LaunchPad", "the telemetry counts it: %s on %s" % [str(hit.get("collisions")), hit.get("collision_object", "")])
```

In `godot/tests/e2e_open_loop.gd`, replace:

```gdscript
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
```

with:

```gdscript
	_check(app.world.get_node_or_null("Pilot") != null and app.world.get_node_or_null("Emitter_parked-quad") != null, "the pilot and the parked quad are marked")
	_check(app.world.get_node_or_null("Handset") != null, "and the pilot's radio")
```

In `godot/tests/test_hud.gd`, replace:

```gdscript
	eq(hud.video_text(), "", "cleared with the telemetry")
	hud.queue_free()
```

with:

```gdscript
	eq(hud.video_text(), "", "cleared with the telemetry")
	hud.queue_free()


func test_the_link_line_shows_lq_rssi_and_snr() -> void:
	var hud := await _hud()
	hud.update_view(_view({"telemetry": _telemetry({"lq_pct": 97.0, "rssi_dbm": -62.4, "snr_db": 46.6})}))
	eq(hud.link_text(), "LINK UP   LQ 97 %   -62 dBm   SNR 47", "the line")
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false, "lq_pct": 0.0, "rssi_dbm": -130.0, "snr_db": -21.1})}))
	eq(hud.link_text(), "LINK DOWN   LQ 0 %   -130 dBm   SNR -21", "down")
	hud.queue_free()


func test_collisions_have_their_toast_text() -> void:
	eq(Hud.collision_text("BuildingA at 7.2 m/s"), "Hit BuildingA at 7.2 m/s", "an object")
	eq(Hud.collision_text("ground at 3.1 m/s (2 collisions)"), "Hard landing at 3.1 m/s (2 collisions)", "the ground")
```

In `godot/tests/test_scene.gd`, replace:

```gdscript
		"emitters": [{"name": "parked-quad", "position": Vector3(-60.0, 1.0, -30.0), "freq_mhz": 5695.0, "power_mw": 25.0}],
```

with:

```gdscript
		"emitters": [{"name": "parked-quad", "position": Vector3(-60.0, 1.0, -30.0), "freq_mhz": 5695.0, "power_mw": 25.0}],
		"handset": {"position": Vector3(2.0, 1.2, 3.0), "antennas": [{"name": "tx", "kind": "omni", "aim": Vector3.UP}]},
```

In `godot/tests/test_scene.gd`, replace:

```gdscript
	ok((emitter.get_node("Label") as Label3D).text.contains("5695 MHz"), "labelled with its frequency")
```

with:

```gdscript
	ok((emitter.get_node("Label") as Label3D).text.contains("5695 MHz"), "labelled with its frequency")
	var handset: Node3D = world.get_node("Handset")
	eq(handset.position, Vector3(2.0, 1.2, 3.0), "the handset where the pilot holds it")
	var stick: MeshInstance3D = handset.get_node("Antenna_tx")
	ok((stick.basis * Vector3.UP).is_equal_approx(Vector3.UP), "its dipole upright")
```

In `godot/tests/test_scene.gd`, replace:

```gdscript
	eq(world.built_names().size(), 4, "two objects, the pilot, one emitter")
```

with:

```gdscript
	eq(world.built_names().size(), 5, "two objects, the pilot, one emitter, the handset")
```

In `godot/tests/test_scene.gd`, replace:

```gdscript
	smaller["emitters"] = []
```

with:

```gdscript
	smaller["emitters"] = []
	smaller["handset"] = {}
```

In `godot/tests/test_scene.gd`, replace:

```gdscript
	eq(world.find_children("*", "CollisionObject3D", true, false).size(), 0, "nothing to collide with: the simulator knows only the ground")
```

with:

```gdscript
	eq(world.find_children("*", "CollisionObject3D", true, false).size(), 0, "no Godot physics: the simulator does the collisions")
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh unit`
Expected: FAIL: `test_hud.gd` does not parse (`link_text` and `collision_text` do not exist), so Godot exits 1; `test_scene.gd` finds no `Handset` node.

- [ ] **Step 3: Implement**

In `godot/scripts/app.gd`, replace:

```gdscript
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
```

with:

```gdscript
			hud.add_toast("Betaflight UART output dropped: %s" % message, "warn")
		"collision":
			hud.add_toast(hud.collision_text(message), "warn")
```

In `godot/ui/hud.gd`, replace:

```gdscript

## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
```

with:

```gdscript

func link_text() -> String:
	return _link.text


## The toast for a collision event's message ("BuildingA at 7.2 m/s", "ground at 3.1 m/s (2 collisions)").
static func collision_text(message: String) -> String:
	if message.begins_with("ground at "):
		return "Hard landing at %s" % message.trim_prefix("ground at ")
	return "Hit %s" % message


## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
```

In `godot/ui/hud.gd`, replace:

```gdscript
		_link.text = "LINK %s   LQ %d %%   %d dBm" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"]]
```

with:

```gdscript
		_link.text = "LINK %s   LQ %d %%   %d dBm   SNR %d" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"], roundi(t.get("snr_db", 0.0))]
```

In `godot/world/world.gd`, replace:

```gdscript
## field (launch pad, pylons, gates, buildings), the pilot's spot and other transmitters come from the world file the
## simulator loaded (`OfsClient.get_world()`), so what you see is what the video link sees. Nothing here collides with
## the drone: the simulator's ground is a plane at height 0, and it knows the objects only for the video signal.
```

with:

```gdscript
## field (launch pad, pylons, gates, buildings), the pilot's spot, the pilot's radio and other transmitters come from
## the world file the simulator loaded (`OfsClient.get_world()`), so what you see is what the simulator flies in: the
## drone hits these objects, and they block the video and control links. Godot only draws them; the simulator does
## the collisions.
```

In `godot/world/world.gd`, replace:

```gdscript
const EMITTER_COLOR := Color(0.85, 0.25, 0.85)
```

with:

```gdscript
const EMITTER_COLOR := Color(0.85, 0.25, 0.85)
const HANDSET_COLOR := Color(1.0, 0.8, 0.2)
```

In `godot/world/world.gd`, replace:

```gdscript
		_add_pilot(world["pilot_position"], world.get("antennas", []))
```

with:

```gdscript
		_add_pilot(world["pilot_position"], world.get("antennas", []))
	var handset: Dictionary = world.get("handset", {})
	if handset.has("position"):
		_add_handset(handset["position"], handset.get("antennas", []))
```

In `godot/world/world.gd`, replace:

```gdscript

## A post up to the transmitter, labelled with its frequency.
```

with:

```gdscript

## The pilot's radio: a small box where it is held, with a stick along each antenna's axis.
func _add_handset(at: Vector3, antennas: Array) -> void:
	var handset := Node3D.new()
	handset.name = "Handset"
	handset.position = at
	var body := BoxMesh.new()
	body.size = Vector3(0.2, 0.12, 0.08)
	handset.add_child(_mesh("Body", body, HANDSET_COLOR, Vector3.ZERO))
	for a in antennas:
		var aim: Vector3 = a["aim"]
		var stick := CylinderMesh.new()
		stick.top_radius = 0.01
		stick.bottom_radius = 0.01
		stick.height = 0.2
		var antenna := _mesh("Antenna_" + String(a["name"]), stick, HANDSET_COLOR.darkened(0.3), aim * 0.15)
		antenna.basis = Basis(Quaternion(Vector3.UP, aim))
		handset.add_child(antenna)
	_add_built(handset)


## A post up to the transmitter, labelled with its frequency.
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `GODOT_BIN=<Godot_v4.7.2-stable_win64_console.exe> bash scripts/run-godot-tests.sh all`
Expected: PASS (TOTAL: 296 checks, 0 failed; `E2E PASSED` for the open-loop and errors runs; the open-loop run prints `ok   falling back raises a collision toast: ["Hit LaunchPad at ... m/s"]`)

- [ ] **Step 5: Commit**

```bash
git add godot/scripts/app.gd godot/tests/e2e_open_loop.gd godot/tests/test_hud.gd godot/tests/test_scene.gd godot/ui/hud.gd godot/world/world.gd
git commit -F - <<'EOF'
feat(godot): the HUD link line shows the SNR, collisions raise toasts, the pilot's radio is marked on the field

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```

---

### Task 8: Docs, carried debt and the final check

The two models get their research notes (equations, every constant with its provenance, the link budget table); `docs/dev-setup.md` documents quad schema 4, the handset, collisions and the manual check; the carried-debt files record what is left and the rulings.

**Files:**
- Create: `docs/research/collision.md`
- Create: `docs/research/elrs-link.md`
- Create: `docs/superpowers/m3c-carried-debt.md`
- Modify: `README.md`
- Modify: `docs/dev-setup.md`
- Modify: `docs/research/video-link.md`
- Modify: `docs/superpowers/m3a-carried-debt.md`
- Modify: `docs/superpowers/m3b-carried-debt.md`

**Interfaces:**
- Consumes: everything above.
- Produces: docs only.

- [ ] **Step 1: Write the docs**

In `README.md`, replace:

```markdown
Status: **M3b — the analog video link.**
```

with:

```markdown
Status: **M3c — collisions and the control link on the geometry.**
```

In `README.md`, replace:

```markdown
  breaks the picture up with distance, attitude, buildings and interference (M3b, `docs/research/video-link.md`).
```

with:

```markdown
  breaks the picture up with distance, attitude, buildings and interference (M3b, `docs/research/video-link.md`).
- The drone collides with the field's objects, and the ExpressLRS link fades with range, buildings, antenna
  orientation and the ground bounce on the same propagation model (M3c, `docs/research/collision.md`,
  `docs/research/elrs-link.md`).
```

In `docs/dev-setup.md`, replace:

```markdown
  The game draws them from the world the server loaded (`GetWorld`), so what you see is what the video link sees.
  Nothing collides with the drone (the simulator's ground is the plane d = 0);
```

with:

```markdown
  The game draws them from the world the server loaded (`GetWorld`), so what you see is what the simulator flies in:
  the drone hits them (M3c, below);
```

In `docs/dev-setup.md`, replace:

```markdown
5. Untick Video effects on the F2 screen: the picture is clean whatever the link does.
```

with:

```markdown
5. Untick Video effects on the F2 screen: the picture is clean whatever the link does.

## Collisions and the ELRS link on the geometry (M3c)

**Quad files are schema 4.** The ELRS link now comes from the geometry between the pilot's radio and the quad, so
`[radio]` lost its fixed figures (`rssi_dbm`, `snr_db`, `loss_good`, `loss_bad`, `p_good_to_bad`, `p_bad_to_good`,
`rf_mode`, `tx_power`) and gained `tx_power_mw` (10, 25, 50, 100, 250, 500 or 1000; default 250) and
`[[radio.antennas]]` (one dipole, or two for diversity: `name`, `kind = "omni"`, `gain_dbi`, `polarization`,
`mount_frd`). `packet_rate_hz` must be an ExpressLRS 2.4 GHz LoRa rate (50, 150, 250, 500) that divides `sim.base_hz`.
A schema-3 file fails to load with a message naming the changes; port it from `quads/opendrone-5f-freestyle.toml`.

**The world file's `[handset]`** (optional) places the pilot's radio (default: 0.5 m below the goggles) and its
antennas (default: one upright 2 dBi dipole). The shipped world holds it at chest height beside the launch pad.

**Collisions.** The drone hits every object of the world file and the ground: its feet stand on the pad and on roofs
(the `[ground]` spring-damper), and spheres on the prop tips and the body bounce off walls, posts and pylons
(`[collision]`: `restitution`, `friction_coeff`, optional `spheres_frd_m`). A hit at 1 m/s or more is a `collision`
event and a toast in the game; `State.collision` (Python `state.collision`) counts them. The model is
`docs/research/collision.md`.

**The link.** Per packet: path loss, the dipoles' patterns and polarization, the frame's shadow, the buildings, the
ground bounce and fading give the RSSI; the packet rate's sensitivity decides the loss; LQ is the share of the last
100 packets. `State.radio` gains `snr_db`, `active_antenna` and `downlink_lq_pct`; the HUD's `LINK` line shows the SNR.
The model and its numbers are `docs/research/elrs-link.md`. Betaflight SITL ignores the link statistics frames (its
OSD link-quality element reads 0) until M3d; it does see packets stop, and fails safe.

Manual check (with the game; real Betaflight for 3 and 4):
1. Fly into building A's wall: the drone bounces off and a `Hit BuildingA` toast appears. Clip a gate post with a prop:
   the drone spins away.
2. Land on a roof (building C is the low wide one): the drone stands on it.
3. Fly low (1-2 m) away behind building B (the tall one east of the course): about 300 m out the HUD's LQ falls and
   Betaflight fails safe; climb a few metres and it may come back (the ground bounce).
4. Set `tx_power_mw = 10` in `quads/opendrone-5f-freestyle.toml` (put 250 back afterwards), reload (R) and fly behind
   building B again: the link drops about 100-150 m out.
```

Create `docs/research/collision.md` with:

```markdown
# Collisions with the world

How the quad touches the ground and the world file's objects (boxes and vertical cylinders). The code is
`crates/ofs-physics/src/collision.rs` (contacts, the broad phase, collision events) and
`crates/ofs-physics/src/rigid_body.rs` (the two contact models in the rigid body's step); the shapes and their signed
distance are `crates/ofs-core/src/shape.rs`. The design is
`docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md`.

The Godot client only draws the objects; the simulator does all the contacts, so a Python script and the game see the
same crash.

## Two contact models

**Landing contact points** (the quad file's `frame.contact_points_frd_m`, the four feet): a spring-damper with
friction, the model of M1. It now acts against the ground plane **and** every world object: a foot inside an object
is pushed out along the object's surface normal (the gradient of its signed distance) with
`k · depth - c · (velocity along the normal)`, and friction opposes the foot's sliding (`[ground]` gives `k`, `c` and
the friction coefficient). This is how the quad stands on the launch pad or lands on a roof.

**Collision spheres** (the quad file's `[collision]`): impulse contact against every object and the ground. After each
step's integration, every sphere inside something is found; the deepest contact pushes the whole body out along its
normal, then each contact in turn (deepest first, in a fixed order) gets:
1. a normal impulse that reverses the contact point's inward speed scaled by the restitution (`collision.restitution`,
   0.3), computed through the mass and the inertia tensor so an off-centre hit turns the quad. Below an inward speed
   of 0.2 m/s the restitution is 0, so nothing jitters at rest;
2. a Coulomb friction impulse against the contact point's sliding, at most `collision.friction_coeff` (0.5) times the
   normal impulse.

The impulses change the velocity within one step, so the accelerometer (`body.accel_ned_mps2`) feels them as a spike,
as a real IMU does in a crash.

**Why impulses for the spheres:** a spring stiff enough for a 12 cm gate post would need a far smaller time step; a
soft one lets a sphere sink 8-17 cm at 20-30 m/s, through the post. At 8 kHz the quad moves at most a few millimetres
per step, so a sphere is caught the step it touches, however thin the object.

**Why spheres stay above the feet:** a sphere reaching below the feet would hold the quad up instead of its legs and
turn every landing into an impulse. The defaults keep the lowest sphere at 2.5 cm below the centre and the feet at 3 cm.

## The default spheres

Without `collision.spheres_frd_m`, the quad gets 6 spheres of 1.5 cm radius evenly spaced on each prop's tip circle
(centred on the motor, radius `prop.diameter_m / 2`, in the motor plane) and one of 2.5 cm at the centre of mass: 25
spheres. On the shipped 5" quad no gap between them lets a 12 cm post through. A hard landing (several metres) compresses
the legs past the prop tips, which then strike, and the quad bounces: that is two collisions, as it would be.

## Objects standing on the ground

As for the radio links, an object standing on the ground continues below it: a quad that ends up inside a building
(a start position inside one, or a crash deep into it) is pushed out through the nearest side or the top, never down
into the ground where it would stay wedged.

## The broad phase

Each object's bounding box is kept; a sphere or foot is tested only against objects whose box, grown by the quad's
reach (the farthest foot or sphere surface from the centre of mass), contains the point. The shipped field has 30
objects; the cost per step is a handful of box tests.

## Events

A collision is a sphere or foot starting to touch an object (or the ground) at 1 m/s or more inward. A target touched
in consecutive steps is one contact; it must be free for 20 ms before touching it again counts anew. Gentle landings
raise nothing. The bus counts collisions (`body.collisions`) and keeps the last one's speed and target
(`body.collision_speed_mps`, `body.collision_object`: the object's index in the world file, -1 the ground); the server
turns each new count into a `collision` event ("BuildingA at 7.2 m/s") and `State.collision`. The game shows a toast
("Hit BuildingA at 7.2 m/s", "Hard landing at 3.1 m/s").

## Simplifications

- No damage: a crash changes the quad's motion only (crash damage is an M4 fault).
- The objects never move and do not collide with one another.
- Spheres approximate the props and the frame; the props do not stop or slow when they strike.
- The launch pad is a 5 mm mat: the quad starts on the ground at home, its feet 5 mm into the pad, and steps up onto
  it gently (no collision). A thicker pad would lift it with a jolt.
```

Create `docs/research/elrs-link.md` with:

```markdown
# The ExpressLRS control link model

How the simulator decides which of the pilot's radio packets reach the quad's receiver, and what the receiver tells
Betaflight about the link. The code is `crates/ofs-radio/src/elrs.rs` (the link, a model on the bus) on top of the
shared radio maths in `crates/ofs-rf` (the propagation of `docs/research/video-link.md`, the Rician fader, diversity
and the frame's shadow). The design is `docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md`.

The sensitivities and the `rf_mode` values come from ExpressLRS's source; every other constant is an **estimate**,
named in one place so it can be replaced by a measured value.

## Once per packet (50 to 500 Hz)

1. The handset samples the sticks (AETR, then AUX1-4) into a CRSF RC frame; `radio.latency_packets` delays it.
2. For the handset's antenna (the world file's `[handset]`) and each receiver antenna on the quad (the quad file's
   `[[radio.antennas]]`), compute the received power at 2440 MHz:

   `RSSI = Ptx + Gtx(direction) + Grx(direction) - FSPL - polarization loss - body shadow - obstruction`, with the
   ground bounce added by phase and a Rician fade on top, exactly as the video link does.
3. With two antennas on either end, each end uses its best antenna and changes only for one 2 dB better (the handset
   picks by path, the receiver by RSSI).
4. The packet is lost with probability `PER = 1 / (1 + exp((RSSI - sensitivity) / 1 dB))`, one uniform draw per
   packet. The transmitter being off (no pilot) or the `radio_link_loss` fault loses every packet.
5. A received packet writes its RC frame to Betaflight's UART; every `radio.link_stats_interval_packets` received
   packets a LINK_STATISTICS frame follows. LQ is the share of the last 100 packets received. With no packet in the
   last 100 the link is down: the receiver goes silent (ExpressLRS's default) and Betaflight's own failsafe acts.

## Constants

| What | Value | Provenance |
|---|---|---|
| Frequency | 2440 MHz, no hopping | middle of the 2.4 GHz ISM band |
| Sensitivity, 500 Hz (LoRa SF5) | -105 dBm | ExpressLRS `RxSensitivity`, SX1280 (master 15c78990e7, `src/src/common.cpp`) |
| Sensitivity, 250 Hz (SF6) | -108 dBm | same |
| Sensitivity, 150 Hz (SF7) | -112 dBm | same |
| Sensitivity, 50 Hz (SF8) | -115 dBm | same (the design's -117 was a misremembered figure) |
| PER slope | 1 dB | estimate (LoRa demodulation is steep) |
| Noise floor | -174 + 10 log10(812.5 kHz) + 6 dB = -108.9 dBm | LoRa bandwidth; the 6 dB noise figure is an estimate |
| Receiver's telemetry power | 100 mW | estimate |
| Handset power | 10, 25, 50, 100, 250, 500, 1000 mW | the quad file's `radio.tx_power_mw` (ExpressLRS's power levels) |
| Dipole | 2 dBi, linear, doughnut pattern floored 20 dB below its peak | `docs/research/video-link.md` |

LoRa decodes below the noise floor, so the SNR the link reports (RSSI minus the noise floor) can go negative while
packets still arrive: the 50 Hz sensitivity, -115 dBm, is an SNR of -6.1 dB (500 Hz's -105 dBm is +3.9 dB).

Packet rates must divide `sim.base_hz` (the model steps on whole base ticks): at the shipped 8 kHz, 50, 250 and 500
Hz do, 150 Hz does not.

## LINK_STATISTICS

The frame carries, as ExpressLRS sends them: `uplink_rssi_1` and `_2` (each receiver antenna's RSSI as -dBm; the
first again with one antenna), `uplink_lq`, `uplink_snr`, `active_antenna`, `rf_mode` (ExpressLRS's rate enum: 50 Hz
21, 150 Hz 24, 250 Hz 27, 500 Hz 29), `uplink_tx_power` (Crossfire's power index: 10 mW 1, 25 mW 2, 50 mW 8, 100 mW 3,
250 mW 7, 500 mW 4, 1000 mW 5), and the downlink's RSSI, LQ and SNR (the same path back from the receiver's 100 mW,
with its own fade and its own 100-packet window).

**Betaflight SITL ignores these frames today.** Its build compiles CRSF link statistics (`USE_CRSF_LINK_STATISTICS`)
only with CRSF telemetry, which SITL leaves out (see `docs/research/sitl-interface.md` §5), so Betaflight's OSD
link-quality element reads 0 whatever the link does. Betaflight still sees the link where it matters: RC frames stop
when packets are lost, and its RX-loss failsafe acts. Enabling the statistics is M3d's work (the SITL patch).

## Link budget check

250 mW at 500 Hz, with 2 dBi dipoles at both ends at their peak and co-polarized, without fading, ground bounce or
body shadow, reaches the sensitivity at 43.6 km; 10 mW reaches it at 8.7 km (a unit test pins both). Real links are
much shorter because of the effects below.

In the shipped world (handset beside the launch pad, quad level, no fading):

| Quad | 250 mW | 10 mW |
|---|---|---|
| 100 m out, behind building B, 1.5 m up | -77 dBm, no loss | -91 dBm, no loss |
| 150 m, behind B (and C), 1.5 m up | -99 dBm, 0.2 % loss | -113 dBm, link lost |
| 300 m, behind B, 1.5 m up | -108 dBm, 96 % loss | link lost |
| 300 m, behind B, 5 m up | -99 dBm, 0.3 % loss | link lost |
| 100 m straight above the handset | -81 dBm | -95 dBm |

What the table shows:
- **Buildings** cost up to their `rf_loss_db`, fading in over a few metres at their edges (knife-edge diffraction),
  and two buildings on the path add up. Behind a building the fading turns Rayleigh-like, so packets drop in bursts
  before the average says so.
- **The ground bounce** with linear antennas makes deep fades that move with height: a few metres up or down can be
  the difference between a full link and a lost one. This is the classic 2.4 GHz behaviour of a low quad.
- **Antenna orientation:** both dipoles upright put each in the other's null straight overhead (-20 dB each); a quad
  banked 90 degrees crosses its dipole with the handset's (another 20 dB).
- **The frame's shadow** (up to 8 dB) costs signal when the handset is ahead of and below the quad.

## Where it shows

- Bus: `radio.rssi_dbm`, `radio.snr_db`, `radio.antenna`, `radio.lq_pct`, `radio.downlink_lq_pct`, `radio.link_up`.
- Protocol: `State.radio` (`rssi_dbm`, `snr_db`, `active_antenna`, `lq_pct`, `downlink_lq_pct`), `link_down` /
  `link_up` events.
- Game: the HUD's `LINK` line (LQ, RSSI, SNR).
```

In `docs/research/video-link.md`, replace:

```markdown
The code is `crates/ofs-video/src/propagation.rs` (the physics, pure functions) and `crates/ofs-video/src/link.rs`
(the receiver, a model on the bus). The design is `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md`.
```

with:

```markdown
The code is `crates/ofs-rf/src/propagation.rs` (the physics, pure functions, shared with the ExpressLRS link of
`docs/research/elrs-link.md`) and `crates/ofs-video/src/link.rs` (the receiver, a model on the bus). The design is `docs/superpowers/specs/2026-10-09-m3b-video-link-design.md`.
```

In `docs/superpowers/m3a-carried-debt.md`, replace:

```markdown
- **CRSF telemetry back to the radio.** The HUD keeps reading the battery from the simulator's own models; SITL's CRSF telemetry also needs the atomic-block shim from M0 (§5).
```

with:

```markdown
- **CRSF telemetry back to the radio (M3d).** The HUD keeps reading the battery from the simulator's own models; SITL's CRSF telemetry also needs the atomic-block shim from M0 (§5). The same build switch leaves out CRSF link statistics (`USE_CRSF_LINK_STATISTICS`), so Betaflight ignores the LINK_STATISTICS frames the M3c link sends and its OSD link-quality element reads 0.
```

In `docs/superpowers/m3b-carried-debt.md`, replace:

```markdown
- **Collision with world objects.** The world file's boxes and cylinders are drawn and block the video signal, but the drone flies through them: the physics contact model still knows only the ground plane. The world file is the natural input for it.
- **ELRS on the shared propagation.** `ofs-video::propagation` is pure and frequency-agnostic; the ELRS link still uses the quad file's fixed RSSI, SNR and loss figures. Moving it onto the same geometry would make range and buildings cost control-link quality too.
```

with:

```markdown
- **Collision with world objects.** Done in M3c (`docs/research/collision.md`).
- **ELRS on the shared propagation.** Done in M3c: the propagation moved to `ofs-rf` and the ELRS link uses it (`docs/research/elrs-link.md`).
```

Create `docs/superpowers/m3c-carried-debt.md` with:

```markdown
# M3c carried debt

These items were deliberately left out of M3c, or surfaced while building it. They are listed so M3d, M3e and M4
planning and the maintainer can pick them up.

## Deferred features
- **Betaflight's link statistics (M3d).** SITL compiles `USE_CRSF_LINK_STATISTICS` only with CRSF telemetry, which it
  leaves out, so Betaflight ignores the LINK_STATISTICS frames: its OSD link-quality and RSSI-dBm elements read 0. The
  SITL patch of M3d (the atomic-block shim for CRSF telemetry) should enable both.
- **900 MHz ExpressLRS, dynamic power, frequency hopping, 2.4 GHz interference (Wi-Fi, other pilots).**
- **Crash damage (M4).** A collision changes the motion only; prop strikes do not stop or slow the motors.
- **Packet rates that do not divide the base rate.** 150 Hz does not divide 8 kHz, so the shipped quad cannot use it;
  the link model steps on whole base ticks.
- **The FLRC and DVDA ExpressLRS modes** (F500, F1000, D250, D500): only the four LoRa rates are modelled.
- **Frequency-dependent object losses.** An object's `rf_loss_db` caps both the 5.8 GHz and the 2.4 GHz knife-edge loss.

## Modelling simplifications (estimates, see docs/research/elrs-link.md and docs/research/collision.md)
- The packet error rate is a logistic curve 1 dB wide around the sensitivity; the noise figure (6 dB) and the
  receiver's telemetry power (100 mW) are estimates.
- The downlink is computed every packet (no telemetry ratio), from the same path and its own fade.
- The handset picks its antenna by the unfaded path, not by telemetry as ExpressLRS does.
- Collision spheres approximate the props and the frame; the objects are static and do not collide with each other.

## Rulings made while planning
- **The 50 Hz sensitivity is -115 dBm**, as ExpressLRS's source gives it for the SX1280 (master 15c78990e7); the spec's
  -117 was a misremembered figure.
- **`rf_mode` is ExpressLRS's rate enum** (50 Hz 21, 150 Hz 24, 250 Hz 27, 500 Hz 29) and `uplink_tx_power` Crossfire's
  power index, as ExpressLRS fills LINK_STATISTICS.
- **The body sphere is 2.5 cm, not 4 cm.** A 4 cm sphere reaches below the 3 cm feet and holds the quad up instead of
  its legs; gaps between the default spheres stay under 1.5 cm.
- **The launch pad is a 5 mm mat** (it was 2 cm): the quad starts on the ground at home, so its feet start inside the
  pad, which (standing on the ground) continues below it and lifts them onto its top; 5 mm makes that a gentle step
  instead of a 2 cm jolt.
- **Collision shapes standing on the ground are rooted below it**, as the radio links' obstacles are: otherwise a quad
  inside a building near its bottom is pushed down into the ground and stays wedged there.
- **Collision state travels as a `Collision` message** (`count`, `speed_mps`, `object`) in `State.collision`, rather than
  the spec's single `State.collision_speed_mps`, so clients see what was hit and how often; the bus gains
  `body.collisions` for the same reason. Several collisions between two state samples make one event saying how many.
- **A hard landing is several collisions.** From a few metres the legs compress past the prop tips, which strike and
  bounce the quad: each touch after 20 ms apart is a new collision. The open-loop tests use a 15 cm drop for the
  single-collision case.
- **The live failsafe test checks that Betaflight will not arm 50 km out at 10 mW**, not the OSD link-quality cell
  (Betaflight ignores link statistics, above). The open-loop test cannot move the quad from home to 50 km, so the
  50 km sessions start there and the link never comes up (no `link_down` event).
```

- [ ] **Step 2: Run the tests to see them pass**

Run: `cargo test --workspace --locked`
Expected: PASS (383 tests)

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings

Run: `wsl_run ~/.cargo/bin/cargo test --workspace --locked`
Expected: PASS on Linux

Run: `wsl_run ~/.cargo/bin/cargo test --locked -p ofs-fc -p ofs-sim -p ofs-video --test sitl_live --test vtx_live -- --ignored --test-threads=1`
Expected: PASS against real Betaflight

Run: `OFS_SIM_BIN=<a firewall-allowed ofs-sim.exe> OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/hugow/ofs/betaflight/obj/main/betaflight_SITL.elf" python -m pytest python/tests -q`
Expected: PASS (38 passed)

Run: `OFS_SIM_BIN=<the same> OFS_SITL_LAUNCH=<the same> GODOT_BIN=<Godot console exe> bash scripts/run-godot-tests.sh all`
Expected: PASS, including the game flying real Betaflight

- [ ] **Step 3: Commit**

```bash
git add README.md docs/dev-setup.md docs/research/collision.md docs/research/elrs-link.md docs/research/video-link.md docs/superpowers/m3a-carried-debt.md docs/superpowers/m3b-carried-debt.md docs/superpowers/m3c-carried-debt.md
git commit -F - <<'EOF'
docs: the collision and ELRS link models, M3c setup and manual check, carried debt

Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>
EOF
```
