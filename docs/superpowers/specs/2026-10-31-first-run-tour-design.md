# A first-run tour: how to use the client (#124; epic #135)

Godot client only (`godot/`); no protocol, server, Rust, Python, world or quad change. Epic #135's rules bind:
presentation, onboarding and persistence in the client, the settings chain (`user://ofs_client.cfg` < `OFS_*` <
flags) not forked, the e2e suites' menu-less path untouched. The brief is issue #124; no design conversation took
place, so every ruling below is the author's call and open to change on review.

## Why

F1 lists the keys but nothing walks a new pilot through what the client *is*: a menu that picks the flight, two
cameras, an HUD, a controls screen, a DVR, race mode, a stick recorder, a link-cut key for failsafe tests. The
issue is the tutorial on using the simulator; teaching the flying is #125's job.

## Goals

1. On first run the client **offers** an interactive tour; each step points at a real control or screen and, where
   practical, waits for the pilot to do the thing (cycle the World row, open the Controls screen, arm, press **C**,
   start the DVR, cut the link).
2. The tour can be **skipped** at any moment and **restarted** from the main menu and from a flight; it is offered
   **once**, its state persisted under `user://`.
3. F1 stays the reference: the tour's key names and F1's come from **one shared list**, so the two cannot drift.
4. The step machine (advance, skip, resume) is unit-tested in GDScript; the e2e path never sees the tour.

## Non-goals

- Teaching how to fly (#125 flight school): no lessons, no scripted flights, no pass/fail.
- Any new setting, `OFS_*` variable or flag; any tour state in `ofs_client.cfg` (D2, D8).
- Pausing or gating the simulator; intercepting or remapping any existing key (D4).
- Highlight boxes or arrows anchored to individual widgets — steps name their control, the card is one fixed
  panel (D4); per-step card geometry is out.
- Translation; audio; a second tour for new features in later updates (D8 — see Open questions).

## Design calls

**D1. The tour is a pure, state-fed step machine** — `godot/scripts/tour.gd`, `RefCounted`, no engine calls (the
`scripts/calibration.gd` precedent), so the headless tests drive it with scripted state. The owner feeds it one
state dictionary per frame; a step declares how it completes: *level* (a state key that must become true), *change*
(a state key whose value must differ from its value when the step was entered) or *next* (the card's button only).
`advance()` (the button), `skip()`, `resume(index)` and a `finished(outcome)` signal are the whole control surface.
A step never blocks: **Next always advances**, so a locked row or a missing controller can never stick the tour.

**D2. First run = the menu's first boot, and the state lives in its own file.** The offer is made in `app.gd`'s
`_ready`, in the `settings["menu"]` branch only (right after `_show_menu()`), when `user://tour.json` does not say
the tour was already offered, skipped or done — and the file is written ("offered") the moment the card shows, so a
crash mid-offer cannot re-offer. The offer never fires on a later `main_menu.open()` and never under `OFS_MENU=off`,
which `e2e_open_loop.gd`, `e2e_errors.gd`, `e2e_betaflight.gd` and `shots.gd` all set — that is the whole e2e guard,
so no new setting, env var or flag exists (re-verify at execution that `settings.gd` is untouched).

**D3. Ten steps in two phases, every anchor a screen that exists today.**

| # | id | phase | completes when | points at |
|---|----|-------|----------------|-----------|
| 0 | welcome | menu | Start the tour (the offer card) | the main menu |
| 1 | world | menu | `world_path` changed | the menu's World row |
| 2 | quad | menu | `quad_path` changed | the menu's Quad row |
| 3 | controls | menu | the controls screen opened from the menu | the Controls button / F2 screen |
| 4 | start | menu | the phase is flying | the Start flying button |
| 5 | arm | flight | telemetry `motors_spinning` | the aux 1 switch, the HUD's lines |
| 6 | camera | flight | the chase view is on | the **C** key (both cameras) |
| 7 | dvr | flight | the DVR is recording | the **D** key |
| 8 | link_cut | flight | the radio is cut | the **K** key (failsafe test) |
| 9 | finish | flight | Finish (button only) | F1 (**N** race, **I** stick recorder, **Esc** menu, **T** replay) |

Race mode and the stick recorder are named in the finish step rather than given interactive steps: their triggers
would need a gated world and a CSV cleanup, for little teaching. The HUD is named in the arm step.

**D4. An overlay over a live client, never a pause.** The card is one `PanelContainer`, top-centre, on a new
`CanvasLayer` "Tour" at layer 8 in `main.tscn` (Hud 4, ControlsMenu 5, MainMenu 6, SettingsScreen 7 — so it sits
above every screen it teaches). The sim stays live and every key keeps its meaning: steps complete by *observing*
existing state (`chase_view`, `dvr.recording`, `radio_cut`, `controls_menu.is_menu_mode()`, the telemetry's
`motors_spinning`), never by intercepting input. Nothing is paused; a pilot who wants stillness presses **P**.

**D5. Skip and replay.** Every card carries Skip; Esc declines the offer and skips the tour while the menu phase
(the card consumes `ui_cancel` only when no session is flying); in a flight Esc keeps its own meaning — returning
to the menu ends the tour as skipped (D7). Replay: the main menu gains a **Tour** button (restart at step 0) and a
flight gains the **T** key (resume at step 5, `first_flight_step()` — the menu steps would be noise mid-flight),
advertised by a new F1 line "T replay the tour". Every end writes the file: the last step's Finish writes "done",
every other end writes "skipped".

**D6. One key list.** `godot/scripts/keys.gd` owns `const HINTS` — action name to key text: `pause P, reload R,
camera C, dvr D, link_cut K, sticks I, race N, next_course Shift+N, hud H, help F1, controls F2, fullscreen F11,
menu Esc, tour T`. `hud.gd` builds its help text from it (today's `HELP_LINES` wording, plus the T line) and the
tour's step texts interpolate the same values; a test asserts every `HINTS` value appears in the help text and
that the tour steps carry `HINTS`' values, so renaming a key moves F1 and the tour together.

**D7. The machine self-heals at the phase boundaries.** While active in the menu phase, a session that starts
flying jumps the machine straight to step 5 (the pilot pressed Start flying early; steps 1–4 are moot). While
active in the flight phase, the flight ending (Esc, a failure) finishes the tour as skipped. An offer that was
never accepted is declined the moment a session starts flying — the pilot flew past it, and the file says
"skipped", not "offered"-forever.

**D8. Storage.** `user://tour.json`, `{"version": 1, "state": "offered" | "skipped" | "done"}` — the
`user://controls.json` precedent: the machine exposes `to_json()`/`from_json()`, the app does the `FileAccess`
(the machine stays engine-free). No file (or garbage in it) means unseen: warn, offer, overwrite. The tour's state
never enters `ofs_client.cfg`, and nothing about the tour is a setting.

**D9. The card.** Step text, a "3/10" counter, and two buttons: Next (labelled "Start the tour" on step 0,
"Finish" on step 9) and Skip. The Next button takes focus when a step shows, so the gamepad flies the tour. The
overlay polls a state-provider `Callable` installed by `app.gd` (the `controls_menu._process` precedent —
`app._process` is disabled while the menu is up, so the tour cannot be fed from there).

## What is left to the builder

The card's look (the F2 panel's dim style), the exact wording beyond the ruled facts, the card's size and margin.

## Checked by eye (headless Godot renders nothing) — for #163

A clean `user://` (rename the dir), a real controller, a windowed run: the offer over the menu on first boot and
its absence on the second; the ten steps end to end (arm with the aux switch, C both ways, D start/stop, K cut and
restore); Skip mid-tour; both replays (menu button, T in a flight); the card above the F2 and Settings screens;
gamepad navigation of the card; `user://tour.json` after each ending.

## Open questions for the maintainer

1. **Re-offer after an update?** Ruled no — the tour is offered exactly once per install (the issue says "once").
   If a future tour deserves a second offer, D8 grows a `seen_version` field; nothing else changes.
2. **The T key.** Free today (verified: no `KEY_T` handler). If you would rather keep T reserved, the replay key is
   one constant (`keys.gd`'s `HINTS["tour"]`) plus one `match` arm in `app.gd`.
