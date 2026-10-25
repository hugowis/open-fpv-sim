# Controller setup: calibration, shaping and profiles (#127; epic #135)

Godot client only (`godot/`); no protocol, server, Rust or Python change. The brief is issue #127; no design
conversation took place, so every ruling below is the author's call and open to change on review.

## Why

The F2 screen binds axes and switches and flips an axis, but a USB radio in joystick mode rarely reads -1..+1 end to
end or rests at exactly 0, there is one global deadzone, and one setup replaces the last (a pilot with a gamepad *and* a
radio re-binds every time).

## Goals

1. **Calibration wizard** per controller: centres, then end points, per axis. Stored once, applied whenever that
   controller's axis is read.
2. **Per-axis shaping** for roll, pitch and yaw: invert (already there), deadband, travel. Throttle and aux get
   invert and calibration only.
3. **Named profiles**, switchable from the controls screen; **presets chosen by device name** for a new profile.
4. A **live monitor** on the controls screen: the stick boxes and aux squares (the HUD's `SticksView`) and the name of
   the controller(s) Godot sees, next to the existing per-channel bars.
5. Existing `user://controls.json` files (the bare bindings file) keep loading: they become the profile "Default".

## Non-goals

- Rates, expo, curves: Betaflight's. The handset corrects the physical axis; it never shapes the response.
- Calibration of buttons, keys or the keyboard; per-profile calibration (calibration belongs to the controller).
- Renaming or duplicating profiles (create with a name, delete); mixing channels; stick-to-switch conversions.
- Reading raw HID names beyond what Godot exposes (`Input.get_joy_name`, `Input.get_joy_guid`).

## Rulings

**R1. Order of operations**, per stick axis: raw -> calibrate -> invert -> deadband -> travel.
- *Calibrate (centred, roll/pitch/yaw)*: `(raw - centre) / (max - centre)` above the centre, `(raw - centre) / (centre -
  min)` below it, clamped to [-1, 1]. Each side is scaled on its own, so an off-centre rest and unequal throws both come
  out right. *Calibrate (range, throttle and aux)*: `(raw - min) / (max - min) * 2 - 1`, centre unused (a radio throttle
  rests at the bottom). A calibration with a side narrower than 0.05 is ignored (raw is used), as is an uncalibrated axis.
- *Deadband*: the existing rescale, `sign(v) * (|v| - d) / (1 - d)`; `d` is the channel's own `deadband`, else the global
  `deadzone`. The global slider stays (it is the default; presets set it) and per-channel sliders override it.
- *Travel* `t` in [0.3, 1.0]: `clamp(v / t, -1, 1)`, i.e. full output at fraction `t` of the throw (a gamepad whose
  sticks stop at 80% gets `t = 0.8`). Never above 1: this is endpoint trim, not a limiter.

**R2. Calibration is keyed by controller, not profile.** `Input.get_joy_guid(device)` is the key (name fallback when the
guid is empty), with the display name kept beside it; per axis `{min, center, max}`. It is looked up at read time from
the binding's resolved device. Two identical radios share a guid and a calibration; that is accepted.

**R3. The wizard is a state machine** (`scripts/calibration.gd`, no engine calls) fed with `{axis: value}` samples plus
the frame time, so the headless tests drive it with scripted samples. Steps: *rest* (hands off; 1 s of samples averaged;
any axis moving more than 0.1 restarts the step), then *sweep* (move every stick to all extremes and flip the switches;
min and max per axis; the pilot presses Done). An axis whose total swing is under 0.5 is **not** calibrated (it was
not touched), and the existing calibration for it stays. The UI polls the controller (`Input.get_joy_axis` for
`0..JOY_AXIS_MAX-1`, the first connected pad, or a pad picker when several), shows each moving axis as a bar, and merges
the result into the store, then saves.

**R4. Profiles** are snapshots of one mapper: preset name, `deadzone`, `bindings`, `shaping`. The store
(`scripts/control_profiles.gd`) owns the live `Controls` object (so the rest of the client does not change), `active`,
the profile dictionary, and the calibration table. File v2:

```json
{"version": 2, "active": "Radio", "calibration": {"<guid>": {"name": "...", "axes": {"2": {"min": -0.8, "center": 0.0, "max": 0.9}}}},
 "profiles": {"Radio": {"preset": "radio", "deadzone": 0.02, "bindings": {}, "shaping": {"roll": {"deadband": 0.03, "travel": 0.9}}}}}
```

A file without `profiles` is the old format: it loads as the single profile `Default` (its `deadzone` and `bindings`
as they were). Invalid files change nothing, as before. At least one profile always exists; the last cannot be deleted.
Switching or creating snapshots the outgoing profile first; every change is saved at once, like the F2 screen.

**R5. Presets by device name.** `Controls.preset_for_device(name)` lowercases and substring-matches: gamepad markers
(`xbox`, `playstation`, `dualshock`, `dualsense`, `ps4`, `ps5`, `wireless controller`, `gamepad`, `switch pro`, `8bitdo`)
-> `gamepad`; radio markers (`radiomaster`, `taranis`, `jumper`, `tx12`, `tx16`, `zorro`, `boxer`, `frsky`, `flysky`,
`opentx`, `edgetx`, `radio`, `transmitter`) -> `radio`; anything else "" (the caller chooses). **New profile** names
itself after the first connected pad (or "Profile N") and starts from that pad's preset, falling back to `radio`. A first
run (no `controls.json`) with a pad connected at start does the same for the one default profile.

**R6. UI.** Everything stays on the F2 screen (a pilot already looks there), which grows a profile row (picker, name
field, New, Delete), a Calibrate button, a Deadband and a Travel slider on the roll/pitch/yaw rows, the stick view and a
controller line. The calibration wizard is its own panel over the screen. The **main menu gains a Controls button**
that opens the same screen (Close button, Esc and `ui_cancel` come back to the menu), so the screen is reachable without
a flight. `main_menu.gd`/`app.gd` changes there are small.

## What is left to the builder

Layout, wording, slider ranges inside those above, colours.

## Checked by eye / with real hardware (headless Godot has no joystick and draws nothing)

A real gamepad and a real radio in joystick mode: the guid and name Godot reports; that the wizard sees each axis; that
calibrated end points reach exactly -1 and +1; a controller unplugged and replugged keeps its calibration; the preset
the name guess picks; layout of the F2 screen at 1280x720; gamepad focus navigation through the new rows.
