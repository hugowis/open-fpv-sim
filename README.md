# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M2b — the Godot pilot client.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
- A Godot pilot client flies the simulator from a game window, through the same radio link (see below).

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
- SITL interface findings: `docs/research/sitl-interface.md`
- Developer setup: `docs/dev-setup.md`

## Quick start (open loop, no firmware)

    cargo build -p ofs-sim
    python -m pip install -e "python[dev]"
    python -c "import ofs; s = ofs.launch(binary='target/debug/ofs-sim'); s.load('quads/opendrone-5f-freestyle.toml', open_loop_fc=True); s.set_sticks(throttle=0.6); print(s.run(1.0)); s.close()"

## With real Betaflight (from the repository root)

    bash scripts/build-sitl.sh                      # Linux or WSL2
    export OFS_SIM_BIN=target/debug/ofs-sim
    export OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf   # Windows: see docs/dev-setup.md
    python python/examples/hover.py                 # lockstep: arm and hold 1 m
    python python/examples/serve_realtime.py        # real time; connect Betaflight Configurator to tcp://127.0.0.1:5761

## The Godot pilot client

A game window on top of the same simulator: FPV and chase cameras, an HUD (phase, radio link, battery, sticks),
and Betaflight flying through the simulated ExpressLRS/CRSF link into real firmware — failsafe, reboots and the
Configurator port all behave as in the Python session. A USB radio in joystick mode, a gamepad or the keyboard
flies it; the F2 screen sets the bindings, and they persist to `user://controls.json` (Godot's per-user data dir,
`%APPDATA%\Godot\app_userdata\Open FPV Sim\` on Windows, `~/.local/share/godot/app_userdata/Open FPV Sim/` on Linux).

    cargo build -p ofs-sim -p ofs-godot
    godot --path godot                             # Godot 4.7; see docs/dev-setup.md for the binary

Keys: **P** pause/resume, **R** reload the quad (retry after an error), **C** FPV/chase camera, **K** cut the radio
link (failsafe test), **H** show/hide the HUD, **F1** help, **F2** controls setup, **F11** fullscreen, **Esc** closes
the screens. Arm with the aux 1 switch; aux 2 selects Angle mode.

Settings: built-in defaults < the `[client]` section of `user://ofs_client.cfg` < `OFS_*` environment variables <
command-line flags after `--`. By default the game starts `ofs-sim` itself (from `target/release`, else
`target/debug`, else `PATH`) on `127.0.0.1:50051` and loads `quads/opendrone-5f-freestyle.toml` in real time.
Overrides:

| Setting | Environment | Flag |
|---|---|---|
| server binary | `OFS_SIM_BIN` | `--sim=` |
| server address | `OFS_SERVER_ADDR` | `--server=` |
| quad file | `OFS_QUAD` | `--quad=` |
| data dir | `OFS_DATA_DIR` | `--data-dir=` |
| open loop (no firmware) | `OFS_OPEN_LOOP` | `--open-loop` |
| SITL launch command | `OFS_SITL_LAUNCH` | `--sitl-launch=` |

    godot --path godot -- --open-loop              # fly the model without Betaflight

With `OFS_SITL_LAUNCH` set (see "With real Betaflight" above), the game starts Betaflight and arms it through the
simulated radio link.

License: GPL-3.0-or-later.
