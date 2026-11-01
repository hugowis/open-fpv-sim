# Crash damage: degrade and repair (#157) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A crash costs something: every sphere contact's normal impulse accrues per-part damage above provenance-marked thresholds, expressed only through the existing fault seams (bent-prop thrust and imbalance, motor torque loss, battery series resistance), with a part-selective `Repair` RPC on protocol 17 and `State.damage` for the workbench's rows. Closes #157 (the repair UI in the workbench is the ruled follow-on, not this plan).

**Architecture:** `ofs-physics` reports each contact's impulse on a shared feed (mechanism); a new pure module `ofs-sim/src/damage.rs` maps impulse and body-frame geometry to per-part damage levels and degradation curves (policy, constants, estimates with provenance); the vehicle drains the feed once per tick, composes damage with script-injected faults through one seam writer, and exposes `VehicleState.damage`. Onset is derived (never journaled); `Repair` is journaled. Protocol 17 carries `State.damage` and the RPC; Python, the Rust client and the Godot bridge surface them read-only.

**Tech Stack:** Rust workspace (MSRV 1.85), tonic/prost proto (version 17), Python `ofs` (generated stubs via grpc_tools), Godot 4.7.2 headless tests through the gdext bridge.

**Spec:** `docs/superpowers/specs/2026-10-31-crash-damage-design.md` — the design calls DC1-DC12 there are the binding rules; the epic #160 body and the workbench spec's DC8 (`docs/superpowers/specs/2026-10-31-drone-workbench-design.md:114-122`) are the contracts above this plan.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- No golden digest changes (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`): no golden run crashes hard enough to damage — re-verify at execution by running the suites; a moved golden is a bug unless a golden flight provably crashes, and then it is called out in the commit message.
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- Regenerate Python stubs after the proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`; the three generated files change and nothing else under `python/ofs/v1`.
- Python tests need `PYTHONPATH=<worktree>/python`. Godot tests: `bash scripts/run-godot-tests.sh unit` (WSL; the open-loop set needs the built server and extension: `bash scripts/run-godot-tests.sh open-loop`).
- Commit trailers: none prescribed; an executing session that adds one names its own identity.
- Every anchor in this plan was verified against `main` at `bd4cc20` on 2026-10-31; re-verify file:line anchors before editing (the B6 lesson: an evidence SHA copied on trust was wrong). If open PR #170 (DVR audio) has merged, re-check only `PROTOCOL_VERSION` and the proto's field numbers before Task 5.

## Design calls (decided in the spec; the executor ledgers any change)

1. **Ride the seams** (DC1): damage writes `fault.prop.{i}.thrust_scale`, `prop.{i}.imbalance_kgm`, `fault.motor.{i}.scale`, `fault.battery.{weak_cell,r_offset,v_offset}` — the same signals `Vehicle::set_fault` writes (`crates/ofs-sim/src/vehicle.rs:1396-1407`). No new bus signal, no new `Fault` variant, no model change in the consumers.
2. **The trigger is the per-contact normal impulse** (DC2): `collision::resolve` fills an out-param with `(centre_frd_m, impulse_n_s)` for every contact with `j > 0`; `RigidBody::with_contact_feed` pushes them on a `ContactFeed`; the vehicle drains it per tick. Events, landing points and friction impulses are untouched.
3. **Impulse → part by geometry** (DC3): nearest motor within its prop's tip radius + sphere radius + `PROP_DISC_MARGIN_M` → that prop; `j ≥ MOTOR_DAMAGE_IMPULSE_N_S` also accrues the motor; outside every disc → the battery.
4. **Accrual** (DC4): `level += (j − T)/(J_full − T)` for `j ≥ T`, capped at 1; thresholds 1.5/5.0 (prop), 4.0/10.0 (motor), 6.0/15.0 (battery) N·s — estimates, provenance in the spec's table.
5. **Curves** (DC5): level 1 = prop `thrust_scale` 0.5 and `+2.0e-5 kg·m` imbalance; motor `scale` 0.2; battery `+0.03 Ω` and `+0.1 V` on cell 0. Linear in the level.
6. **The gyro shakes through the existing imbalance term** (DC6): no `GyroFault`, no new IMU input; the test records `imu.gyro_frd_radps`.
7. **Composition with script faults** (DC7): the vehicle remembers the script's absolute per-part values; scales multiply, imbalance and offsets add; one composer writes the seams for `set_fault`, the damage path, `clear_faults` and `repair`. `clear_faults` also clears crash damage.
8. **Determinism** (DC8): onset applies inside `step_ticks`'s per-tick loop (Run-split independent); `Repair` is journaled (`Action::Repair`); a run without a damaging contact is bit-identical to today.
9. **Protocol 17** (DC9): `State` fields 21-23 (`damage_prop`, `damage_motor`, `damage_battery`), `rpc Repair(RepairRequest) returns (Empty)`; no new event kind — onset and repair ride `FAULT_INJECTED`/`FAULT_CLEARED` through `active_fault_names`.
10. **Repair is part-selective** (DC10): refuses out-of-range indices and an empty request; `Load` resets by rebuilding the vehicle; nothing is persisted anywhere.
11. **Clients read-only now** (DC11/DC12): Python `State.damage` + `sim.repair`, Rust `Telemetry` fields, Godot `get_telemetry` keys; the workbench UI is the follow-on after #128.

## Review Focus

- A quad resting on its body sphere for minutes accumulates no damage (weight ≈ 8e-4 N·s per tick against a 6 N·s floor) — Task 3's `a_resting_quad_accumulates_no_damage`.
- A landing soft enough to raise no collision event also raises no damage, and a 2 m/s prop-first landing bends the prop without touching the battery — Tasks 1 and 3.
- A scripted `PropDamage` on a prop that then crashes composes (the crash neither erases nor doubles it), and repairing the crash damage leaves the script fault standing — Tasks 3 and 4.
- `Repair` refuses an empty request and out-of-range indices with nothing half-applied — Task 4.
- The same crash split across `Run` calls differently gives one digest (lockstep unchanged) — Task 3's `two_run_splittings_of_the_same_crash_agree`.
- A client speaking protocol 16 is refused by the handshake after the bump, not misread — Task 5.

---

### Task 1: The damage mapping module (pure)

**Files:**
- Create: `crates/ofs-sim/src/damage.rs`; Modify: `crates/ofs-sim/src/lib.rs` (`pub mod damage;`)
- Test: unit tests inside `damage.rs`

**Interfaces:**
- Consumes: `ofs_physics::collision::ContactImpulse` (Task 2 defines it; until then this task declares its own input — see Step 3's note).
- Produces (all `pub`, `Debug, Clone, PartialEq` where derived):
  - `pub const PROP_BEND_IMPULSE_N_S: f64 = 1.5;` `pub const PROP_FULL_IMPULSE_N_S: f64 = 5.0;` `pub const MOTOR_DAMAGE_IMPULSE_N_S: f64 = 4.0;` `pub const MOTOR_FULL_IMPULSE_N_S: f64 = 10.0;` `pub const BATTERY_DAMAGE_IMPULSE_N_S: f64 = 6.0;` `pub const BATTERY_FULL_IMPULSE_N_S: f64 = 15.0;` `pub const PROP_DISC_MARGIN_M: f64 = 0.01;` `pub const MAX_PROP_THRUST_LOSS: f64 = 0.5;` `pub const MAX_PROP_IMBALANCE_KGM: f64 = 2.0e-5;` `pub const MAX_MOTOR_LOSS: f64 = 0.8;` `pub const MAX_BATTERY_R_OFFSET_OHM: f64 = 0.03;` `pub const MAX_BATTERY_V_OFFSET_V: f64 = 0.1;` `pub const BATTERY_FAULT_CELL: usize = 0;` — each with a doc comment naming it an estimate per the spec's table.
  - `pub struct DamageGeometry { pub motor_positions_frd_m: Vec<DVec3>, pub prop_tip_radius_m: Vec<f64>, pub sphere_radius_m: f64 }`
  - `pub enum Part { Prop(usize), Motor(usize), Battery }`
  - `pub struct PartDamage { pub props: Vec<f64>, pub motors: Vec<f64>, pub battery: f64 }` with `PartDamage::new(motors: usize) -> Self` (all zero), `fn is_clean(&self) -> bool`, `fn active_fault_names(&self) -> Vec<&'static str>` (`"prop_damage"`, `"motor_failure"`, `"weak_cell"` — any level > 0).
  - `pub fn accrue(damage: &mut PartDamage, geo: &DamageGeometry, hit: &ContactImpulse) -> Vec<Part>` — the parts whose level rose.
  - `pub fn prop_thrust_scale(level: f64) -> f64` (`1.0 - MAX_PROP_THRUST_LOSS * level`), `pub fn prop_imbalance_kgm(level: f64) -> f64`, `pub fn motor_scale(level: f64) -> f64`, `pub fn battery_r_offset_ohm(level: f64) -> f64`, `pub fn battery_v_offset_v(level: f64) -> f64`.
  - `pub fn repair(damage: &mut PartDamage, props: &[usize], motors: &[usize], battery: bool)` — zeroes the named levels.
  - `pub struct DamageInfo { pub props: Vec<f64>, pub motors: Vec<f64>, pub battery: f64 }` (`Debug, Clone, PartialEq, Default`) — what `VehicleState.damage` carries.

- [ ] **Step 1: Write the failing tests** inside `damage.rs` (`#[cfg(test)]`), with a helper `fn geo() -> DamageGeometry` shaped like the shipped 5-inch quad — four motors at `(±0.08, ±0.08, 0.0)`, `prop_tip_radius_m` all `0.0635`, `sphere_radius_m` 0.02 — and a helper `fn hit(centre: DVec3, j: f64) -> ContactImpulse`:
  - `a_hit_below_every_threshold_changes_nothing` (prop-centre hit at 1.4 N·s, chassis hit at 5.9 N·s: `accrue` returns empty, `is_clean()`).
  - `a_prop_hit_accrues_the_prop_and_its_curves` (hit at `(0.08, 0.08, 0.0)` with 1.5 N·s: level 0.0 — exactly at threshold counts; 3.25 N·s: level 0.5, `prop_thrust_scale(0.5) == 0.75`, `prop_imbalance_kgm(0.5) == 1.0e-5`).
  - `a_hard_prop_hit_also_damages_the_motor` (10 N·s on the same centre: prop level 1.0 capped, motor level 1.0).
  - `a_chassis_hit_damages_the_battery_only` (hit at `DVec3::ZERO` with 15 N·s: battery level 1.0, props and motors clean, `active_fault_names() == ["weak_cell"]`).
  - `a_hit_outside_every_disc_is_chassis` (hit at `(0.3, 0.0, 0.0)` with 15 N·s: battery, no props — the margin rule).
  - `repeated_hits_stack_and_saturate` (two 5 N·s prop hits: level 1.0, not 2.0).
  - `repair_clears_only_the_named_parts` (damage prop 0, prop 1, motor 2, battery; `repair(&mut d, &[1], &[], false)`: prop 1 clean, the rest unchanged).
  - `the_motor_curves_and_battery_offsets_interpolate` (`motor_scale(1.0) == 0.2`, `battery_r_offset_ohm(1.0) == 0.03`, `battery_v_offset_v(0.5) == 0.05`).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib damage` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `damage.rs`. Until Task 2 exists, take the hit as a local `pub struct ContactImpulse { pub centre_frd_m: DVec3, pub impulse_n_s: f64 }` **defined in this module**; Task 2's step 5 replaces it with a re-export of `ofs_physics::collision::ContactImpulse` (`pub use ofs_physics::collision::ContactImpulse;`) with no test change. Geometry: nearest motor by 3-D distance; disc membership `dist <= tip_radius + PROP_DISC_MARGIN_M + sphere_radius` — take the sphere radius as a field of `DamageGeometry` (`pub sphere_radius_m: f64`, 0.02 for the generated set) so custom `spheres_frd_m` quads classify honestly. A hit inside a disc: prop accrues for `j >= PROP_BEND_IMPULSE_N_S`, motor additionally for `j >= MOTOR_DAMAGE_IMPULSE_N_S`; outside every disc: battery for `j >= BATTERY_DAMAGE_IMPULSE_N_S`. Cap at 1.0; clamp negatives away.
- [ ] **Step 4: Run** `cargo test -p ofs-sim --lib damage` — Expected: PASS. Then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing consumes the module yet).
- [ ] **Step 5: Commit** `feat(sim): the crash-damage mapping - impulse thresholds per part kind (#157)`.

### Task 2: Contact impulses reported per sphere contact (ofs-physics)

**Files:**
- Modify: `crates/ofs-physics/src/collision.rs` (the `ContactImpulse` type and `resolve`'s out-param), `crates/ofs-physics/src/rigid_body.rs` (`ContactFeed`, `RigidBody::with_contact_feed`), `crates/ofs-physics/src/lib.rs` only if it re-exports siblings, `crates/ofs-physics/tests/collision.rs` (two existing `resolve` call sites, one new test), `crates/ofs-physics/tests/rigid_body.rs` (one new test)
- Test: `crates/ofs-physics/tests/collision.rs`, `crates/ofs-physics/tests/rigid_body.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub struct ContactImpulse { pub centre_frd_m: DVec3, pub impulse_n_s: f64 }` (`Debug, Clone, Copy, PartialEq`) in `collision.rs`; `pub fn resolve(..., impulses: &mut Vec<ContactImpulse>) -> Vec<(i32, f64)>` — same signature plus the final parameter, cleared first, one entry per contact whose normal impulse `j > 0`, in the resolved (sorted) order, friction excluded; `#[derive(Clone)] pub struct ContactFeed(Arc<Mutex<Vec<ContactImpulse>>>)` with `ContactFeed::new() -> Self`, `fn push(&self, c: ContactImpulse)`, `fn take(&self) -> Vec<ContactImpulse>` in `rigid_body.rs`; `RigidBody::with_contact_feed(p: AirframeParams, s: BodyState, feed: ContactFeed, bus: &mut Bus) -> Self` — `RigidBody::new` keeps its three-argument shape and reports nothing (a feed nobody drains must not grow).

- [ ] **Step 1: Write the failing tests:**
  - In `tests/collision.rs` (reuse the file's `post`, `params`, `state`, `sim` helpers, `crates/ofs-physics/tests/collision.rs:8-49`): `a_stopped_sphere_reports_its_normal_impulse` — one sphere (`(DVec3::ZERO, 0.04)`), the quad 2 cm inside the post face moving at 8 m/s into it, restitution 0.3; call `resolve` directly (the file's existing direct-call pattern, tests/collision.rs:230) with `&mut impulses`; assert one entry, `centre_frd_m == DVec3::ZERO`, and `impulse_n_s` within 5% of `1.3 * 0.65 * effective_mass_fraction * 8` — assert instead the computable bound `(1.0 + 0.3) * 0.65 * 8.0` as an upper bound and `> 1.0` as a floor (the lever arm turns some impulse into spin; the pin is the presence, the magnitude bracket, and the position). A second `resolve` call on the now-slowed state reports nothing (`v_n >= 0`).
  - `a_resting_sphere_reports_no_growing_impulses` — the resting rig of `a_quad_rests_on_a_roof_with_spheres_and_legs_for_ten_seconds` style, one step: every entry's `impulse_n_s < 0.01`.
  - In `tests/rigid_body.rs`: `the_feed_carries_the_wall_strike` — `RigidBody::with_contact_feed` on a one-sphere quad stepping into the `wall()` object at 10 m/s; `feed.take()` after one `sch.step()` returns a non-empty vector with every `impulse_n_s > 0`; a second step of the now-separated body returns empty.
- [ ] **Step 2: Run** `cargo test -p ofs-physics --locked impulse feed` — Expected: FAIL (no `ContactImpulse`, no `with_contact_feed`).
- [ ] **Step 3: Implement** the type and the out-param in `collision.rs`: clear `impulses` at the top of `resolve`; in the contact loop (after `j` is computed and applied, `collision.rs:144-161`) push `ContactImpulse { centre_frd_m: c.centre_frd, impulse_n_s: j }` when `j > 0`. Update the two direct `resolve` call sites in `tests/collision.rs` (around lines 230-232) to pass `&mut Vec::new()` bindings. Add `ContactFeed` and `RigidBody::with_contact_feed` in `rigid_body.rs`: the model holds `contact_feed: Option<ContactFeed>`; `step` calls `resolve` with a local `Vec` and, when a feed is set, pushes every entry (`self.contact_feed.as_ref().map(|f| impulses.iter().for_each(|c| f.push(*c)))`).
- [ ] **Step 4: Run** `cargo test -p ofs-physics --locked` and `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (the sim does not use the feed yet).
- [ ] **Step 5: Repoint `damage.rs`** at the physics type: replace Task 1's local `ContactImpulse` with `pub use ofs_physics::collision::ContactImpulse;` in `crates/ofs-sim/src/damage.rs`; `cargo test -p ofs-sim --lib damage` — Expected: PASS unchanged.
- [ ] **Step 6: Commit** `feat(physics): contact impulses reported per sphere contact (#157)`.

### Task 3: Crash damage in the vehicle — onset, degradation, composition

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (build wiring, `Vehicle` fields, the seam composer, `apply_fault`'s three arms, `active_fault_names`, `clear_faults`, `step_ticks`, `VehicleState`, `state()`), `crates/ofs-sim/src/damage.rs` (`DamageGeometry` construction helper if useful)
- Create: `crates/ofs-sim/tests/crash_damage.rs`
- Test: `crates/ofs-sim/tests/crash_damage.rs`, unit tests inside `vehicle.rs` (beside the fault tests at `vehicle.rs:1993-2062`), `crates/ofs-sim/tests/open_loop_vehicle.rs` (one test)

**Interfaces:**
- Consumes: Task 1's `damage::{PartDamage, DamageGeometry, Part, accrue, prop_thrust_scale, prop_imbalance_kgm, motor_scale, battery_r_offset_ohm, battery_v_offset_v, BATTERY_FAULT_CELL, DamageInfo}`, Task 2's `ContactFeed`/`with_contact_feed`.
- Produces: `Vehicle` fields (private) `damage: PartDamage`, `damage_geo: DamageGeometry`, `script_prop_faults: Vec<Option<(f64, f64)>>` (thrust scale, imbalance), `script_motor_faults: Vec<Option<f64>>`, `script_battery_fault: Option<(f64, f64)>` (r, v), `contact_feed: ContactFeed`; `VehicleState.damage: DamageInfo`; `active_fault_names()` now returns the merged, deduplicated, ORDER-sorted names of script faults and `PartDamage::active_fault_names()`; `clear_faults()` additionally resets the damage and the script memory.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-sim/tests/crash_damage.rs`, on the `open_loop_vehicle.rs` helper pattern (quad `opendrone-5f-freestyle.toml`, `FcKind::OpenLoop`, `WorldConfig::open_field()`, `crates/ofs-sim/tests/open_loop_vehicle.rs:9-21`):
  - A helper `fn dropped_from(m: f64) -> Vehicle` — load the quad, set `cfg.initial.position_ned_m = [0.0, 0.0, -m]` (the `mount.rs` mutation pattern, `crates/ofs-sim/tests/mount.rs:12-21`), build, zero sticks, `run_for` until `state().collision_event_count > 0` (loop 0.1 s slices, panic at 10 s), then `run_for(0.5)`.
  - `a_hard_drop_damages_the_quad` — `dropped_from(30.0)`: `state().damage.battery > 0` (a flat drop lands on the body sphere — battery per DC3), `active_fault_names()` contains `"weak_cell"`, and cell 0 of `state().battery_cell_voltages` sags below the other cells once the motors draw current (the `WeakCell` seam, observable in `VehicleState`). The integration tests assert through public surfaces only (`state()`, `active_fault_names()`); every bus-signal value assertion lives in the in-crate unit tests, where `vehicle.h` is reachable.
  - `a_soft_landing_damages_nothing` — `dropped_from(0.05)` (below the 1 m/s event threshold): `state().damage == DamageInfo::default()`, `active_fault_names()` empty.
  - `a_resting_quad_accumulates_no_damage` — `dropped_from(0.05)`, then `run_for(5.0)`: still clean.
  - `a_tilted_crash_bends_a_prop` — from 1 m, sticks `roll: 1.0, throttle: 0.5` until a collision event: some `damage.props[i] > 0`; the motor may stay clean (below the motor threshold) — assert the prop only. If this scripted crash does not hit a prop sphere on the first try, adjust the recipe (more throttle, less height) and record the working one in the test's comment — the assertion is the contract, the recipe is the builder's (spec: "What is left to the builder").
  - `crash_damage_accrues_with_a_scripted_fault_present` — inject `Fault::PropDamage { prop: 0, thrust_scale: 0.9, imbalance_kgm: 1.0e-5 }` (via `set_fault`, the `mount.rs:23` pattern), then the tilted crash damaging prop 0: `state().damage.props[0] > 0` and `active_fault_names()` still names `"prop_damage"` (the composition values themselves are pinned by the in-crate unit test below).
  - `two_run_splittings_of_the_same_crash_agree` — `dropped_from(30.0)` in one `run_for` versus 100 × `run_for(t/100)`: equal `digest()`.
  - `a_damaged_prop_shakes_the_recorded_gyro` — two vehicles at `[0.0, 0.0, -1.0]`, one with `Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 2.0e-5 }` injected (the direct seam — the point is the recorded signal, not the crash), both `start_recording(&[SignalRate { pattern: "imu.gyro_frd_radps".into(), rate_hz: Some(2000.0) }], &path)` (the `recording.rs:70` pattern), 60% throttle for 0.5 s, stop and finish; read both recordings (the `records(path, topic)` helper, `recording.rs:55`) and assert the damaged run's gyro y peak-to-peak over the last 0.25 s exceeds the clean run's by a factor > 2 (the imbalance force rotates in the prop plane; the exact statistic is the builder's to pin — write the number the data gives and keep the factor).
  - In `open_loop_vehicle.rs`: `a_clean_flight_carries_no_damage` — the `resting_quad_stays_put` pattern plus 1 s of 60% throttle hover from the ground: `state().damage == DamageInfo::default()`.
  - In `vehicle.rs`'s unit tests: `the_composer_writes_the_seams_from_both_sides` — build the open-loop freestyle quad in-crate, `set_fault(PropDamage{prop: 0, thrust_scale: 0.8, imbalance_kgm: 1e-5}, true)`, assert the bus reads `(0.8, baseline + 1e-5)`; set `vehicle.damage.props[0] = 1.0` and call the composer (a `pub(crate) fn write_prop_seam(&mut self, i: usize)`): bus reads `(0.8 * 0.5, baseline + 1e-5 + 2.0e-5)`; deactivate the script fault (`active: false`): bus reads `(0.5, baseline + 2.0e-5)`.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test crash_damage --test open_loop_vehicle damage` and `cargo test -p ofs-sim --lib vehicle` — Expected: FAIL (no `damage` field, no `DamageInfo` on `VehicleState`).
- [ ] **Step 3: Implement** in `vehicle.rs`:
  - `build`: construct `let contact_feed = ContactFeed::new();` before the models, build the `RigidBody` with `RigidBody::with_contact_feed(airframe, initial, contact_feed.clone(), &mut bus)`, and build `DamageGeometry { motor_positions_frd_m: f.motor_positions_frd_m.iter().map(|p| v3(*p)).collect(), prop_tip_radius_m: cfg.props.iter().map(|p| p.diameter_m / 2.0).collect(), sphere_radius_m: 0.02 }` (custom `spheres_frd_m` quads: the first sphere's radius, `QuadConfig::collision_spheres`, `crates/ofs-config/src/lib.rs:1062`).
  - `Vehicle`: the five new fields; `script_*` vectors sized `n`, all `None`; `damage: PartDamage::new(n)`.
  - `fn apply_contact_damage(&mut self)` — `let hits = self.contact_feed.take(); if hits.is_empty() { return; }` then for each hit `damage::accrue(&mut self.damage, &self.damage_geo, &hit)` and rewrite the seam of every `Part` it returns. Call it in `step_ticks`'s loop after the `apply_due` block (`vehicle.rs:1618-1626`).
  - `fn write_prop_seam(&mut self, i: usize)`, `fn write_motor_seam(&mut self, i: usize)`, `fn write_battery_seam(&mut self)` — the composer of DC7: `thrust_scale = script.map_or(1.0, |(t, _)| t) * prop_thrust_scale(self.damage.props[i])`, `imbalance = baseline[i] + script.map_or(0.0, |(_, im)| im) + prop_imbalance_kgm(level)`, `motor scale = script.map_or(1.0, |s| s) * motor_scale(level)`, `battery r/v = script.map_or((0.0, 0.0), |(r, v)| (r, v)) + (curve_r, curve_v)` with the weak-cell index `BATTERY_FAULT_CELL + 1` when either the script fault or the battery level is active, `0` when neither.
  - `apply_fault`'s `PropDamage`, `MotorFailure`, `WeakCell` arms (`vehicle.rs:1396-1407`): store (or clear, `active = false`) the script memory, then call the seam writer instead of writing the bus directly.
  - `active_fault_names`: merge `self.active_faults` with `self.damage.active_fault_names()`, deduplicate, sort by the existing `ORDER` table (`vehicle.rs:1565-1580`).
  - `clear_faults`: after the existing resets, `self.damage = PartDamage::new(self.damage.props.len());` clear the script memory, then rewrite all seams (they return to identity/baseline through the composer; `init_fault_identities` already ran).
  - `VehicleState.damage: DamageInfo` filled in `state()` (`vehicle.rs:1749`'s neighbourhood) from `self.damage`; add the field to the struct at `vehicle.rs:285-313` and every literal constructor of `VehicleState` (search `VehicleState {`).
- [ ] **Step 4: Run** `cargo test -p ofs-sim --locked` and then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS, goldens included (no golden flight crashes; a golden digest change is a stop-and-investigate, Global Constraints).
- [ ] **Step 5: Commit** `feat(sim): crash damage in the vehicle, riding the fault seams (#157)`.

### Task 4: Repair and the resets — journal, clear_faults, Load

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (`repair`, `replay_action`), `crates/ofs-sim/src/journal.rs` (`Action::Repair`, write and parse), `crates/ofs-sim/src/damage.rs` (nothing new — `damage::repair` exists)
- Test: `crates/ofs-sim/tests/crash_damage.rs`, unit tests inside `journal.rs` (beside its fault round trips, `journal.rs:297`), `crates/ofs-sim/tests/journal.rs`

**Interfaces:**
- Consumes: Task 3's composer and `PartDamage`; `damage::repair`.
- Produces: `pub fn repair(&mut self, props: &[usize], motors: &[usize], battery: bool) -> Result<(), SimError>` on `Vehicle` — refuses an empty request ("nothing to repair: name a prop, a motor or the battery") and out-of-range indices exactly as `check_fault` refuses (`vehicle.rs:1316-1339`), otherwise zeroes the named levels, rewrites their seams, and journals `Action::Repair { props: Vec<usize>, motors: Vec<usize>, battery: bool }` (a new `Action` variant, `journal.rs:19-30`, written as `repair props=0,2 motors= battery=1` and parsed back, mirroring `ResetRace`'s arm).

- [ ] **Step 1: Write the failing tests:**
  - `crash_damage.rs`: `repair_is_part_selective` — the tilted-crash rig damaging props 0 (and whatever else); `repair(&[0], &[], false)`: `state().damage.props[0] == 0.0`, the other damage untouched; `repair(&[], &[], true)` then the remaining parts: everything clean, `active_fault_names()` empty. (The seam values a repair restores are pinned in-crate in Task 3's `the_composer_writes_the_seams_from_both_sides`, extended here if the executor prefers one home.)
  - `an_empty_repair_is_refused_and_an_out_of_range_index_too` — `repair(&[], &[], false)` and `repair(&[9], &[], false)` on a four-motor quad: `SimError::InvalidArgument`, and the damage state unchanged after each refusal.
  - `clear_faults_also_clears_crash_damage` — crash, `clear_faults()`, assert clean and `active_fault_names()` empty.
  - `the_crash_repair_round_trip` — the e2e the issue asks for: the tilted crash, then `state().damage.props[i] > 0`, then the recorded-gyro statistic of Task 3's `a_damaged_prop_shakes_the_recorded_gyro` measured on this vehicle (damaged: high), `repair` everything (clean), then the same statistic back at the clean run's level — crash → fly worse → repair → fly right, with the seam identity (thrust scale 1.0, imbalance at the part baseline) asserted by the in-crate unit test.
  - `journal.rs` unit (in `src/journal.rs` beside `all_faults`): `a_repair_action_round_trips` — write and read back `Action::Repair { props: vec![0, 2], motors: vec![1], battery: true }` and `Action::Repair { props: vec![], motors: vec![], battery: false }`, and the journal round trip in `tests/journal.rs` if it enumerates actions.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --locked repair journal` — Expected: FAIL.
- [ ] **Step 3: Implement** `Action::Repair` (write and parse arms in `journal.rs`, following `ResetRace`'s lines), `Vehicle::repair`, and the `replay_action` arm (`vehicle.rs:1639-1645`): `Action::Repair { props, motors, battery } => return self.repair(props, motors, battery)`.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(sim): repair and the damage resets - journal, clear_faults, Load (#157)`.

### Task 5: Protocol 17 — State.damage and the Repair RPC

**Files:**
- Modify: `proto/ofs/v1/sim.proto` (header comment, `State` fields 21-23, `message RepairRequest`, `rpc Repair`), `crates/ofs-proto/src/lib.rs` (version 17, doc comment), `crates/ofs-proto/tests/messages.rs` (version 17), `python/ofs/client.py` (`PROTOCOL_VERSION = 17` at line 26), `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` (regenerated), `crates/ofs-sim/src/session.rs` (`state_msg`), `crates/ofs-sim/src/server.rs` (the `repair` handler)
- Test: `crates/ofs-sim/src/session.rs` unit tests (beside `a_new_collision_count_raises_one_collision_event_naming_the_object`), `crates/ofs-sim/tests/grpc.rs`, `crates/ofs-proto/tests/messages.rs`

**Interfaces:**
- Consumes: Task 4's `Vehicle::repair`; Task 3's `VehicleState.damage`.
- Produces: the wire shapes of Design call 9 — `State`: `repeated double damage_prop = 21;` `repeated double damage_motor = 22;` `double damage_battery = 23;` (re-verify 21-23 are the next free numbers against the worktree's `State`, `proto/ofs/v1/sim.proto:229-250`; sibling coordination per the umbrella: #156's `State.moving_objects` also plans field 21 — whoever lands second takes the next free numbers); `message RepairRequest { repeated uint32 props = 1; repeated uint32 motors = 2; bool battery = 3; }`; `rpc Repair(RepairRequest) returns (Empty);` beside `ClearFaults` (line 38).

- [ ] **Step 1: Write the failing tests:**
  - `messages.rs`: bump the expected version to 17 (this is the failing test).
  - `session.rs` unit: `state_msg_carries_the_damage` — a hand-built `VehicleState` with `damage: DamageInfo { props: vec![0.0, 0.5], motors: vec![0.0, 0.0, 0.0, 0.0], battery: 0.25 }`; `state_msg()` carries the same numbers; a clean state carries all zeros and empty-but-present repeated fields.
  - `grpc.rs`: `repair_clears_damage_seen_on_the_state` — load the open-loop freestyle quad, drive a hard drop through `Run` until a collision event, assert `GetState().damage_battery > 0` (or `damage_prop` — whichever the drop produces; pin the flat drop → battery), `Repair { props: all, motors: all, battery: true }`, assert `GetState()` clean; `an_empty_repair_is_refused_over_grpc` — `InvalidArgument`.
- [ ] **Step 2: Run** `cargo test -p ofs-proto -p ofs-sim --locked damage repair protocol` — Expected: FAIL.
- [ ] **Step 3: Implement**: the proto additions (header comment: "protocol version 17: protocol 16 plus crash damage - State.damage and the Repair RPC (#157)"), bump `PROTOCOL_VERSION` and the Python constant, regenerate the stubs (Global Constraints command), fill `state_msg`, and add the server handler beside `clear_faults` (`server.rs:554-559`): parse the three fields, `vehicle.repair(&props, &motors, battery).map_err(sim_error)`, `Ok(pb::Empty {})`. Sweep live version references: `grep -rn "protocol 16\|PROTOCOL_VERSION" --include=*.rs --include=*.py --include=*.gd --include=*.md .` outside `docs/superpowers` and update (READMEs, `docs/dev-setup.md` if it names 16).
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS (SITL tests skip without `OFS_SITL_LAUNCH`; `test_proto_stubs.py` must pass against the regenerated stubs).
- [ ] **Step 5: Commit** `feat(proto)!: protocol 17 - State.damage and the Repair RPC (#157)`.

### Task 6: Clients — Python, Rust, the Godot bridge (read-only)

**Files:**
- Modify: `python/ofs/client.py` (`Damage` dataclass, `State.damage`, `_state`, `repair`), `crates/ofs-client/src/model.rs` (`Telemetry` fields, `from_pb`), `crates/ofs-godot/src/lib.rs` (`get_telemetry` keys and its doc line at `crates/ofs-godot/src/lib.rs:251`), `python/README.md`, `crates/ofs-client/tests/model.rs`, `python/tests/test_scripting.py`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 5's proto.
- Produces: Python `@dataclass(frozen=True) class Damage: props: tuple = (); motors: tuple = (); battery: float = 0.0`, `State.damage: Damage = field(default_factory=Damage)`, `def repair(self, props=(), motors=(), battery=False) -> None` on the sim client (the `_call` pattern of `clear_faults`, `python/ofs/client.py:559-563`); Rust `Telemetry { pub damage_prop: Vec<f64>, pub damage_motor: Vec<f64>, pub damage_battery: f64 }`; Godot `get_telemetry()` keys `damage_props` (Array of float), `damage_motors`, `damage_battery`.

- [ ] **Step 1: Write the failing tests:**
  - Rust, `crates/ofs-client/tests/model.rs` (the `World::from_pb`/telemetry conversion pattern): `the_damage_state_reaches_the_telemetry` — a `pb::State` with the three fields set converts to the `Telemetry` fields; a default `pb::State` gives empty vectors and 0.0.
  - Python, `python/tests/test_scripting.py` (the open-loop launch pattern): `the_state_carries_damage_and_repair_clears_it` — launch the open-loop quad over the flat world, sticks full pitch-down throttle until a collision event arrives on the watch stream (bounded wait), assert `state().damage` has a positive entry, `sim.repair(props=(0, 1, 2, 3), motors=(0, 1, 2, 3), battery=True)`, assert clean; plus a unit assertion that a default `State()` carries `Damage()`.
  - Godot, `godot/tests/e2e_open_loop.gd` (which already reads `app.client.get_telemetry()`, line 101): assert the dictionary has `damage_props` and `damage_motors` with 4 entries each and `damage_battery == 0.0` after a clean hover (the e2e flight does not crash; presence and shape are the check).
- [ ] **Step 2: Run** `cargo test -p ofs-client --locked` and `PYTHONPATH=python python -m pytest python/tests/test_scripting.py -q` — Expected: FAIL.
- [ ] **Step 3: Implement** the three clients. Python dataclass frozen with defaults so older constructors keep working (the `Race` pattern); no settings key, no HUD change, no worker command (the follow-on's).
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `PYTHONPATH=python python -m pytest python/tests -q`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and WSL `bash scripts/run-godot-tests.sh open-loop` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): damage state and repair in the Python, Rust and Godot clients (#157)`.

### Task 7: Docs, changelog and roadmap

**Files:**
- Modify: `docs/research/faults.md` (a "Crash damage" section), `docs/research/collision.md` (the "Where it shows" closing line and a pointer), `docs/roadmap.md` (the Physics line at `docs/roadmap.md:149`), `CHANGELOG.md` ("Unreleased — crash damage"), `python/README.md` (events and `State.damage`, `sim.repair`), `docs/validation.md` only if it enumerates waiting-for-data models (read it; today it names thrust-stand and blackbox-step only — if crash damage belongs in no list, change nothing)

**Interfaces:**
- Consumes: everything landed.
- Produces: documentation only.

- [ ] **Step 1: Write the faults.md section** "Crash damage (#157)": the mapping (DC2-DC4 in plain words — the entry impulse per sphere contact, the disc rule, the thresholds), the degradation curves, the composition with script-injected faults, the resets (`Repair`, `clear_faults`, `Load`), the constants table copied from the spec (every row "estimate", with the sizing arithmetic), and "Where it shows" (the fault events, `State.damage`, the seams, the workbench follow-on).
- [ ] **Step 2: Update** collision.md's closing "nothing is damaged" to point at the faults.md section; the roadmap's Physics backlog line drops "crash damage" and a known-limitations bullet is added (one-cell battery sag; no repair UI until the workbench, `Load` is the reset); the changelog (Added: crash damage, `State.damage`, `Repair`, protocol 17 breaking for clients of 16); python/README.md.
- [ ] **Step 3: Check** every changed doc's claims against the code by reading (no runs beyond the gates), and `grep -rn "protocol 16" docs README.md python/README.md` outside `docs/superpowers` for stale references.
- [ ] **Step 4: Run** the gates: `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-physics -p ofs-sim -p ofs-proto -p ofs-client -p ofs-godot`, `PYTHONPATH=python python -m pytest python/tests -q`, and WSL `cargo test --workspace --locked --no-fail-fast -- --test-threads=1` (goldens must not move) — Expected: PASS.
- [ ] **Step 5: Commit** `docs: crash damage - the catalog section, changelog and roadmap (#157)`.

## Ledger

- Empty. The spec's rulings stand as written; an executor changing one records it here with the date and the reason.
