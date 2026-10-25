# Menu and settings Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans. Steps use checkbox syntax.

**Goal:** A main menu (#123) and a settings screen (#126) in the Godot client.

**Spec:** `docs/superpowers/specs/2026-10-25-menu-and-settings-design.md`

**Tech Stack:** GDScript (Godot 4.7.2). Tests: `godot/tests/test_*.gd` (unit, headless) and the open-loop e2e.

## Global Constraints

- The chain `ofs_client.cfg` < `OFS_*` < flags stays the only source of settings. No file other than the cfg.
- The e2e suites and `shots.gd` keep their menu-less path (`OFS_MENU=off`).
- Do not touch `godot/drone/drone.gd`, `godot/tests/test_scene.gd`, `worlds/park.toml`, `worlds/meadow.toml`
  (another worktree owns them); README and CHANGELOG conflicts are resolved on rebase.
- Gates: `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets --locked -- -D warnings`;
  `scripts/run-godot-tests.sh all` in WSL (touch Rust sources first; never with Windows cargo at the same time; the
  runner prints `0 failed` even on a SCRIPT ERROR, so grep the output for `SCRIPT ERROR`).
- Write GDScript with Edit/Write, never a Bash heredoc. Commit `type(scope): what`, one per task.

## Review Focus

- A flag or env var is never overwritten in the cfg by a locked row (Task 3).
- Stop then Start in one process: the port is free again and a second session reaches `flying` (Task 5, e2e).
- Esc with help or the F2 screen open closes those first (Task 5).

### Task 1: New settings and the override report (`settings.gd`)

- [ ] Tests in `test_settings.gd`: defaults of `menu`, `race_start`, `fullscreen`, `hud_visible`; each from env and flag
  (`--menu=off`); `overrides({OFS_OPEN_LOOP:"1"}, ["--open-loop"])` reports the flag, env alone reports the variable,
  untouched keys are absent.
- [ ] Add the keys to `defaults`, `ENV_NAMES`, `FLAG_NAMES`; add `static func overrides(env, args) -> Dictionary`
  (key -> "--flag" or "OFS_NAME") and make `load_settings` share the env read through a helper.

### Task 2: The catalog (`scripts/catalog.gd`)

- [ ] Tests in `test_catalog.gd` against a fake data dir under `user://`: lists `*.toml` of several roots sorted by
  name, labels by `name = "…"` else file stem, counts `[[gates]]`, ignores non-toml and missing dirs, de-duplicates,
  `with_current(list, path)` appends an unlisted current path.
- [ ] Implement `Catalog.roots(settings, kind)`, `list(dirs)`, `with_current(list, path)`; register the suite in
  `run_tests.gd`.

### Task 3: The cycle button and the settings screen

- [ ] `ui/cycle_button.gd` (options, index, `set_value` without a signal, `changed(value)`; press / ui_right next,
  ui_left previous), tested in `test_cycle_button.gd`.
- [ ] `ui/settings_screen.gd`: `setup(settings, locks)`, rows per spec R7, signal `setting_changed(key, value)`,
  `closed`, locked rows disabled with a "set by" note, Esc / Back closes. Tests in `test_settings_screen.gd`: effective
  values shown, a locked row is disabled and emits nothing, an edit emits the key and value, text rows emit on submit.

### Task 4: The main menu

- [ ] `ui/main_menu.gd`: lists from `Catalog`, cycle rows for world / quad / race mode (gates only) / controller
  (R6), Start / Settings / Quit, signals `start_requested(choices)`, `settings_requested`, `quit_requested`, saves
  choices (R5) through a `save` Callable, locks rows from `overrides`. Tests in `test_main_menu.gd` with a fake data dir:
  rows list the files, remembered selection restored, a world without gates hides the race row, no `sitl_launch`
  locks the controller to open loop, a flag-locked world row is disabled, Start emits the chosen paths.

### Task 5: Wire the app

- [ ] `main.tscn`: `MainMenu` (layer 6) and `SettingsScreen` (layer 7) nodes.
- [ ] `app.gd`: `_ready` boots to the menu when `settings.menu`, else `_start_session()` (the old tail of `_ready`);
  `_on_setting_changed(key, value)` is the one apply-and-save path (F2 handlers route to it, F2 displays sync);
  `_return_to_menu()` (Esc, stops the DVR, `client.stop()`, hides the FPV box and HUD); Start applies choices and
  calls `_start_session()`; free flight hides race state (R2); fullscreen and HUD defaults applied at boot.
- [ ] HUD help gets the Esc line. `e2e_*.gd` and `shots.gd` set `OFS_MENU=off`; the open-loop e2e gains a menu round
  trip (Esc -> menu -> pick another quad -> Start -> flying again, free flight hides the race line).

### Task 6: Docs and gates

- [ ] README (settings table, keys, a "The menu" paragraph), CHANGELOG `Unreleased`, `docs/roadmap.md` if it lists the
  menu, `docs/architecture.md` if it lists the Godot files.
- [ ] Run every gate; list the by-eye checks in the PR description.
