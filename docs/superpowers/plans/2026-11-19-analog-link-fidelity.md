# Analog Link Fidelity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rf_loss_db` frequency-dependent, give the ground bounce its own obstruction and add one specular wall reflection to the video link, alternate the handset's transmit antenna per packet, and rule on UART contention with arithmetic.

**Architecture:** `RfLoss` (scalar or log-log curve) lives in `ofs-rf` and is used by `ofs-config` (which gains an `ofs-rf` dependency). Propagation computes each ray's obstruction separately and combines the ground ray with its own loss; object reflections are one bounce off box vertical faces, incoherent, behind a video-only flag. `ElrsLink` keeps a round-robin transmit-antenna index. #63 produces a research note, no code.

**Tech Stack:** Rust workspace (`ofs-rf`, `ofs-config`, `ofs-video`, `ofs-radio`, `ofs-sim`), TOML world files, serde, per-OS golden digests. No Godot or protocol changes.

**Spec:** `docs/superpowers/specs/2026-11-19-analog-link-fidelity-design.md`

## Global Constraints

- No task adds or removes an RNG draw. Determinism is the model's contract.
- `rf_loss_db = 25.0` stays valid; the curve form is additive; no `schema_version` bump.
- #25 and #62 move no shipped value, bus digest or golden. #64 names every digest it moves and traces it; an unexplained one is investigated first.
- `WALL_REFLECTION_DB` and any unsourced number stay `estimated`; sourced values cite ITU-R P.2040 in `docs/research/video-link.md`.
- One PR per issue: Tasks 1–3 = #25, Task 4 = #62, Tasks 5–6 = #64, Task 7 = #63.
- The golden write commands are `OFS_GOLDEN_WRITE=1 cargo test -p ofs-radio --test golden --locked` and `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden --locked`. Goldens are per OS/arch; macOS uses `.github/workflows/macos-survey.yml` with `write_goldens`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean before each PR.

## Review Focus

- A scalar-world load reproduces today's obstacle cap, state `rf_loss_db` and bus digests exactly (test: Task 3's pin runs).
- A malformed curve (one point, unequal lengths, unsorted or non-positive frequencies, negative dB) is rejected with the field named, never a panic (test: Task 2).
- With the bounce blocked and the direct ray clear, only the bounce loses; terrain values do not move (test: Task 5).
- A reflection from a face the path cannot see, from a cylinder, or from the reflecting object itself adds nothing (test: Task 6).
- A one-antenna handset behaves exactly as before, and a two-antenna handset alternates 0,1,0,1 (test: Task 4).

---

### Task 1: `RfLoss` — scalar or log-log curve

**Files:**
- Create: `crates/ofs-rf/src/rf_loss.rs`
- Modify: `crates/ofs-rf/src/lib.rs` (`pub mod rf_loss;` and re-export `RfLoss`, `RfLossCurve`)
- Modify: `crates/ofs-rf/Cargo.toml` (add `serde.workspace = true`)
- Test: `crates/ofs-rf/tests/rf_loss.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Deserialize)]
  #[serde(untagged)]
  pub enum RfLoss { Db(f64), Curve(RfLossCurve) }

  #[derive(Debug, Clone, PartialEq, Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct RfLossCurve { pub at_mhz: Vec<f64>, pub db: Vec<f64> }

  impl RfLoss {
      pub fn db_at(&self, freq_mhz: f64) -> f64;
      pub fn max_db(&self) -> f64;
      pub fn is_obstacle(&self) -> bool; // max_db() > 0.0
  }
  impl From<f64> for RfLoss;
  impl Default for RfLoss; // Db(0.0)
  impl RfLossCurve { pub fn validate(&self) -> Result<(), String>; }
  ```

- [ ] **Step 1: Write the failing tests** in `crates/ofs-rf/tests/rf_loss.rs`

```rust
use ofs_rf::{RfLoss, RfLossCurve};

fn close(a: f64, b: f64) { assert!((a - b).abs() < 1e-9, "{a} != {b}"); }

#[test]
fn a_scalar_is_the_same_at_every_frequency() {
    let loss = RfLoss::from(25.0);
    for f in [900.0, 2400.0, 5800.0] { close(loss.db_at(f), 25.0); }
    assert!(loss.is_obstacle());
}

#[test]
fn a_curve_interpolates_as_a_power_law() {
    // 10 dB at 100 MHz, 20 dB at 1000 MHz: 14.142… dB at the geometric middle.
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![100.0, 1000.0], db: vec![10.0, 20.0] });
    close(loss.db_at(316.22776601683796), 10.0 * 2f64.sqrt());
}

#[test]
fn a_curve_clamps_outside_its_points() {
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] });
    close(loss.db_at(100.0), 12.0);
    close(loss.db_at(10000.0), 25.0);
}

#[test]
fn a_zero_point_falls_back_to_linear_db() {
    // ln(f) 900..5800, dB 0..25: half way in ln(f) is 12.5, where log-log is undefined.
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 25.0] });
    let middle = (900f64.ln() + 5800f64.ln()) / 2.0;
    close(loss.db_at(middle.exp()), 12.5);
}

#[test]
fn only_a_positive_curve_is_an_obstacle() {
    assert!(!RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 0.0] }).is_obstacle());
    assert!(RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 1.0] }).is_obstacle());
    close(RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![3.0, 25.0] }).max_db(), 25.0);
}

#[test]
fn a_curve_validates_its_shape() {
    let ok = RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] };
    assert!(ok.validate().is_ok());
    for bad in [
        RfLossCurve { at_mhz: vec![900.0], db: vec![12.0] },
        RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0] },
        RfLossCurve { at_mhz: vec![5800.0, 900.0], db: vec![12.0, 25.0] },
        RfLossCurve { at_mhz: vec![0.0, 5800.0], db: vec![12.0, 25.0] },
        RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![-1.0, 25.0] },
    ] { assert!(bad.validate().is_err(), "{bad:?}"); }
}

#[test]
fn serde_accepts_a_scalar_and_a_table() {
    assert_eq!(toml::from_str::<RfLoss>("25.0").unwrap(), RfLoss::Db(25.0));
    assert_eq!(
        toml::from_str::<RfLoss>("{ at_mhz = [900.0, 5800.0], db = [12.0, 25.0] }").unwrap(),
        RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] })
    );
}
```

Add `toml.workspace = true` to `[dev-dependencies]` of `ofs-rf` for the last test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-rf --test rf_loss`
Expected: FAIL, `RfLoss` not found (or `toml` unresolved until the dev-dependency is added).

- [ ] **Step 3: Implement `RfLoss`** in `crates/ofs-rf/src/rf_loss.rs`

Interpolation: clamp below/above the points; with a single point return its dB; with both endpoint dB `> 0.0` interpolate `ln(db)` linearly in `ln(f)` (the power law); otherwise interpolate dB linearly in `ln(f)`. Validation requires at least two points, equal lengths, finite positive strictly increasing frequencies, finite non-negative dB. `max_db` is the largest listed dB (0.0 for `Db`). Derive `Deserialize` as shown; `Db(f64)` accepts a TOML float.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ofs-rf --test rf_loss`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-rf/src/rf_loss.rs crates/ofs-rf/src/lib.rs crates/ofs-rf/Cargo.toml crates/ofs-rf/tests/rf_loss.rs Cargo.lock
git commit -m "feat(rf): a scalar or log-log RfLoss curve"
```

---

### Task 2: World config, state scalar and mesh data use `RfLoss`

**Files:**
- Modify: `crates/ofs-config/Cargo.toml` (add `ofs-rf.workspace = true`)
- Modify: `crates/ofs-config/src/world.rs`: `ObjectSection.rf_loss_db: RfLoss`, `MaterialSection.rf_loss_db: RfLoss`, validation
- Modify: `crates/ofs-sim/src/session.rs`: the proto `WorldObject.rf_loss_db` becomes `o.rf_loss_db.db_at(5800.0)`
- Modify: `crates/ofs-sim/src/vehicle.rs` (`world_obstacles`): filter `is_obstacle()`, map `rf_loss_db: o.rf_loss_db.db_at(5800.0)`; `mesh_obstacles` still uses `WorldMeshData.loss_db` as `Vec<f64>`
- Modify: `crates/ofs-sim/tests/rotated_boxes.rs` and any `ObjectSection` literal: `rf_loss_db: 25.0.into()`
- Test: `crates/ofs-config/tests/world.rs`, the inline `session.rs` tests

**Interfaces:**
- Consumes: `RfLoss`, `RfLossCurve` from Task 1.
- Produces: `ObjectSection.rf_loss_db: RfLoss`, `MaterialSection.rf_loss_db: RfLoss`; the state's `rf_loss_db` and the link's obstacle/mesh values are the 5.8 GHz value until Task 3 threads the frequency.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-config/tests/world.rs`: a world with `rf_loss_db = 25.0` loads and validates; a world with `rf_loss_db = { at_mhz = [900.0, 5800.0], db = [12.0, 25.0] }` loads and validates; a table with one point, unequal lengths, descending frequencies or a negative dB fails validation with `rf_loss_db` in the message. Follow the file's existing `write(dir.path(), text)` helper and `world::load` pattern.

In `crates/ofs-sim/src/session.rs`'s inline tests add: a curve object (`at_mhz = [900.0, 5800.0], db = [12.0, 25.0]`) reaches `pb::WorldObject.rf_loss_db` as `25.0`; the existing scalar world still reports its number.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-config --test world && cargo test -p ofs-sim --lib session`
Expected: FAIL, the field still expects `f64` / the state still uses the raw value.

- [ ] **Step 3: Implement**

Change `ObjectSection` and `MaterialSection` to `RfLoss`; call `RfLossCurve::validate()` from `WorldConfig::validate` with the field name. Keep `WorldMeshData.loss_db: Vec<f64>` for now, built with `db_at(5800.0)` in `load_meshes_with`, and keep `Obstacle.rf_loss_db: f64` by mapping `o.rf_loss_db.db_at(5800.0)` in `world_obstacles`; filter with `is_obstacle()`. In `session.rs` write `db_at(5800.0)`. Update the `ObjectSection` literals with `.into()`. This task is representation-only: the link still sees the 5.8 GHz scalar, so no value or digest moves.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ofs-config && cargo test -p ofs-sim --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-config crates/ofs-sim/src/session.rs crates/ofs-sim/tests/rotated_boxes.rs Cargo.lock
git commit -m "feat(config): rf_loss_db accepts a frequency curve"
```

---

### Task 3: Propagate the frequency through obstruction and mesh loss

**Files:**
- Modify: `crates/ofs-rf/src/propagation.rs`: `Obstacle.rf_loss: RfLoss` (drop `Copy`, keep `Clone`), `MeshObstacle.loss_db: Vec<RfLoss>`, `obstruction_loss_db(..., freq_mhz)`, `mesh_loss_db(..., freq_mhz)`, `obstruction_db` passes the frequency
- Modify: `crates/ofs-config/src/world.rs`: `WorldMeshData.loss_db: Vec<RfLoss>`, `load_meshes_with` keeps the curves
- Modify: `crates/ofs-video/src/link.rs`, `crates/ofs-sim/src/vehicle.rs` (`world_obstacles`, `mesh_obstacles`), all `Obstacle`/`MeshObstacle` literals in tests
- Test: `crates/ofs-rf/tests/propagation.rs`, the existing bus pins

**Interfaces:**
- Consumes: `RfLoss`, `ObjectSection.rf_loss_db`, `WorldMeshData.loss_db`.
- Produces:
  ```rust
  pub struct Obstacle { pub shape: Shape, pub rf_loss: RfLoss }
  pub struct MeshObstacle { pub mesh: Arc<TriMesh>, pub loss_db: Vec<RfLoss> }
  pub fn obstruction_loss_db(obstacle: &Obstacle, a: DVec3, b: DVec3, freq_mhz: f64) -> f64;
  pub fn mesh_loss_db(m: &MeshObstacle, a: DVec3, b: DVec3, freq_mhz: f64) -> f64;
  ```
  `WorldMeshData.loss_db` becomes `Vec<RfLoss>`; `obstruction_db` keeps its meaning (objects + terrain + meshes on the direct path).

- [ ] **Step 1: Write the failing test** in `crates/ofs-rf/tests/propagation.rs`

A box behind the path with a curve `at_mhz = [900.0, 5800.0], db = [10.0, 25.0]` caps the knife-edge loss at 10 dB for a 900 MHz path and 25 dB for a 5800 MHz path, when the unobstructed knife-edge loss exceeds both. Build the existing `building` fixture with the curve; assert `obstruction_db(..., freq)` for both frequencies. A scalar fixture keeps its single value at both.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ofs-rf --test propagation`
Expected: FAIL, `rf_loss_db` still a `f64` / the frequency argument missing.

- [ ] **Step 3: Implement the field and signatures; update every caller**

`obstruction_loss_db` computes `wavelength_m(freq_mhz)` and caps at `obstacle.rf_loss.db_at(freq_mhz)`. `mesh_loss_db` maps each crossed material to `m.loss_db.get(i).map_or(0.0, |r| r.db_at(freq_mhz))`. `load_meshes_with` stores the material `RfLoss` values in `WorldMeshData.loss_db` (no `db_at`). `world_obstacles` filters `o.rf_loss_db.is_obstacle()` and clones the curve into `rf_loss`; `mesh_obstacles` clones `m.loss_db`. Update `Obstacle` literals to `rf_loss: 25.0.into()` and `MeshObstacle` literals to `vec![18.0.into()]`; replace `..building` struct updates with a clone where `Copy` was relied on.

- [ ] **Step 4: Run the tests, the pins and the gates**

Run: `cargo test -p ofs-rf -p ofs-video -p ofs-radio -p ofs-config`
Run: `cargo test -p ofs-sim --test flat_world_pin --test rotated_boxes`
Expected: PASS, pins unchanged (a scalar world's digest does not move).
Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-rf crates/ofs-video crates/ofs-sim crates/ofs-radio
git commit -m "feat(rf): cap obstruction at the ray's frequency"
```

---

### Task 4: The handset transmits on its antennas round-robin (#62)

**Files:**
- Modify: `crates/ofs-radio/src/elrs.rs`: add `tx_antenna`/`last_tx_antenna` to `ElrsLink`, replace the `handset_diversity.active()` transmit choice, advance per step, add `pub fn current_tx_antenna(&self) -> usize`
- Test: `crates/ofs-radio/tests/elrs.rs`
- Modify: `docs/research/elrs-link.md` (handset antenna paragraph), `docs/roadmap.md` (#62)

**Interfaces:**
- Produces: `ElrsLink::current_tx_antenna(&self) -> usize` (the index the last packet used).

- [ ] **Step 1: Write the failing test** in `crates/ofs-radio/tests/elrs.rs`

Build an `ElrsLink` from `LinkParams::ideal(500)` with `handset_antennas` = two dipoles on different axes (e.g. `DVec3::NEG_Z` and `DVec3::X`), step the link four times with a transmitting frame, and assert `current_tx_antenna()` is `[0, 1, 0, 1]`. Name it `the_handset_alternates_its_transmit_antenna`. A one-antenna link steps four times and stays `0`; the existing tests/goldens are the second half of that check.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ofs-radio --test elrs the_handset_alternates`
Expected: FAIL, the method does not exist.

- [ ] **Step 3: Implement**

Select `handset_antennas[tx_antenna]` before the uplink path (no RNG draw), store it as `last_tx_antenna`, and advance `tx_antenna` at the end of the step. Leave `handset_diversity.choose(downlink)` for the downlink receive antenna. No other behaviour changes.

- [ ] **Step 4: Run the tests and the unchanged goldens**

Run: `cargo test -p ofs-radio && cargo test -p ofs-radio --test golden && cargo test -p ofs-sim --test radio_golden`
Expected: PASS. Every shipped handset has one antenna, so no golden moves. If a golden moves, the shipped data has a second antenna and that is a finding to report, not a file to regenerate.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-radio/src/elrs.rs crates/ofs-radio/tests/elrs.rs docs/research/elrs-link.md docs/roadmap.md
git commit -m "feat(radio): the handset alternates transmit antennas per packet"
```

---

### Task 5: Shared path function and per-ray obstruction (#64, part 1)

**Files:**
- Modify: `crates/ofs-rf/src/propagation.rs`: add `Multipath`, change `path_gain` to take it, compute the direct and ground rays' obstruction separately, keep terrain as one whole-path term
- Modify: `crates/ofs-video/src/link.rs`: `LinkParams.object_reflections`, `vtx_paths` calls `path_gain` instead of `unobstructed_gain_db - obstruction`, `interference_dbm` takes `Multipath`
- Modify: `crates/ofs-radio/src/elrs.rs`: `LinkParams.object_reflections` (false), `path_gain` call
- Modify: `crates/ofs-sim/src/vehicle.rs`: radio false, video false until Task 6
- Test: `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-video/tests/link.rs`
- Docs: `docs/research/video-link.md` (multipath row/note)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Multipath { pub ground_bounce: bool, pub object_reflections: bool }

  pub fn path_gain(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, obstacles: &[Obstacle],
                   terrain: Option<&Terrain>, meshes: &[MeshObstacle], multipath: Multipath) -> PathGain;
  ```
  `PathGain.obstruction_db` remains the direct ray's objects + terrain + meshes.

- [ ] **Step 1: Write the failing test** in `crates/ofs-rf/tests/propagation.rs`

`the_ground_ray_takes_its_own_obstruction`: tx at `(0, 0, -10)`, rx at `(100, 0, -10)`, `ground_bounce: true`, a 2 m box at `(40, 0, -2)` that crosses the tx→bounce leg but not the direct line at z = -10. With the box, the gain drops; the direct obstruction (`PathGain.obstruction_db`) is 0; without the box the gain equals the open two-ray gain. Use a frequency well above the box's Fresnel size so the knife-edge loss is visible.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ofs-rf --test propagation the_ground_ray`
Expected: FAIL, `Multipath`/the signature does not exist.

- [ ] **Step 3: Implement the ray split**

Direct amplitude from `ray_gain_db` minus `obstruction_db(tx, rx)`; ground amplitude from the image bounce minus `object_mesh_obstruction_db(tx, bounce) + object_mesh_obstruction_db(bounce, rx)` (a private helper, no terrain); combine with the existing phase term. Subtract `terrain_loss_db` once from the combined power (today's whole-path approximation). `PathGain.obstruction_db` is the direct call's full value. Update `vtx_paths` (per antenna, subtract the body shadow) and `interference_dbm`; both pass `Multipath { ground_bounce, object_reflections: false }` for now. Update every `path_gain`/`interference_dbm` caller and `LinkParams` literal with `object_reflections: false`.

- [ ] **Step 4: Run the tests and trace the golden diff**

Run: `cargo test -p ofs-rf -p ofs-video -p ofs-radio`
Run: `cargo test -p ofs-radio --test golden` and `cargo test -p ofs-sim --test radio_golden` (without writing).
Expected: `ofs-rf`/`ofs-video` PASS. The golden runs fail with the changed scenario names. List them; only a scenario whose bounced path meets an obstacle may move (the `obstacle-*` cases); `two-antennas-fading` and a clear bounce must not. Investigate anything else before writing.
Then on Windows: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-radio --test golden --locked` and `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden --locked`. Commit the Windows files. The PR records the macOS/Linux regeneration still owed (`macos-survey.yml` with `write_goldens`; Linux via WSL), and CI's golden jobs are expected to fail until those are committed.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-rf crates/ofs-video crates/ofs-radio crates/ofs-sim docs/research/video-link.md
git commit -m "feat(rf): the ground ray carries its own obstruction"
```

---

### Task 6: One specular reflection off box faces (#64, part 2)

**Files:**
- Modify: `crates/ofs-rf/src/propagation.rs`: reflection geometry, `WALL_REFLECTION_DB`, use `Multipath.object_reflections`
- Modify: `crates/ofs-sim/src/vehicle.rs`: the video `LinkParams.object_reflections = true`; radio stays false
- Test: `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-video/tests/link.rs`
- Docs: `docs/research/video-link.md`, `docs/roadmap.md` (#64)

**Interfaces:**
- Consumes: `Multipath` from Task 5.
- Produces: `pub const WALL_REFLECTION_DB: f64 = -6.0;` (estimated).

- [ ] **Step 1: Write the failing tests** in `crates/ofs-rf/tests/propagation.rs`

- `a_side_wall_adds_a_reflected_ray`: a wall beside a clear tx→rx path, with a face the specular point falls inside; `object_reflections: true` gives a higher gain than `false`.
- `a_wall_outside_the_specular_path_adds_nothing`: the same wall shifted so the mirrored line misses the face; the gain equals the flag-off gain.
- `a_yawed_face_reflects_from_its_own_side`: yaw the wall; the reflection appears only on the side the face looks at.
- `the_reflecting_object_does_not_block_its_own_ray`: a reflection's gain is not reduced by the reflecting box's own knife-edge.
- `a_cylinder_does_not_reflect`: a cylinder in the same place adds nothing.
- `the_flag_off_reproduces_todays_gain`: `object_reflections: false` equals the pre-task gain for the same scene.

In `crates/ofs-video/tests/link.rs`: `the_video_flag_lets_a_wall_reflect` with `LinkParams.object_reflections = true` and a wall beside the pilot; a building between the quad and the pilot still costs its loss.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-rf --test propagation a_side_wall`
Expected: FAIL, no reflection.

- [ ] **Step 3: Implement**

For each `Shape::Box` obstacle's four vertical faces (normal rotated by `yaw_rad`, the other horizontal half and `half.z` spanning the rectangle): mirror tx across the face plane, intersect the mirrored tx→rx line with the plane, reject the point outside the rectangle or above ground (`q.z > 0`), reject when tx and rx are on the same side. Compute the ray with `ray_gain_db(tx, tx.antenna.polarization.reflected(), rx, out_dir, in_dir, len, freq) + WALL_REFLECTION_DB`, its obstruction from every *other* obstacle and mesh on both legs, add its power incoherently to the direct/ground power, then apply the whole-path terrain term. Only when `multipath.object_reflections` is true. Share the reflection geometry across the goggles' antennas (the positions are the same). Enable it in the video `LinkParams` only.

- [ ] **Step 4: Run the tests, trace the pins and goldens**

Run: `cargo test -p ofs-rf -p ofs-video -p ofs-radio`
Run: `cargo test -p ofs-sim --test flat_world_pin --test rotated_boxes`
Expected: the tests PASS. `flat_world_pin` must not move. `rotated_boxes` may move if the slab's vertical face reflects to the goggles; trace the cause and, if it is the reflection, update the pin with a comment naming this model and regenerate the Linux/WSL pin too. Otherwise investigate.
Run the radio/sim goldens as in Task 5; the video signals in the flight scenarios may move; name each digest and trace it before writing.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-rf crates/ofs-video crates/ofs-sim docs/research/video-link.md docs/roadmap.md
git commit -m "feat(video): reflect the VTX signal off box walls"
```

---

### Task 7: UART contention ruling (#63)

**Files:**
- Create: `docs/research/uart-contention.md`
- Modify: `docs/roadmap.md` (the ruling) or create a follow-up issue if the answer is "model it"
- No code

**Interfaces:**
- Consumes: the frame length the radio link writes (`crates/ofs-radio/src/elrs.rs` and its tests), the UART baud in the SITL diff (115200), the FC write path (`crates/ofs-fc/src/sitl/bridge.rs`).
- Produces: a written ruling.

- [ ] **Step 1: Collect the real numbers**

Record in the note: the CRSF frame length the link actually writes (read the encoder/test helper; do not assume 26 bytes), the telemetry frame length, the telemetry slot divisor, the packet slot at 500 Hz (2 ms), the baud (115200 → 11.52 bytes/ms) and the resulting frame time and occupancy per slot.

- [ ] **Step 2: Write the ruling**

State the threshold used (model if the worst case leaves the RC frame unable to fit its slot, or drops appear at the default settings). Add the arithmetic and the code paths. If "not modelled", add one line to `docs/roadmap.md` under the FC/SITL limits. If "model it", write the follow-up issue body in the note (a half-duplex budget on the `Receiver`: baud, frame lengths, telemetry ratio, queue/delay/drop) and note it is out of this wave.

- [ ] **Step 3: Verify the note is reproducible**

Re-run the arithmetic from the cited sizes and confirm the note's occupancy numbers. No Rust test changes.

- [ ] **Step 4: Commit**

```bash
git add docs/research/uart-contention.md docs/roadmap.md
git commit -m "docs(research): rule on UART contention"
```

---

## PR grouping

1. **#25** — Tasks 1–3. No shipped value or digest moves; the curve is exercised by the config/propagation tests.
2. **#62** — Task 4. No golden moves.
3. **#64** — Tasks 5–6. The named digest moves; the macOS/Linux goldens regenerate before merge.
4. **#63** — Task 7. Docs only.
