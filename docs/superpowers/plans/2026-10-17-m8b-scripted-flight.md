# M8b: Scripted Flight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Python script can fly a precise path. Open-loop sessions get an opt-in built-in stabilizer (#66), and a scripted pilot flies a closed circuit through the gates M8a added, on the stabilizer in CI and against real Betaflight SITL (#53), asserting the lap completes. Closes #66 and #53.

**Architecture:** A pure control law in `ofs-fc` (`stabilizer.rs`: angle → rate → motor mixer built from the frame's geometry) wrapped by a `Stabilizer` scheduler model that replaces `OpenLoopFc` when the session asks for it (`FcKind::OpenLoopStabilized`, protocol 12 `LoadRequest.stabilize`, carried in repro bundles). Its stick semantics are Betaflight's angle mode, so one Python pilot (`python/examples/circuit.py`: a position controller that outputs sticks) flies both. A new world `worlds/circuit.toml` has four gates in a loop.

**Tech Stack:** Rust workspace (MSRV 1.85), tonic/prost proto + generated Python stubs, Python `ofs`, Betaflight SITL in WSL for the live test.

**Spec:** none beyond GitHub issues #66 and #53 (`gh issue view <n> --repo hugowis/open-fpv-sim`) and the "Design calls" below, which are binding; M8a's `docs/race-timing.md` defines the gates and laps this milestone flies.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; WSL `cargo test --workspace --locked --no-fail-fast -- --test-threads=1` green with `OFS_SITL_LAUNCH` set; `cargo +1.85 check` on the changed crates (not `ofs-godot`, which needs 1.94).
- **No golden digest moves** (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`): `open_loop_fc` alone stays throttle-only and byte-identical. A moved golden is a bug.
- `cargo fmt` is not a gate; do not reformat existing files.
- **Line endings:** never rewrite a file through Python text mode on Windows (it writes CRLF); use Edit/Write or `open(..., newline="")`. Check new files with `git ls-files --eol` before committing (all LF).
- Worktree-isolated sessions refuse heredocs and compound git commands: use Edit/Write for files; WSL goes through PowerShell with a script file in the scratchpad (`wsl -d Ubuntu -- bash <path>`); put `$HOME/.cargo/bin` on PATH in those scripts.
- Regenerate the Python stubs after the proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`; only the three generated files change.
- Python tests need `PYTHONPATH=<worktree>/python` and a built `target/debug/ofs-sim`.
- Live SITL (WSL): `CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf`; `pkill -9 -x betaflight_SITL` first if a port is busy. SITL tests from Python run against `target/debug/ofs-sim` built in WSL.
- Commit trailers: a blank line, then `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` on its own line.

## Design calls (decided here; the executor ledgers any change)

1. **Opt-in, never default.** `open_loop_fc = true` keeps meaning "throttle to every motor". The stabilizer is `open_loop_fc = true` plus `stabilize = true` (`FcKind::OpenLoopStabilized`, TOML value `open_loop_stabilized`; protocol 12 `LoadRequest.stabilize = 8`; Python `load(..., stabilize=True)`). `stabilize` without `open_loop_fc` is refused with `invalid_argument` ("stabilize needs open_loop_fc"). A repro bundle records it (`stabilize` in `bundle.toml`; absent = false) and replays it.
2. **It is a test stand-in, not Betaflight.** Angle mode only. Inputs are the simulator's truth (`body.att_q`, `body.rate_frd_radps`): no IMU noise, no filters, no RPM filter. No arming (it is live whenever throttle > `IDLE_THROTTLE = 0.05`; below that every motor is 0, so a quad on the ground stays put), no rates or expo, no I-term, no TPA, no anti-gravity, no airmode. Docs keep saying real tuning needs SITL.
3. **Stick semantics = Betaflight angle mode**, so one script flies both: roll stick `+` leans right (positive roll), pitch stick `+` is forward (nose down, so the aerospace pitch is negative), yaw stick `+` turns clockwise seen from above (yaw rate about down, positive), each in [-1, 1]. Roll and pitch sticks set the attitude target `stick * MAX_ANGLE_DEG`, `MAX_ANGLE_DEG = 60.0` (Betaflight's default `angle_limit`); the yaw stick sets a yaw rate `stick * MAX_YAW_RATE_DPS`, `MAX_YAW_RATE_DPS = 200.0`; throttle is the collective command, as in the throttle-only stand-in (`cmd = throttle`).
4. **Control law.** Cascade: `rate_target = KP_ANGLE * (angle_target - angle)` clamped to `MAX_RATE_DPS = 400` for roll and pitch; `out = KP_RATE * (rate_target - rate)` per axis (P only); mixer `cmd_i = clamp(throttle + out_roll*mr_i + out_pitch*mp_i + out_yaw*my_i, 0, 1)` with `mr_i = -y_i / max|y|`, `mp_i = x_i / max|x|`, `my_i = -spin_i` from `frame.motor_positions_frd_m` and `frame.motor_spin` (+1 = clockwise seen from above). Starting gains `KP_ANGLE = 15.0` (1/s), `KP_RATE_ROLL_PITCH = 0.035`, `KP_RATE_YAW = 0.03` (command per rad/s) are estimates from the airframe's inertia and thrust slope; the executor tunes them against the behaviour tests below and records the final values in the ledger, the constants' doc comment and `docs/scripted-flight.md`. Roll/pitch/yaw errors use the truth attitude's Euler angles (aerospace Z-Y-X), the yaw loop is rate-only.
5. **One pilot, two backends.** `python/examples/circuit.py` fly a list of waypoints with truth state from `sim.state()`: altitude PID to throttle (like `hover.py`), a position-to-velocity-to-tilt cascade to the roll and pitch sticks with the heading held at north by a yaw P (so roll is east and pitch is north), a 3 m waypoint capture radius and a 6 m/s speed cap. For Betaflight it adds the 4 s still boot, arms with aux 1 and selects angle mode with aux 2 (`hover.py`'s `ARM_AND_ANGLE`); the stabilizer ignores aux.
6. **The circuit.** `worlds/circuit.toml`: the pilot and receiver sections of `flat.toml`, no objects, four gates 4 m wide and 3 m tall centred at d = -1.5 on a 40 m square loop flown clockwise from above: `Gate0` (20, 0) heading 90 (east, start/finish), `Gate1` (40, 20) heading 0, `Gate2` (60, 0) heading 270, `Gate3` (40, -20) heading 180. Waypoints for the pilot: take off to 1.5 m, (20, -20), (20, 20), (60, 20), (60, -20), (20, -20), (20, 20); the first pass of Gate0 starts the clock, the last completes lap 1.
7. **Tests assert the race, not the path.** The scripted-circuit tests pass when the events are `gate_passed` (start), three `gate_passed` splits for gates 1 to 3, then `lap_completed` for lap 1, in that order, `state.race.laps == 1` and no `collision` event, within 120 simulated seconds.
8. **Not in this milestone:** an IMU-fed or filtered stabilizer, altitude hold or position hold inside the stabilizer, rate/acro mode, arming logic, a stabilized Godot session (the game has no `stabilize` option), tuning realism against a real Betaflight tune (#51).

## Review Focus

- Every stick sign is checked in simulation, not assumed: roll `+` goes east (at heading north), pitch `+` goes north, yaw `+` increases heading (task 2).
- Full stick plus full throttle, and throttle at 0 on the ground, give finite, bounded motor commands and no NaN (task 2).
- With `stabilize` the same seed and sticks give bit-identical runs; without it nothing changes (tasks 2 and 3).
- `stabilize` without `open_loop_fc` is refused and leaves the previous session alone; a repro bundle of a stabilized session replays to the same digest (task 3).
- A pilot that never reaches a waypoint ends with a clear failure (timeout message naming the waypoint), not a hang (task 4).
- The circuit test fails when the lap does not complete (mutation: move Gate2 off the path and watch it fail) (task 4).

---

### Task 1: The control law and the mixer (pure, `ofs-fc`)

**Files:**
- Create: `crates/ofs-fc/src/stabilizer.rs`; Modify: `crates/ofs-fc/src/lib.rs` (`pub mod stabilizer;`)
- Test: unit tests inside `stabilizer.rs`

**Interfaces:**
- Produces (all `pub`): constants `MAX_ANGLE_DEG`, `MAX_YAW_RATE_DPS`, `MAX_RATE_DPS`, `IDLE_THROTTLE`, `KP_ANGLE`, `KP_RATE_ROLL_PITCH`, `KP_RATE_YAW` (Design calls 3 and 4); `pub struct Mixer { /* per motor: roll, pitch, yaw weights */ }` with `Mixer::new(positions_frd_m: &[[f64; 3]], spin: &[i8]) -> Mixer`, `fn weights(&self, motor: usize) -> (f64, f64, f64)` and `fn mix(&self, throttle: f64, roll: f64, pitch: f64, yaw: f64, out: &mut [f64])` (clamps each command to [0, 1]); `pub struct Sticks { roll, pitch, yaw, throttle: f64 }`; `pub struct Attitude { roll_rad, pitch_rad: f64 }`; `pub fn euler_zyx(q: DQuat) -> (f64, f64, f64)` (roll, pitch, yaw in radians); `pub fn control(sticks: &Sticks, attitude: Attitude, rate_frd_radps: DVec3) -> (f64, f64, f64)` returning the `(roll, pitch, yaw)` outputs before mixing.

- [ ] **Step 1: Write the failing tests:**
  - `the_x_mixer_has_betaflights_signs`: for the shipped frame (positions `[-0.08, 0.08, 0]`, `[0.08, 0.08, 0]`, `[-0.08, -0.08, 0]`, `[0.08, -0.08, 0]`, spin `[1, -1, -1, 1]`): motor 0 (rear right, CW) weights `(-1.0, -1.0, -1.0)`, motor 1 (front right, CCW) `(-1.0, 1.0, 1.0)`, motor 2 `(1.0, -1.0, 1.0)`, motor 3 `(1.0, 1.0, -1.0)` (roll weight `-y/max|y|`, pitch `x/max|x|`, yaw `-spin`).
  - `mixing_clamps_every_command_into_zero_to_one`: throttle 0.95 with roll 0.5 saturates at 1.0; throttle 0.05 with roll -0.5 floors at 0.0; a mix that stays inside is exact.
  - `a_level_still_quad_with_centred_sticks_asks_for_nothing`: `control` with zero sticks, zero attitude, zero rate returns `(0, 0, 0)`.
  - `sticks_set_angle_and_yaw_rate_targets`: roll stick 1.0 at zero attitude and zero rate returns a roll output of `KP_RATE_ROLL_PITCH * KP_ANGLE * MAX_ANGLE_DEG.to_radians()` (within the `MAX_RATE_DPS` clamp: assert the clamp too — a target that exceeds 400 deg/s is held at 400); a pitch stick `+0.5` asks for a **negative** pitch angle (nose down), so the pitch output is negative; a yaw stick `+0.5` at zero yaw rate returns `KP_RATE_YAW * 0.5 * MAX_YAW_RATE_DPS.to_radians()`.
  - `the_loop_pushes_back_against_error`: attitude rolled +10 degrees and stick 0 gives a negative roll output; a positive roll rate with stick 0 gives a negative roll output.
  - `euler_angles_round_trip_a_known_attitude`: build `DQuat::from_euler`/axis-angle for roll 0.3, pitch -0.2, yaw 1.0 (Z-Y-X, FRD body to NED) and read it back within 1e-12.
- [ ] **Step 2: Run** `cargo test -p ofs-fc --lib stabilizer` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the module per Design calls 3 and 4. `control` does not read throttle (the model handles the idle floor).
- [ ] **Step 4: Run** the same command — Expected: PASS; then `cargo clippy -p ofs-fc --all-targets --locked -- -D warnings`.
- [ ] **Step 5: Commit** `feat(fc): the stabilizer's control law and mixer (#66)`.

### Task 2: The `Stabilizer` model and the vehicle (tuning happens here)

**Files:**
- Modify: `crates/ofs-fc/src/stabilizer.rs` (the model), `crates/ofs-config/src/lib.rs` (`FcKind::OpenLoopStabilized`; the one `FcKind::Sitl` comparison is unaffected), `crates/ofs-sim/src/vehicle.rs` (`build`: a `FcKind::OpenLoopStabilized` arm next to `FcKind::OpenLoop`, with the VTX handling the open-loop arm has), `crates/ofs-sim/src/server.rs`/`repro.rs` only where a `match` on `FcKind` stops compiling
- Create: `crates/ofs-sim/tests/stabilizer.rs`
- Test: `crates/ofs-sim/tests/stabilizer.rs` (a vehicle over `quads/opendrone-5f-freestyle.toml` and the open field with `fc_override: Some(FcKind::OpenLoopStabilized)`), `crates/ofs-config/tests/config.rs` (the new kind parses)

**Interfaces:**
- Consumes: Task 1's `Mixer`, `control`, `euler_zyx`, constants.
- Produces: `pub struct Stabilizer` (`impl Model`, name `"fc.stabilizer"`, `rate_divisor` as the open-loop FC's) built by `Stabilizer::new(positions_frd_m: &[[f64; 3]], spin: &[i8], rate_divisor: u32, bus: &mut Bus) -> Stabilizer`: reads `rc.roll/pitch/yaw/throttle`, `body.att_q`, `body.rate_frd_radps`; writes `fc.motor.N.cmd`; below `IDLE_THROTTLE` every command is 0. `FcKind::OpenLoopStabilized`.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-sim/tests/stabilizer.rs` (helper: a vehicle with the stabilizer; `fly(v, sticks, seconds)`; a throttle that roughly hovers is found by a small helper that bisects `vz` over 2 s of flight, or fixed at the value the executor measures and ledgers):
  - `level_sticks_hold_the_attitude`: hover throttle, zero sticks, 5 s: roll and pitch stay within 1 degree and yaw drifts less than 2 degrees.
  - `a_roll_stick_leans_the_quad_to_the_commanded_angle_and_stays`: roll 0.3: within 0.5 s the roll is within 10 % of `0.3 * MAX_ANGLE_DEG` (18 degrees), with overshoot under 25 %, and over the next 2 s it stays within 2 degrees of it.
  - `the_stick_signs_move_the_quad_the_way_a_pilot_expects`: at heading north, roll +0.2 for 2 s gives positive east velocity and no north drift beyond 20 % of it; pitch +0.2 gives positive **north** velocity (and a negative pitch angle); yaw +0.3 for 1 s increases the yaw angle at about `0.3 * MAX_YAW_RATE_DPS` (within 15 %).
  - `a_quad_on_the_ground_with_no_throttle_stays_dark`: throttle 0, full stick: every motor command is 0 and the quad has not moved after 1 s.
  - `full_sticks_and_full_throttle_stay_finite_and_bounded`: all sticks at 1.0, throttle 1.0 for 2 s: no error, all commands in [0, 1], position finite.
  - `the_same_seed_and_sticks_give_the_same_run`: two vehicles, a scripted stick sequence, identical `state()` and `digest()`.
  - in `ofs-config/tests/config.rs`: a quad file with `[fc] kind = "open_loop_stabilized"` loads.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test stabilizer` — Expected: FAIL (variant missing, then behaviour).
- [ ] **Step 3: Implement** the model and the vehicle arm. **Tune** `KP_ANGLE`, `KP_RATE_ROLL_PITCH` and `KP_RATE_YAW` until the tests pass with margin (a quick throwaway sweep test or a scratch Python loop is fine; delete it). If the yaw sign test fails, flip `my_i` and say so in the ledger, then fix Task 1's expected weights to match physics (the physics is the truth, the derivation in Design call 4 is the hypothesis).
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and clippy — Expected: PASS, goldens untouched.
- [ ] **Step 5: Commit** `feat(sim): an opt-in open-loop stabilizer (#66)`; ledger the final gains.

### Task 3: Selecting it: protocol 12, the server, repro bundles and the clients

**Files:**
- Modify: `proto/ofs/v1/sim.proto` (header comment "protocol version 12", `LoadRequest.stabilize = 8`), `crates/ofs-proto/src/lib.rs` and `tests/messages.rs` (version 12), `python/ofs/client.py` (`PROTOCOL_VERSION = 12`, `load(..., stabilize=False)`), regenerated `python/ofs/v1/*`, `crates/ofs-sim/src/server.rs` (the `Load` handler), `crates/ofs-sim/src/repro.rs` (`stabilize` in `BundleSource` and `bundle.toml`, replay), `crates/ofs-client/src/{model,worker}.rs` (`LoadSettings.stabilize`, default false), `docs/repro-bundle.md`
- Test: `crates/ofs-sim/tests/grpc.rs`, `crates/ofs-sim/tests/repro.rs`, `python/tests/test_client.py` or a new `python/tests/test_stabilizer.py`

**Interfaces:**
- Consumes: Task 2's `FcKind::OpenLoopStabilized`.
- Produces: `LoadRequest.stabilize`; `sim.load(path, open_loop_fc=True, stabilize=True)`; `BundleSource.stabilize: bool`.

- [ ] **Step 1: Write the failing tests:** gRPC `a_stabilize_load_without_open_loop_fc_is_refused_and_keeps_the_old_session` (kind `invalid_argument`, the old session still answers `GetState`); gRPC `a_stabilized_session_levels_itself` (load `open_loop_fc + stabilize`, throttle hover value, `Run(3 s)`: attitude within 1 degree of level, where the throttle-only session would have tumbled or drifted); repro `a_stabilized_session_replays_to_the_same_digest` (write a bundle for a stabilized session, replay, equal digests; an old bundle without the key replays as before); protocol test expects 12; Python `test_a_stabilized_open_loop_session_holds_level_and_obeys_a_roll_stick` through `sim.load(QUAD, open_loop_fc=True, stabilize=True)`, and `stabilize=True` without `open_loop_fc` raises `ofs.InvalidArgument`.
- [ ] **Step 2: Run** the new tests — Expected: FAIL.
- [ ] **Step 3: Implement.** `opts.fc_override` is `Some(OpenLoopStabilized)` when both flags are set, `Some(OpenLoop)` with only `open_loop_fc`, and `stabilize` alone is refused before anything is built. The repro bundle's existing `open_loop_fc` flag stays; add `stabilize` beside it and rebuild with the same override on replay. Regenerate the Python stubs; update every live "protocol 11" reference outside `docs/superpowers` and the CHANGELOG history (`grep -rn "protocol 11"`).
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `PYTHONPATH=python python -m pytest python/tests -q`, clippy — Expected: PASS.
- [ ] **Step 5: Commit** `feat(proto)!: protocol 12 - stabilize on Load (#66)`.

### Task 4: The circuit world, the pilot and the open-loop test

**Files:**
- Create: `worlds/circuit.toml`, `python/examples/circuit.py`, `python/tests/test_circuit.py`
- Modify: `crates/ofs-config/tests/world.rs` (the shipped circuit world loads)

**Interfaces:**
- Consumes: Task 3's `stabilize`, M8a's `World.gates`, `State.race` and the `gate_passed`/`lap_completed` events.
- Produces: `circuit.fly_circuit(sim, world=None, betaflight=False, max_seconds=120.0) -> dict` returning `{"events": [(kind, message), ...], "laps": int, "seconds": float, "collisions": int, "states": [...]}` and raising `RuntimeError` naming the waypoint when one is not reached within its time budget; `circuit.WAYPOINTS` (Design call 6); a `__main__` that flies the stabilizer (or Betaflight with `--betaflight`) and prints the summary.

- [ ] **Step 1: Write the failing tests:** `crates/ofs-config/tests/world.rs::the_circuit_world_loads` (four gates named `Gate0`..`Gate3` with headings 90, 0, 270, 180 and the centres of Design call 6); `python/tests/test_circuit.py::test_the_stabilized_quad_flies_the_circuit` (load `quads/opendrone-5f-freestyle.toml` with `world=circuit`, `open_loop_fc=True, stabilize=True`, lockstep; `fly_circuit`; assert Design call 7: the event kinds in order `gate_passed` x4 then `lap_completed`, messages `Gate0 (gate 0) start`, `Gate1 (gate 1) split ...`, ..., `lap 1: ... s (best)`, `race.laps == 1`, zero collisions, `seconds < 120`); `test_a_missed_gate_fails_the_test` (run the pilot with `WAYPOINTS` replaced by a path that stays 10 m clear of Gate2 and assert the result has no lap: `laps == 0`, so the first test's assertions would fail); `test_an_unreachable_waypoint_raises_naming_it` (a waypoint 10 km away with `max_seconds=5` raises `RuntimeError` mentioning the waypoint); `test_the_circuit_is_deterministic` (two runs, equal event lists and equal final state).
- [ ] **Step 2: Run** them — Expected: FAIL (no world, no module).
- [ ] **Step 3: Implement** the world file (comments in the style of `flat.toml`; a note that it is the M8b circuit) and the pilot (Design call 5). Tune the pilot's gains with the stabilizer (speed cap 6 m/s, tilt cap 25 degrees of stick equivalent); the pilot must not read the gates' events to steer (it flies waypoints), only to report.
- [ ] **Step 4: Run** `PYTHONPATH=python python -m pytest python/tests/test_circuit.py -q` and the whole Python suite — Expected: PASS; also run `python python/examples/circuit.py` once by hand and note the lap time in the ledger.
- [ ] **Step 5: Commit** `feat: a closed circuit and a scripted pilot that flies it (#53)`.

### Task 5: The same circuit against real Betaflight (live, gated)

**Files:**
- Create: `python/tests/test_sitl_circuit.py`
- Modify: `.github/workflows/ci.yml` (add the new file to the `sitl` job's explicit pytest list on the line that names `test_sitl_hover.py` and the others), `python/examples/circuit.py` (Betaflight branch: boot, arm, angle mode; gains if the response differs)

**Interfaces:**
- Consumes: Task 4's `fly_circuit(..., betaflight=True)`.

- [ ] **Step 1: Write the failing test** `test_betaflight_flies_the_circuit` (skipped without `OFS_SITL_LAUNCH`, like `test_sitl_hover.py`): load the shipped quad with `world=circuit`, `seed=1`, `fly_circuit(sim, betaflight=True)`, assert Design call 7. It follows `test_sitl_hover.py` for the warm-up load that applies the diff on a fresh data dir.
- [ ] **Step 2: Run** it in WSL (script file) — Expected: FAIL (Betaflight branch missing / the pilot's gains do not suit Betaflight's response).
- [ ] **Step 3: Implement** the Betaflight branch; tune the pilot for Betaflight's angle mode without breaking Task 4's test (gains may differ per backend: put them in two constants dicts keyed by backend). Record the lap time and real-time factor in the ledger. If Betaflight's armed idle or throttle curve needs a different hover guess, keep it as a constant with a comment.
- [ ] **Step 4: Run** the live test three times in a row (not flaky), then the other Python SITL files that the CI job lists — Expected: PASS.
- [ ] **Step 5: Commit** `test: the scripted circuit against real Betaflight (#53)`.

### Task 6: Docs, changelog and roadmap

**Files:**
- Create: `docs/scripted-flight.md`
- Modify: `CHANGELOG.md` ("Unreleased — M8b: scripted flight"), `docs/roadmap.md` (the Python limitation "Open-loop sessions have no stabilizer…" becomes a statement of what the stabilizer is not), `README.md`, `python/README.md`, `docs/race-timing.md` (its last paragraph says the stabilizer is future), `docs/repro-bundle.md` (done in task 3; check), `docs/dev-setup.md` (protocol 12)

- [ ] **Step 1: Write `docs/scripted-flight.md`:** what the stabilizer is and is not (Design call 2 in plain words, with the sentence that real tuning needs SITL), the stick semantics (call 3), the final gains and where they came from (call 4), how to use it from Python (`load(..., open_loop_fc=True, stabilize=True)`), the circuit and the pilot (calls 5 to 7) with a short snippet, the two ways to run the live test.
- [ ] **Step 2: Update** the changelog (Added: stabilizer, circuit world and pilot, live test; Changed: protocol 12, breaking for protocol-11 clients), the roadmap, READMEs and `race-timing.md`.
- [ ] **Step 3: Check** links and snippets by running the snippet's logic against the shipped files; `grep -rn "protocol 11" docs README.md python/README.md` outside `docs/superpowers`.
- [ ] **Step 4: Run** the whole gate: Windows `cargo test --workspace --locked --no-fail-fast`, clippy, `cargo +1.85 check`, `PYTHONPATH=python python -m pytest python/tests -q`, and the WSL workspace + live run with `OFS_SITL_LAUNCH` set (goldens unmoved) — Expected: PASS.
- [ ] **Step 5: Commit** `docs: scripted flight, M8b changelog and roadmap (#66, #53)`.

## Ledger

Recorded 2026-10-18, from the PR #85 review's deferred-minors list (deviations the ledger had not received):

- `fly_circuit` returns `final` (the last State) rather than `states` as the plan's Design call sketched.
- The gRPC stabilizer test shipped as `a_stabilized_session_obeys_a_roll_stick_and_a_plain_open_loop_one_ignores_it`,
  not the plan's working name `a_stabilized_session_levels_itself`.
- The Python stabilize test checks the roll response; level holding is checked by the Rust test.
