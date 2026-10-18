# M8a: Gates and Lap Timing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A world file can declare an ordered list of gates; the simulator detects the quad flying through them, times splits and laps, and publishes the result as events and state, so the Godot HUD shows a lap timer and scripts (and M8b's scripted circuit, #53) can assert a course was flown. Closes #71.

**Architecture:** A `[[gates]]` section in the world file (`ofs-config`). A pure race state machine plus geometry in `ofs-sim/src/race.rs`, wrapped by a `GateTimer` scheduler model that reads `body.pos_ned_m` after the physics step, publishes `race.*` bus signals and queues events. The session turns queued events into two new event kinds; `State` carries a `RaceState`; `World` carries the gates (protocol 11). The Rust client, the Godot bridge and the Python client expose them; Godot draws the gate openings and a HUD line. Worlds without gates add no model and no signals, so no existing recording or golden changes.

**Tech Stack:** Rust workspace (MSRV 1.85), tonic/prost proto, Python `ofs` (generated stubs via grpc_tools), Godot 4.7.2 GDScript + the gdext bridge.

**Spec:** none beyond GitHub issue #71 (`gh issue view 71 --repo hugowis/open-fpv-sim`) and the "Design calls" below, which are the binding rules; `docs/roadmap.md` lists what the world model does not do.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- No golden digest changes in this milestone (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`): worlds without gates must be byte-identical. A moved golden is a bug.
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- Regenerate Python stubs after the proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`; the three generated files change and nothing else under `python/ofs/v1`.
- Python tests need `PYTHONPATH=<worktree>/python`. Godot tests: `bash scripts/run-godot-tests.sh unit` in WSL (it downloads Godot 4.7.2); CI's `godot` job runs the same.
- Commit trailers: a blank line, then `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` on its own line.

## Design calls (decided here; the executor ledgers any change)

1. **World schema.** `[[gates]]`, ordered, each `{ name, center_ned_m, heading_deg, width_m, height_m }`. `heading_deg` is the direction of travel through the gate (0 = north, 90 = east, clockwise, like `pilot.facing_deg`); `width_m` and `height_m` are the opening, centred on `center_ned_m`; the gate stands vertical. Names follow `check_name` (own namespace), at most `MAX_GATES = 64`, every number finite, `width_m` and `height_m` > 0, and the opening's bottom edge not below the ground (`center d + height/2 <= 0`). A gate is a **trigger**, not an obstacle: it has no collision and no RF loss (a frame made of posts stays a set of `[[objects]]`, as in `flat.toml`).
2. **Pass rule.** Each tick the model tests the segment from the previous pose to the new pose against the *next expected* gate only. It counts when the signed distance along the heading goes from `< 0` to `>= 0` and the crossing point lies inside the opening (`|lateral| <= width/2`, `|vertical offset| <= height/2`) — the quad's centre, no margin for its size. Wrong gate, wrong direction, outside the opening: ignored, with no effect on state. The time is interpolated inside the step (`t = step start + fraction * dt`), so it does not depend on the tick rate.
3. **Race rules.** The clock starts on the first valid crossing of gate 0 (a `gate_passed` event, split 0). Gates 1..n-1 in order each give a `gate_passed` with the time since the lap start. Crossing gate 0 again after the last gate completes a lap (`lap_completed`) and starts the next one; no `gate_passed` is raised for that crossing. With one gate, every valid crossing after the first completes a lap, but only after the quad has been farther than `REARM_M = 3.0` m from the gate's plane (along the direction of travel) since the last count (so hovering through the plane does not run up laps). There is no lap limit and no reset except a new `Load`.
4. **Wire.** Protocol 11. `Gate { name, center_ned_m, heading_deg, width_m, height_m }` and `World.gates = 8`. `RaceState { present, running, next_gate, gate_count, laps, current_lap_s, last_lap_s, best_lap_s }` and `State.race = 20`; `last_lap_s` and `best_lap_s` are 0 until a lap completes (`laps > 0` says they are real). `EventKind` `GATE_PASSED = 22`, `LAP_COMPLETED = 23`. Messages (a documented format scripts may parse): `"<name> (gate <i>) start"`, `"<name> (gate <i>) split <s> s"`, `"lap <n>: <s> s"` with `" (best)"` appended when it is the fastest lap so far (the first lap is). Seconds have three decimals. Events carry the crossing time, not the tick-end time.
5. **Bus.** Only when the world has gates: `race.running`, `race.next_gate`, `race.laps`, `race.lap_start_s`, `race.last_lap_s`, `race.best_lap_s` (scalar f64, default recording rate 50 Hz from the `*` row). `State.race` is built from them; only events travel through a shared queue.
6. **Not in this milestone:** lap limits, a reset call, ghost/best-lap replay, gate highlighting of the next gate in Godot, gates with a tilt or roll, a margin for the quad's size.

## Review Focus

- A quad that grazes just outside the opening, or crosses at the exact edge, is not counted (task 2).
- Flying through a gate backwards, or through gate 2 before gate 1, changes nothing and does not poison later laps (task 2).
- A very fast quad whose consecutive samples straddle the plane by metres is still counted once, at an interpolated time (task 2).
- Hovering and jittering through the only gate does not run up laps (task 2).
- A world with no gates adds no `race.*` signal, no model and no `State.race` content beyond `present = false` (task 3).
- Loading a second world (or the same one again) starts a fresh race: no state leaks across sessions (task 3).
- A client that predates protocol 11 is refused by the handshake rather than misreading the new fields (task 4).

---

### Task 1: The `[[gates]]` world section (ofs-config)

**Files:**
- Modify: `crates/ofs-config/src/world.rs`, `crates/ofs-config/src/lib.rs` (re-exports, if worlds types are re-exported there), `worlds/flat.toml`
- Test: `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Produces: `pub const MAX_GATES: usize = 64;` `pub struct GateSection { pub name: String, pub center_ned_m: [f64; 3], pub heading_deg: f64, pub width_m: f64, pub height_m: f64 }` (`Deserialize`, `deny_unknown_fields`, `Debug, Clone, PartialEq`); `WorldConfig.gates: Vec<GateSection>` (`#[serde(default)]`, empty in `open_field()`).

- [ ] **Step 1: Write the failing tests** in `crates/ofs-config/tests/world.rs`:
  - extend `the_shipped_world_loads`: `w.gates.len() == 3`; gate 0 is `{ name "Gate0", center [30.0, 0.0, -1.22], heading_deg 0.0, width_m 3.08, height_m 2.44 }`; gates 1 and 2 are at north 60 and 90 with the same size.
  - `the_open_field_has_no_gates`: `WorldConfig::open_field().gates.is_empty()`.
  - `every_gate_problem_is_reported_at_once`: build a text from `world_text()` with one extra `[[gates]]` block appended per problem — a zero `width_m`, a `height_m = nan`, a `heading_deg = inf`, a centre `[10.0, 0.0, -0.5]` with `height_m = 2.0` (bottom edge below ground), a name repeated — and assert each message starts with the expected `gates[i].<field>: ...` text (the style of the neighbouring `every_world_problem_is_reported_at_once`); plus `gates: at most 64 gates` for 65 blocks.
  - `a_world_file_with_a_gate_missing_a_field_is_refused` (TOML parse error names the missing field).
- [ ] **Step 2: Run** `cargo test -p ofs-config --test world gate` — Expected: FAIL (no `gates` field / unknown field).
- [ ] **Step 3: Implement** `GateSection`, the `gates` field and the validation rules of Design call 1 inside `WorldConfig::validate` (field paths `gates[i].name` etc.; the count rule reports under `gates`). Add the three gates to `worlds/flat.toml` after the existing gate objects, with a comment that the post/bar objects are the frame and the `[[gates]]` the opening (inner width 3.08 m between the posts, 2.44 m under the bar), centres at north 30, 60, 90, `d = -1.22`, `heading_deg = 0.0`. Fix `flat.toml`'s header comment if it still says nothing collides with the drone (it is stale: objects collide since M3c) and mention the gates.
- [ ] **Step 4: Run** `cargo test -p ofs-config --locked` and then `cargo test --workspace --locked --no-fail-fast` — Expected: ofs-config PASS; if another test counts or enumerates what `flat.toml` holds, note it for later tasks but do not fix it here (nothing consumes `gates` yet, so none should fail).
- [ ] **Step 5: Commit** `feat(config): [[gates]] in the world file (#71)`.

### Task 2: Gate geometry and the race state machine (pure)

**Files:**
- Create: `crates/ofs-sim/src/race.rs`; Modify: `crates/ofs-sim/src/lib.rs` (`pub mod race;`)
- Test: unit tests inside `race.rs`

**Interfaces:**
- Consumes: `ofs_config::world::GateSection`.
- Produces (all `pub`):
  - `pub const REARM_M: f64 = 3.0;`
  - `pub struct Gate { pub name: String, … }` with `Gate::from_config(&GateSection) -> Gate` and `Gate::crossing(&self, p0: DVec3, p1: DVec3) -> Option<f64>` — the fraction in `[0, 1)` of the step at which the segment crosses the opening forward, else `None`.
  - `pub enum RaceEvent { GatePassed { gate: usize, time_s: f64, split_s: f64 }, LapCompleted { lap: u32, time_s: f64, lap_s: f64, best: bool } }` (`Debug, Clone, Copy, PartialEq`).
  - `pub struct RaceStatus { pub running: bool, pub next_gate: usize, pub laps: u32, pub lap_start_s: f64, pub last_lap_s: f64, pub best_lap_s: f64 }` (`Default`; `last_lap_s`/`best_lap_s` are 0 before the first lap).
  - `pub struct RaceTracker` with `RaceTracker::new(gates: Vec<Gate>) -> Self`, `fn step(&mut self, t0_s: f64, dt_s: f64, p1: DVec3) -> Option<RaceEvent>` (`p0` is the previous call's `p1`; the first call only records the position), `fn status(&self) -> RaceStatus`, `fn gate_count(&self) -> usize`, `fn gate_name(&self, i: usize) -> &str`.

- [ ] **Step 1: Write the failing tests** (names are the contract; use a helper that builds a `GateSection` and a tracker over a straight three-gate course along north at 30/60/90 m, opening 3 m × 2 m centred at `d = -1.5`):
  - geometry: `a_step_through_the_opening_crosses_at_the_right_fraction` (fraction 0.25 for a step from 1 m before to 3 m after); `a_step_beside_above_or_below_the_opening_does_not_cross`; `a_step_exactly_at_the_opening_edge_counts_and_one_just_outside_does_not`; `a_step_against_the_heading_does_not_cross`; `a_step_ending_on_the_plane_crosses_and_the_next_step_does_not_cross_again` (s0 < 0 ≤ s1 rule); `a_gate_headed_east_is_crossed_eastward_only` (heading 90).
  - tracker: `the_clock_starts_at_gate_zero_and_gates_are_counted_in_order` (events: start, split, split; `status()` after each); `a_gate_out_of_order_is_ignored` (fly gate 2 before 1: no event, `next_gate` unchanged, then the proper order still works); `a_lap_completes_at_gate_zero_after_the_last_gate` (`laps == 1`, `lap_s` = time between the two gate-0 crossings, `best` true, `last_lap_s == best_lap_s`); `only_a_faster_lap_is_best` (second lap slower → `best == false`, `best_lap_s` unchanged; third faster → true); `crossing_times_are_interpolated_inside_the_step` (step of 1 s crossing at its 0.3: `time_s` = t0 + 0.3); `a_reverse_crossing_after_the_start_changes_nothing`; `a_very_fast_step_over_the_plane_counts_once` (a 40 m step); `a_single_gate_course_counts_a_lap_only_after_leaving_and_coming_back` (jitter ±0.1 m through the plane after the start: no lap; go 5 m out, come back and cross: lap 1); `a_course_with_no_gates_never_raises_anything`.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib race` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** per Design calls 2 and 3. Geometry in NED: forward `f = (cos h, sin h, 0)`, lateral `l = (-sin h, cos h, 0)`, `s(p) = (p - c)·f`; a crossing needs `s0 < 0 && s1 >= 0`; the point `q = p0 + frac*(p1 - p0)` is inside when `|(q-c)·l| <= w/2` and `|q.z - c.z| <= h/2`. The tracker tests only `gates[next_gate]`; the `REARM_M` rule applies only to the gate that was last counted being the next one again (one-gate courses). `best` is true when the lap is the first or strictly faster than `best_lap_s`.
- [ ] **Step 4: Run** `cargo test -p ofs-sim --lib race` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(sim): gate geometry and the race state machine (#71)`.

### Task 3: The `GateTimer` model, bus signals and the vehicle (ofs-sim)

**Files:**
- Modify: `crates/ofs-core/src/names.rs` (the six `RACE_*` constants), `crates/ofs-sim/src/race.rs` (the model and its handle), `crates/ofs-sim/src/vehicle.rs` (`build`, `Vehicle`, `VehicleState`, `Handles`)
- Create: `crates/ofs-sim/tests/gates.rs`
- Test: `crates/ofs-sim/tests/gates.rs`, `crates/ofs-sim/tests/open_loop_vehicle.rs`

**Interfaces:**
- Consumes: Task 2's `RaceTracker`, `RaceEvent`, `RaceStatus`.
- Produces: `names::{RACE_RUNNING, RACE_NEXT_GATE, RACE_LAPS, RACE_LAP_START_S, RACE_LAST_LAP_S, RACE_BEST_LAP_S}` (`"race.running"`, …); `pub struct GateTimer` (`impl Model`, name `"race.gates"`, `rate_divisor` 1) built by `GateTimer::new(gates: &[GateSection], bus: &mut Bus) -> (GateTimer, RaceHandle)`; `#[derive(Clone, Default)] pub struct RaceHandle` with `fn take_events(&self) -> Vec<RaceEvent>` (the model pushes under a mutex only when an event happens); `Vehicle::take_race_events(&mut self) -> Vec<RaceEvent>`; `Vehicle::gate_names(&self) -> Vec<String>` or equivalent for the session's messages; `VehicleState.race: RaceInfo` with `RaceInfo { present: bool, running: bool, next_gate: u32, gate_count: u32, laps: u32, current_lap_s: f64, last_lap_s: f64, best_lap_s: f64 }` (`Default`, all zero/false when the world has no gates); `current_lap_s = time_s - lap_start_s` while running, else 0.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-sim/tests/gates.rs` (a bare `Scheduler` at 8000 Hz with a test `Mover` model that writes `body.pos_ned_m` from a closure of `time_s`, registered *before* the `GateTimer`, the way `crates/ofs-radio/tests/band.rs` drives a rig):
  - `a_course_flown_in_order_raises_start_splits_and_a_lap` — events and `race.*` signal values at the end (`race.laps == 1`, `race.last_lap_s` equals the lap event's `lap_s`).
  - `a_quad_at_100_mps_is_counted_once_per_gate` (at 8 kHz the step is 12.5 mm; also repeat at 100 Hz base to show the interpolation holds).
  - `the_signals_exist_only_when_the_world_has_gates` (a `GateTimer` is never built for an empty list — assert through `Vehicle` in the next test).
  In `open_loop_vehicle.rs`:
  - `a_world_with_gates_has_a_race_that_has_not_started` (flat world: `state().race` is `present`, `gate_count == 3`, not running, `laps == 0`, and `signal("race.laps") == Some(0.0)`);
  - `a_world_without_gates_has_no_race_and_no_race_signals` (`state().race == RaceInfo::default()`, `signal("race.laps") == None`);
  - `two_vehicles_from_one_world_share_no_race_state` (build two over the flat world; both report the initial `RaceInfo` and an empty `take_race_events()`; nothing in the model or handle is a `static`).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test gates --test open_loop_vehicle race gates` — Expected: FAIL (compile errors, then assertions).
- [ ] **Step 3: Implement** the model (reads `body.pos_ned_m`; each tick computes `t0 = ctx.time_s`, `dt = ctx.dt_s`; on `Some(event)` pushes to the handle and rewrites the `race.*` signals from `status()`; when nothing happens it writes nothing) and wire it in `build()` right after the `VideoLink` push and before the recorder: `if !opts.world.gates.is_empty() { … }`. `Handles` gains `race: Option<…>` looked up with `bus.signal` after the timer created them; `Vehicle::state()` fills `RaceInfo`. Use the pose the physics wrote *this tick* as `p1` and the timer's own remembered position as `p0`.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` — Expected: PASS. If a recording or signal-listing test that loads `flat.toml` now sees six extra signals, update that expectation (the six names are part of the contract) and say so in the commit message.
- [ ] **Step 5: Commit** `feat(sim): the GateTimer model, race signals and State.race (#71)`.

### Task 4: Protocol 11, session events and the server messages

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs` (version 11, doc comment), `crates/ofs-proto/tests/messages.rs` (version 11), `python/ofs/client.py` (`PROTOCOL_VERSION = 11`), `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` (regenerated), `crates/ofs-sim/src/session.rs` (`changes`, `world_msg`, `state_msg`), `crates/ofs-sim/src/server.rs` only if it constructs `pb::World`/`pb::State` literally
- Test: `crates/ofs-sim/src/session.rs` unit tests (next to `a_new_collision_count_raises_one_collision_event_naming_the_object`), `crates/ofs-sim/tests/grpc.rs`, `crates/ofs-proto/tests/messages.rs`

**Interfaces:**
- Consumes: Task 3's `Vehicle::take_race_events`, `VehicleState.race`, gate names.
- Produces: the wire shapes of Design call 4; `Session::changes` appends the race events after the existing ones.

- [ ] **Step 1: Write the failing tests:**
  - `race_events_become_gate_passed_and_lap_completed_events_with_the_documented_text` (session unit test; feed events through a helper that formats a `RaceEvent` — make the formatter a `pub(crate) fn race_event(e: &RaceEvent, gate_names: &[String]) -> pb::Event` so it is testable without a vehicle): start → `"Gate0 (gate 0) start"`, split → `"Gate1 (gate 1) split 8.412 s"`, lap → `"lap 1: 31.204 s (best)"` and `"lap 2: 33.000 s"`; `Event.time_s` is the crossing time.
  - `world_msg_carries_the_gates` and `state_msg_carries_the_race` (hand-built `WorldConfig`/`VehicleState`).
  - in `grpc.rs`: `get_world_returns_the_flat_fields_gates` (3 gates, first named `Gate0`, heading 0, width 3.08) and `a_state_has_a_race_with_three_gates_for_the_flat_world`.
  - in `messages.rs`: bump the expected version to 11 (this is the failing test).
- [ ] **Step 2: Run** `cargo test -p ofs-proto -p ofs-sim --locked gate race protocol` — Expected: FAIL.
- [ ] **Step 3: Implement** the proto additions (comment the header "protocol version 11: protocol 10 plus gates, lap timing and the race state (M8a)"), bump the three version places, regenerate the Python stubs with the Global Constraints command, and fill the messages. Search `grep -rn "protocol 10\|protocol_version\|Protocol 10" --include=*.rs --include=*.md --include=*.py --include=*.gd .` outside `docs/superpowers` and update live references.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS (the Python SITL tests skip without `OFS_SITL_LAUNCH`).
- [ ] **Step 5: Commit** `feat(proto)!: protocol 11 - gates, lap events and the race state (#71)`.

### Task 5: Clients — Rust, the Godot bridge and Python

**Files:**
- Modify: `crates/ofs-client/src/model.rs` (`WorldGate`, `World.gates`, `RaceInfo` in `Telemetry`, `EventKind::{GatePassed, LapCompleted}` and their `as_str` and `from_pb` arms), `crates/ofs-godot/src/lib.rs` (`get_world` and `get_telemetry` docs and keys), `python/ofs/client.py` (`Gate`, `Race`, `World.gates`, `State.race`, `_world`, `_state`), `python/README.md`
- Test: `crates/ofs-client` unit/integration tests (see the existing `World::from_pb` tests), `python/tests/test_scripting.py` or `test_client.py`

**Interfaces:**
- Consumes: Task 4's proto.
- Produces: `WorldGate { name: String, center: DVec3 /* Godot frame */, heading_deg: f64, width_m: f64, height_m: f64 }`; `Telemetry.race: RaceInfo` (present, running, next_gate, gate_count, laps, current_lap_s, last_lap_s, best_lap_s); Godot dictionary keys `gates` (Array of `{name, center: Vector3, heading_deg, width, height}`) in `get_world()` and `race_present, race_running, race_next_gate, race_gate_count, race_laps, race_lap_s, race_last_lap_s, race_best_lap_s` in `get_telemetry()`; event kind strings `"gate_passed"`, `"lap_completed"`; Python `Gate(name, center_ned_m, heading_deg, width_m, height_m)`, `Race(present, running, next_gate, gate_count, laps, current_lap_s, last_lap_s, best_lap_s)` (all defaulted), `World.gates: tuple = ()`, `State.race: Race = field(default_factory=Race)`.

- [ ] **Step 1: Write the failing tests:** Rust — `a_gate_is_converted_to_godots_frame` (NED `(30, 0, -1.22)` → Godot `(0, 1.22, -30)`; width/height/heading unchanged), `the_race_state_reaches_the_telemetry`, `the_new_event_kinds_have_names`; Python — `the_world_lists_its_gates` and `a_state_carries_a_race` against an open-loop `ofs-sim` loaded with `world="flat"` (follow the existing `test_scripting.py` pattern for launching), and a unit test that `_event` names `GATE_PASSED` as `"gate_passed"`.
- [ ] **Step 2: Run** the new tests — Expected: FAIL.
- [ ] **Step 3: Implement.** Python dataclasses are frozen with defaults so older code that builds a `State` keeps working.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `PYTHONPATH=python python -m pytest python/tests -q`, `cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): gates, the race state and the new events in the Rust, Godot and Python clients (#71)`.

### Task 6: Godot — gate openings, the lap HUD line and toasts

**Files:**
- Modify: `godot/world/world.gd` (`build` draws `world.get("gates", [])`), `godot/ui/hud.gd` (a `_race` label under the video line, `race_text()`, included in `set_hud_visible`), `godot/scripts/app.gd` (`_on_event` arms), `godot/tests/test_scene.gd`/`test_hud.gd` (and `shots.gd` only if cheap)
- Test: `godot/tests/test_scene.gd`, `godot/tests/test_hud.gd`

**Interfaces:**
- Consumes: Task 5's dictionary keys and event kinds.
- Produces: scene nodes named `Gate_<name>` (a `Node3D` holding a double-sided translucent `QuadMesh` of width × height and a `Label3D` "START" for gate 0 and the gate number for the others); `hud.race_text()`.

- [ ] **Step 1: Write the failing tests:** `test_scene.gd` — building a world dictionary with two gates (one heading north, one east) creates nodes `Gate_A` and `Gate_B` (`built_names()`), the quad's world-space normal (`global_transform.basis.z`) points along travel (north = `-Z`, east = `+X`), and an empty dictionary clears them. `test_hud.gd` — `update_view` with telemetry `race_present = false` leaves `race_text()` empty; with `race_present`, not running: `"RACE  fly through gate 0 to start"`; running: `"LAP 2   GATE 3/5   0:12.4   LAST 31.2   BEST 29.9"` (the `LAST`/`BEST` parts only when `race_laps > 0`; time as `m:ss.s`); `app.gd`'s `_on_event("gate_passed", …)` and `("lap_completed", …)` add a toast with the event message.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the new assertions.
- [ ] **Step 3: Implement.** Rotation about Y for a gate with NED heading `h`: `rotation.y = PI - deg_to_rad(h)` puts the quad's local `+Z` along `(sin h, 0, -cos h)`. Material: unshaded, alpha 0.25, cull disabled; gate 0 yellow, the rest cyan. Place `_race` at `(1.0, 0.0, 14, 134)` with `right_aligned = true`, styled like its siblings.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` (WSL; the e2e tests start `ofs-sim`) — Expected: PASS. Eyeball `godot --path godot -s res://tests/shots.gd -- --out=<dir>` if a display is available; otherwise state that the visual check was not done.
- [ ] **Step 5: Commit** `feat(godot): draw gate openings and show the lap timer (#71)`.

### Task 7: Docs, changelog and roadmap

**Files:**
- Create: `docs/race-timing.md`
- Modify: `CHANGELOG.md` ("Unreleased — M8a: gates and lap timing"), `docs/roadmap.md` (remove "Race timing" from the backlog if listed; add the limits of Design call 6 under a new "Race timing" known-limitations bullet), `README.md` (the world-file and keys sections where gates belong), `python/README.md` (events and `State.race`), `docs/dev-setup.md` only if it names protocol 10

- [ ] **Step 1: Write `docs/race-timing.md`:** the schema with a worked example, the pass rule and race rules (Design calls 1 to 3) in plain words, the event message formats and `State.race` meaning, a short Python snippet that watches `gate_passed`/`lap_completed` and reads `sim.state().race`, and the limits (centre point, no tilt, no reset).
- [ ] **Step 2: Update** the changelog (Added: gates, events, state, protocol 11 breaking for clients of protocol 10; Changed: `flat.toml` gets three gates), the roadmap and the READMEs.
- [ ] **Step 3: Check** every `docs/` link and code snippet: run the snippet's logic against the flat world's numbers by reading, and `grep -rn "protocol 10" docs README.md python/README.md` outside `docs/superpowers`.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-config -p ofs-sim -p ofs-client -p ofs-proto -p ofs-godot`, and WSL `cargo test --workspace --locked --no-fail-fast -- --test-threads=1` (goldens must not move) — Expected: PASS.
- [ ] **Step 5: Commit** `docs: race timing, M8a changelog and roadmap (#71)`.

## Ledger

- Ruled 2026-10-18 (PR #84's deferred list): the Review Focus sentence saying the exact opening edge is not counted is
  wrong; the code and Design call 2 count it (`Gate::crossing` counts s1 >= 0, and `a_gate_headed_east_is_crossed_eastward_only`
  pins it).
