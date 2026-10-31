# The First-Run Tour Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On first run the client offers an interactive tour of itself — ten steps anchored to screens that exist, waiting where practical for the pilot to do the thing — skippable, replayable from the menu and a flight, offered exactly once. Closes #124 (epic #135).

**Architecture:** A pure, engine-free step machine `godot/scripts/tour.gd` (the `calibration.gd` precedent) fed one state dictionary per frame; a card overlay `godot/ui/tour_overlay.gd` on a new CanvasLayer 8 that owns the buttons and polls a state-provider Callable; `app.gd` offers the tour once at the menu's first boot, persists `offered/skipped/done` in `user://tour.json` (its own file, never `ofs_client.cfg`), and replays it from a new menu button and the T key. Key names come from one shared list `godot/scripts/keys.gd` that F1's help text is also built from. Nothing is paused, no key is intercepted, no setting is added.

**Tech Stack:** GDScript (Godot 4.7.2), headless-tested through `scripts/run-godot-tests.sh`.

**Spec:** `docs/superpowers/specs/2026-10-31-first-run-tour-design.md` (the binding rulings D1–D9); the brief is issue #124, the epic contract issue #135.

## Global Constraints

- Client only: nothing outside `godot/`, `README.md` and `CHANGELOG.md` changes; no Rust, Python, proto or world file (epic #135: presentation, onboarding, persistence in the client). Do not run cargo or pytest.
- The settings chain is untouched: `godot/scripts/settings.gd` (its `ENV_NAMES`/`FLAG_NAMES`), `user://ofs_client.cfg` and the flags gain nothing; the tour's state lives only in `user://tour.json` (spec D2, D8). Re-verify at execution.
- The e2e suites' menu-less path keeps working: `OFS_MENU=off` is set by `godot/tests/e2e_open_loop.gd:84`, `e2e_errors.gd:39`, `e2e_betaflight.gd:55` and `shots.gd:40` (re-verify the four), and the tour offers only in the `settings["menu"]` branch of `app.gd._ready` — never on a later `main_menu.open()`.
- Godot tests in WSL: `bash scripts/run-godot-tests.sh unit` for the suites (it downloads Godot 4.7.2 and builds `-p ofs-sim -p ofs-godot`; `OFS_GODOT_DL=/mnt/c/dev/open-fpv-sim/build/godot-dl` reuses the download), `bash scripts/run-godot-tests.sh all` once the e2e guards land. Read the WHOLE output: a SCRIPT ERROR still ends in `0 failed`.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: Write/Edit for files; multi-line WSL commands go through a scratchpad script file run with `wsl -d Ubuntu -- bash <path>`.
- Files stay LF: `tr -cd '\r' < file | wc -c` prints 0 for every file touched.
- Commit locally after each task with the exact message in its last step; no trailer prescribed (recent merged work carries none — an executing session that adds one names its own identity). Never push, never open a PR.

## Design calls (decided in the spec; the executor ledgers any change)

1. **The machine is pure and state-fed** (D1): `scripts/tour.gd`, no engine calls; a step completes on a level key, a changed key or the button; Next always advances, so nothing can stick.
2. **First run is the menu's first boot** (D2): the offer fires in `app.gd._ready` only in the `settings["menu"]` branch and only while `user://tour.json` is absent/unseen; the file is written "offered" the moment the card shows.
3. **Ten steps, two phases** (D3): menu welcome/world/quad/controls/start, flight arm/camera/dvr/link_cut/finish, each waiting on state that exists today (`world_path`, `controls_menu.is_menu_mode()`, phase flying, `motors_spinning`, `chase_view`, `dvr.recording`, `radio_cut`).
4. **Overlay, never a pause** (D4): CanvasLayer 8 card, top-centre; the sim stays live; steps observe, they never intercept.
5. **Skip and replay** (D5): Skip on every card; Esc skips only while no session flies (in flight it keeps returning to the menu, which ends the tour); replay from the menu's Tour button (step 0) and the T key in a flight (step 5); every end writes the file — done or skipped.
6. **One key list** (D6): `scripts/keys.gd` `HINTS`; F1's help text and the tour's step texts both format from it; a test pins the overlap.
7. **Self-healing boundaries** (D7): a menu-phase tour jumps to step 5 when a session flies; a flight-phase tour ends skipped when the flight ends; an unaccepted offer is declined by a session starting.
8. **`user://tour.json`** (D8): `{"version": 1, "state": "offered"|"skipped"|"done"}`; the machine serialises, the app does the FileAccess; garbage means unseen (warn, offer, overwrite).

## Review Focus

- A crash between the card showing and any pilot choice must not re-offer: the file is written with the offer, and a later manual menu open never offers (tasks 2, 4, 5).
- A locked World row (`--world` on the command line) or a quad-less install must not stick the tour: every step advances on Next (task 2).
- The pilot pressing Start flying mid-menu-tour, or Esc mid-flight-tour: the jump and abandon rules fire, no orphan card (task 2).
- Esc in a flight must keep its own meaning — back to the menu — not become the tour's skip (tasks 3, 4).
- Renaming a key in `keys.gd` must move F1 and every tour text together; a step hardcoding "C" fails the drift test (tasks 1, 2).
- The automated boots (`e2e_*`, `shots.gd`) see no card, and the e2e leaves no `user://tour.json` behind on the developer's machine (task 5).

---

### Task 1: One key list for F1 and the tour

**Files:**
- Create: `godot/scripts/keys.gd`
- Modify: `godot/ui/hud.gd` (`HELP_LINES` at lines 18–30 and its use at line 84; re-verify), `godot/tests/test_hud.gd`
- Test: `godot/tests/test_hud.gd`

**Interfaces:**
- Produces: `godot/scripts/keys.gd` (`extends RefCounted`, like `settings.gd`) with `const HINTS := {"pause": "P", "reload": "R", "camera": "C", "dvr": "D", "link_cut": "K", "sticks": "I", "race": "N", "next_course": "Shift+N", "hud": "H", "help": "F1", "controls": "F2", "fullscreen": "F11", "menu": "Esc", "tour": "T"}` and nothing else.
- Produces on `hud.gd`: `const HELP_LINES` is replaced by `const HELP_LINES` entries that carry `{action}` placeholders plus `static func help_text() -> String` returning `"\n".join(HELP_LINES.map(func(l): return (l as String).format(Keys.HINTS)))`; `_ready` sets `_help.text = help_text()`. The visible text is today's word for word (verified at `hud.gd:18-30`), with `{tour}` inserted into the F1 line: `"{help} this help        {controls} controls setup        {tour} replay the tour        {fullscreen} fullscreen        {menu} back to the menu"`.

- [ ] **Step 1: Write the failing tests** in `test_hud.gd`:
  - `every_key_hint_appears_in_the_help_text`: for every value of `Keys.HINTS`, `hud.help_text().contains(value)` (the new `"T"` included).
  - `the_help_gained_the_tour_line`: `hud.help_text().contains("T replay the tour")` and the F1 line still names F2, F11 and Esc.
  - `the_help_text_is_what_it_was_plus_the_tour_line`: the help text's lines equal today's eleven lines with only the F1 line changed (paste the expected lines into the test; this pins the no-drift promise for everything except the T insertion).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL (`keys.gd` missing / no `help_text`).
- [ ] **Step 3: Implement** `keys.gd` and the `hud.gd` change above (`const Keys = preload("res://scripts/keys.gd")`; the placeholder lines keep today's wording exactly, only the F1 line grows `{tour} replay the tour`).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS (nothing else consumes `HELP_LINES`; verified: only `hud.gd:84`).
- [ ] **Step 5: Commit** `refactor(godot): one key list for F1 and the tour (#124)`.

### Task 2: The step machine and its state file

**Files:**
- Create: `godot/scripts/tour.gd`, `godot/tests/test_tour.gd`
- Modify: `godot/tests/run_tests.gd` (add `"res://tests/test_tour.gd"` to `SUITES`, after `test_settings.gd`)
- Test: `godot/tests/test_tour.gd`

**Interfaces:**
- Consumes: Task 1's `Keys.HINTS`.
- Produces (`extends RefCounted`, no engine calls):
  - `signal finished(outcome: String)` (`"done"` or `"skipped"`, emitted once per run) and `signal step_changed`.
  - `static func first_flight_step() -> int` (5), `static func step_count() -> int` (10), `static func step_id(index: int) -> String`, `static func step_text(index: int) -> String` (the step's text with `Keys.HINTS` formatted in).
  - `var offered := false`, `var active := false`, `var persisted := "unseen"` (`"unseen"|"offered"|"skipped"|"done"`).
  - `func offer() -> void` (step 0, `offered = true`, `active = false`, `persisted = "offered"`), `func feed(state: Dictionary) -> void`, `func advance() -> void` (on step 0 it also sets `active = true`; on the last step it finishes `"done"`), `func skip() -> void`, `func resume(index: int) -> void` (`offered = active = true`, fresh entry snapshot, outcome cleared), `func current() -> int`, `func outcome() -> String` ("" while running), `func to_json() -> String`, `func from_json(text: String) -> bool`.
  - The steps (const `STEPS`, each `{"id", "phase": "menu"|"flight", "wait" or "wait_change", "text"}`); texts format from `Keys.HINTS`:

    | id | phase | completes on | text (exact) |
    |---|---|---|---|
    | welcome | menu | next | Welcome to Open FPV Sim. This short tour shows the client, not how to fly: the menu, a flight and the tools in between. Each step waits while you try the thing; Skip leaves any time. |
    | world | menu | change `world_path` | The World row cycles the worlds on disk - fields, circuits, hills. Turn it, or press Next. |
    | quad | menu | change `quad_path` | The Quad row picks the quad. The files come from the quads folder; each one flies differently. |
    | controls | menu | level `controls_from_menu` | Controllers live on one screen: press Controls for bindings, calibration and profiles, then {menu} back to the menu. |
    | start | menu | level `flying` | Press Start flying. The tour continues in the flight. |
    | arm | flight | level `motors_spinning` | Arm with the aux 1 switch and raise the throttle gently. The HUD's lines - link, battery, altitude - are the goggles' instruments. |
    | camera | flight | level `chase` | Two cameras: {camera} switches the FPV view and the chase camera. Press {camera}, then {camera} again to come back. |
    | dvr | flight | level `dvr` | {dvr} records the goggles picture to a DVR file ({camera} first if you are still in the chase camera). Press {dvr}, then {dvr} again to stop. |
    | link_cut | flight | level `link_cut` | {link_cut} cuts the radio link to practise the failsafe; {link_cut} again restores it. Press {link_cut}. |
    | finish | flight | next | That is the client. {help} lists every key, {race} starts a race, {sticks} records the sticks, {menu} returns to the menu - and {tour} replays this tour. Fly! |

  - `feed` consumes exactly these state keys: `flying`, `controls_from_menu`, `motors_spinning`, `chase`, `dvr`, `link_cut` (booleans) and `world_path`, `quad_path` (strings). Rules, in order: nothing after an outcome; an inactive offer skips the moment `flying` is true (D7); an active menu-phase step below 5 jumps to `first_flight_step()` when `flying` is true; an active flight-phase step skips when `flying` is false; otherwise the current step's wait is evaluated (a `wait_change` key against a snapshot taken when the step was entered).
- [ ] **Step 1: Write the failing tests** in `test_tour.gd` (machine only; no nodes):
  - `the_steps_are_ten_in_two_phases`: ids in order; `step_count() == 10`; `first_flight_step() == 5`; steps 0–4 phase "menu", 5–9 "flight".
  - `the_offer_writes_offered_and_a_session_declines_it`: `offer()` sets `offered`, not `active`, `persisted == "offered"`; `feed({"flying": true})` → `outcome() == "skipped"`, `persisted == "skipped"`.
  - `next_always_advances_and_the_last_step_finishes_done`: from `offer()`, nine `advance()` calls end in `outcome() == "done"` and `persisted == "done"`; a tenth changes nothing and `finished` fired exactly once.
  - `a_change_step_completes_only_when_its_value_moves`: at the world step, feeding the entry `world_path` again stays; a different value advances; the quad step behaves the same.
  - `level_steps_complete_on_their_flag`: each of `controls_from_menu`, `flying` (step 4), `motors_spinning`, `chase`, `dvr`, `link_cut` advances its step when true and not when false.
  - `starting_a_session_jumps_to_the_flight_steps`: `offer()`, `advance()` once (world), `feed({"flying": true})` → `current() == 5`.
  - `the_flight_phase_ends_skipped_when_the_flight_ends`: `resume(6)`, `feed({"flying": true})` stays, `feed({"flying": false})` → `"skipped"`.
  - `resume_restarts_with_a_fresh_snapshot`: `resume(1)` + a changed `world_path` advances; `resume(1)` again with the unchanged value does not.
  - `the_state_file_round_trips`: `to_json`/`from_json` for each of offered/skipped/done; `from_json("not json")` and `from_json("{\"version\":1,\"state\":\"nonsense\"}")` return false and change nothing.
  - `step_texts_name_the_keys_from_the_shared_list`: `step_text(3)` contains `Keys.HINTS["menu"]`; 6 contains `["camera"]`; 7 contains `["dvr"]` and `["camera"]`; 8 contains `["link_cut"]`; 9 contains `["help"]`, `["race"]`, `["sticks"]`, `["menu"]` and `["tour"]`.
  - `feed_after_the_end_is_ignored`: after `skip()`, `feed` and `advance` change nothing.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (parse error: `tour.gd` missing).
- [ ] **Step 3: Implement** `tour.gd` per the Interfaces (a `_entry: Dictionary` snapshot taken in `offer`, `resume` and whenever a step is entered; `step_text` returns `(STEPS[i]["text"] as String).format(Keys.HINTS)`).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the tour step machine (#124)`.

### Task 3: The card

**Files:**
- Create: `godot/ui/tour_overlay.gd`, `godot/tests/test_tour_overlay.gd`
- Modify: `godot/tests/run_tests.gd` (add `"res://tests/test_tour_overlay.gd"` after `test_tour.gd`)
- Test: `godot/tests/test_tour_overlay.gd`

**Interfaces:**
- Consumes: Task 2's machine.
- Produces (`extends CanvasLayer`; built in code like `calibration_wizard.gd` — a dim `PanelContainer`, top-centre anchors, a text `Label`, a counter `Label`, `_next: Button`, `_skip: Button`):
  - `signal advanced`, `signal skipped`; `var machine` (set by `setup(machine)`; the overlay connects `step_changed` → refresh, `finished` → hide), `var state_provider: Callable` (stands in for the app's `_tour_state`; the tests install a fake).
  - `func setup(machine) -> void`, `func open_offer() -> void` (visible at the machine's current step), `func open_at(index: int) -> void` (`machine.resume(index)` then show), `func close() -> void`, `func is_open() -> bool`, `func step_text() -> String` and `func counter_text() -> String` (`"3/10"` for index 2; for the tests).
  - `_process(delta)`: while visible and `machine != null`, `machine.feed(state_provider.call())`; refresh the card when the machine says the step changed.
  - Button labels: `_next.text` is "Start the tour" on step 0, "Finish" on step 9, "Next" between; `_skip.text` is "Skip"; `_next` grabs focus when a step shows (gamepad).
  - `_input(event)`: `ui_cancel` while visible and the last fed state's `flying` is false → `skipped.emit()`, consume the event; while a session flies, Esc falls through untouched (D5).

- [ ] **Step 1: Write the failing tests** in `test_tour_overlay.gd` (a real `Tour.new()` machine, `await add_to_tree(card)` per the `testing.gd` pattern):
  - `the_card_shows_the_step_the_counter_and_the_right_button`: `open_at(0)` → `step_text() == Tour.step_text(0)`, `counter_text() == "1/10"`, `_next.text == "Start the tour"`, `_skip.text == "Skip"`.
  - `the_buttons_signal`: `_next.pressed.emit()` → `advanced` once; `_skip.pressed.emit()` → `skipped` once.
  - `feeding_state_advances_the_card`: `open_at(1)` with a provider returning the entry `world_path`; one `_process(0.016)` keeps step 1; the provider's `world_path` changed and another `_process` shows step 2 and `"3/10"`.
  - `escape_skips_only_while_no_session_is_flying`: `_input` an Esc `InputEventKey` with the last state `flying: false` → `skipped` emitted; with `flying: true` → nothing emitted (Esc is the menu key's, not the tour's).
  - `finishing_hides_the_card`: advance the machine to its end via the provider and the button → the card is not visible.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (file missing).
- [ ] **Step 3: Implement** `tour_overlay.gd` (the `FpvBox`-style full-rect mouse-transparent root is not needed — the card is a small panel; set `layer = 8` in code as a default so the node works even before `main.tscn` sets it).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the tour card (#124)`.

### Task 4: The offer, the replays and the skip in the app

**Files:**
- Modify: `godot/scripts/app.gd` (the `_ready` menu branch around lines 106–109, `_unhandled_key_input` around lines 261–304, a new `_tour_state`/`_tour_save`/`_on_tour_finished`/`_on_menu_tour`; re-verify line anchors), `godot/ui/main_menu.gd` (the buttons block around lines 56–65 and the `row()` map at 110–112), `godot/main.tscn` (a `[node name="Tour" type="CanvasLayer" layer=8]` with the overlay script, after `SettingsScreen`), `godot/tests/test_main_menu.gd`
- Test: `godot/tests/test_main_menu.gd`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces on `app.gd`: `const Tour = preload("res://scripts/tour.gd")`, `const TOUR_PATH := "user://tour.json"`, `var tour := Tour.new()`, `@onready var tour_overlay: CanvasLayer = $Tour`; in `_ready`, before the menu branch, load the file (`tour.from_json(FileAccess.get_file_as_string(TOUR_PATH))` when it exists; on false, `push_warning` and leave `persisted == "unseen"`); in the `settings["menu"]` branch, after `_show_menu()`: `if tour.persisted == "unseen": tour.offer(); tour_overlay.open_offer(); _tour_save()`. Wiring: `tour_overlay.setup(tour)`, `tour_overlay.state_provider = _tour_state`, `tour_overlay.advanced.connect(func() -> void: tour.advance())`, `tour_overlay.skipped.connect(tour.skip)`, `tour.finished.connect(_on_tour_finished)` (save the file, toast "Tour finished - T replays it" on done only). `KEY_T` in the flight `match` arm: `if not tour_overlay.is_open(): tour_overlay.open_at(Tour.first_flight_step())`. `main_menu.tour_requested.connect(_on_menu_tour)` (`open_at(0)` when closed).
  `func _tour_state() -> Dictionary` (the provider; `client.get_telemetry()` empty before a session):
```gdscript
func _tour_state() -> Dictionary:
	var t: Dictionary = client.get_telemetry() if client != null else {}
	return {
		"flying": client != null and client.get_phase() == "flying",
		"controls_from_menu": controls_menu.is_menu_mode(),
		"motors_spinning": bool(t.get("motors_spinning", false)),
		"chase": chase_view, "dvr": dvr != null and dvr.recording, "link_cut": radio_cut,
		"world_path": settings.get("world_path", ""), "quad_path": settings.get("quad_path", ""),
	}
```
  `func _tour_save() -> void`: `FileAccess.open(TOUR_PATH, FileAccess.WRITE)`; on null, `hud.add_toast("Could not save the tour state to %s" % TOUR_PATH, "warn")`; else `store_string(tour.to_json())`.
- Produces on `main_menu.gd`: `signal tour_requested`, a `_tour` Button "Tour" in the box between the Controls and Settings buttons, and `"tour": _tour` in `row()`.
- Note: `app.gd`'s `_show_menu()` disables the app's `_process` (line 139) — that is why the card polls its own provider, not the app's frame.

- [ ] **Step 1: Write the failing test** in `test_main_menu.gd`: `the_menu_has_a_tour_button` — `menu.row("tour").text == "Tour"`, it sits between Controls and Settings in the box's child order, and `row("tour").pressed.emit()` records `["tour"]` (wire `tour_requested` into the existing `events` list the way `controls_requested` is).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — Expected: FAIL (no tour row).
- [ ] **Step 3: Implement** the `main_menu.gd` button, the `main.tscn` node (`[ext_resource type="Script" path="res://ui/tour_overlay.gd" id="15"]` and the node; layer 8, above `SettingsScreen`'s 7) and the `app.gd` wiring above. The Esc chain in `_unhandled_key_input` needs no change: the card consumes Esc itself only while no session flies (Task 3), and in flight Esc returns to the menu, which ends the tour through the machine's flight-end rule.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh unit` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(godot): the first-run offer, the replays and the skip (#124)`.

### Task 5: The e2e guards

**Files:**
- Modify: `godot/tests/e2e_open_loop.gd` (boot checks after line 92, the F1/F2 block after line 185, the menu return after line 371; re-verify anchors)
- Test: `godot/tests/e2e_open_loop.gd` (runs with a real `ofs-sim`)

**Interfaces:**
- Consumes: Task 4's `app.tour_overlay`, `app.tour`, `Tour.first_flight_step()` (add `const Tour = preload("res://scripts/tour.gd")` beside the suite's existing `Catalog` preload).

- [ ] **Step 1: Add the checks** (the suite boots under `OFS_MENU=off`, set at its line 84):
  - after the "reaches the flying phase" check: `_check(not app.tour_overlay.is_open(), "the menu-less boot offers no tour")`;
  - after the F1/F2 block: `_press(app, KEY_T)`; `_check(app.tour_overlay.is_open() and app.tour.current() == Tour.first_flight_step(), "T replays the tour at the arm step")`; `app.tour_overlay._skip.pressed.emit()`; `_check(not app.tour_overlay.is_open(), "Skip ends it")`; then remove `user://tour.json` if it exists (`DirAccess.remove_absolute`) so the run leaves no tour state on the machine (the T replay writes it);
  - after the Esc-to-menu check near the end: `_check(not app.tour_overlay.is_open(), "opening the menu later does not re-offer the tour")`.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh all` (WSL; builds the server and the extension) — Expected: PASS (the three new checks included).
- [ ] **Step 3: Run** `bash scripts/run-godot-tests.sh errors` — Expected: PASS (`e2e_errors.gd` boots menu-less too and must stay green untouched).
- [ ] **Step 4: Commit** `test(godot): the tour stays out of the automated launches (#124)`.

### Task 6: Docs, changelog and the by-hand list

**Files:**
- Modify: `README.md` (the keys sentence at lines 116–119 gains "**T** replays the tour"; the menu paragraph at 108–113 gains the first-run offer and the replays), `CHANGELOG.md` (a new `### A first-run tour (#124)` group under `## Unreleased`, Added: the offer, the ten steps, skip and the two replays, `user://tour.json`; the F1 T line)
- Test: none (prose).

- [ ] **Step 1: Update** the README and the changelog as above; check every claim against the code (key names from `keys.gd`, the file name, the replay entry points).
- [ ] **Step 2: Check** `grep -rn "tour" README.md CHANGELOG.md docs/roadmap.md` — the roadmap has no tour entry to remove (verified 2026-10-31); nothing else names the tour.
- [ ] **Step 3: Run** `bash scripts/run-godot-tests.sh unit` once more — Expected: PASS (docs touch nothing, the run proves the tree is whole).
- [ ] **Step 4: Commit** `docs: the first-run tour (#124)`.
- [ ] **Step 5: When the PR closes**, post the spec's "Checked by eye" list to #163 as a comment (the umbrella's manual-QA rule) and tick #135's #124 checkbox by the close.

## Ledger

- Empty. Ruled 2026-10-31 against `main` at `87c5914`; every anchor above re-verified in the worktree at planning time — the executor re-verifies line anchors before editing (the B6 lesson).
