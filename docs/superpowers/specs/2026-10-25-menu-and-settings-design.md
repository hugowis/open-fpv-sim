# A main menu and a settings screen (#123, #126; epic #135)

Godot client only (`godot/`); no protocol, server, Rust or Python change. One branch, one PR: both screens share the
plumbing (the settings chain's source report, the cycle widget, the app's apply-a-setting path). The brief is the two
issues; no design conversation took place, so every ruling below is the author's call and open to change on review.

## Why

The client boots straight into the FPV view and every choice (world, quad, Betaflight or open loop, the DVR) is an
environment variable, a flag or a hand edit of `user://ofs_client.cfg`. A pilot should not need the README to pick a
map, and should not need a text editor to turn the video effects off.

## Goals

1. The game opens in a **main menu**: Start flying, a world, a quad, a flight mode (free flight or race), the flight
   controller (Betaflight or open loop), Settings, Quit. Keyboard and gamepad navigate it; the last choices are kept.
2. **Esc** from a flight returns to the menu in the same process; Start flies the (possibly different) choices.
3. A **settings screen** edits the client settings in `ofs_client.cfg`, shows the effective value of each, and marks
   what a flag or environment variable overrides (and locks that row for this run).
4. The env var / flag / config chain is untouched: no new file, no second source of truth.

## Non-goals

- Tutorials, flight school, controller calibration (#124, #125, #127). The F2 controls screen stays as it is.
- Editing quads or worlds; a settings screen in the flight (it opens from the menu only).
- Exposing `seed`, `overrun_policy`, `state_rate_hz`, `stick_rate_hz`: they are developer knobs for the e2e and
  determinism work. Config file only, as today.

## Rulings

**R1. New settings go through the existing chain.** `settings.gd` gains four keys, each with the usual
env / flag / `[client]` triple, listed in the README table:

| key | default | env | flag |
|---|---|---|---|
| `menu` | `true` | `OFS_MENU` | `--menu=on/off` |
| `race_start` | `true` | `OFS_RACE_START` | `--race-start=on/off` |
| `fullscreen` | `false` | `OFS_FULLSCREEN` | `--fullscreen` |
| `hud_visible` | `true` | `OFS_HUD` | `--hud=on/off` |

`menu` off boots straight into the flight as today. The e2e suites and `shots.gd` set `OFS_MENU=off`.

**R2. "Race start" means the lap clock and the next-gate highlight.** The simulator needs no separate mode: a world with
gates always has a course. In *free flight* the client hides the course's race state (HUD race line, next-gate
highlight) and flies the world as scenery; in *race* it shows them, as today (`race_start` defaults to `true`, so
nothing changes without the menu). The menu offers the choice only for a world that declares `[[gates]]`.

**R3. Worlds and quads are listed from files.** `*.toml` in `<repo>/worlds`, `<data_dir>/worlds`, `<repo>/quads` and
`<data_dir>/quads`, de-duplicated by path, labelled by the file's top-level `name = "…"` (else the file name), sorted
by label. The current `world_path` / `quad_path` is kept in the list when it lies elsewhere, so a flag or a saved
custom path never vanishes. (The issue says "the data dir's `worlds/` and `quads/`"; the shipped ones live in the
checkout and the data dir holds the user's, so both are read.)

**R4. Starting is stop + start, not Reload.** The server's Reload re-reads the same quad and world from the session's
original settings, so a different choice needs a new session: Esc stops the client (which ends a server this node
started, and its Betaflight) and Start starts it again with the settings chosen. This is also what a new `server_bin`
or `sitl_launch` from the settings screen needs. A recording running at Esc is stopped through the usual DVR stop path.

**R5. Menu choices persist through `save_value`.** World, quad, flight controller and flight mode are saved to
`ofs_client.cfg` the moment they change, as `world_path`, `quad_path`, `open_loop`, `race_start`.

**R6. Betaflight is offered only when `sitl_launch` is set.** Without it the controller row reads "Open loop" and is
locked with a hint to set the launch command in Settings; Start passes `open_loop = true` for that session without
saving it. (Today that setup ends in a "firmware" error.)

**R7. The settings screen is a view of the chain.** `AppSettings.overrides(env, args)` reports, per key, the flag or
environment variable that wins this run. A row whose key is overridden shows the effective value, is disabled, and says
`set by --open-loop` or `set by OFS_OPEN_LOOP`. Other rows read and write `[client]`. Rows: Video effects, Picture
(aspect), HUD shown at start, Fullscreen, DVR feed, DVR export (live); open loop, simulator binary, server address,
data dir, SITL launch command (at the next Start, which the menu makes immediate). A value the screen writes is not
re-read from the file: the app's `settings` dictionary is updated and saved, so the screen and the F2 screen agree.

**R8. Widgets.** One small `CycleButton` (press or →/d-pad right: next; ←: previous) serves every choice, because the
F2 screen already works that way and it needs no popups for the gamepad. Text rows are `LineEdit`s (a keyboard job).
Godot's focus navigation (`ui_up/ui_down`, which the default input map binds to arrows and d-pad) moves between rows.
The main-menu Start button takes focus when the menu opens.

**R9. Esc.** Esc closes help, then the F2 screen, as today. With neither open it returns to the menu. In the menu, Esc
on the settings screen goes back to the menu; on the menu itself it does nothing (no accidental quit).

## What is left to the builder

Layout and look (a dim panel like F2's); the order of rows; wording of the hint lines.

## Checked by eye (headless Godot renders nothing)

The menu's look and focus highlight; gamepad navigation; the fullscreen switch; the picture aspect applied from the
settings screen; a recording stopped by Esc; the locked-row wording against a real flag.
