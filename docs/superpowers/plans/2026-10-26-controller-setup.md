# Controller setup (#127) - plan

Spec: `docs/superpowers/specs/2026-10-26-controller-setup-design.md`. Test-first, one commit per task.
Unit suites: `scripts/run-godot-tests.sh unit` in WSL (see the PR notes for the env vars).

1. **Shaping maths** - `godot/scripts/axis_shaping.gd` (`calibrate`, `shape`) + `tests/test_axis_shaping.gd`.
2. **Controls** - `shaping`, `calibration`, `preset_for_device`, `to_profile`/`from_profile` (`to_json`/`from_json`
   stay), `read` uses R1; tests in `test_controls.gd` (fake input gains `get_joy_guid`).
3. **Wizard state machine** - `godot/scripts/calibration.gd` + `tests/test_calibration.gd`.
4. **Profile store** - `godot/scripts/control_profiles.gd` + `tests/test_control_profiles.gd` (v1 migration, round trip,
   switch, new, delete, invalid file).
5. **F2 screen** - profile row, shaping sliders, stick view, controller line, Calibrate button and
   `ui/calibration_wizard.gd`; `controls_menu.setup(profiles, path)`; `tests/test_controls_menu.gd` and
   `tests/test_calibration_wizard.gd`.
6. **App and main menu** - the store in `app.gd`, first-run device preset, Controls button in the main menu.
7. **Docs and gates** - README, `docs/dev-setup.md`, CHANGELOG; unit and e2e suites, PR notes (real-hardware list).
