# Rotated boxes in the world Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A box in the world file can be yawed; the collision solver, the radio and video obstruction, and the Godot client all see the same rotated box; a world that writes no yaw is bit-for-bit today. Closes #28.

**Architecture:** `ofs_core::shape::Shape::Box` gains a `yaw_rad` field with an exact `yaw_rad == 0.0` fast path that runs today's code (the bit-for-bit rule). The signed distance, the normal, the bounds and the RF rooting all rotate inside `Shape`, so every consumer — collision spheres, landing points, the golden-section obstruction search — rotates through the one enum. `[[objects]]` boxes gain `yaw_deg` (`ofs-config`), `object_shape` canonicalizes it into the shape (`ofs-sim`), and it travels to the client as `WorldObject.yaw_deg` (protocol 17); Godot turns the node by `PI - deg_to_rad(yaw)`.

**Tech Stack:** Rust workspace (glam `DVec3`, MSRV 1.85), tonic/prost proto, Python `ofs` (generated stubs via grpc_tools), Godot 4.7.2 GDScript + the gdext bridge.

**Spec:** `docs/superpowers/specs/2026-10-31-rotated-boxes-design.md` (read it first; its rulings are binding).

## Global Constraints

- Run Rust tests in WSL: from the worktree root, `CARGO_TARGET_DIR=$HOME/ofs/target-rotboxes ~/.cargo/bin/cargo test -p <crate> --locked` (own target dir, so other sessions do not collide). `pkill -x betaflight_SITL`, never `pkill -f`.
- Files are LF. After writing any file: `tr -cd '\r' < file | wc -c` must print 0.
- No golden moves (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`) and no change to `crates/ofs-sim/tests/flat_world_pin.rs`: worlds that write no yaw are byte-identical. The object-world pin of Task 1 is recorded BEFORE any other change.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo +1.85 check` passes on every changed crate.
- Protocol 17 is a coordinated bump: `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` change together (`crates/ofs-proto/tests/messages.rs:6-10` enforces it). Regenerate the Python stubs after the proto edit: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` (the table in `docs/dev-setup.md:21`); `python/tests/test_proto_stubs.py` fails if you forget.
- Python tests need `PYTHONPATH=<worktree>/python`. Godot tests: `bash scripts/run-godot-tests.sh unit` in WSL (it downloads Godot 4.7.2).
- Do not edit historical CHANGELOG entries or historical "protocol 16" attributions (`CHANGELOG.md`, `docs/research/collision.md:208`): update statements of what we speak now, not of when something arrived.
- Open PR #170 (DVR audio) touches `crates/ofs-client/src/model.rs` and `crates/ofs-godot/src/lib.rs` too and bumps no protocol: expect a trivial rebase if it merges mid-task; nothing in this plan depends on it.
- Commit trailers: none prescribed; add none.

## Design calls (decided in the spec; the executor ledgers any change)

1. **Yaw only.** `yaw_deg`, degrees clockwise from north (0 = north, 90 = east) — the convention of `pilot.facing_deg` and `gates[].heading_deg`. Pitch/roll are declined: `rooted_on`'s straight-down RF rooting is exact only with the vertical axis fixed, and no consumer needs a tilt.
2. **`Shape::Box { center, half, yaw_rad }`** with an identity fast path on `yaw_rad == 0.0` in `signed_distance`, `normal` and `bounds`: bit-identity by construction, not by an IEEE argument.
3. **Canonicalize at `object_shape`**: `yaw_deg.rem_euclid(360.0).to_radians()` — exact, so `0`, `-0.0`, `360`, `720` all take the identity path. Validation is finite-only (the house angle rule) plus the cross-field rule: `yaw_deg` on a cylinder is a problem.
4. **The AABB of a rotated box is exact**: `(|c|·hₓ + |s|·h_y, |s|·hₓ + |c|·h_y, h_z)` — the broad phase stays correct; a rotated box is convex, so the RF golden-section search stays exact.
5. **Protocol 17**: `WorldObject.yaw_deg = 9` (`double`, box only, 0 = axis-aligned). The house bumps on every wire change; the handshake refuses a protocol-16 client.
6. **No digest change, no new signal**: rotation reaches the lockstep digest through the body state, collision events and link signals it shapes, exactly as position and `rf_loss_db` do.
7. **Meadow demonstrates it**: Gate1 gets its bar (`yaw_deg = 135.0`, across the travel); `worlds/meadow.toml:64` documents the wait. No other shipped world changes.

## Review Focus

- A box with no `yaw_deg`, with `0.0`, with `-0.0` and with `360.0` all produce `Shape::Box { yaw_rad: 0.0 }` — the object-world pin holds (Tasks 1-3).
- The rotated AABB contains the rotated corners at awkward angles (30, 45, 90, 210 degrees), so the broad phase never misses a yawed wall (Task 2).
- The golden-section obstruction search bottoms out where the rotated faces are: yaw 90 degrees equals the swapped-extents wall within 1e-12 (Task 4).
- The identity path computes no trig, so no existing golden or pin can move on any OS (Tasks 1-2).
- A pre-protocol-17 client is refused by the handshake, not left misreading `WorldObject` (Task 5).
- Godot turns the box by the same yaw the physics collides: a yaw-90 box's long axis lies along east, and a box without `yaw_deg` keeps exactly today's node transform (Task 6).

---

### Task 1: Baseline pin of an object world (do this BEFORE any other change)

**Files:**
- Create: `crates/ofs-sim/tests/rotated_boxes.rs`

**Interfaces:**
- Consumes: `ofs_sim::vehicle::{build, BuildOptions, Sticks}`, `Vehicle::{run_for, digest, state}` (the pattern of `crates/ofs-sim/tests/flat_world_pin.rs`), `ofs_config::{FcKind, QuadConfig, WorldConfig}`, `ofs_config::world::{ObjectSection, Shape}`.
- Produces: `object_world()` (a helper later tasks extend) and per-OS digest constants that Tasks 2-4 must keep passing unmodified.

- [ ] **Step 1:** Write the test with placeholder constants:

```rust
//! Pins the bus digest of a world with a box in the flight path, recorded on `main` before the rotated-boxes work,
//! so a world that writes no yaw can be shown to run bit-identically afterwards. The quad climbs into a slab and
//! rests against it (spheres and landing points touch the box, the RF loss runs through it), so the whole object
//! chain feeds the digest. libm differs per OS: like flat_world_pin.rs, Windows and Linux/WSL have their own pins.
#![cfg(any(windows, target_os = "linux"))]
use std::path::Path;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks};

#[cfg(target_os = "linux")]
const PIN_HIT: u64 = 0;
#[cfg(target_os = "linux")]
const PIN_REST: u64 = 0;
#[cfg(windows)]
const PIN_HIT: u64 = 0;
#[cfg(windows)]
const PIN_REST: u64 = 0;

/// The open field with one 10 x 10 m slab hanging 3 m over home: the climbing quad meets its underside.
pub fn object_world() -> WorldConfig {
    let mut w = WorldConfig::open_field();
    w.objects.push(ofs_config::world::ObjectSection {
        name: "Slab".into(),
        shape: ofs_config::world::Shape::Box,
        center_ned_m: [0.0, 0.0, -23.0],
        size_m: Some([10.0, 10.0, 40.0]),
        radius_m: None,
        height_m: None,
        color: [0.6, 0.6, 0.6],
        rf_loss_db: 25.0,
    });
    w
}

#[test]
fn an_unrotated_object_world_is_pinned() {
    let cfg: QuadConfig = load(Path::new(&format!("{}/../../quads/opendrone-5r-race.toml", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: object_world() };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.set_sticks(&Sticks { throttle: 0.3, ..Sticks::default() }).unwrap();
    v.run_for(1.0).unwrap();
    let hit = v.digest();
    assert!(v.state().collision_event_count >= 1, "the flight must touch the slab, else the pin proves nothing");
    v.set_sticks(&Sticks { throttle: 0.0, ..Sticks::default() }).unwrap();
    v.run_for(1.0).unwrap();
    let rest = v.digest();
    println!("hit={hit} rest={rest}");
    assert_eq!(hit, PIN_HIT, "digest after the climb into the slab");
    assert_eq!(rest, PIN_REST, "digest after resting against it");
}
```

- [ ] **Step 2:** Run `cargo test -p ofs-sim --test rotated_boxes` in WSL — FAIL showing the real digests in the `println!`. Fill the Linux constants; if you can also run natively on Windows, fill those too (else ledger it and keep the `cfg` gate). If the collision assertion fails (the quad misses the slab), widen `size_m` to `[20.0, 20.0, 40.0]` and re-record — the pin must exercise the box.
- [ ] **Step 3:** Run again: PASS. Commit `test(sim): pin an object world's bus digest before the rotated boxes (#28)`.

### Task 2: The yawed box in `Shape`

**Files:**
- Modify: `crates/ofs-core/src/shape.rs`, and every `Shape::Box {` construction in `crates/ofs-core/tests/shape.rs`, `crates/ofs-physics/tests/collision.rs`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-radio/tests/elrs.rs`, `crates/ofs-radio/tests/golden.rs`, `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-video/tests/link.rs`, `crates/ofs-sim/src/vehicle.rs` (22 sites in 8 files; with shape.rs's 5 match arms that is every `Shape::Box {` in the workspace — the compiler lists them)
- Test: `crates/ofs-core/tests/shape.rs`

**Interfaces:**
- Produces: `Shape::Box { center: DVec3, half: DVec3, yaw_rad: f64 }` (radians, clockwise from north; `0.0` = today's box exactly). No signature anywhere else changes: `signed_distance`, `normal`, `bounds`, `rooted_on` keep theirs and now honour the yaw.

- [ ] **Step 1:** Write the failing tests in `crates/ofs-core/tests/shape.rs` (beside the existing `BOX` rig):

```rust
fn yawed(yaw_rad: f64) -> Shape {
    Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(2.0, 1.0, 5.0), yaw_rad }
}

#[test]
fn a_yaw_of_90_degrees_is_the_swapped_extents_box() {
    // cos(pi/2) is 6.1e-17, not 0, so compare with a tolerance; every other value agrees to the last useful bit.
    let b = yawed(std::f64::consts::FRAC_PI_2);
    let swapped = Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(1.0, 2.0, 5.0), yaw_rad: 0.0 };
    for p in [DVec3::new(14.0, 25.0, -6.0), DVec3::new(9.0, 21.5, -2.0), DVec3::new(12.0, 18.0, -11.0)] {
        assert!((b.signed_distance(p) - swapped.signed_distance(p)).abs() < 1e-12);
    }
    close_dir(b.normal(DVec3::new(14.0, 20.0, -5.0)), DVec3::Y, "yaw 90: the local +x face now faces east");
}

#[test]
fn a_45_degree_face_normal_is_the_rotated_axis() {
    // The point is inside, nearest the box's own +x face; the outward normal is that face's axis rotated to
    // its heading, the north-east diagonal.
    let n = yawed(std::f64::consts::FRAC_PI_4).normal(DVec3::new(12.0, 20.0, -5.0));
    close_dir(n, DVec3::new(std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2, 0.0).normalize(), "north-east");
}

#[test]
fn the_bounds_of_a_yawed_box_are_its_exact_corners() {
    let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let b = yawed(30f64.to_radians()).bounds();
    let want = DVec3::new(c.abs() * 2.0 + s.abs() * 1.0, s.abs() * 2.0 + c.abs() * 1.0, 5.0);
    assert!((b.max - DVec3::new(10.0, 20.0, -5.0) - want).length() < 1e-12);
    assert!((DVec3::new(10.0, 20.0, -5.0) - b.min - want).length() < 1e-12);
    // The corners are on the bounds at every awkward angle (the broad phase never misses).
    for yaw in [0.7, std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2, 3.665] {
        let shape = yawed(yaw);
        let (sn, cs) = (yaw.sin(), yaw.cos());
        for (u, v) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            // The local corner (u*2, v*1) rotated to its heading: (c*ln - s*le, s*ln + c*le).
            let corner = DVec3::new(10.0 + u * cs * 2.0 - v * sn, 20.0 + u * sn * 2.0 + v * cs, -5.0);
            assert!(shape.bounds().grown(1e-9).contains(corner), "yaw {yaw}: a corner outside the AABB");
        }
    }
}

#[test]
fn a_zero_yaw_gives_todays_values_exactly() {
    // Hand-computed on dyadic points (all exact in f64): the values the axis-aligned box of half (2, 1, 5)
    // has always returned, through the identity path, bit for bit.
    let b = yawed(0.0);
    assert_eq!(b.signed_distance(DVec3::new(13.5, 20.0, -5.0)), 1.5);   // 1.5 m out past the +x face
    assert_eq!(b.signed_distance(DVec3::new(10.0, 20.0, -5.0)), -1.0); // the centre: the nearest face is 1 m away
    assert_eq!(b.signed_distance(DVec3::new(11.0, 21.5, -2.0)), 0.5);  // an edge, diagonally out past the y face
    for p in [DVec3::new(13.5, 20.0, -5.0), DVec3::new(10.0, 20.0, -5.0)] {
        assert_eq!(yawed(-0.0).signed_distance(p), b.signed_distance(p), "-0.0 is identity too");
    }
}

#[test]
fn rooting_keeps_the_yaw() {
    let b = yawed(0.8).rooted_on(None);
    let Shape::Box { yaw_rad, .. } = b else { panic!("still a box") };
    assert_eq!(yaw_rad, 0.8);
    assert!(b.signed_distance(DVec3::new(10.0, 20.0, 500.0)) < 0.0, "rooted down");
}
```

- [ ] **Step 2:** Run `cargo test -p ofs-core --test shape` — FAIL (no `yaw_rad` field).
- [ ] **Step 3:** Implement in `crates/ofs-core/src/shape.rs`. Add the field to the variant (doc: radians, clockwise from north, `0` = the world's axes). Add one helper and branch on identity in each method:

```rust
/// The point in the box's local frame (the north/east pair rotated back by the yaw); identity yaw returns `d`.
fn local(yaw_rad: f64, d: DVec3) -> DVec3 {
    if yaw_rad == 0.0 { d } else { let (s, c) = yaw_rad.sin_cos(); DVec3::new(c * d.x + s * d.y, -s * d.x + c * d.y, d.z) }
}
/// A local direction back to the world frame; identity yaw returns `n`.
fn world_dir(yaw_rad: f64, n: DVec3) -> DVec3 {
    if yaw_rad == 0.0 { n } else { let (s, c) = yaw_rad.sin_cos(); DVec3::new(c * n.x - s * n.y, s * n.x + c * n.y, n.z) }
}
```

  - `signed_distance`: `let d = local(yaw_rad, p - center);` then today's formula on `d` unchanged.
  - `normal`: compute today's logic on `local(yaw_rad, p - center)` (the tie-break and `signum` stay in the local frame), then `world_dir(yaw_rad, n)`.
  - `bounds`: identity yaw keeps today's arm; else `let (s, c) = yaw_rad.sin_cos();` and `half_world = DVec3::new(c.abs() * half.x + s.abs() * half.y, s.abs() * half.x + c.abs() * half.y, half.z)`.
  - `rooted_on`'s box arm: bind and pass through `yaw_rad`; the guard `center.z + half.z >= ground_d(center) - ON_GROUND_M` is unchanged (yaw moves no vertical extent).
  - Update the module doc (shape.rs:1-3) to say boxes may be yawed.

  Then fix the 22 construction sites across the workspace (`yaw_rad: 0.0`); the compiler lists them.
- [ ] **Step 4:** `cargo test -p ofs-core -p ofs-physics -p ofs-rf -p ofs-radio -p ofs-video --locked` PASS (the existing suites are the unrotated regression); `cargo test -p ofs-sim --test rotated_boxes --test flat_world_pin` PASS. Clippy clean. Commit `feat(core): a yawed box in Shape, bit-identical unrotated (#28)`.

### Task 3: `yaw_deg` in the world file

**Files:**
- Modify: `crates/ofs-config/src/world.rs` (`ObjectSection`, validation)
- Test: `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Produces: `ObjectSection { ..., /// Box only: the heading of the box's north axis (0 = north, 90 = east, clockwise). Absent = axis-aligned. #[serde(default)] pub yaw_deg: Option<f64> }`. Everything that reads `WorldConfig` is unchanged (`Option` default `None`).

- [ ] **Step 1:** Write the failing tests in `crates/ofs-config/tests/world.rs` (beside `the_shipped_world_loads`):
  - `yaw_deg_parses_and_defaults`: a world text with a box carrying `yaw_deg = 135.0` loads with `objects[i].yaw_deg == Some(135.0)`; the same box without the key is `None`; `the_open_field_is_valid_and_bare` still holds (its `objects.is_empty()` assertion).
  - `every_yaw_problem_is_reported_at_once`: from the same text, `yaw_deg = nan` → `"objects[0].yaw_deg: must be finite"`; a cylinder with `yaw_deg = 10.0` → `"objects[1].yaw_deg: yaw_deg is for a box"`.
- [ ] **Step 2:** Run `cargo test -p ofs-config --test world yaw` — FAIL (unknown field).
- [ ] **Step 3:** Implement: the field on `ObjectSection` (world.rs:341-360), and in the objects loop of `validate` (world.rs:543-561) inside the existing `match o.shape`: the `Box` arm checks `c.check(o.yaw_deg.is_none_or(|y| y.is_finite()), &field("yaw_deg"), "must be finite")`; the `Cylinder` arm checks `c.check(o.yaw_deg.is_none(), &field("yaw_deg"), "yaw_deg is for a box")` — the placement of the existing cross rules (`world.rs:553`, `world.rs:558`). `WorldConfig::open_field()` needs nothing (no objects); the Task 1 test's `ObjectSection` literal gains `yaw_deg: None` (the compiler says so) — the pin test's assertions stay untouched.
- [ ] **Step 4:** Tests PASS; whole `cargo test -p ofs-config --locked` green. Commit `feat(config): yaw_deg on a box in the world file (#28)`.

### Task 4: The consumers — collisions, the link, replay

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (`object_shape` at :753, its `#[cfg(test)]` module), `crates/ofs-sim/tests/rotated_boxes.rs`
- Test: `crates/ofs-physics/tests/collision.rs`, `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-sim/tests/rotated_boxes.rs`

**Interfaces:**
- Consumes: Task 2's `Shape::Box { .., yaw_rad }`, Task 3's `ObjectSection.yaw_deg`.
- Produces: `object_shape` maps `o.yaw_deg.map_or(0.0, |y| y.rem_euclid(360.0).to_radians())` into `yaw_rad`. `world_objects` and `world_obstacles` (vehicle.rs:655, :744) keep their signatures and now carry the yaw through.

- [ ] **Step 1: Physics.** In `crates/ofs-physics/tests/collision.rs` (the rig of `params`/`state`/`resolve` at :23-62, :242). This is the consumer's proof of Task 2's engine — it passes the moment it is written; if it does not, the bug is in `Shape`, not here:

```rust
#[test]
fn a_sphere_bouncing_off_a_yawed_wall_leaves_along_the_rotated_normal() {
    // A wall thin along its own north axis, yawed 45 degrees, so its near face's outward normal is the
    // -(north-east) diagonal. The body (one sphere at the centre, r = 0.04) starts touching that face and
    // slides north into it at 2 m/s: the contact turns the velocity about the rotated normal — v - (1+e)(v.n)n
    // with e = 0.5 gives (0.5, -1.5) before friction; assert the signs and the slowdown, not the exact bits.
    // 2 m/s (not 1) so the inward speed 2*cos(45) = 1.4 m/s clears EVENT_MIN_SPEED_MPS and one event fires.
    let p = params(
        vec![WorldObject {
            name: "wall".into(),
            shape: ofs_core::shape::Shape::Box {
                center: DVec3::new(0.2, 0.2, -5.0),
                half: DVec3::new(0.25, 5.0, 5.0),
                yaw_rad: std::f64::consts::FRAC_PI_4,
            },
        }],
        vec![(DVec3::ZERO, 0.04)],
        0.5,
    );
    let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect::<Vec<_>>();
    let mut touch = TouchState::new(p.objects.len());
    let mut s = state(DVec3::new(0.0, 0.0, -0.06), DVec3::new(2.0, 0.0, 0.0));
    let events = ofs_physics::collision::resolve(&p, &bounds, 0.04, &mut s, &[], &mut touch, 0.0);
    assert_eq!(events.len(), 1, "one hard touch");
    assert!(s.vel_ned_mps.x < 1.0, "slowed along the approach: {:?}", s.vel_ned_mps);
    assert!(s.vel_ned_mps.y < -0.5, "deflected west along the rotated face: {:?}", s.vel_ned_mps);
}
```

  Run `cargo test -p ofs-physics --test collision` — PASS (re-check the geometry comments, not the engine, if not).
- [ ] **Step 2: RF.** In `crates/ofs-rf/tests/propagation.rs`:

```rust
#[test]
fn a_yawed_wall_obstructs_where_its_rotated_faces_are() {
    let a = DVec3::new(-20.0, 0.0, -5.0);
    let b = DVec3::new(20.0, 0.0, -5.0);
    let wall = |half: DVec3, yaw: f64| Obstacle { shape: Shape::Box { center: DVec3::new(0.0, 0.0, -5.0), half, yaw_rad: yaw }, rf_loss_db: 60.0 };
    let unrotated = obstruction_loss_db(&wall(DVec3::new(0.5, 2.0, 5.0), 0.0), a, b, 0.0517);
    let yawed = obstruction_loss_db(&wall(DVec3::new(0.5, 2.0, 5.0), std::f64::consts::FRAC_PI_2), a, b, 0.0517);
    let swapped = obstruction_loss_db(&wall(DVec3::new(2.0, 0.5, 5.0), 0.0), a, b, 0.0517);
    assert!((unrotated - yawed).abs() > 1.0, "the rotated wall meets the path with its thin side");
    assert!((swapped - yawed).abs() < 1e-12, "yaw 90 degrees is the swapped-extents wall (convexity keeps the search exact)");
}
```

  Run `cargo test -p ofs-rf --test propagation` — PASS (Task 2's engine already serves it); if not, the bug is in `Shape`, fix it there.
- [ ] **Step 3: The sim consumes the field.** In `object_shape` (vehicle.rs:753): `let yaw = o.yaw_deg.map_or(0.0, |y| y.rem_euclid(360.0).to_radians());` and `Shape::Box { center, half: DVec3::new(s[0], s[1], s[2]) * 0.5, yaw_rad: yaw }`. In the `#[cfg(test)]` module, beside `objects_on_hilly_ground_root_to_the_terrain` (:2300), add `objects_carry_the_yaw_and_360_is_identity`: `world_objects` of a box with `yaw_deg = 135.0` gives `Shape::Box { yaw_rad, .. }` with `yaw_rad == 135f64.to_radians()`; `yaw_deg = 360.0` and `0.0` both give exactly `0.0`; `world_obstacles`' rooted shape keeps the yaw (`let Shape::Box { yaw_rad, .. } = obstacles[0].shape`).
- [ ] **Step 4: Sim tests** in `crates/ofs-sim/tests/rotated_boxes.rs`:
  - `a_yawed_object_world_replays_exactly_and_differs_from_the_unyawed`: `let mut w = object_world(); w.objects[0].yaw_deg = Some(45.0);` — two vehicles from it (same quad/seed/sticks as the pin flight) replay to equal digests after each phase, and the yawed flight's digest differs from the unyawed `object_world()`'s (a 45-degree slab meets the climber with a corner, not a face).
  - The pin test of Task 1 still passes unmodified, its `yaw_deg: None` literal aside (the bit-for-bit claim).
- [ ] **Step 5:** `cargo test -p ofs-sim --test rotated_boxes --test flat_world_pin` and `cargo test --workspace --locked --no-fail-fast` PASS; clippy clean. Commit `feat(sim): collisions and the link see the rotated boxes (#28)`.

### Task 5: Protocol 17 and the clients

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs` (:1, :12), `crates/ofs-proto/tests/messages.rs` (:8), `crates/ofs-sim/src/session.rs` (`world_msg` at :244-259, its unit tests), `crates/ofs-client/src/model.rs` (`WorldObject` at :188-202, `from_pb` at :364-380), `crates/ofs-godot/src/lib.rs` (`get_world` objects at :480-491, its doc at :455), `python/ofs/client.py` (:26, `WorldObject` at :133-140, `_world` at :339-347), the regenerated `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py`
- Test: `crates/ofs-proto/tests/messages.rs`, `crates/ofs-sim/src/session.rs` unit tests, `crates/ofs-client/tests/model.rs`, `python/tests/test_proto_stubs.py`

**Interfaces:**
- Produces: `WorldObject.yaw_deg` (field 9, `double`; 0 = axis-aligned) on the wire; `ofs_client::model::WorldObject.yaw_deg: f64`; Godot `get_world()["objects"][i]["yaw_deg"]`; Python `WorldObject.yaw_deg: float = 0.0`. Sibling coordination (the umbrella's rule): #156's `WorldObject.motion` also plans field 9 — whoever lands second takes the next free field, re-verified against the worktree's `sim.proto` at execution.

- [ ] **Step 1: Failing tests.** messages.rs: `assert_eq!(PROTOCOL_VERSION, 17);`. session.rs unit test `the_world_message_carries_a_box_yaw`: a hand-built `WorldConfig` (the `building`-helper style, vehicle.rs:2278) with a yawed box → `world_msg(...).objects[0].yaw_deg == 135.0`; a box without `yaw_deg` → `0.0`. ofs-client `tests/model.rs`: `a_yawed_box_reaches_the_client` — a `pb::World` with one `WorldObject { yaw_deg: 135.0, .. }` parses to `yaw_deg == 135.0`, and a default `WorldObject` parses to `0.0`.
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3: Implement.** In `sim.proto` under `WorldObject` (after `rf_loss_db = 8;`): `double yaw_deg = 9;  // box only: heading of the box's north axis (0 = north, 90 = east, clockwise); 0 = axis-aligned`; update the header comment (:5) to "protocol version 17: protocol 16 plus a box's yaw (WorldObject.yaw_deg)". Bump `PROTOCOL_VERSION` and the crate doc (ofs-proto/src/lib.rs:1, :12) to 17; fix messages.rs's other expectations if it names 16 anywhere. `world_msg`: `yaw_deg: o.yaw_deg.unwrap_or(0.0),`. Client model: the field and `yaw_deg: o.yaw_deg,`; leave `size`'s Godot reordering (:369) alone — the yaw is frame-free. Bridge: `e.set("yaw_deg", o.yaw_deg);` and one doc word. Python: `PROTOCOL_VERSION = 17`, `yaw_deg: float = 0.0` at the end of the frozen dataclass (positional callers keep working), `_world` appends `o.yaw_deg`. Regenerate the stubs with the Global Constraints command; `test_proto_stubs.py` passes.
- [ ] **Step 4:** `cargo test --workspace --locked --no-fail-fast`; `PYTHONPATH=python python -m pytest python/tests/test_client.py python/tests/test_proto_stubs.py -q`. Update live protocol references: `grep -rn "protocol 16" --include=*.md --include=*.py --include=*.gd . | grep -v superpowers/ | grep -v CHANGELOG` — update `docs/dev-setup.md:268` ("speak protocol 16") and the walk's last item (:284-285); leave historical attributions (Global Constraints).
- [ ] **Step 5:** Commit `feat(proto)!: protocol 17 - a box's yaw in the World message (#28)`.

### Task 6: Godot draws the yaw

**Files:**
- Modify: `godot/world/world.gd` (`build` at :61-66, `_box` at :239-242)
- Test: `godot/tests/test_scene.gd`

**Interfaces:**
- Consumes: Task 5's `"yaw_deg"` dict key.
- Produces: box nodes turned by their yaw; a world dict without `"yaw_deg"` draws exactly today's nodes.

- [ ] **Step 1:** Failing tests in `test_scene.gd` (beside the object-building tests at :38-68, in the file's own style — `var world := World.new()`, `await add_to_tree(world)`, the `ok`/`eq` helpers of `testing.gd`):

```gdscript
func test_a_yawed_box_is_drawn_rotated_and_no_yaw_is_today_node() -> void:
	var world := World.new()
	await add_to_tree(world)
	var dict := {"objects": [
		{"name": "Bar", "shape": "box", "center": Vector3.ZERO, "size": Vector3(1.0, 1.0, 8.0), "color": Color.WHITE, "yaw_deg": 90.0},
		{"name": "Plain", "shape": "box", "center": Vector3.ZERO, "size": Vector3(1.0, 1.0, 8.0), "color": Color.WHITE},
	]}
	world.build(dict)
	# The size's z (the box's north extent, 8 m) yawed 90 degrees lies along east.
	var bar: MeshInstance3D = world.get_node("Bar")
	var long_axis: Vector3 = bar.global_transform.basis * Vector3(0.0, 0.0, 1.0)
	ok(long_axis.distance_to(Vector3(1.0, 0.0, 0.0)) < 1e-6, "yaw 90 turns the long (north) axis to east")
	var plain: MeshInstance3D = world.get_node("Plain")
	eq(plain.transform, Transform3D(), "no yaw_deg leaves the node exactly unrotated (center at the origin)")
	world.queue_free()
```
- [ ] **Step 2:** Run `bash scripts/run-godot-tests.sh unit` (WSL) — FAIL on the new assertions.
- [ ] **Step 3:** Implement: `"box": _box(o["name"], o["center"], o["size"], o["color"], float(o.get("yaw_deg", 0.0)))` and:

```gdscript
func _box(node_name: String, center: Vector3, size: Vector3, color: Color, yaw_deg := 0.0) -> void:
	var mesh := BoxMesh.new()
	mesh.size = size
	var instance := _mesh(node_name, mesh, color, center)
	# NED yaw h turns the box's north axis to the Godot image of heading h, (sin h, 0, -cos h); rotation.y
	# of PI - h puts a node's +Z exactly there. The box is 180-symmetric, but skip the turn entirely for
	# yaw 0 so today's nodes keep today's transform.
	if not is_zero_approx(yaw_deg):
		instance.rotation.y = PI - deg_to_rad(yaw_deg)
	_add_built(instance)
```

- [ ] **Step 4:** `bash scripts/run-godot-tests.sh all` (the e2e half starts `ofs-sim`) — PASS. Commit `feat(godot): draw the rotated boxes (#28)`.

### Task 7: Meadow's Gate1 bar and the docs

**Files:**
- Modify: `worlds/meadow.toml` (:2-4 header, :64 comment, a new bar object), `crates/ofs-config/tests/world.rs` (`the_meadow_world_loads` at :103), `CHANGELOG.md`, `docs/dev-setup.md`, `docs/research/collision.md`, `docs/roadmap.md` (:21), `python/README.md`

**Interfaces:**
- Consumes: Tasks 3-6 (the field, the wire, the drawing).

- [ ] **Step 1:** In `worlds/meadow.toml`, after the Gate1 posts (:65-77), add — Gate0's bar proportions (:57-63), across Gate1's travel (heading 45 + 90):

```toml
# The bar Gate1 was missing: a box yawed across the diagonal travel (#28).
[[objects]]
name = "Gate1Bar"
shape = "box"
center_ned_m = [60.0, 45.0, -5.075]
size_m = [8.3, 0.15, 0.15]
yaw_deg = 135.0
color = [0.2, 0.8, 0.35]
```

  Rewrite the header lines that say the diagonal gate gets posts only, and the `:64` comment; the Gate2 (pitched) note stays. In `world.rs` config tests, extend `the_meadow_world_loads`: the bar exists, is a box, `yaw_deg == Some(135.0)`. Run `cargo test -p ofs-config --locked` — PASS.
- [ ] **Step 2:** Docs. CHANGELOG `## Unreleased` top section (the neighbours' style): `### Rotated boxes in the world (#28)` — Added: `yaw_deg` (box only), protocol 17 (`WorldObject.yaw_deg`, breaking), the Godot drawing, meadow's Gate1 bar; Changed: nothing for worlds that write no yaw (pin, goldens). `docs/dev-setup.md`: the `[[objects]]` line (:118) gains `yaw_deg`; the protocol walk (Task 5's edits) — re-check :268 reads 17. `docs/research/collision.md`: a "Rotated boxes" paragraph — the local-frame SDF and normal, the exact AABB, the identity fast path, `rem_euclid` canonicalization, the trig-in-the-SDF determinism note (identity path has none). `docs/roadmap.md:21`: strike "boxes are axis-aligned". `python/README.md`: protocol number if it names one.
- [ ] **Step 3:** `grep -rn "axis-aligned" --include=*.md . | grep -v superpowers/ | grep -v CHANGELOG` — every live hit updated or confirmed historical. `tr -cd '\r' < <file> | wc -c` is 0 for every changed file (`git diff --name-only main | xargs -I{} bash -c 'tr -cd "\r" < {} | wc -c | grep -q "^0$" || echo {}'`).
- [ ] **Step 4:** Full gates: `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-core -p ofs-config -p ofs-sim -p ofs-proto -p ofs-client` (`ofs-godot` pins `rust-version = "1.94"` and is skipped by the msrv job — an erratum fixed 2026-11-01, the original list named it), `PYTHONPATH=python python -m pytest python/tests -q`, WSL `bash scripts/run-godot-tests.sh all`. Expected: PASS.
- [ ] **Step 5:** Commit `docs: the rotated boxes and meadow's yawed Gate1 bar (#28)`.

## Ledger

- Planned 2026-10-31 against worktree `bd4cc20` (protocol 16, `#169` merged). The phase umbrella named by the task was uncommitted when this plan was written; it ships in the same PR — its coordination rule applies: if a sibling pair (#156 or #157) landed first, the protocol number and `WorldObject`'s field number shift (Task 5's note).
- Open PR #170 (DVR audio) touches `crates/ofs-client/src/model.rs` and `crates/ofs-godot/src/lib.rs`, the same two files as Task 5, and bumps no protocol: rebase only.
- Re-verify at execution: the object-world pin constants (Task 1, they do not exist yet); the climb-into-slab script actually touches the box (Task 1 Step 2's assertion); the Godot `rotation.y = PI - deg_to_rad(yaw)` against the shipped gate formula (the m8a derivation, re-derived in the spec's "Godot" section); `test_scene.gd`'s node-reference helpers before writing Task 6's test.
- The 22 `Shape::Box {` construction sites in 8 files (plus shape.rs's 5 match arms) were counted at planning time (Task 2 Files); the compiler re-lists them, so a drift after a merge is self-correcting.
- MSRV 1.85: `rem_euclid` on `f64`, `is_none_or`, and `sin_cos` all predate it; `map_or` over `Option<f64>` is used as written.
