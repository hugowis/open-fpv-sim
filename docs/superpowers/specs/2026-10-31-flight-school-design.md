# A flight school: interactive flying lessons (#125; epic #135)

Godot client only (`godot/`), plus one shipped quad file and three committed data CSVs. No protocol, no server, no
Rust source change: protocol stays 15, and every pass criterion is computed in GDScript from what the client already
holds — the telemetry dictionary (`crates/ofs-godot/src/lib.rs`, `get_telemetry`, around line 263), the smoothed pose
(`get_pose`, around line 205), and the event stream `app.gd`'s `_on_event` already receives. The brief is issue #125;
no design conversation took place, so every ruling below is the author's call and open to change on review. Anchors
were verified at `main` = 87c5914; re-verify at execution (the B6 sweep has since merged as `b37aae5` and touched nearby files).

## Why

Nobody is born able to fly acro, and this simulator is a hard place to learn: real Betaflight, a degrading analog
picture, no forgiveness. Everything a lesson needs already arrives in the client — the state stream, gate and lap
events, angle mode on aux 2, **R** to reload, race reset — and recorded stick inputs can be replayed. Nothing teaches
it (#125's own words). The menu (#141, f122b83) is where the school's door goes; controller setup (#149, ec8f0b2) is
the screen the first lesson points at.

## Goals

1. A **flight school** screen, opened from the main menu, listing graded lessons; each names its goal and shows live
   progress (hold time, distance drifted, degrees turned, gates passed).
2. **Five lessons** flown in `worlds/circuit.toml`: pre-flight (arm/disarm), lift-off and a held hover, forward
   flight and a coordinated 180° turn, a figure of eight around two pylons, and a first lap of the gate course.
3. Success is **detected from existing state and events**; the lesson state machines are unit-tested in GDScript
   against scripted snapshot sequences (a hover that drifts out of bounds loses its hold; a clean gate pass advances
   the lesson) — the issue's own test requirement.
4. A lesson starts **reset-friendly**: **R** restarts it, `reset_race` (the **N** key path) gives the lap lesson a
   fresh clock, **Esc** exits to the school screen.
5. Optional **"watch it first"**: the maneuver flown by replaying a recorded inputs CSV in the `play_inputs` format
   (`python/ofs/client.py`, around line 565).
6. **Progress persists** across runs: which lessons were passed.

## Non-goals

- Any change to `ofs-sim`, `ofs-proto`, `ofs-client` or `ofs-godot` — no protocol 16 discussion, no new RPC wrappers.
- Any change to `worlds/*.toml` or the physics; no new world for the school (`circuit.toml` "may do", says #125).
- Adaptive coaching, voice, subtitles; multi-lesson scoring curves or medals; leaderboards (ghosts are #133's lane).
- New settings keys, `OFS_*` variables or flags — nothing here is a setting; the chain
  `user://ofs_client.cfg` < `OFS_*` < flags is untouched (menu spec R1/R7, epic #135).
- Recording lessons or exporting lesson results.

## Design calls (decided here; the executor ledgers any change)

**D1. Everything is scored client-side from the existing surface.** The lesson runner reads, per frame: the telemetry
dictionary (`time_s`, `altitude_m`, `speed_mps`, `motors_spinning`, `race_present`, `race_running`, `race_laps`,
`race_next_gate`, `race_gate_count` — all documented on `get_telemetry`), the pose (`get_pose().origin`: Godot frame,
x = east, y = up, z = south, so `north_m = -origin.z`, `east_m = origin.x`; `world.gd`'s `gate_basis` comment names
the mapping), heading derived from the basis (drone forward is −Z, `godot/drone/drone.gd` line 4), and the events
`app.gd` already routes (`gate_passed`, `lap_completed`, `collision`). Horizontal position is *not* in the telemetry
dictionary (only `altitude_m` is, `crates/ofs-client/src/model.rs` around line 471) — `get_pose()` is the position
source, which is why nothing needs the wire.

**D2. Lessons are data; one pure state machine runs them.** `godot/scripts/lessons.gd` holds the lesson definitions
as dictionaries (id, title, goal text, stage list, time limit, race mode, optional demo path, optional pylons).
`godot/scripts/lesson_runner.gd` is a `RefCounted` with **no engine calls** (the `calibration.gd` pattern, controller
setup R3): `setup(lesson)`, `feed(snapshot)` once per frame with a plain dictionary, and `status()` returning
`{state, stage_index, stage_count, text, progress}` where `state` is `running | passed | failed`. Unit tests feed
scripted snapshots — no Godot scene, no extension, no server. The stage kinds are a closed set:

| kind | pass when | progress shown |
|---|---|---|
| `motors` `{spinning}` | `motors_spinning == spinning` | "motors on" / "motors off" |
| `hold` `{radius_m, min_m, max_m, seconds}` | inside radius of the entry anchor and in the altitude band for `seconds` cumulative | held s / target s, drift m, altitude m |
| `reach` `{north_m, east_m, radius_m, min_m, max_m}` | horizontal distance to the point ≤ radius, altitude in band | distance to go m |
| `turn` `{degrees, min_speed_mps, min_m, max_m}` | cumulative absolute heading change ≥ degrees, only counted while speed ≥ min and altitude in band | turned° / target° |
| `figure_eight` `{a, b, max_dist_m, min_m, max_m}` | cumulative bearing around each pylon ≥ 360° with **opposite signs**, within distance and band | around each pylon° |
| `lap` `{}` | `race_laps ≥ 1` | next gate / gate count |

**D3. Failure is soft.** A hold broken by drift or altitude resets that stage's accumulator (the drift distance is
shown), never the lesson. A lesson fails only on: a `collision` event, the lesson's `time_limit_s` running out, or the
quad on the ground (`altitude_m ≤ 0.05`) after a `lift-off`-class stage began. A failed or passed lesson ends its
state machine and the panel says so; **R** (the existing reload, `app.gd` around line 272) restarts the lesson from
its first stage; **Esc** exits. No new keys.

**D4. The lesson list for v1** — five, in order, all in `worlds/circuit.toml` (its four 6 m × 4 m gates on a 40 m
square, `worlds/circuit.toml` from line 25; the field around them is empty — room for hover and turn work):

1. `pre_flight` — motors on, motors off (D6).
2. `hover` — lift off and hold within 2.5 m of the lift-off spot, 1–4 m up, 10 s cumulative.
3. `forward_turn` — reach a point 20 m north (the Gate0 line), then a coordinated 180° turn back at ≥ 1 m/s, 0.5–8 m.
4. `figure_eight` — both pylons looped in opposite senses (markers at 10 m north, ±5 m east; pylons drawn, D8).
5. `circuit` — one full lap: `race_laps ≥ 1` in race mode (`race_start` true for this lesson only).

The figure of eight is scored by accumulating the bearing angle of the quad's position around each marker (unwrapped
per frame, like the turn's heading); two loops around one marker never passes. Demos exist only where a good recording
exists (D9); the button is hidden otherwise.

**D5. Progress lives in `user://flight_school.json`, not in the settings chain.** `{"version": 1, "passed":
{"<lesson id>": true}, "best": {"hover": 12.4, "circuit": 41.7}}` — the `user://controls.json` precedent
(`app.gd` line 11), written whole on change by `godot/scripts/lesson_progress.gd`; an unreadable or invalid file
changes nothing and the school shows every lesson unpassed. It is progress, not a setting: no `[client]` key, no
`OFS_*`, no flag, and nothing the settings screen shows.

**D6. Arming adapts to the flight controller the session actually runs.** Under Betaflight the pre-flight lesson
teaches aux 1 arm / disarm and aux 2 Angle mode (the reference convention, `controls.gd` line 7; the HUD help already
says it, `hud.gd` line 29). Without SITL the session is open loop (there is no arming there), and the same lesson —
same scored state `motors_spinning` — teaches throttle up / throttle cut. The lesson data carries both texts; the
school picks by `MainMenu.effective_open_loop(settings)` (`main_menu.gd` line 105), the same decision Start uses.

**D7. Open-loop lessons fly a shipped school quad.** `quads/school-5f.toml`: a copy of `quads/opendrone-5f-freestyle.toml`
with `[fc] kind = "open_loop_stabilized"` (a valid kind, `crates/ofs-config/src/lib.rs` around line 343; the sitl-only
`launch`/`betaflight_diff` checks do not apply, around line 1396), provenance comments marking every parameter as
copied from the 5F freestyle. `docs/scripted-flight.md` (line 91) already promises that such a file flies stabilized
in Godot: angle-mode-like sticks without Betaflight, so a default install — no SITL launch — can pass every lesson,
and the e2e suite can drive one open loop. Betaflight sessions fly the pilot's chosen quad. Loading the school quad takes one care: the server refuses an `open_loop_fc` override on an `open_loop_stabilized` quad (`crates/ofs-sim/src/server.rs:306-312` — "load with stabilize = true, or without the open_loop_fc override"), and the Godot bridge carries no `stabilize` key, so a lesson's choices always send `open_loop: false` when they name the school quad — the file's own stabilized kind flies; an inherited `open_loop: true` (an `OFS_OPEN_LOOP` boot, an open-loop menu pick) would be refused at Load. The file joins the menu's
quad list (honest data, the catalog lists every `*.toml`) and `every_shipped_quad_loads_and_validates`
(`crates/ofs-config/tests/quads.rs` line 18) — it must load and validate, which a copy of a shipped quad does.

**D8. Lesson sessions are menu starts with choices, and they draw their own pylons.** Starting a lesson calls the
existing `_start_session(choices)` (`app.gd` line 113) with `{world_path: worlds/circuit.toml, quad_path: the school
quad when open loop, open_loop: false with it (D7), race_start: only the circuit lesson}` — choices merge into the session copy and are never saved
to the cfg (`_on_menu_start` already works this way, line 145). The figure-eight pylons are two
`MeshInstance3D` cylinders the app adds to the world node (named `Lesson_Pylon_A`/`Lesson_Pylon_B`, the world.gd
`_add_built` idiom) and removes when the lesson ends; no world file changes. Free flight's race-hiding (line 241) is
untouched: the runner reads `client.get_telemetry()` directly, not the HUD's filtered view, so gate state reaches the
lesson even when the HUD line is hidden.

**D9. "Watch it first" replays a CSV client-side.** `godot/scripts/demo_player.gd` parses an inputs CSV in the
server's `play_inputs` format (`t_s` plus any of roll/pitch/yaw/throttle/aux1..aux4; rows hold until the next;
`tx`/`link_loss` columns ignored) and, during a demo, the app writes the row for the current `time_s` into its
existing `sticks_override` slot (`app.gd` line 47 — the same path the e2e suite flies with, `e2e_open_loop.gd` line
98), so the pilot's controller is simply not read: no bridge change, no `PlayInputs` exposure, nothing new on the
wire. The demo is paced by telemetry time (it pauses with **P**), ends holding its last row, and the panel says
"your turn — R to fly it". The CSVs are committed under `godot/lessons/`, recorded once by a small Python driver
(D10); replayed sticks in a real-time session will not reproduce the lockstep recording bit-exactly, and need not —
a demo is a visual aid, not a ghost. Demos ship for `hover`, `forward_turn` and `circuit` (the circuit pilot already
exists, `python/examples/circuit.py`); a demo of arm/disarm teaches nothing and a figure-of-eight recording is the
hardest to author — both stay out until someone records them (the data's optional `demo` key decides).

**D10. The recordings are made by a Python example script.** `python/examples/flight_school_demos.py` flies each
demonstrated maneuver on the school quad (`ofs.launch()`, `load("quads/school-5f.toml", world="worlds/circuit.toml")`)
with the existing patterns — an altitude loop for the hover (`python/examples/hover.py`'s PID), waypoints for the
forward-and-turn (the `circuit.py` pilot's), `circuit.fly_circuit` for the lap — wrapped in
`record_inputs()` / `stop_inputs_recording()` (`python/ofs/client.py` around line 584), writing the CSVs to a given
directory. It is run by hand when a lesson's numbers change; the committed CSVs are data, guarded by a small pytest
that parses them (header, increasing `t_s`, a sane duration), the way `flips.csv` is committed data today.

**D11. The school screen and the in-flight panel.** `godot/ui/flight_school.gd` (CanvasLayer over the menu, opened by
a **Flight school** button on the main menu between Start and Controls — the `controls_requested` pattern,
`main_menu.gd` line 58): the lesson list with passed marks from D5, the highlighted lesson's goal text, **Start**,
**watch it first** where a demo exists, **Back**. `godot/ui/lesson_panel.gd` (a CanvasLayer visible only during a
lesson): lesson title, goal, the current stage's instruction, the progress numbers from `status()`, the pass/fail
line, and the R/Esc hints. **Esc** ends the lesson (stopping the session, `_show_menu`, `app.gd` line 134) and
reopens the school screen over the menu; the F1 help gains a "Flight school is in the main menu" line. The e2e
menu-less path (`OFS_MENU=off`, set by every e2e suite and `shots.gd`) never sees any of this.

**D12. Not in v1:** per-lesson medal times, lesson chaining (a curriculum that unlocks), a demo of every lesson,
in-lesson stick-overlay coaching (the HUD's sticks view already shows the inputs), scoring under `race_reset`
semantics other than the lap lesson's fresh **N**, and any lesson-authoring UI beyond editing `lessons.gd`.

## What is left to the builder

Panel and screen layout (a dim panel like the calibration wizard's), wording of the instruction lines, the pylons'
look, the hover band and radius numbers if playtesting says 2.5 m / 1–4 m / 10 s is the wrong difficulty.

## Open questions for the maintainer

1. **Demo replay, client-side or through the bridge?** D9 rules the GDScript replay (zero Rust). The alternative is
   exposing the server's existing `PlayInputs`/`StopInputs` RPCs through `ofs-client` + `ofs-godot` — deterministic,
   and `set_sticks` is refused while it plays — but it grows the Rust surface of a "mostly Godot" epic. Reversible
   either way; the demo player's CSV parsing and pacing would survive the swap.
2. **The school quad as a shipped `quads/` file** (D7): it will appear in the menu's quad list and in
   `every_shipped_quad_loads_and_validates`. Acceptable, or should it hide from the catalog (a filter in
   `catalog.gd`)?
3. **The figure of eight in v1** (D4): it is in #125's done-when, but it is the most complex scorer and the only
   lesson that draws new things into the world. Keep it, or defer to a follow-up and ship four lessons?

## Checked by eye (headless Godot renders nothing)

The school screen's look and focus order; the panel during a real hover (do the progress numbers read as teaching?);
the pylons against the circuit field; a demo flown start to finish on a real display; a Betaflight run of the
pre-flight lesson (arm on a real switch); that the hover's 2.5 m radius and 10 s feel learnable, not punishing.
