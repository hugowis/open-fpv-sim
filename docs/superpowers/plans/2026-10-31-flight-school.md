# Flight School Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Graded flying lessons in the Godot client — pre-flight, hover, a coordinated turn, a figure of eight, a first lap — scored client-side from the existing telemetry, pose and race events, with persisted progress and optional recorded-inputs demos. Closes #125.

**Architecture:** Lessons are data (`godot/scripts/lessons.gd`); one pure state machine runs them (`godot/scripts/lesson_runner.gd`, fed a per-frame snapshot dictionary, no engine calls — the `calibration.gd` pattern); a school screen over the main menu and an in-flight panel present them; `app.gd` wires it into the existing `_start_session(choices)` / `_show_menu()` / `sticks_override` paths. Progress is `user://flight_school.json`. Demos replay committed inputs CSVs through `sticks_override`.

**Tech Stack:** GDScript (Godot 4.7.2), one shipped quad TOML (`quads/school-5f.toml`), one hand-run Python example that records the demo CSVs, pytest over the committed CSVs. No Rust source change.

**Spec:** `docs/superpowers/specs/2026-10-31-flight-school-design.md` — its Design calls D1–D12 are the binding rules; this plan argues from them. Anchors verified at `main` = 87c5914 (2026-10-31); re-verify at execution, especially `godot/scripts/app.gd` (the B6 sweep merged as `b37aae5` after this base).

## Global Constraints

- No change to any crate's source, to `proto/`, or to `worlds/*.toml`; protocol stays 15. `cargo test --workspace --locked` must still pass because `crates/ofs-config/tests/quads.rs` enumerates `quads/*.toml` (line 18, `every_shipped_quad_loads_and_validates`).
- The settings chain is not forked: no new key in `godot/scripts/settings.gd` (`ENV_NAMES`, `FLAG_NAMES`, `defaults`), nothing new read or written in `user://ofs_client.cfg`. Lesson progress lives in `user://flight_school.json`.
- The e2e menu-less path keeps working: `godot/tests/e2e_betaflight.gd`, `e2e_errors.gd`, `e2e_open_loop.gd` and `shots.gd` set `OFS_MENU=off` (e2e_open_loop.gd line 84) and none of them may need a change to pass except the additions Task 7 makes to `e2e_open_loop.gd`.
- Godot tests: `bash scripts/run-godot-tests.sh unit` in WSL (it builds the Rust first; never with Windows cargo at the same time); the runner prints `0 failed` even on a SCRIPT ERROR, so grep the output for `SCRIPT ERROR`. Every new suite is registered in `godot/tests/run_tests.gd` in the task that creates it. The e2e needs `bash scripts/run-godot-tests.sh open-loop` (WSL).
- Python: `PYTHONPATH=python python -m pytest python/tests -q` (SITL tests skip without `OFS_SITL_LAUNCH`).
- Write GDScript with Write/Edit, never a Bash heredoc (worktree sessions refuse them). `cargo fmt` is not a gate; do not reformat existing files.
- Commit `type(scope): what (#125)`, one per task.
- Every lesson instruction that names a control says what it teaches in both controller worlds: the scored state is mode-agnostic, the wording is not (spec D6).

## Design calls

The spec's D1–D12 bind this plan. The four an executor is most likely to test against:

1. **Scoring surface (D1):** telemetry keys from `get_telemetry()`, position/attitude from `get_pose()` (Godot frame: x = east, y = up, z = south → `north_m = -origin.z`, `east_m = origin.x`; drone forward is −Z), events from `_on_event`. Nothing else exists for lessons; nothing else is added.
2. **Stage kinds (D2):** exactly `motors`, `hold`, `reach`, `turn`, `figure_eight`, `lap` — the runner rejects any other kind at `setup`.
3. **Failure (D3):** only `collision`, the time limit, or `altitude_m <= 0.05` during an airborne stage (`hold`, `turn`, `figure_eight`). Drift or a band bust resets the accumulator, never the lesson.
4. **Lesson starts are menu starts (D8):** `_start_session(choices)`; the choices are never written to the cfg and never touch `settings`.

## Review Focus

- A hover that drifts out of the radius must **lose its accumulated hold** (showing the drift), not keep it and not fail the lesson — Task 1's `test_a_hover_that_drifts_out_of_bounds_loses_its_hold` pins the issue's own example.
- A heading unwrap bug at ±180° would inflate a turn to ~350° in one frame — Task 2's `test_a_turn_accumulates_across_the_north_wrap` pins the shortest-delta rule.
- Two loops around **one** pylon must not pass the figure of eight — Task 2's `test_two_loops_of_one_pylon_do_not_pass_the_figure_of_eight`.
- A frame hitch (a 2 s gap between snapshots) must not credit 2 s of holding — Task 1's `test_a_frame_hitch_does_not_inflate_the_hold` (accumulation clamps `dt`).
- Lesson state must not leak: **R** (reload) restarts the current lesson from stage 0; starting another lesson, and Esc, clear the runner, pylons and panel — Task 7's e2e block drives pre-flight, then the figure eight, then Esc, in one process.
- A hand-edited or corrupt `flight_school.json` must read as "nothing passed" and not break the school — Task 4's `test_an_invalid_file_reads_as_no_progress`.
- A demo CSV without a `t_s` column must produce an error string and no demo, not a crash at replay — Task 8's `test_a_csv_without_t_s_is_refused`.

---

### Task 1: The lesson runner — `motors`, `hold`, `reach`

**Files:**
- Create: `godot/scripts/lesson_runner.gd`
- Create: `godot/tests/test_lesson_runner.gd`
- Modify: `godot/tests/run_tests.gd` (add `"res://tests/test_lesson_runner.gd"` to `SUITES`)

**Interfaces:**
- Produces: `LessonRunner extends RefCounted` with
  - `static func heading_deg(basis: Basis) -> float` — NED heading of a body basis, wrapped to [0, 360): `rad_to_deg(atan2(-basis.z.x, basis.z.z))` (identity → 0/north; a −90° yaw about Y → 90/east).
  - `static func north_east(origin: Vector3) -> Dictionary` — `{"north_m": -origin.z, "east_m": origin.x}`.
  - `func setup(lesson: Dictionary, sitl := false) -> void` — validates stages (kinds of this task plus the Task 2 names; unknown kind pushes an error and leaves state `failed` with the message as `reason`).
  - `func feed(snapshot: Dictionary) -> void` — one per frame.
  - `func status() -> Dictionary` — `{"state": "running"|"passed"|"failed", "stage_index": int, "stage_count": int, "text": String, "reason": String, "progress": Dictionary}`.
- The snapshot contract every feeder must meet (Task 7 builds it):
```gdscript
{
	"time_s": float, "altitude_m": float, "speed_mps": float, "motors_spinning": bool,
	"north_m": float, "east_m": float, "heading_deg": float,
	"race_present": bool, "race_running": bool, "race_laps": int, "race_next_gate": int,
	"race_gate_count": int,
	"events": Array  # of {"kind": String, "time_s": float}, since the previous feed
}
```
- Stage dictionaries (lesson data, Tasks 3): `{"kind": "motors", "spinning": true/false, "text": String, "text_sitl": String (optional)}; {"kind": "hold", "radius_m": float, "min_m": float, "max_m": float, "seconds": float, "text": ...}; {"kind": "reach", "north_m": float, "east_m": float, "radius_m": float, "min_m": float, "max_m": float, "text": ...}`. Progress shapes: motors `{"spinning": bool, "want": bool}`; hold `{"held_s": float, "target_s": float, "drift_m": float, "altitude_m": float}`; reach `{"distance_m": float, "radius_m": float}`.
- Semantics (spec D2/D3): `hold` anchors at the stage's first snapshot; accumulates `min(dt, 0.25)` per feed while `altitude_m` in `[min_m, max_m]` and horizontal distance to the anchor `<= radius_m`; resets to 0 on the first broken frame after a held one. `reach` passes inside the radius with altitude in band. `dt` is the `time_s` delta (first feed defines `t0` for the time limit). `collision` in `events` fails any stage; `altitude_m <= 0.05` fails only `hold`/`turn`/`figure_eight` stages; `time_s - t0 > time_limit_s` fails while running. `text_sitl` wins over `text` only when the feeder says so — the runner takes the choice at `setup(lesson, sitl := false)`.
- Consumes: nothing (pure GDScript; `Vector3`/`Basis` maths only).

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_lesson_runner.gd` (`extends "res://tests/testing.gd"`, `const Runner = preload("res://scripts/lesson_runner.gd")`, a `_snap(t, alt := 1.5, north := 0.0, east := 0.0, spinning := true, extra := {}) -> Dictionary` helper that fills the whole snapshot contract above, merged with `extra`):
  - `test_heading_deg_reads_the_basis` — `Runner.heading_deg(Basis.IDENTITY)` ≈ 0; `Runner.heading_deg(Basis(Vector3.UP, deg_to_rad(-90.0)))` ≈ 90 (east); `≈ 350` for a +10° yaw.
  - `test_north_east_maps_the_godot_frame` — origin `(2.0, 1.0, -30.0)` → north 30, east 2.
  - `test_a_pre_flight_lesson_passes_when_the_motors_spin_then_stop` — stages `[motors spinning=true, motors spinning=false]`; feeds with `spinning=false` keep stage 0 running, `spinning=true` advances, `spinning=false` again passes the lesson.
  - `test_a_hover_that_drifts_out_of_bounds_loses_its_hold` — one `hold` stage `{seconds: 3.0, radius_m: 2.5, min_m: 1.0, max_m: 4.0}`; feeds at t = 0, 1, 2, 2.9 accumulate `held_s` 2.9; a feed at north 6.0 resets `held_s` to 0 and reports `drift_m` > 5; feeds at t = 4, 5, 6 back at the anchor pass the lesson at t = 6.
  - `test_a_frame_hitch_does_not_inflate_the_hold` — feeds at t = 0 then t = 2.0 (same position): `held_s` grows by at most 0.25.
  - `test_a_hold_below_the_band_resets_too` — altitude 0.5 (< `min_m` 1.0) for two feeds: `held_s` stays 0.
  - `test_a_reach_stage_passes_inside_the_radius_and_shows_the_distance` — `reach {north_m: 20.0, east_m: 0.0, radius_m: 4.0, min_m: 0.5, max_m: 30.0}`: at north 12 `progress.distance_m` ≈ 8; at north 19.5, altitude 3, passes.
  - `test_a_collision_event_fails_the_lesson` — a feed whose `events` holds `{"kind": "collision", "time_s": 1.0}` → `state == "failed"`, `reason` contains "collision".
  - `test_touching_the_ground_during_a_hold_fails` — altitude 0.03 during the hold → failed.
  - `test_the_time_limit_fails_a_stalled_lesson` — `time_limit_s: 5.0`, feeds to t = 6 never satisfying the stage → failed with a time reason.
  - `test_an_unknown_stage_kind_fails_setup_with_the_kind_named` — `setup` with `{"kind": "loop"}` → `status().reason` contains "loop".
  - `test_text_sitl_wins_when_setup_says_sitl` — a stage with both texts reads `text_sitl` after `setup(lesson, true)`, `text` after `setup(lesson)`.
- [ ] **Step 2: Register the suite** in `godot/tests/run_tests.gd` (`SUITES`, after `test_main_menu.gd`), then run `bash scripts/run-godot-tests.sh unit` in WSL — Expected: FAIL (the suite fails to load: `lesson_runner.gd` missing; grep for the error, do not trust `0 failed`).
- [ ] **Step 3: Implement** `godot/scripts/lesson_runner.gd` per the Interfaces: fields `_lesson`, `_sitl`, `_stage := 0`, `_t0 := -1.0`, `_last_t := -1.0`, `_held_s`, `_anchor := {}`, `_reason := ""`, `_state := "running"`; `feed` computes `dt`, drains `events` for `collision`, applies the stage's rule, advances stages, ends in `passed`/`failed`. No `Input`, no nodes, no `Engine` calls.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS (grep confirms no `SCRIPT ERROR`).
- [ ] **Step 5: Commit** `feat(godot): the lesson runner - motors, hold and reach stages (#125)`.

### Task 2: The runner's `turn`, `figure_eight` and `lap` stages

**Files:**
- Modify: `godot/scripts/lesson_runner.gd`
- Modify: `godot/tests/test_lesson_runner.gd`

**Interfaces:**
- Adds stage dictionaries: `{"kind": "turn", "degrees": float, "min_speed_mps": float, "min_m": float, "max_m": float, "text": ...}` (progress `{"turned_deg": float, "target_deg": float}`); `{"kind": "figure_eight", "a": [north, east], "b": [north, east], "max_dist_m": float, "min_m": float, "max_m": float, "text": ...}` (progress `{"a_deg": float, "b_deg": float}`); `{"kind": "lap", "text": ...}` (progress `{"next_gate": int, "gate_count": int}`).
- Semantics: `turn` accumulates the shortest signed heading delta per feed (`d = wrap(heading - last)` into ±180) as `abs` sum, only while `speed_mps >= min_speed_mps` and altitude in band; passes at `>= degrees`. `figure_eight` accumulates the same shortest-delta unwrapping of the bearing `atan2(east - c_east, north - c_north)` around each centre into signed accumulators `_a_deg`/`_b_deg`; passes when `abs(a_deg) >= 360 and abs(b_deg) >= 360 and sign(a_deg) != sign(b_deg)`; a feed farther than `max_dist_m` from the midpoint freezes both accumulators that frame (no reset — a wide eight is a style fault, not a failure). `lap` passes when `race_laps >= 1` (with `race_present`); progress reports `race_next_gate`/`race_gate_count`.

- [ ] **Step 1: Write the failing tests** (same `_snap` helper; bearings via loops of synthetic positions):
  - `test_a_turn_accumulates_across_the_north_wrap` — headings 350 → 5 → 10 → 20 at speed 3, altitude 2: `turned_deg` ≈ 30 after the last feed; a single 350→10 jump counts 20, not 340.
  - `test_a_turn_only_counts_while_fast_enough_and_in_band` — the same headings at speed 0.5: `turned_deg` stays 0; at altitude 30 (above `max_m` 8): stays 0.
  - `test_a_figure_of_eight_needs_both_pylons_in_opposite_senses` — `a = [10.0, -5.0]`, `b = [10.0, 5.0]`: feeds walking a 3 m circle around `a` (heading positions `north = 10 + 3cos(rad)`, `east = -5 + 3sin(rad)` for 0° to 360°) → `a_deg` ≈ 360, still `running`; then the same circle around `b` walked the other way → `b_deg` ≈ −360, `passed`.
  - `test_two_loops_of_one_pylon_do_not_pass_the_figure_of_eight` — two full circles around `a` only: `running`, `b_deg` 0.
  - `test_a_lap_passes_when_the_race_counts_one` — `lap` stage; feeds with `race_present: true, race_laps: 0, race_next_gate: 2, race_gate_count: 4` show progress `NEXT 2/4` numbers and stay `running`; a feed with `race_laps: 1` passes.
  - `test_a_lap_ignores_a_race_that_is_not_present` — `race_present: false`, `race_laps: 1`: still `running`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (new stage kinds reject at setup).
- [ ] **Step 3: Implement** the three stage rules inside `feed`'s dispatch (a `_stage_progress()` per kind keeps `status()` flat).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): turn, figure-of-eight and lap lesson stages (#125)`.

### Task 3: The lesson catalogue and the school quad

**Files:**
- Create: `godot/scripts/lessons.gd`
- Create: `quads/school-5f.toml`
- Create: `godot/tests/test_lessons.gd`
- Modify: `godot/tests/run_tests.gd`

**Interfaces:**
- Produces: `Lessons extends RefCounted` with `const LESSONS: Array` (the five dictionaries below, in order) and `static func by_id(id: String) -> Dictionary` (`{}` when unknown), `static func validate(list: Array) -> PackedStringArray` (unique ids; known stage kinds; finite, positive `seconds`/`radius_m`/`degrees`; `time_limit_s` > 0; a `demo` key, when present, is a bare file name ending `.csv`).
- The five lessons (spec D4/D6/D7; every `text` names the control, every `text_sitl` names the Betaflight switch):
  1. `pre_flight` — goal "Spin the motors up and shut them down safely."; stages `[{kind: motors, spinning: true, text: "Raise the throttle until the motors spin.", text_sitl: "Arm on aux 1 with the throttle down (Space, then throttle up)."}, {kind: motors, spinning: false, text: "Close the throttle until the motors stop.", text_sitl: "Disarm on aux 1."}]`; `time_limit_s: 120`, `race: false`.
  2. `hover` — goal "Lift off and hold a hover over the spot."; stages `[{kind: hold, radius_m: 2.5, min_m: 1.0, max_m: 4.0, seconds: 10.0, text: "Hover between 1 and 4 m, within 2.5 m of where you lifted off, for 10 seconds.", text_sitl: "Angle mode (aux 2) makes this a throttle-and-trim job."}]`; `time_limit_s: 240`.
  3. `forward_turn` — goal "Fly forward to the start gate and turn back."; stages `[{kind: reach, north_m: 20.0, east_m: 0.0, radius_m: 5.0, min_m: 1.0, max_m: 8.0, text: "Fly forward to the start gate line, 1 to 8 m up."}, {kind: turn, degrees: 180.0, min_speed_mps: 1.0, min_m: 0.5, max_m: 8.0, text: "Turn 180 degrees and come back, keeping some speed through the turn."}]`; `time_limit_s: 240`.
  4. `figure_eight` — goal "Fly a figure of eight around the two pylons."; `pylons: [{"name": "A", "north_m": 10.0, "east_m": -5.0}, {"name": "B", "north_m": 10.0, "east_m": 5.0}]`; stages `[{kind: figure_eight, a: [10.0, -5.0], b: [10.0, 5.0], max_dist_m: 15.0, min_m: 0.5, max_m: 12.0, text: "Loop one pylon, then the other the other way round. The counter follows your track."}]`; `time_limit_s: 300`.
  5. `circuit` — goal "Fly one full lap of the course."; `race: true`; stages `[{kind: lap, text: "Pass all four gates in order and cross the start gate again. The green gate is next."}]`; `time_limit_s: 300`.
  Every lesson also carries `world: "circuit.toml"` and `title`.
- `quads/school-5f.toml`: a copy of `quads/opendrone-5f-freestyle.toml` with `name = "School 5F (stabilized)"`, `[fc] kind = "open_loop_stabilized"`, and a header comment: the parameters are the 5F freestyle's, copied unchanged for the flight school (spec D7, provenance marked); the sitl-only `launch`/`betaflight_diff` keys stay (they are read for `kind = "sitl"` only, `crates/ofs-config/src/lib.rs` around line 1396).

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_lessons.gd`: `test_every_lesson_validates` (`Lessons.validate(Lessons.LESSONS)` is empty); `test_the_five_lessons_are_the_ruled_set` (ids, in order: `pre_flight, hover, forward_turn, figure_eight, circuit`); `test_every_lesson_names_goal_title_world_and_time_limit`; `test_only_the_circuit_lesson_races` (`race` true only there); `test_by_id_finds_and_misses`; `test_no_lesson_has_a_demo_yet` (Task 9 adds `demo` keys; this test is updated there).
- [ ] **Step 2: Run** `cargo test -p ofs-config --locked` — Expected: PASS **before** the quad file exists; then create `quads/school-5f.toml` and run again — Expected: PASS (the shipped-quads test enumerates and validates it; a failure names the field to fix).
- [ ] **Step 3: Run** `bash scripts/run-godot-tests.sh unit` in WSL — Expected: FAIL (`lessons.gd` missing), then implement `lessons.gd` and register the suite — Expected: PASS.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing else consumes the new quad file; note any surprise in the ledger).
- [ ] **Step 5: Commit** `feat(godot): the lesson catalogue and the school quad (#125)`.

### Task 4: Lesson progress — `user://flight_school.json`

**Files:**
- Create: `godot/scripts/lesson_progress.gd`
- Create: `godot/tests/test_lesson_progress.gd`
- Modify: `godot/tests/run_tests.gd`

**Interfaces:**
- Produces: `LessonProgress extends RefCounted` with `const PATH := "user://flight_school.json"` and statics:
  - `static func load(path := PATH) -> Dictionary` — the parsed file, or `{}` when missing; a file that does not parse pushes a warning and returns `{}` (nothing is written back).
  - `static func with_pass(progress: Dictionary, id: String, best := {}) -> Dictionary` — pure: sets `passed[id] = true`, merges `best` under `best[id]` (replacing it), stamps `"version": 1`.
  - `static func save(path: String, progress: Dictionary) -> bool` — writes JSON (the `controls.json` style: `JSON.stringify(progress, "\t")`); false when the write fails.
- Consumes: nothing.

- [ ] **Step 1: Write the failing tests** (`extends "res://tests/testing.gd"`, `const LessonProgress = preload(...)`, `const DIR := "user://lesson-progress-test"`; remove `DIR` recursively at the start of each test, the `test_main_menu.gd` `_remove` pattern):
  - `test_a_pass_round_trips_through_the_file` — `save(DIR + "/school.json", with_pass({}, "hover", {"hold_s": 12.4}))`; `load(...)` reports `passed.hover == true` and `best.hover.hold_s ≈ 12.4` and `version == 1`.
  - `test_a_second_pass_replaces_the_best` — `with_pass(with_pass({}, "hover", {"hold_s": 9.0}), "hover", {"hold_s": 12.0})` keeps one `best.hover` with `hold_s ≈ 12.0`.
  - `test_an_invalid_file_reads_as_no_progress` — write `"not json{"` by hand; `load` returns `{}`.
  - `test_a_missing_file_reads_as_no_progress` — `load(DIR + "/absent.json") == {}`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL, then implement and register — Expected: PASS.
- [ ] **Step 3: Commit** `feat(godot): lesson progress in user://flight_school.json (#125)`.

### Task 5: The flight school screen and its menu entry

**Files:**
- Create: `godot/ui/flight_school.gd`
- Modify: `godot/ui/main_menu.gd`, `godot/main.tscn`, `godot/tests/run_tests.gd`
- Modify: `godot/tests/test_main_menu.gd`
- Create: `godot/tests/test_flight_school.gd`

**Interfaces:**
- Produces: `FlightSchool extends CanvasLayer` — a dim panel (the `calibration_wizard.gd` look: `PanelContainer`, `StyleBoxFlat`, centered, built in code). Signals `lesson_requested(id: String)`, `demo_requested(id: String)`, `closed`. API: `setup(lessons: Array, progress: Dictionary)` (the lesson list and `{passed: {id: true}}`), `open()` (refreshes, grabs the first unpassed lesson's row focus), `close()`, `is_open()`, `select(id: String)` (for tests and keyboard focus), `row(name: String) -> Control` (`"start"`, `"watch"`, `"back"`, or a lesson id), `goal_text() -> String` (the selected lesson's goal, for tests).
- `main_menu.gd` gains a `_school` Button "Flight school" between `_start` and `_controls_button` (the `for node in [...]` list at line 58), signal `school_requested`, and `row("school")` in the map at line 110.
- `main.tscn` gains, after `SettingsScreen`: `[node name="FlightSchool" type="CanvasLayer" parent="."]` `layer = 8` with `script = ExtResource("15")` (an `[ext_resource type="Script" path="res://ui/flight_school.gd" id="15"]`).

- [ ] **Step 1: Write the failing tests.**
  - In `godot/tests/test_flight_school.gd`: `test_the_screen_lists_lessons_with_passed_marks` (`setup` with two fake lessons and `{"passed": {"hover": true}}`; the hover row's text contains "passed", the other does not); `test_the_goal_of_the_selection_is_shown` (`select("hover")` → `goal_text()` contains the lesson's goal); `test_start_emits_the_selected_lesson` (`row("start").pressed.emit()` → `lesson_requested` with "hover"); `test_the_watch_button_follows_the_demo` (a fake lesson with `"demo": "hover.csv"` shows an enabled `row("watch")` whose press emits `demo_requested`; a lesson without `demo` hides it); `test_back_emits_closed`.
  - In `godot/tests/test_main_menu.gd`: extend the `_menu` events wiring with `menu.school_requested.connect(func(): events.append(["school"]))` and add `test_the_school_button_emits` (`menu.row("school").pressed.emit()` appends `["school"]`).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL, then implement both scripts, the scene node, and register `test_flight_school.gd` — Expected: PASS.
- [ ] **Step 3: Commit** `feat(godot): the flight school screen and its menu entry (#125)`.

### Task 6: The in-flight lesson panel

**Files:**
- Create: `godot/ui/lesson_panel.gd`
- Modify: `godot/main.tscn`
- Create: `godot/tests/test_lesson_panel.gd`
- Modify: `godot/tests/run_tests.gd`

**Interfaces:**
- Produces: `LessonPanel extends CanvasLayer` (layer 4, the scene's **last** node so it draws over the `Hud` and under the F2/menu layers; `visible = false` until a lesson starts). API: `set_lesson(lesson: Dictionary, demo := false) -> void`, `update(status: Dictionary) -> void` (the runner's `status()`), `clear() -> void`, `text() -> String` and `title() -> String` for tests. It builds its labels in code (the `hud.gd` `_place`/`_style` idiom): title top-left under the status line, instruction and progress centered under the banner, R/Esc hint at the bottom.
- `main.tscn`: `[ext_resource type="Script" path="res://ui/lesson_panel.gd" id="16"]` and, as the last node, `[node name="LessonPanel" type="CanvasLayer" parent="."]` with `layer = 4` and `script = ExtResource("16")`.
- Progress rendering: `hold` → `"HOLD 4.2 / 10 s   DRIFT 1.8 m   ALT 2.1 m"`; `turn` → `"TURNED 95° / 180°"`; `figure_eight` → `"PYLON A 210°   PYLON B -95°"`; `lap` → `"NEXT 2/4"`; `reach` → `"12 m to go"`; `motors` → `"motors on"` / `"motors off"`. Passed → `"Lesson complete — Esc for the school, R to fly it again."`; failed → the `reason` plus the same hint; a demo run prefixes `"DEMO"` to the title.

- [ ] **Step 1: Write the failing tests** in `godot/tests/test_lesson_panel.gd`: `set_lesson` shows the title and goal; `update` with a running hold status shows all four hold numbers; a lap status shows `NEXT 2/4`; a passed status shows "Lesson complete"; a failed status shows the reason; `set_lesson(lesson, true)` shows "DEMO"; `clear()` empties `text()` and hides.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL, then implement and register — Expected: PASS.
- [ ] **Step 3: Commit** `feat(godot): the in-flight lesson panel (#125)`.

### Task 7: App wiring — start a lesson, feed it, end it

**Files:**
- Modify: `godot/scripts/app.gd`, `godot/ui/hud.gd` (one help line), `godot/main.tscn` (LessonPanel node, Task 6)
- Modify: `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Tasks 1–6 (`LessonRunner`, `Lessons`, `LessonProgress`, `FlightSchool`, `LessonPanel`, `MainMenu.effective_open_loop`).
- Produces on `app.gd` (used by the e2e): `@onready var flight_school: CanvasLayer`, `@onready var lesson_panel: CanvasLayer`, `var progress_path := LessonProgress.PATH` (the e2e points it at `user://e2e-school.json` so the run does not touch the real file), `func lesson_active() -> bool`, `func _start_lesson(id: String, demo := false) -> void`, `func _end_lesson() -> void`, `var _lesson_runner = null`, `var _lesson_events: Array`.
- Wiring rules (spec D6/D8/D11):
  - `_ready`: `flight_school.setup(Lessons.LESSONS, LessonProgress.load(progress_path))`; connect `main_menu.school_requested` → `main_menu.close(); flight_school.open()`; `flight_school.lesson_requested` → `_start_lesson(id)`; `flight_school.demo_requested` → `_start_lesson(id, true)` (Task 9 fills the demo path; until then the watch button is hidden by Task 5's rule, so the connection is dead code for one task); `flight_school.closed` → `main_menu.open()`.
  - `_in_menu()` (line 158) also returns true while `flight_school.is_open()`.
  - `_start_lesson`: `main_menu.close()`, `flight_school.close()`; lesson = `Lessons.by_id(id)`; choices `{"world_path": AppSettings.repo_root() + "/worlds/" + lesson.world, "race_start": lesson.get("race", false)}` plus `"quad_path": <repo>/quads/school-5f.toml` when `MainMenu.effective_open_loop(settings)` else leave `settings["quad_path"]`; `_lesson_runner = LessonRunner.new(); _lesson_runner.setup(lesson, not effective_open_loop)`; `lesson_panel.set_lesson(lesson, demo)`; add the pylons (`figure_eight` only): two `MeshInstance3D` (`CylinderMesh`, top radius 0.25, height 3, at `Vector3(east, 1.5, -north)`, unshaded orange `StandardMaterial3D`), named `Lesson_Pylon_A`/`Lesson_Pylon_B`, children of the `world` node; then `fpv_box.visible = true; hud.visible = true; _start_session(choices)`.
  - `_process`, after the telemetry read (around line 239) and only while `lesson_active()`: build the snapshot — `t = client.get_telemetry()`, `pose = client.get_pose()`, `LessonRunner.north_east(pose.origin)`, `heading_deg: LessonRunner.heading_deg(pose.basis)`, the race keys from `t`, `events: _lesson_events` then `_lesson_events = []` — `feed` it, `lesson_panel.update(_lesson_runner.status())`; on a transition to `passed`, `LessonProgress.save(progress_path, LessonProgress.with_pass(LessonProgress.load(progress_path), id, _best(lesson, runner)))` and a toast; `_best` returns `{"hold_s": status.progress.held_s}` for `hover` and `{"lap_s": t.race_last_lap_s}` for `circuit`, else `{}`.
  - `_on_event` (line 490): while `lesson_active()`, append `{"kind": kind, "time_s": _time_s}` to `_lesson_events` (the existing toast handling is unchanged).
  - `_on_phase_changed`: on `"loading"` while `lesson_active()` — a reload (**R** restarts the race too, per `docs/race-timing.md`): rerun `setup` on the runner, reset the panel with `set_lesson(lesson, false)`, clear `_lesson_events`. On `"stopped"`/`"failed"` the lesson keeps its state; the panel shows the last status.
  - `_unhandled_key_input`, `KEY_ESCAPE` (line 297): after the help/F2 `closing` check, `if _end_lesson_if_active(): return` — `_end_lesson()` clears the runner, panel and pylons (free the `Lesson_Pylon_*` children), then `_show_menu()` and `flight_school.open()`.
  - `hud.gd` `HELP_LINES` (line 18): add `"Flight school: main menu -> Flight school."` after the Esc line.
  - A lesson never writes to `ofs_client.cfg`: the choices go into `_start_session`'s local merge only (line 113–115; re-verify at execution).

- [ ] **Step 1: Write the failing e2e additions** in `godot/tests/e2e_open_loop.gd`, after the existing menu round-trip block (the lines after "free flight hides the race line"):
  - `app.progress_path = "user://e2e-school.json"` (remove it if it exists).
  - `app.main_menu.open(); app.main_menu.row("school").pressed.emit()` → `app.flight_school.is_open()` and the flight is stopped.
  - `app.flight_school.select("pre_flight"); app.flight_school.row("start").pressed.emit()` → wait `flying` (30 s); `app.client.get_quad_name() == "School 5F (stabilized)"`; `app.lesson_active()`.
  - `app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.5, "aux": [-1.0, -1.0, -1.0, -1.0], "status": ""}` → wait `app.lesson_panel.text().contains("motors off")` (30 s; motors spin, stage 1 passes, stage 2's instruction shows); then `sticks_override` with `throttle: 0.0` → wait `app.lesson_panel.text().contains("Lesson complete")` (the motors stop) and assert `FileAccess.get_file_as_string("user://e2e-school.json").contains("pre_flight")`.
  - **R** restarts the lesson: `_press(app, KEY_R)`, wait `flying`, then `app.lesson_panel.text()` shows the first stage's instruction again ("Raise the throttle" / "Arm") and no longer "Lesson complete".
  - `app.main_menu.open()` is not needed — press Esc: `_press(app, KEY_ESCAPE)` → `app.flight_school.is_open()` and `not app.lesson_active()` and the world node has no `Lesson_Pylon_A` (`app.get_node("FpvViewport/World").get_node_or_null("Lesson_Pylon_A") == null`).
  - `app.flight_school.select("figure_eight"); app.flight_school.row("start").pressed.emit()` → wait `flying`; both `Lesson_Pylon_A` and `Lesson_Pylon_B` exist under `FpvViewport/World`; `_press(app, KEY_ESCAPE)` → back at the school; `app.flight_school.row("back").pressed.emit()` → `app.main_menu.is_open()`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh open-loop` in WSL — Expected: FAIL on the new `_check`s.
- [ ] **Step 3: Implement** the wiring rules above; keep `_show_menu` untouched (the lesson's Esc path calls it, then opens the school over the menu).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` and `bash scripts/run-godot-tests.sh open-loop` — Expected: PASS (grep for `SCRIPT ERROR`).
- [ ] **Step 5: Commit** `feat(godot): wire lessons into the app - start, pylons, Esc, e2e (#125)`.

### Task 8: The demo player

**Files:**
- Create: `godot/scripts/demo_player.gd`
- Create: `godot/tests/test_demo_player.gd`
- Modify: `godot/tests/run_tests.gd`

**Interfaces:**
- Produces: `DemoPlayer extends RefCounted`:
  - `func load_text(text: String) -> String` — parses the `play_inputs` CSV (`python/ofs/client.py` line 565 is the format's contract): lines starting `#` are comments; the header must contain `t_s`; columns `roll,pitch,yaw,throttle,aux1..aux4` optional (`tx`/`link_loss` ignored); `t_s` strictly increasing, every value numeric. Returns `""` or an error naming the first bad line.
  - `func sticks(elapsed_s: float) -> Dictionary` — `{roll, pitch, yaw, throttle, aux: [4]}` of the last row at or before `elapsed_s`; columns never mentioned default `0.0` (sticks, throttle) and `-1.0` (aux).
  - `func duration_s() -> float` — the last row's `t_s`.
  - `func load_file(path: String) -> String` — `FileAccess.get_file_as_string` + `load_text`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing tests:** `test_a_two_row_csv_holds_each_row` (`"t_s,throttle,roll\n0.0,0.0,0.0\n1.0,0.5,0.1\n"`: `sticks(0.5).throttle ≈ 0.0`, `sticks(1.5).throttle ≈ 0.5` and `.roll ≈ 0.1`); `test_unmentioned_columns_default` (`"t_s,throttle\n0.0,0.4\n"`: `aux == [-1,-1,-1,-1]`, `roll == 0`); `test_tx_and_link_loss_are_ignored` (a CSV with them parses); `test_a_csv_without_t_s_is_refused` (`"roll\n0.1\n"` → non-empty error); `test_decreasing_t_s_is_refused`; `test_comments_are_skipped` (a leading `#` line, the `python/examples/flips.csv` shape); `test_duration_is_the_last_row`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL, then implement and register — Expected: PASS.
- [ ] **Step 3: Commit** `feat(godot): the demo player for recorded inputs CSVs (#125)`.

### Task 9: The demo recordings and their replay

**Files:**
- Create: `python/examples/flight_school_demos.py`
- Create: `godot/lessons/hover.csv`, `godot/lessons/forward_turn.csv`, `godot/lessons/circuit.csv` (recorded by the script, committed)
- Create: `python/tests/test_flight_school_demos.py`
- Modify: `godot/scripts/lessons.gd` (`demo` keys), `godot/scripts/app.gd` (the demo path), `godot/tests/test_lessons.gd` (the no-demo test flips)

**Interfaces:**
- `python/examples/flight_school_demos.py` (run by hand: `python python/examples/flight_school_demos.py --out godot/lessons`): `ofs.launch()`, `sim.load("quads/school-5f.toml", world="worlds/circuit.toml")` (the stabilized kind is in the file, `docs/scripted-flight.md` line 91), then per maneuver `path = sim.record_inputs()`, fly, `sim.stop_inputs_recording()`, copy the returned path to `--out/<id>.csv`. The flights: `fly_hover(sim)` — the `hover.py` altitude PID (hover guess 0.45, `kp, ki, kd = 0.15, 0.10, 0.10`, 12 s at 2 m, sticks at `roll = clamp(-north * 0.08, -0.3, 0.3)`, `pitch` likewise on east — tune by hand once, the committed CSV is the artifact); `fly_forward_turn(sim)` — climb to 3 m, fly north to (20, 0) at up to 4 m/s leaning at the target (the `circuit.py` pilot's position loop), then roll 0.35 with a yaw stick until the heading has turned 180°, return to (0, 0); `fly_lap(sim)` — load `python/examples/circuit.py` by `importlib` (the `docs/scripted-flight.md` snippet) and call `circuit.fly_circuit(sim)`.
- `app.gd` demo path: `_start_lesson(id, true)` sets `_demo := DemoPlayer.new()` whose `load_file("res://lessons/" + lesson.demo)` failing toasts the error and starts the lesson without a demo; once telemetry is live (`_process`, the first non-empty `t` while a demo waits) record `_demo_t0 := t.time_s`; each frame `sticks_override = _demo.sticks(t.time_s - _demo_t0)` while rows remain (`elapsed <= _demo.duration_s()`), after which `sticks_override = null` (the pilot's controller takes over; the panel title keeps the DEMO tag until **R**). `_end_lesson`, a phase change to `"loading"`, and Esc all null `sticks_override`.
- `lessons.gd`: `"demo": "hover.csv"`, `"forward_turn.csv"`, `"circuit.csv"` on those three lessons (spec D9 — `pre_flight` and `figure_eight` get none).

- [ ] **Step 1: Write the failing pytest** `python/tests/test_flight_school_demos.py`: for each of the three names, the file `godot/lessons/<name>` exists; lines starting `#` skipped, the header contains `t_s`, `t_s` strictly increases, the last `t_s` ≥ 10.0, and the header mentions `throttle`. Run `PYTHONPATH=python python -m pytest python/tests/test_flight_school_demos.py -q` — Expected: FAIL (no files).
- [ ] **Step 2: Write `flight_school_demos.py`** and run it by hand (`python python/examples/flight_school_demos.py --out godot/lessons`); eyeball each CSV's flight before committing (start the game, watch the demo; adjust the hover guess or waypoint speeds if a demo misses its own lesson's point — spec's Checked-by-eye list). Re-run the pytest — Expected: PASS.
- [ ] **Step 3:** Add the `demo` keys to `lessons.gd`, flip `test_no_lesson_has_a_demo_yet` to `test_demos_exist_for_hover_forward_turn_and_circuit` (present on exactly those three, each a bare `.csv` name), and implement the `app.gd` demo path. Extend the e2e: with the circuit lesson selected, `row("watch")` is visible; start it, wait `flying`, assert `app.sticks_override != null` and `app.lesson_panel.title().contains("DEMO")`, then Esc. Run `bash scripts/run-godot-tests.sh unit` and `... open-loop` — Expected: PASS.
- [ ] **Step 4: Run** `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): watch it first - demo recordings and replay (#125)`.

### Task 10: Docs, changelog and the gates

**Files:**
- Create: `docs/flight-school.md`
- Modify: `README.md` (a "The flight school" paragraph after the menu paragraph, around line 108), `CHANGELOG.md` (`## Unreleased` → `### A flight school (#125)` in the house style), `docs/roadmap.md` (only if it lists the school)

**Interfaces:** none (prose).

- [ ] **Step 1: Write `docs/flight-school.md`:** the five lessons and what each scores (the spec's D2 table in plain words), how to start one (menu → Flight school), what **R**/**N**/**Esc** do during a lesson, the `user://flight_school.json` file, the demo CSVs and how to re-record them (`python python/examples/flight_school_demos.py --out godot/lessons`), and how to add a lesson (a dictionary in `godot/scripts/lessons.gd`; the stage kinds and their keys; the pylon and `race` flags).
- [ ] **Step 2: Update** the README and CHANGELOG (Added: the school, the five lessons, progress file, demos, the school quad; the menu's new button).
- [ ] **Step 3: Check** every path and key the docs name against the code by reading (`grep -n "flight_school\|lesson_" godot/scripts/app.gd godot/scripts/lessons.gd`).
- [ ] **Step 4: Run the gates:** WSL `bash scripts/run-godot-tests.sh all` (grep for `SCRIPT ERROR`), `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `PYTHONPATH=python python -m pytest python/tests -q` — Expected: all PASS (no Rust source changed; clippy guards the odd drift).
- [ ] **Step 5: Commit** `docs: the flight school (#125)`.

The PR description lists the by-eye checks (the spec's "Checked by eye" list) for #163, the manual-QA collector, the way every Game-wave PR did.

## Ledger

- Empty. Executors append a line per deviation from the spec's D1–D12 or this plan's interfaces, with the reason.
