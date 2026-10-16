# M6a — Rotor and frame physics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace three v1 physics simplifications with real models, each with an analytic test and provenance: rotor gyroscopic coupling (#16), windmilling drag on an unpowered rotor (#17), and a frame response between the motors and the flight controller's IMU (#18).

**Architecture:** All three are small additions to existing models, no new crate. Gyroscopic coupling is one more term in `RigidBody`'s Euler equation. Windmilling adds an advance-ratio dependence to the propeller's torque, fed by a new per-prop inflow signal that `RigidBody` publishes. The frame response is a second-order transmissibility filter, discretised once at the first IMU step, applied to the IMU's vibration disturbance only. Every new knob is optional part data that is off when absent, so a part without it is bit-identical to today.

**Tech Stack:** Rust workspace (`ofs-core`, `ofs-physics`, `ofs-electrical`, `ofs-sensors`, `ofs-config`, `ofs-sim`), glam `DVec3`, TOML library parts.

**Spec:** There is no dedicated M6 spec. This plan argues from the M6 section of `docs/superpowers/specs/2026-10-13-phase2-real-gear-trusted-numbers-design.md` §4 and the "done when" lines of issues #16, #17 and #18. The modelling calls the issues leave open are in "Design calls" below; they are the plan author's, for review.

**Scope note:** M6 is eight issues across five subsystems. This is plan **M6a** (#16, #17, #18). **M6b** (per-cell battery #19, thermal #20, receiver thresholds as world data #21) and **M6c** (validation harness #49, repro bundle #40, phase-close docs) are separate plans, written after this one lands.

## Design calls (the author's, not agreed with anyone)

1. **Gyroscopic term.** Rotor angular momentum `h = Σ spinₖ · Jₖ · ωₖ` along body +Z (`Jₖ` = `rotor_inertia_kgm2[k]`, which is already motor + prop inertia; `ωₖ` = mechanical rad/s). The body equation gains `− w × h`, with `h` held over the step like the torque. Rotor inertia is not added to the body's inertia tensor. Pole pairs never enter: the term uses mechanical speed.
2. **Windmilling is an advance-ratio correction to the prop's torque, not a new force.** With advance ratio `J = V/(nD)` (`V` the inflow along the prop axis, `n` rev/s) the torque coefficient becomes `Cp·(1 − J/J0)` for `V > 0`. `J0` is the zero-torque advance ratio, a new prop-part parameter `windmill_j0`; `0.0` (the default) turns the effect off. Torque is evaluated in the expanded form `Cp·ρ·D⁴·n·(D·n − V/J0)/2π`, so it has no `1/n` singularity and falls to zero as the rotor stops. Thrust is unchanged. Climb (`V ≤ 0`) is unchanged. A rotor at exactly 0 rad/s stays at 0 (the model is linear in `n` there); this is documented, not fixed.
3. **The effect applies to every rotor, powered or not**, so Betaflight-visible signals stay continuous; powered flight in a descent sees a torque reduction that is real physics. At `V = 0` the result is bit-identical to today.
4. **Inflow signal.** `RigidBody` publishes `prop.{i}.inflow_mps` = the mount point's velocity along body +Z (`v_body.z + (rate × position).z`), positive when the mount moves down. `Propeller` runs before `RigidBody` in a tick, so it reads the previous tick's value (deterministic, one tick late). The prop axis is assumed to be body Z.
5. **Frame response.** `T(s) = (1 + 2ζ·s/ωₙ) / (1 + 2ζ·s/ωₙ + s²/ωₙ²)`, the base-excitation transmissibility of a mass on a spring and damper: unity at DC, peak `√(1+4ζ²)/(2ζ)` at `ωₙ`, −40 dB/decade above `√2·ωₙ`. Discretised with a Tustin (bilinear) transform prewarped at `ωₙ`. It filters the IMU's vibration terms only (gyro roll/pitch disturbance, accel x/y), identically on each axis, not the true body rate or specific force. Parameters live in the frame part as `[frame.fc_mount]` (`natural_hz`, `damping_ratio`); absent means rigid, as today.
6. **Constants shipped.** `prop.windmill_j0 = 1.0` in the shipped prop (estimated). The shipped frame stays rigid (no `fc_mount`); a second frame part, `opendrone-5f-softmount`, carries `natural_hz = 120.0`, `damping_ratio = 0.15` (estimated) as the soft-mounted example. No new quad file.

## Global Constraints

- Every new constant in a library part carries a `[provenance."<path>"]` entry; none of the new values is measured, so all are `source = "estimated"` with a note.
- A new optional parameter that is absent or zero must leave the run bit-identical to before (same bus values, same MCAP bytes), except for the new `prop.{i}.inflow_mps` channel that Task 2 adds to recordings.
- Every task's behaviour has a unit test against an analytic result, with the formula written in the test.
- Determinism: no RNG anywhere in these models.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass at the end of every task (CI runs both; MSRV is Rust 1.85, so no newer language features).
- The ELRS radio goldens (`crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`) change in Tasks 1 and 2. Regenerate them in the same commit as the change that causes it, each OS on its own: Windows natively, Linux from WSL (commands in Task 1, Step 8).
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Work on a branch `m6a-rotor-frame-physics`, in a worktree (`superpowers:using-git-worktrees`). Do not tick items in the epic issue #73 or close #16–#18; the maintainer does that on merge.

## Review Focus

Failure modes the spec implies but a straight reading of the tasks would not test, most likely first. Each has its test in the named task.

1. A rotor that has stopped or is spinning at a different speed from its partner (a dead motor): the coupling must be exactly zero at ω = 0 and must not cancel when the speeds differ (Task 1: `an_unequal_rotor_pair_still_couples`, `a_stopped_rotor_adds_no_gyroscopic_torque`).
2. An open-loop build with mixed motor pole pairs (the roadmap's named watch item): must build, fly and stay finite (Task 1: `mixed_pole_pairs_fly_finite_in_open_loop`).
3. A prop that is nearly stopped or stopped in a fast descent: windmill torque must be finite, with no `1/n` blow-up (Task 2: `windmill_torque_is_finite_as_the_rotor_stops`).
4. A climb or a hover with windmilling enabled: unchanged from the static model (Task 2: `a_climbing_or_still_prop_is_unchanged`).
5. A motor dying in a descent: the rotor's speed (hence the eRPM and ESC telemetry Betaflight reads) changes by a bounded amount per tick, no step (Task 2: `a_dying_motor_in_a_descent_changes_speed_without_a_step`).
6. A mount whose natural frequency or damping makes the filter unstable or inaccurate (zero, negative, above the sample-rate limit): refused at config load (Task 3: `fc_mount_values_are_validated`).
7. A motor that stops while the filter is still ringing: the output must decay, not freeze or cut off (Task 3: `the_mount_keeps_ringing_after_the_motors_stop`).
8. A configured mount with no imbalance anywhere: bit-identical to no mount (Task 3: `a_mount_with_no_imbalance_changes_nothing`).

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/ofs-physics/src/rigid_body.rs` | modify | gyroscopic term; publish per-mount inflow |
| `crates/ofs-physics/src/propeller.rs` | modify | advance-ratio torque (`windmill_j0`), `thrust_torque_in_flow` |
| `crates/ofs-core/src/names.rs` | modify | `prop_inflow(i)` |
| `crates/ofs-sensors/src/mount.rs` | create | `SecondOrder` filter and `MountResponse` params |
| `crates/ofs-sensors/src/lib.rs` | modify | `pub mod mount;` |
| `crates/ofs-sensors/src/imu.rs` | modify | run vibration terms through the mount filter |
| `crates/ofs-config/src/lib.rs` | modify | `PropSection.windmill_j0`, `FrameSection.fc_mount`, `FcMountSection`, validation |
| `crates/ofs-sim/src/vehicle.rs` | modify | wire the new params into `build` |
| `library/props/opendrone-5in-prop.toml` | modify | `windmill_j0` + provenance |
| `library/frames/opendrone-5f-softmount.toml` | create | the soft-mounted frame example |
| `docs/research/rotor-dynamics.md` | create | the gyroscopic and windmilling models, constants table |
| `docs/research/faults.md`, `docs/research/component-library.md`, `docs/roadmap.md`, `CHANGELOG.md` | modify | reflect the three models |
| tests | see tasks | `crates/ofs-physics/tests/{rigid_body,propeller}.rs`, `crates/ofs-sim/tests/{open_loop_vehicle,windmill,mount}.rs`, `crates/ofs-sensors/tests/{mount,vibration}.rs`, `crates/ofs-config/tests/{config,part}.rs` |

---

### Task 1: Rotor gyroscopic coupling (#16)

**Files:**
- Modify: `crates/ofs-physics/src/rigid_body.rs` (struct `RigidBody`, `RigidBody::new`, `angular_accel`, `step`)
- Test: `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-sim/tests/open_loop_vehicle.rs`
- Create: `docs/research/rotor-dynamics.md`
- Modify: `docs/roadmap.md`, `CHANGELOG.md`; regenerate `crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`

**Interfaces:**
- Consumes: `names::motor_omega(i)` (`motor.{i}.omega_radps`, written by `EscMotor`), `AirframeParams.rotor_inertia_kgm2: Vec<f64>`, `MotorMount.spin: f64`.
- Produces: no new public API. `RigidBody` gains a private `omega: Vec<Signal<f64>>`, and `angular_accel(p: &AirframeParams, w: DVec3, torque: DVec3, h: DVec3) -> DVec3` (private) gains the rotor angular momentum `h`.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-physics/tests/rigid_body.rs`, using its existing helpers (`params`, `at`, `sim`, `vec3`, `set_scalar`). Each test builds `params(mounts, vec![])`, sets `p.rotor_inertia_kgm2 = vec![J; mounts.len()]`, starts with `rate_frd_radps = (2, 0, 0)` at `at(DVec3::new(0.0, 0.0, -1000.0))`, writes the rotors' speeds with `set_scalar(&mut s, &names::motor_omega(k), ω)`, and steps once. Frame inertia is the helper's `(0.0025, 0.0025, 0.0045)`.
  - `a_spinning_rotor_precesses_a_rotating_body`: one mount at the origin, `spin: 1.0`, `J = 1.0e-4`, ω = 2000 rad/s, so `h = 0.2`. After one `s.step()`, `rate.y` equals `ω_x·h/I_y · dt` = `2·0.2/0.0025/8000` = `0.02` within 1 %. This is the analytic precession `ω̇ = −(w × h)/I`.
  - `counter_rotating_pairs_cancel_the_gyroscopic_torque`: two mounts at the origin with `spin` +1 and −1, the same `J` and ω; after `run_for(0.5)`, `rate.y.abs() < 1e-9`.
  - `an_unequal_rotor_pair_still_couples`: the same two mounts with ω = 2000 and 1000; after one step `rate.y` equals `2·(J·1000)/0.0025/8000` within 1 %.
  - `a_stopped_rotor_adds_no_gyroscopic_torque`: one mount, ω left at 0; after `run_for(0.5)`, `rate.y == 0.0` exactly and `rate.x == 2.0` within 1e-9 (a torque-free pure-roll spin is a fixed point).
  - `the_gyroscopic_torque_does_no_work`: one mount at the origin, `J = 1.0e-4`, ω = 1000 (so `h = 0.1`); start with `rate = (0.3, 0.2, 5.0)` and inertia `(0.002, 0.003, 0.004)`; after `run_for(2.0)`, the rotation energy `½ Σ Iᵢwᵢ²` is within 1e-3 relative of its start (`w · (w × h) = 0`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-physics --test rigid_body --locked`
Expected: `a_spinning_rotor_precesses_a_rotating_body` and `an_unequal_rotor_pair_still_couples` FAIL (`rate.y` stays 0); the other three PASS already (they hold with no coupling). That is the point: they pin the cases that must not regress.

- [ ] **Step 3: Implement** in `crates/ofs-physics/src/rigid_body.rs`:
  - Add field `omega: Vec<Signal<f64>>` to `RigidBody`, filled in `new` with `bus.signal(&names::motor_omega(i))` for each mount.
  - Change `fn angular_accel(p: &AirframeParams, w: DVec3, torque: DVec3, h: DVec3) -> DVec3` to `(torque - w.cross(i * w) - w.cross(h) - p.angular_damping_nms * w) / i`.
  - In `step`, inside the existing mounts loop, accumulate `h.z += m.spin * self.p.rotor_inertia_kgm2[k] * bus.get(self.omega[k])`; pass `h` to both `angular_accel` calls (held over the step, like `torque_body`).
  - Update the module doc comment's first line to mention gyroscopic coupling.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-physics --locked`
Expected: all PASS, including the pre-existing tests (they use `rotor_inertia_kgm2 = 0`, so `h = 0` and nothing moves).

- [ ] **Step 5: Write the mixed-pole-pairs test** in `crates/ofs-sim/tests/open_loop_vehicle.rs`, `mixed_pole_pairs_fly_finite_in_open_loop`: `let mut cfg = load(Path::new(QUAD)).unwrap(); cfg.motors[1].pole_pairs = 5;` (loading validated the file; the mutation is for the open-loop build, which does not require uniform pole pairs), `let mut v = vehicle_in(&cfg, WorldConfig::open_field());`, `v.set_sticks(&throttle(0.7)); v.run_for(1.0).unwrap();`, then assert `v.state().pos_ned_m.is_finite()`, `rate_frd_radps.is_finite()`, and that the quad climbed (`pos_ned_m.z < -0.5`).

- [ ] **Step 6: Run it**

Run: `cargo test -p ofs-sim --test open_loop_vehicle --locked`
Expected: PASS.

- [ ] **Step 7: Document.** Create `docs/research/rotor-dynamics.md` with a "Rotor gyroscopic coupling" section: the equation `I·ẇ = τ − w×(I·w) − w×h − c·w`, `h = Σ spinₖ·Jₖ·ωₖ ẑ`, why it pairs with the existing `−spin·J·ω̇` reaction term, that mechanical speed is used (pole pairs irrelevant), that rotor inertia is not folded into the body inertia, and a constants table (`rotor_inertia_kgm2`: the motor and prop parts' own, estimated). In `docs/roadmap.md` remove "No rotor gyroscopic coupling;" from the Physics list and rewrite the "Vibration is rigid-body…" bullet to drop its pole-pair sentence. Add a `CHANGELOG.md` bullet under a new `## Unreleased — M6a: rotor and frame physics` heading, "Added" (rotor gyroscopic coupling) and "Changed" (every seeded flight with spinning rotors differs slightly from 0.1.0; the radio goldens were regenerated).

- [ ] **Step 8: Regenerate the goldens and run everything.**

Run (PowerShell): `$env:OFS_GOLDEN_WRITE=1; cargo test -p ofs-sim --test radio_golden --locked; Remove-Item Env:OFS_GOLDEN_WRITE`
Then from WSL, in `/mnt/c/dev/open-fpv-sim` (writes the Linux file): `OFS_GOLDEN_WRITE=1 CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test -p ofs-sim --test radio_golden --locked`
Then: `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`; and from WSL `CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test --workspace --locked` (no write flag).
Expected: both digest files changed in `git diff --stat`; every test PASSES on both OSes. If another test fails on a numeric tolerance, it pins a flight the coupling legitimately moves: report it with the numbers before loosening anything.

- [ ] **Step 9: Commit**

```bash
git add crates/ofs-physics crates/ofs-sim/tests docs CHANGELOG.md
git commit -m "feat(physics): rotor gyroscopic coupling (#16)"
```

---

### Task 2: Windmilling drag on an unpowered rotor (#17)

**Files:**
- Modify: `crates/ofs-core/src/names.rs`, `crates/ofs-physics/src/propeller.rs`, `crates/ofs-physics/src/rigid_body.rs`, `crates/ofs-config/src/lib.rs` (`PropSection`, `validate`), `crates/ofs-sim/src/vehicle.rs` (`build`, the `PropParams` literal at the top of `build`), `library/props/opendrone-5in-prop.toml`
- Test: `crates/ofs-physics/tests/propeller.rs`, `crates/ofs-physics/tests/rigid_body.rs`, `crates/ofs-config/tests/config.rs`, create `crates/ofs-sim/tests/windmill.rs`
- Modify: `docs/research/rotor-dynamics.md`, `docs/research/faults.md`, `docs/research/component-library.md`, `docs/roadmap.md`, `CHANGELOG.md`; regenerate the goldens

**Interfaces:**
- Consumes: Task 1's `RigidBody` (adds a second published signal beside it).
- Produces:
  - `ofs_core::names::prop_inflow(i: usize) -> String` = `"prop.{i}.inflow_mps"`.
  - `ofs_physics::propeller::PropParams.windmill_j0: f64` (0.0 = off).
  - `ofs_physics::propeller::thrust_torque_in_flow(p: &PropParams, omega_radps: f64, inflow_mps: f64) -> (f64, f64)`; `thrust_torque(p, omega)` stays and delegates with `0.0`.
  - `ofs_config::PropSection.windmill_j0: f64` with `#[serde(default)]`.

- [ ] **Step 1: Write the failing propeller tests** in `crates/ofs-physics/tests/propeller.rs`. The file's `prop()` helper builds `PropParams` with a struct literal: add `windmill_j0: 0.0` there and add `fn windmill_prop(j0: f64) -> PropParams` (flat `ct = 0.11`, `cp = 0.045`, `windmill_j0: j0`). Tests (D = 0.127, ρ = 1.225, n = 100 rev/s, `omega = 2π·100`):
  - `windmilling_is_off_without_j0`: with `j0 = 0.0`, `thrust_torque_in_flow(&p, omega, 15.0) == thrust_torque(&p, omega)` (exact tuple equality).
  - `torque_vanishes_at_the_zero_torque_advance_ratio`: `j0 = 1.0`, `V = 1.0·n·D`; `q.abs() < 1e-12`, and thrust equals the static thrust exactly.
  - `torque_reverses_past_the_zero_torque_advance_ratio`: `V = 2.0·n·D`; `q` equals `−` the static torque within 1e-12 (`Cp·(1 − 2) = −Cp`).
  - `a_climbing_or_still_prop_is_unchanged`: for inflow `0.0` and `-5.0`, equals the static result exactly.
  - `windmill_torque_is_finite_as_the_rotor_stops`: `omega = 1e-9`, inflow 20.0, `j0 = 1.0`: both outputs finite and `q.abs() < 1e-12`; at `omega = 0.0` the result is `(0.0, 0.0)`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-physics --test propeller --locked`
Expected: compile error (`windmill_j0`, `thrust_torque_in_flow` undefined).

- [ ] **Step 3: Implement** in `crates/ofs-physics/src/propeller.rs` and `crates/ofs-core/src/names.rs`:
  - `PropParams.windmill_j0: f64` documented as "the zero-torque advance ratio; 0 turns windmilling off".
  - `pub fn thrust_torque_in_flow(p: &PropParams, omega_radps: f64, inflow_mps: f64) -> (f64, f64)`. When `omega_radps <= 0.0` return `(0.0, 0.0)` as today. When `p.windmill_j0 > 0.0 && inflow_mps > 0.0`, torque is `cp·ρ·D⁴·n·(D·n − inflow_mps/p.windmill_j0)/(2π)`; otherwise the existing expression, unchanged character for character, so the off path is bit-identical. Thrust is always the existing expression. `thrust_torque` calls it with `0.0`.
  - `Propeller` gains `inflow: Signal<f64>` registered as `names::prop_inflow(index)`; `step` calls `thrust_torque_in_flow(&self.p, omega, bus.get(self.inflow))`.
  - `names::prop_inflow` with a doc comment: axial inflow at the prop, positive while the mount moves down, published by the rigid body.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-physics --test propeller --locked`
Expected: PASS.

- [ ] **Step 5: Write the failing inflow test** in `crates/ofs-physics/tests/rigid_body.rs`, `a_descending_body_publishes_the_inflow_at_each_mount`: one mount at `(0.08, 0.08, 0.0)`, body velocity `(0, 0, 5.0)` and rate `(10, 0, 0)` at identity attitude, far above the ground, one `step()`. The signal `names::prop_inflow(0)` equals `5.0 + 10.0·0.08 = 5.8` within 1e-9 (the mount's downward speed: the body's plus `(rate × position).z`). The test needs `Scheduler`'s `lookup` on the name, as the file's `vec3` helper does for vectors; add a `scalar(&Scheduler, &str) -> f64` helper beside it if there is none.

- [ ] **Step 6: Run to verify it fails, then implement.** `cargo test -p ofs-physics --test rigid_body --locked` fails (signal unknown). In `rigid_body.rs` add `inflow: Vec<Signal<f64>>` registered in `new`, and in the mounts loop of `step` set `bus.set(self.inflow[k], v_body.z + s.rate_frd_radps.cross(m.position_frd_m).z)` using the start-of-step state (move the `v_body` computation above the loop). Re-run: PASS.

- [ ] **Step 7: Config and part data.** In `crates/ofs-config/src/lib.rs` add to `PropSection`: `#[serde(default)] pub windmill_j0: f64` (doc: zero-torque advance ratio; 0 = off) and in `validate` `c.non_negative(p.windmill_j0, &format!("props[{i}].windmill_j0"))`. In `vehicle.rs` `build` pass `windmill_j0: cfg.props[i].windmill_j0` in the `PropParams` literal. In `library/props/opendrone-5in-prop.toml` add `windmill_j0 = 1.0` under `[prop]` and `[provenance."prop.windmill_j0"]` with `source = "estimated"` and a note: "zero-torque advance ratio of a small propeller; order-of-magnitude estimate, not measured". Add a config test in `crates/ofs-config/tests/config.rs`, `windmill_j0_must_not_be_negative`, following that file's pattern for the other `props[i]` fields (a prop part with `windmill_j0 = -1.0` is refused with the field named). Run `cargo test -p ofs-config --locked`: PASS.

- [ ] **Step 8: Write the integration test** `crates/ofs-sim/tests/windmill.rs` (a `Scheduler` at 8000 Hz holding one `Propeller` and one `EscMotor`, built like `crates/ofs-electrical/tests/esc_motor.rs`'s `sim`, with `Propeller::new(0, PropParams { diameter_m: 0.127, flat ct 0.11, flat cp 0.045, windmill_j0: 1.0 }, &mut bus)` first, battery voltage 16 V, motor `kv 2000`, `R 0.1`, `no_load_current_a 0.05`, `rotor_inertia 5e-6`, prop inertia `5e-6`, ESC `tau 0.002`, limit 40 A):
  - Helper `kill_and_fall(inflow: f64, secs: f64) -> Scheduler`: set `motor_cmd(0)` to 0.6, `run_for(0.5)` powered, then write `fault_motor_scale(0) = 0.0` and `prop_inflow(0) = inflow`, `run_for(secs)`.
  - `an_unpowered_rotor_settles_at_the_analytic_windmill_speed`: `kill_and_fall(20.0, 5.0)`. Steady state is where aero drive balances friction: with `Ke = 60/(2π·kv)`, `F = Ke·I0`, `κ = 2π·F/(ρ·D⁴·Cp)`, the larger root of `D·n² − (V/J0)·n + κ = 0`, i.e. `n = ((V/J0) + √((V/J0)² − 4Dκ)) / (2D)`. Assert `motor_omega(0)` is within 1 % of `2π·n`.
  - `a_slow_descent_does_not_keep_the_rotor_turning`: `kill_and_fall(5.0, 20.0)`: here `(V/J0)² < 4Dκ`, so no root; assert `motor_omega(0) == 0.0`.
  - `a_dying_motor_in_a_descent_changes_speed_without_a_step`: as the first test, but record `motor_omega(0)` every tick from the kill onward for 1 s; the largest tick-to-tick change is below 1.0 rad/s.

- [ ] **Step 9: Run it**

Run: `cargo test -p ofs-sim --test windmill --locked`
Expected: PASS. If the 1 % assertion misses, print `n` and ω first: the likely cause is the 5 s run not having settled, not the model.

- [ ] **Step 10: Document.** Extend `docs/research/rotor-dynamics.md` with a "Windmilling" section: the corrected torque coefficient and its expanded form, why it is evaluated that way, the steady-state equation from Step 8, why a 1.2 A no-load "friction" makes windmilling start only at high descent speed, the stopped-rotor-stays-stopped limitation, climb unchanged, thrust unchanged, and the constants table (`windmill_j0` estimated). In `docs/research/faults.md` "Motor failure" replace "there is no windmilling model (carried debt)" with a pointer to the new section. Add the `prop.windmill_j0` row to `docs/research/component-library.md`'s prop table. Remove "A dead motor freewheels through its prop's drag only." from `docs/roadmap.md`. Add `CHANGELOG.md` entries (Added: windmilling and `windmill_j0`, the `prop.{i}.inflow_mps` channel; Changed: goldens regenerated).

- [ ] **Step 11: Regenerate the goldens and run everything**, exactly as Task 1 Step 8. Expected: both digest files change (the new `prop.*` channel enters every recording); all tests PASS on both OSes.

- [ ] **Step 12: Commit**

```bash
git add crates library docs CHANGELOG.md
git commit -m "feat(physics): windmilling torque on an unpowered rotor (#17)"
```

---

### Task 3: Frame response between the motors and the IMU (#18)

**Files:**
- Create: `crates/ofs-sensors/src/mount.rs`, `library/frames/opendrone-5f-softmount.toml`
- Modify: `crates/ofs-sensors/src/lib.rs`, `crates/ofs-sensors/src/imu.rs`, `crates/ofs-config/src/lib.rs` (`FrameSection`, `FcMountSection`, `validate`), `crates/ofs-sim/src/vehicle.rs` (the `VibrationParams` literal in `build`)
- Test: create `crates/ofs-sensors/tests/mount.rs`, `crates/ofs-sim/tests/mount.rs`; modify `crates/ofs-sensors/tests/vibration.rs`, `crates/ofs-config/tests/config.rs`, `crates/ofs-config/tests/part.rs`
- Modify: `docs/research/faults.md`, `docs/research/rotor-dynamics.md`, `docs/research/component-library.md`, `docs/roadmap.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing from Tasks 1–2.
- Produces:
  - `ofs_sensors::mount::MountResponse { pub natural_hz: f64, pub damping_ratio: f64 }` (`Debug, Clone, Copy, PartialEq`).
  - `ofs_sensors::mount::SecondOrder` with `pub fn transmissibility(m: MountResponse, dt_s: f64) -> SecondOrder`, `pub fn step(&mut self, x: f64) -> f64`, `pub fn is_idle(&self) -> bool` (true while both state words are exactly 0.0).
  - `ofs_sensors::imu::VibrationParams.mount: Option<MountResponse>`.
  - `ofs_config::FrameSection.fc_mount: Option<FcMountSection>` with `#[serde(default)]`; `FcMountSection { pub natural_hz: f64, pub damping_ratio: f64 }` (`Debug, Clone, Deserialize`, `deny_unknown_fields`).

- [ ] **Step 1: Write the failing filter tests** in `crates/ofs-sensors/tests/mount.rs`. Setup: `fs = 8000 Hz`, `dt = 1/8000`, `MountResponse { natural_hz: 100.0, damping_ratio: 0.1 }`. A helper `gain(f_hz) -> f64` drives `SecondOrder::transmissibility(m, dt)` with `sin(2π·f·t)` for 1.0 s and returns the peak absolute output over the last 0.1 s. The analytic gain is `|T| = √(1 + (2ζr)²) / √((1 − r²)² + (2ζr)²)`, `r = f/fn`.
  - `the_gain_is_unity_at_dc`: a constant input of 1.0 settles to 1.0 within 1e-6.
  - `the_gain_peaks_at_resonance`: `gain(100.0)` is within 3 % of `√(1+4ζ²)/(2ζ)` = 5.10.
  - `the_gain_rolls_off_above_resonance`: `gain(400.0)` is within 3 % of the analytic value (about 0.085); `gain(40.0)` within 3 %.
  - `a_stiff_mount_passes_the_vibration`: natural_hz 800.0, damping 0.7, `gain(50.0)` within 1 % of 1.0.
  - `a_fresh_filter_is_idle_and_a_driven_one_is_not`: `is_idle()` true before any step, false after a non-zero input.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-sensors --test mount --locked`
Expected: compile error (`mount` module missing).

- [ ] **Step 3: Implement `crates/ofs-sensors/src/mount.rs`** and `pub mod mount;` in `lib.rs`. `SecondOrder` is a direct-form-II-transposed biquad (`b0, b1, b2, a1, a2` normalised, state `z1, z2`). Coefficients, with `K = ωn / tan(ωn·dt/2)` (prewarped at `ωn = 2π·natural_hz`), `c = K/ωn`, `a0 = 1 + 2ζc + c²`:
  `b0 = (1 + 2ζc)/a0`, `b1 = 2/a0`, `b2 = (1 − 2ζc)/a0`, `a1 = (2 − 2c²)/a0`, `a2 = (1 − 2ζc + c²)/a0`.
  Derived by substituting the Tustin map into `T(s)`; DC gain is exactly 1 (`b0+b1+b2 = 4/a0 = 1 + a1 + a2`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-sensors --test mount --locked`
Expected: PASS.

- [ ] **Step 5: Write the failing IMU tests** in `crates/ofs-sensors/tests/vibration.rs`. Add `mount: None` to the file's `vib()` literal and a `fn vib_with(mount: MountResponse) -> VibrationParams`. Add a helper `run_for_steps(vibration, omega, imbalance, steps) -> (DVec3, DVec3)` (the existing `run` fixes 400 steps; keep it and have both share the body). With the file's constants (`OMEGA = 300`, rotor frequency `300/2π = 47.7 Hz`):
  - `a_soft_mount_attenuates_the_rotating_force_by_the_analytic_gain`: mount `natural_hz = 20.0`, `damping_ratio = 0.3`, imbalance 1e-3, 4800 steps (0.6 s); the xy accel magnitude equals `|T(r)|·F/m` within 5 %, with `r = 47.7/20`, `F = 1e-3·300²`, `m = 0.65` (a linear filter turns a rotating vector into a scaled rotating vector, so the magnitude is the gain).
  - `a_stiff_mount_matches_the_rigid_body`: `natural_hz = 800.0`, `damping_ratio = 0.7`; the xy magnitude is within 2 % of the no-mount run.
  - `the_mount_keeps_ringing_after_the_motors_stop`: build the `Imu` by hand (as `run` does) with a soft mount, run 2400 steps at `OMEGA` with imbalance, then write `motor.0.omega_radps = 0.0` and step 20 more times: the xy accel magnitude is still above 10 % of its value at the instant the motor stopped, and after 8000 more steps it is below 1 % of it.
  - `a_mount_with_no_imbalance_changes_nothing`: same seed and non-zero noise on both `ImuParams`; one with `mount: Some(..)`, one `None`; imbalance 0.0, 400 steps; `gyro` and `accel` compare equal with `assert_eq!` (bit-identical).

- [ ] **Step 6: Run to verify they fail**

Run: `cargo test -p ofs-sensors --test vibration --locked`
Expected: compile error (`VibrationParams.mount` missing).

- [ ] **Step 7: Implement in `imu.rs`.** Add `pub mount: Option<MountResponse>` to `VibrationParams`. `Imu` gains `mount: Option<MountFilter>` (private, `None` until the first step), where `MountFilter` holds four `SecondOrder` (gyro x, gyro y, accel x, accel y) built from `ctx.dt_s` at the first `step` (the first step has the sample time; `Imu::new` does not). In the vibration block, when `v.mount` is `Some`: pass `rate_disturb.x/.y` and `force_frd.x/.y` through the filters on **every** step (zero input when no motor contributes), and add the filtered terms to `gyro` and `accel` only if `disturbed` or the filters are not all idle, so an all-quiet run stays bit-identical. The `force_frd * (1/mass)` scaling stays where it is. The no-mount path is untouched.

- [ ] **Step 8: Run to verify they pass**

Run: `cargo test -p ofs-sensors --locked`
Expected: PASS, including the pre-existing `sensors.rs` and `vibration.rs` tests.

- [ ] **Step 9: Config.** In `crates/ofs-config/src/lib.rs` add `FcMountSection` and `#[serde(default)] pub fc_mount: Option<FcMountSection>` to `FrameSection`. In `validate`, when present: `natural_hz` finite, `> 0` and `<= sim.base_hz as f64 / 10.0` (the bilinear transform stays accurate); `damping_ratio` finite, `> 0` and `<= 5.0`. Field names in problems: `frame.fc_mount.natural_hz`, `frame.fc_mount.damping_ratio`. Tests in `crates/ofs-config/tests/config.rs`, `fc_mount_values_are_validated`: a quad whose frame sets `natural_hz = 0.0`, `natural_hz = 900.0` (above 800 at 8 kHz), `damping_ratio = 0.0` and `damping_ratio = 6.0` is each refused naming the field; `natural_hz = 120.0, damping_ratio = 0.15` loads. In `crates/ofs-config/tests/part.rs`, `a_frame_part_may_carry_an_fc_mount`: a frame part with `[frame.fc_mount]` and provenance for both leaf paths loads; the same part without the provenance entries is refused with "missing provenance". Follow those files' existing fixture style (`FRAME` constant, `write`).

- [ ] **Step 10: Run config tests**

Run: `cargo test -p ofs-config --locked`
Expected: PASS.

- [ ] **Step 11: Part, wiring, vehicle tests.** Create `library/frames/opendrone-5f-softmount.toml`: a copy of `opendrone-5f.toml` with the name `"OpenDrone 5F frame, soft-mounted FC"`, plus `[frame.fc_mount]` (`natural_hz = 120.0`, `damping_ratio = 0.15`) and the two provenance entries, `source = "estimated"`, note "rubber-grommet FC stack, order of magnitude; not measured". In `vehicle.rs` `build` set `mount: f.fc_mount.as_ref().map(|m| MountResponse { natural_hz: m.natural_hz, damping_ratio: m.damping_ratio })` in the `VibrationParams` literal. Create `crates/ofs-sim/tests/mount.rs` (helper style of `open_loop_vehicle.rs`: load `quads/opendrone-5f-freestyle.toml`, `FcKind::OpenLoop`, `WorldConfig::open_field()`):
  - `a_mount_with_no_imbalance_changes_nothing`: build two vehicles from the same config, one with `cfg.frame.fc_mount = Some(FcMountSection { natural_hz: 120.0, damping_ratio: 0.15 })`; `set_sticks` throttle 0.6, `run_for(0.5)`; `digest()` is equal.
  - `a_mount_changes_what_a_damaged_prop_does_to_the_imu`: the same two vehicles with `Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 5.0e-4 }` set active before the run; digests differ.
  - `the_softmount_library_frame_loads`: `ofs_config::part::load_frame` on `library/frames/opendrone-5f-softmount.toml` yields `frame.fc_mount == Some(..)` with `natural_hz == 120.0`.

- [ ] **Step 12: Run it**

Run: `cargo test -p ofs-sim --test mount --locked`
Expected: PASS.

- [ ] **Step 13: Document.** In `docs/research/faults.md` rewrite the paragraph of "Prop damage and the vibration model" that says "rigid-body transmission, below-resonance amplitude only" to describe the optional mount filter (still rigid when absent) and point to `rotor-dynamics.md`. Add a "Frame response" section to `docs/research/rotor-dynamics.md`: `T(s)`, the peak and roll-off, the Tustin discretisation with the coefficient formulas, why only the vibration terms are filtered, the `natural_hz <= base_hz/10` rule, and the constants table (both estimated). Add the `opendrone-5f-softmount` section to `docs/research/component-library.md`. Delete "Vibration is rigid-body and below resonance" from `docs/roadmap.md`. Add the `CHANGELOG.md` entry (Added: `[frame.fc_mount]`, the soft-mount frame part).

- [ ] **Step 14: Run everything.** Goldens must **not** change in this task.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`; from WSL `CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test --workspace --locked`.
Expected: all PASS with `git status` showing no change to `tests/golden/`.

- [ ] **Step 15: Commit**

```bash
git add crates library docs CHANGELOG.md
git commit -m "feat(sensors): frame response between the motors and the IMU (#18)"
```

---

### Task 4: Wrap-up and hand-off

**Files:**
- Modify: `docs/roadmap.md` (a one-line status under "Post-v1 backlog" naming M6a done and M6b/M6c pending)
- Verify only: everything else

- [ ] **Step 1: MSRV check.** Run `cargo +1.85 check --workspace --all-targets --locked --exclude ofs-godot`. Expected: finishes with no error. (If the 1.85 toolchain is not installed locally, say so and leave it to the `msrv` CI job rather than skipping silently.)

- [ ] **Step 2: Whole-branch verification.** Run the full command set of Task 3 Step 14 once more on a clean tree, plus `python -m pytest python/tests -v` (SITL tests skip without `OFS_SITL_LAUNCH`).
Expected: all PASS.

- [ ] **Step 3: Live-SITL sanity check** (needs the WSL SITL; skip and say so if unavailable): `CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf ~/.cargo/bin/cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` from WSL. Expected: PASS. This is the only check that Betaflight still hovers a quad whose rotors now couple gyroscopically.

- [ ] **Step 4: Roadmap status line and commit.**

```bash
git add docs/roadmap.md
git commit -m "docs: M6a done - gyroscopic coupling, windmilling, frame response"
```

- [ ] **Step 5: Report to the maintainer**, in the PR description: the three models and their constants (all estimated), that two goldens changed per OS and why, the live-SITL result, and that #16–#18 can be ticked in #73 on merge. Then write the M6b plan.
