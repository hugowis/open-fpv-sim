# M3a carried debt

These items were deliberately left out of M3a, or surfaced while building it. They are listed so M3b/M4 planning and the maintainer can pick them up.

## Deferred features
- **Tramp.** The VTX model is protocol-independent; only a Tramp codec is missing, so adding Tramp is a codec behind the same `VtxModel`.
- **The analog link model and degradation shader (M3b).** Layer 3 of the FPV render path (between the OSD and the HUD) and the `vtx.*` bus signals the model will read are already in place.
- **Font page 1 and the HD/digital OSD.** Only the first 256 glyphs (Betaflight's analog font) are converted and drawn from the atlas; HD/digital OSDs use more pages and a different element set.
- **An OSD aspect-ratio setting.** The OSD draws in a fixed 4:3 box fitted to the window height today.
- **The RPM filter and per-motor eRPM.** Needs DShot telemetry emulation in SITL (per-motor eRPM each loop); the ESC-sensor frames carry only an RPM the filter ignores (M0 §5, §7 risk 2).
- **CRSF telemetry back to the radio.** The HUD keeps reading the battery from the simulator's own models; SITL's CRSF telemetry also needs the atomic-block shim from M0 (§5).
- **A thermal model.** The ESC temperature in the KISS frames is a constant 25 degC.
- **VTX faults (M4).** The faults module covers physics and the radio link, not the video chain yet.

## Licensing (flag to the maintainer)
- **The OSD font atlas is GPL-3.0, not "or later".** It is a derivative of Betaflight Configurator's `default.mcm`, whose repository reports GPL-3.0, while this project is GPL-3.0-or-later. The notice beside the atlas (`godot/ui/OSD_FONT_LICENSE.md`) records the facts; the maintainer should decide whether the combination is acceptable for this file.

## Testing and validation gaps
- **`serial_overflow` is never triggered live.** It is covered by a counter test and an event test, but no live run has overflowed a real buffer: the capture and tap buffers (4096 bytes) are far larger than the shipped traffic. Since the minors cleanup, bytes dropped by the simulator's own wires (into SITL's UARTs, or from a tap a model did not keep up with) count in `fc.serial_dropped_bytes` too, so they raise the same event.
- **Resolved in the minors cleanup:** a diff that enables the ESC sensor or sends the OSD over MSP DisplayPort without the matching `[esc_telemetry]` / `[osd]` section (on that UART) is a config error at load, and so is an ESC-sensor battery whose diff does not `set force_battery_cell_count` to `battery.cells` (Betaflight would guess the cell count from the first voltage it sees; a part-charged 6S pack read as 5S and never raised the low-battery warning).
