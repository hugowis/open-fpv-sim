# Best-lap ghost and local records: implementation plan (#133)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The client captures every racing state's pose and keeps the session's fastest clean lap; the records persist per world, course and quad in `user://records.json`; a translucent ghost quad replays the best lap against the pilot, with the record best on the HUD, a live delta, and best/last lap in the main menu. Closes #133.

**Architecture:** All client-side. A pure `LapCapture` in `ofs-client` (new `lap.rs`) fed by the worker's `ingest` — every `pb::State` at up to 240 Hz — detects a session-best lap in the state's `race` block (no events, no ordering problem) and extracts the lap's poses, decimated to 60 Hz. The gdext bridge hands the track to Godot one at a time. A `records.gd` store persists best/last/ghost in `user://records.json` (nested by declared names); a `ghost.gd` (extending `drone.gd`, translucent) plays the track on the lap clock and gives the nearest-point delta; `app.gd` wires capture, records, ghost, HUD and menu. No wire growth: `ofs-sim`, the proto and Python are untouched.

**Tech Stack:** Rust workspace (MSRV 1.85; `ofs-client`, `ofs-godot`), Godot 4.7.2 GDScript + the gdext bridge. No protocol change.

**Spec:** `docs/superpowers/specs/2026-10-31-best-lap-ghost-design.md` (the ten numbered calls are the binding rules). Issues: #133, epic #136.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- The wire does not grow: nothing under `proto/`, `crates/ofs-proto`, `crates/ofs-sim` or `python/` changes. `PROTOCOL_VERSION` stays whatever `main` holds (15 at `crates/ofs-proto/src/lib.rs:12`; open PR #169 lands 16 — re-verify at execution, this plan must not touch it). No golden digest can move because no `ofs-sim` file changes; one that does is a bug.
- The settings chain is not forked: no new key in `godot/scripts/settings.gd` (`ENV_NAMES`, `FLAG_NAMES`, `defaults`), nothing written to `user://ofs_client.cfg`. All persistence goes to `user://records.json`.
- The e2e suites' menu-less path keeps working: records and ghost are passive without a records file (`godot/tests/e2e_open_loop.gd:84` boots `OFS_MENU=off`).
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- Python tests still run green to prove the Python client is untouched: `PYTHONPATH=<worktree>/python python -m pytest python/tests -q` (the SITL tests skip without `OFS_SITL_LAUNCH`).
- Godot tests: `bash scripts/run-godot-tests.sh unit` in WSL (downloads Godot 4.7.2; `OFS_GODOT_DL=/mnt/c/dev/open-fpv-sim/build/godot-dl`), `all` for the e2e. Touch Rust sources before the first run so the extension rebuilds.
- Commit trailer: none prescribed — the most recent merged work (B6, PR #167) carries none; an executing session that adds one names its own identity, never a borrowed one.

## Design calls (decided here; the executor ledgers any change)

1. **Capture source and moment** (spec calls 1–2): the worker's `ingest` feeds the capture with every state's pose (Godot frame, the converted `Sample`) and its `RaceInfo`; a state whose `race.laps` rose completed a lap; it is the session's best exactly when `last_lap_s == best_lap_s` (`race.rs:213`); the window is `[time_s − last_lap_s, time_s]`. Detect the completed lap **before** trimming to the new `current_lap_s` (the completing state already runs the next lap's clock).
2. **Clean** (spec call 3): no `collision` event time inside the window; `gate_hit` does not veto. The record's `last_lap_s` is updated from telemetry on every lap, veto or not.
3. **The stored track** (spec call 4): Godot-frame poses, times normalised to the lap start, decimated to at most `STORE_HZ = 60.0` samples per second (keep every k-th plus the last).
4. **Records** (spec call 5): `user://records.json`, `{"version": 1, "worlds": {world: {course: {quad: {best_lap_s, best_at, last_lap_s, last_at, ghost}}}}}`, declared names as keys (`World.name`, `RaceState.course` — `""` for the unnamed course — `LoadReply.quad_name`, `server.rs:364`), floats rounded on save (positions 3 decimals, rotations 4, times 3). Only the best lap keeps a ghost; a strictly faster clean lap replaces it. A file that does not parse is kept and the store starts empty.
5. **The bridge** (spec call 6): `OfsClient.take_lap_capture()` returns the waiting capture as a Dictionary (`world`, `course`, `quad`, `lap_s`, `times`, `positions` (xyz triplets), `rotations` (quaternion xyzw quadruplets), the last three `PackedFloat32Array`) and takes it; `{}` when none waits. The newest capture replaces an untaken one.
6. **Playback and delta** (spec calls 7–8): the ghost's pose each frame is the track at `race_lap_s` (the pilot's current-lap time); before the clock runs or after a reset it sits at `pose_at(0.0)`, past its finish it parks at the end. When #130 has landed, the wiring calls `ghost.set_quad(<the session's quad path>)` beside the drone's, so the ghost wears the racer's scale (spec call 7). The delta is `race_lap_s − ghost_time_at_nearest_pose` to the drone's drawn position, searched in a ±2 s window around the last match.
7. **Surfaces** (spec calls 9–10): the HUD keeps its line format; the app injects the record best over `race_best_lap_s` when faster, and appends the delta (`-0.8` / `+1.2`, one decimal) while the race and the ghost run. The main menu gains a records line under Course (`Best 29.9 s   Last 31.2 s`). Free flight (and everything without a record) sees no ghost, no delta, no line — `app.gd:241` already forces `race_present` false there.

## Review Focus

- A lap that completes within a state period of a reset or course switch must not splice two races' poses (task 1's `a_reset_clears_the_window_and_the_best_reference`).
- A hiccuping state stream (a missing state, uneven spacing) must yield the right window: times, not sample counts, define it (task 1's `an_uneven_state_stream_still_yields_the_right_window`).
- A records file that does not parse (a hand edit, a future version) must leave the game flying and the file untouched (task 3's `a_missing_or_corrupt_file_starts_empty`).
- The ghost when the pilot resets mid-lap or the race finishes must park at the start, not keep flying the old lap (task 5's e2e check and the `pose_at(0.0)` branch).
- The delta when the pilot is nowhere near the ghost's line (a cut course, a crash off-track) must stay a sane number, not spike or flip sign wildly (task 4's `test_delta_is_negative_ahead_and_positive_behind` and the far-away case).

---

### Task 1: The lap capture (ofs-client)

**Files:**
- Create: `crates/ofs-client/src/lap.rs`; Modify: `crates/ofs-client/src/lib.rs` (`pub mod lap;`), `crates/ofs-client/src/worker.rs` (`Shared`, `ingest`, `reset_state`, `watch_events`), `crates/ofs-client/src/client.rs` (`take_lap_capture`)
- Test: unit tests inside `lap.rs`

**Interfaces:**
- Consumes: `crate::interp::Sample` (`interp.rs:22`), `crate::model::RaceInfo` (`model.rs:259`).
- Produces (all `pub`): `pub const STORE_HZ: f64 = 60.0;` `pub const COLLISION_MEMORY: usize = 8;` `pub struct LapTrack { pub world: String, pub course: String, pub lap_s: f64, pub times: Vec<f64>, pub positions: Vec<[f64; 3]>, pub rotations: Vec<[f64; 4]> }` (`Debug, Clone, PartialEq`); `pub struct LapCapture` (`Default`) with `fn ingest(&mut self, world: &str, sample: Sample, race: &RaceInfo, collisions: &[f64]) -> Option<LapTrack>`; `Client::take_lap_capture(&self) -> Option<LapTrack>`; `Shared.lap: Mutex<Option<LapTrack>>`, `Shared.collisions: Mutex<VecDeque<f64>>`, `Shared.lap_capture: Mutex<LapCapture>`.

- [ ] **Step 1: Write the failing tests** in `lap.rs` (names are the contract; a helper `race(present, running, laps, current_lap_s, last_lap_s, best_lap_s) -> RaceInfo` and one that feeds a straight 10 m/s flight past two gates in 1/240 s steps, the way `race.rs`'s tests fly):
  - `states_without_a_running_race_capture_nothing` (`present = false`, and `running = false` before the start: `None`, and nothing is buffered — the first pushed pose is the first after the clock starts).
  - `a_first_lap_is_captured_as_the_session_best` (feed 10 s at 240 Hz, complete lap 1 with `last == best`: `Some(track)` whose `lap_s` matches, `times[0] == 0.0` and `times.last() == lap_s` within one state period, `course` from the state).
  - `only_a_lap_that_beats_the_session_best_is_captured` (lap 2 slower: `last != best` → `None`; lap 3 faster → `Some`).
  - `the_track_spans_the_lap_window_decimated_to_60_hz` (a 2 s lap fed at 240 Hz: at most `2 * 60 + 2` samples; the first and last window poses are kept; positions/rotations round-trip the fed `Sample`s).
  - `an_uneven_state_stream_still_yields_the_right_window` (drop every 7th state and double one gap: the window still starts at `time_s − lap_s`, no pose before it).
  - `a_collision_inside_the_window_vetoes_the_capture` (`collisions = &[mid-window time]` → `None`; the next, clean best lap still captures).
  - `a_collision_outside_the_window_does_not` (just before the lap start, just after the end → `Some`).
  - `a_reset_clears_the_window_and_the_best_reference` (after `laps` drops to 0 and `best_lap_s` to 0, the next lap's window holds only poses after the reset).
  - `a_world_name_travels_with_the_track`.
- [ ] **Step 2: Run** `cargo test -p ofs-client --lib lap` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement.** `LapCapture` keeps `poses: VecDeque<Sample>`, `prev_laps: u32`, `prev_best_lap_s: f64`. Per call, in order: (1) if `!race.present` → clear everything, update the prevs, return `None`; (2) if `race.laps > prev_laps` → window = poses with `time_s >= now − race.last_lap_s`; when `race.last_lap_s == race.best_lap_s && race.last_lap_s > 0.0` and no collision time lies inside the window → build the decimated `LapTrack` (keep index 0, every `ceil(count / (STORE_HZ * lap_s))`-th, and the last; subtract the window's first time) and remember it as the call's result; (3) push the sample when `race.running`, then trim the front to `now − race.current_lap_s` (the completing state already runs the next lap, so the trim must follow the detection); (4) update the prevs — a `best_lap_s` that fell below `prev_best_lap_s` (a reset) clears the poses — and return the remembered result. In `worker.rs`: `watch_events` pushes a `Collision` event's `time_s` onto `Shared.collisions` (back, front-trimmed to `COLLISION_MEMORY`); `ingest` calls `lap_capture.ingest(world_name, sample, &telemetry.race, collisions)` — the world name from `lock(&self.world)` (`World.name`), the sample the one `ingest` already builds — and stores a `Some(track)` into `Shared.lap`, replacing any untaken one; `reset_state` clears `lap`, `collisions` and the capture. `Client::take_lap_capture` is `lock(&self.shared.lap).take()`.
- [ ] **Step 4: Run** `cargo test -p ofs-client --locked` then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing else consumes `Shared`'s new fields).
- [ ] **Step 5: Commit** `feat(client): capture the session's best lap from the state stream (#133)`.

### Task 2: The bridge accessor (ofs-godot)

**Files:**
- Modify: `crates/ofs-godot/src/lib.rs` (a `#[func]` beside `get_telemetry`, doc comment on the dictionary)
- Test: `godot/tests/test_extension.gd`

**Interfaces:**
- Consumes: Task 1's `Client::take_lap_capture`.
- Produces: `OfsClient.take_lap_capture() -> Dictionary` — `{world: String, course: String, quad: String, lap_s: float, times: PackedFloat32Array, positions: PackedFloat32Array (xyz per pose), rotations: PackedFloat32Array (xyzw per pose)}`, `{}` when nothing waits; `quad` filled from `quad_name()`.

- [ ] **Step 1: Write the failing test** in `test_extension.gd`: `test_take_lap_capture_is_inert_before_start` — a fresh client returns `{}` (the `test_before_start_everything_is_inert` pattern).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL; touch `crates/ofs-godot/src/lib.rs` first) — Expected: FAIL (no such method).
- [ ] **Step 3: Implement** the `#[func]`: `self.client.as_ref().and_then(|c| c.take_lap_capture())` mapped to the dictionary (`PackedFloat32Array::from_iter` over the flattened triples/quadruples; `quad` from `c.quad_name()`), `{}` without a client or capture.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the lap capture on the OfsClient bridge (#133)`.

### Task 3: The records store (Godot)

**Files:**
- Create: `godot/scripts/records.gd`, `godot/tests/test_records.gd`; Modify: `godot/tests/run_tests.gd` (add `res://tests/test_records.gd` to `SUITES`)
- Test: `godot/tests/test_records.gd`

**Interfaces:**
- Consumes: Task 2's capture dictionary shape.
- Produces (`static`, RefCounted): `const RECORDS_PATH := "user://records.json"`; `load_records(path := RECORDS_PATH) -> Dictionary` (the store; `{}` when missing or unparseable, the file untouched); `save_records(store, path := RECORDS_PATH) -> bool`; `entry(store, world: String, course: String, quad: String) -> Dictionary` (`{}` when none); `best_lap_s(store, world, course, quad) -> float` (0.0 when none); `record_last(store, world, course, quad, lap_s: float) -> void` (sets `last_lap_s`/`last_at`); `offer_capture(store, world, course, quad, capture: Dictionary) -> bool` (true when the capture became the best: absent or strictly faster; then `best_lap_s`/`best_at` and `ghost` come from the capture); `ghost_of(store, world, course, quad) -> Dictionary` (`{}` or `{lap_s, times, positions, rotations}`). `best_at`/`last_at` are `Time.get_datetime_string_from_system(true)`.

- [ ] **Step 1: Write the failing tests** in `test_records.gd` (each writes to `user://tmp-records-test.json` and cleans up):
  - `test_a_round_trip_keeps_every_number` (build a store with a two-pose ghost, save, load back: equal floats — positions rounded to 3 decimals, rotations 4, times 3 on save).
  - `test_a_missing_or_corrupt_file_starts_empty` (no file → `{}`; write `"not json"` → `{}` and the file still reads `"not json"`).
  - `test_the_first_capture_becomes_the_best_and_keeps_its_ghost` (`offer_capture` on an empty store → true; `best_lap_s` set; `ghost_of` returns the capture's arrays).
  - `test_only_a_strictly_faster_lap_replaces_the_ghost` (a slower capture → false, best and ghost unchanged; equal → false; faster → true, ghost replaced).
  - `test_record_last_updates_every_lap` (two calls; `last_lap_s` is the second; `best_lap_s` untouched).
  - `test_the_unnamed_course_is_its_own_key` (course `""` and course `"Hunt"` hold separate entries).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL (suite not in `SUITES` / script missing).
- [ ] **Step 3: Implement** `records.gd`: the store is `{"version": 1, "worlds": {world: {course: quad: {…}}}}`; write with `JSON.stringify(store, "\t")` through `FileAccess.WRITE` (a failed open returns false); parse with `JSON.new()` the way `control_profiles.gd:78` does, validating the nested Dictionaries; round with `snappedf` on save. `entry` walks the three levels and inserts nothing.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the local records store in user://records.json (#133)`.

### Task 4: The ghost quad (Godot)

**Files:**
- Create: `godot/drone/ghost.gd`, `godot/tests/test_ghost.gd`; Modify: `godot/tests/run_tests.gd` (add `res://tests/test_ghost.gd`)
- Test: `godot/tests/test_ghost.gd`

**Interfaces:**
- Consumes: `res://drone/drone.gd` (it extends it), Task 3's ghost dictionary shape.
- Produces: `const WINDOW_S := 2.0`; `func setup(track: Dictionary) -> void` (`{times, positions, rotations}` as `PackedFloat32Array`; `{}` clears); `func has_track() -> bool`; `func pose_at(lap_s: float) -> Transform3D` (position lerp, quaternion slerp — `Quaternion.slerp` — between the bracketing samples; the end poses held outside); `func delta(pilot: Vector3, lap_s: float) -> float` (`lap_s − times[nearest]`, `NAN` without a track); `_ready` builds the model (`super._ready()`), then walks every `MeshInstance3D` descendant making its material unshaded, `TRANSPARENCY_ALPHA`, albedo white at alpha 0.35; `set_motors` overridden to idle-spin the props at a fixed rate; a `_cursor` index for the windowed search (reset by `setup`).

- [ ] **Step 1: Write the failing tests** in `test_ghost.gd` (a synthetic track: samples at 0.0, 0.5, …, 2.0 s, positions 1 m apart along +X, identity-quaternion-ish rotations; `add_to_tree` a ghost for the model tests):
  - `test_pose_at_a_sample_time_is_that_pose`; `test_pose_between_samples_interpolates` (0.25 s → the midpoint); `test_pose_clamps_outside_the_track` (−1 s → the first pose; 9 s → the last).
  - `test_delta_is_negative_ahead_and_positive_behind` (pilot at sample k's position with `lap_s` 0.3 s less than `times[k]` → negative ≈ −0.3; 0.3 s more → ≈ +0.3); `test_delta_survives_a_pilot_far_off_the_line` (pilot 50 m away: still the nearest-in-window sample's value, finite); `test_delta_without_a_track_is_nan`; `test_an_empty_track_clears` (`setup({})` → `has_track() == false`).
  - `test_the_model_is_translucent` (every `MeshInstance3D` under a readied ghost has a `material_override` with `transparency != TRANSPARENCY_DISABLED` and albedo alpha < 1).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL.
- [ ] **Step 3: Implement** `ghost.gd` (`extends "res://drone/drone.gd"`; call `super._ready()` first). `pose_at` mirrors `StateBuffer::pose_at_sim`'s bracketing (`interp.rs:131`); `delta` scans `_cursor ± int(WINDOW_S / median step)` samples for the least squared distance to `pilot`, clamped to the array, and updates `_cursor` to the winner.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the ghost quad and its playback (#133)`.

### Task 5: Wiring — capture, records, ghost and the HUD delta

**Files:**
- Modify: `godot/scripts/app.gd` (records, ghost node, per-frame wiring), `godot/ui/hud.gd` (`_update_race` takes the delta), `godot/tests/e2e_open_loop.gd` (two checks), `godot/tests/test_hud.gd`
- Test: `godot/tests/test_hud.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Tasks 2–4 (`take_lap_capture`, `records.gd`, `ghost.gd`).
- Produces in `app.gd`: `const Records = preload("res://scripts/records.gd")`, `const Ghost = preload("res://drone/ghost.gd")`; `var _records := {}`, `var ghost: Node3D` (created in `_ready`, named `"Ghost"`, child of `fpv_viewport`, hidden), `var _race_laps := -1`, `var _ghost_course := -2`, `var _ghost_world := ""`; `hud.update_view`'s dictionary gains optional `race_delta: float` (`NAN` or absent: none).

- [ ] **Step 1: Write the failing tests:**
  - `test_hud.gd`, `test_the_delta_rides_the_lap_line`: `update_view` with `race_present`, running, `race_laps: 1`, `race_last_lap_s: 31.2`, `race_best_lap_s: 29.9`, `race_lap_s: 75.3`, `race_delta: -0.84` → `"LAP 2   NEXT START   1:15.3   LAST 31.2   BEST 29.9   -0.8"`; `race_delta: 1.25` → `"…   +1.3"`; absent and `NAN` → today's line unchanged; the delta never shows before the clock runs or after the finish.
  - `e2e_open_loop.gd`, in the race-mode section (lines ~389–404; re-verify at execution): after race mode is switched back on, `_check(not app.ghost.visible, "no record: no ghost")`; in free flight, the same check.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the HUD assertion.
- [ ] **Step 3: Implement.** In `app.gd` `_ready`: `_records = Records.load_records()`, build the ghost beside the drone. In `_process`, after the existing `racing` computation (`app.gd:244`) and before `hud.update_view`:
  - `var capture: Dictionary = client.take_lap_capture()`; when not empty: `_records` offered via `Records.offer_capture(_records, capture["world"], capture["course"], capture["quad"], capture)`; on true — `Records.save_records(_records)`, `hud.add_toast("New best lap %.3f s - the ghost races it" % capture["lap_s"])`, and re-`setup` the ghost from `Records.ghost_of`. (The menu is fed the store in task 6.)
  - Bookkeeping of the last lap: `var laps := int(telemetry.get("race_laps", 0))`; when `laps > _race_laps and laps > 0`: `Records.record_last(_records, world, course, quad, telemetry["race_last_lap_s"])` and save; `world` is `client.get_world().get("name", "")`, `course` `telemetry.get("race_course", "")`, `quad` `client.get_quad_name()`. `_race_laps = laps` (−1 when the telemetry is empty).
  - The ghost's track follows the course: when `race_course_index` or the world name changes, `ghost.setup(Records.ghost_of(_records, world, course, quad))`.
  - `ghost.visible = racing and ghost.has_track()`; when racing and `race_running`: `ghost.transform = ghost.pose_at(float(telemetry.get("race_lap_s", 0.0)))` and `delta = ghost.delta(drone.transform.origin, lap_s)`; when racing and not running: `ghost.transform = ghost.pose_at(0.0)`. When a record best exists and is faster, `telemetry["race_best_lap_s"] = minf(best, float(telemetry.get("race_best_lap_s", best)))` (the free-flight mutation at `app.gd:241` is the precedent). Pass `"race_delta": delta` into the `hud.update_view` dictionary.
  - `hud.gd`: `update_view` reads `var race_delta: float = view.get("race_delta", NAN)` and hands it to `_update_race(t, race_delta)`; the running branch appends `"   %+.1f" % race_delta` only when `is_finite(race_delta)`.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` (WSL; the e2e starts `ofs-sim`) — Expected: PASS, including the two new e2e checks (the e2e's `user://` holds no records file, so the ghost stays hidden).
- [ ] **Step 5: Commit** `feat(godot): the ghost, the records and the live delta in the game (#133)`.

### Task 6: The records line in the main menu

**Files:**
- Modify: `godot/ui/main_menu.gd` (a `_records` Label after the Course row, `set_records`), `godot/scripts/app.gd` (call `main_menu.set_records(_records)` at ready and after every change), `godot/tests/test_main_menu.gd`
- Test: `godot/tests/test_main_menu.gd`

**Interfaces:**
- Consumes: Task 3's `Records.entry`/`best_lap_s` (via the store), `catalog.gd`'s `label` fields (declared names, `catalog.gd:68`).
- Produces: `func set_records(store: Dictionary) -> void` (stores and refreshes; the menu also refreshes on `open()`); the line reads `Best 29.9 s   Last 31.2 s` and is hidden without a record for the chosen world, course and quad; Course `Default` (setting `""`) resolves to the world's first declared course name (`""` when the world declares none — the implicit course).

- [ ] **Step 1: Write the failing tests** in `test_main_menu.gd`:
  - `test_the_records_line_shows_best_and_last` — `setup` with the shipped settings, `set_records` with a store keyed by the chosen world's label, its first course and the quad's label: the line is `"Best 29.9 s   Last 31.2 s"`; moving the quad row to another file with no record hides it.
  - `test_the_default_course_resolves_to_the_first_declared` — a store keyed under `""` only shows for a world without `[[courses]]`; one keyed under the first course name shows for a world with courses while `course` setting is `""`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL.
- [ ] **Step 3: Implement** in `_refresh`: the world label from `_worlds[Catalog.index_of(_worlds, settings["world_path"])]["label"]`, the quad label likewise, the course `settings["course"]` or the first of `_course_names(...)`; `Records.entry` decides visibility and text.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): best and last lap in the main menu (#133)`.

### Task 7: Docs, changelog, roadmap and the gates

**Files:**
- Modify: `docs/race-timing.md` (replace the "Not included. No ghost or best-lap replay." line — 78 — with a pointer; add a "Best laps, ghosts and local records" section after "Courses and race modes"), `docs/roadmap.md` (drop "There is no ghost or best-lap replay." from the race paragraph, ~line 103), `README.md` (the race bullet, ~30), `CHANGELOG.md` ("Unreleased — Best-lap ghost and local records (#133)": Added — the capture, the records file and keys, the ghost, the HUD delta and the menu line; note "no protocol change")

- [ ] **Step 1: Write** the race-timing section in plain words: what is captured (the session's fastest clean lap from the client's state stream; clean = no collision inside the lap), the records file and its keys, the ghost (translucent, races on the lap clock, parks at its finish), the delta's meaning (nearest point on the ghost's line), the surfaces (HUD BEST/delta, menu line), and the limits (60 Hz track, one ghost per record, races only, no export or sharing, the state-stream window is exact to one state period).
- [ ] **Step 2: Update** the roadmap, the README bullet and the changelog; grep `grep -rn "ghost" docs README.md` outside `docs/superpowers` to catch stragglers.
- [ ] **Step 3: Check** every doc snippet's keys and numbers against the code (the dictionary keys, the file name, the 60 Hz).
- [ ] **Step 4: Run** the gates: `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-client -p ofs-godot`, `PYTHONPATH=python python -m pytest python/tests -q`, and WSL `bash scripts/run-godot-tests.sh all` — Expected: PASS, no golden moved (no `ofs-sim` file changed).
- [ ] **Step 5: Commit** `docs: best-lap ghosts and local records (#133)`.

## Manual QA (what no suite covers; land in #163 when the PR closes)

- Fly a lap on `park`'s Diamond with a stored record: the ghost is translucent in the chase camera, and crossing it in the FPV view shows it through the analog chain (noise, breakup — never a clean overlay).
- Beat a stored best: the "New best lap" toast fires once, the ghost becomes the new lap mid-session (it re-appears at the next start), and the menu line shows the new best after returning to the menu.
- Reset (**N**) mid-lap: the ghost snaps back to the start gate and waits; the delta disappears until the clock runs again.

## Ledger

- Nothing yet.
