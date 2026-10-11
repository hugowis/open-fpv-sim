# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M3e — video extras.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
- A Godot pilot client flies the simulator from a game window, through the same radio link (see below).
- Betaflight's own OSD, a SmartAudio VTX and the battery reach the FPV view (M3a), and a 5.8 GHz analog link model
  breaks the picture up with distance, attitude, buildings and interference (M3b, `docs/research/video-link.md`).
- The world's objects are solid: the quad bounces off buildings and gates, lands on roofs and raises `COLLISION`
  events, and the ExpressLRS link is geometric — its RSSI, SNR and LQ come from the world's geometry, so Betaflight
  fails safe on a real RX loss (M3c, `docs/research/collision.md` and `docs/research/elrs-link.md`).
- The FC's own telemetry rides the simulated link home: the HUD's battery line shows Betaflight's own voltage,
  current and flight mode (and dashes out at link loss), and the simulator's per-motor eRPMs feed Betaflight's
  RPM filter (M3d, `docs/research/elrs-link.md`).
- The picture is delayed analog glass: the goggles show the world at their configured glass-to-glass latency
  (whole PAL fields; dropped fields in the marginal band hold the picture instead of smoothly degrading), the
  **D** key records exactly what the pilot sees as an MJPEG AVI, and a Tramp-configured quad answers
  Betaflight's VTX commands like the SmartAudio one (M3e, `docs/research/video-link.md`).

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
Configurator port all behave as in the Python session. Betaflight's own OSD is drawn in the FPV view, the VTX is
controlled by Betaflight over SmartAudio and shown in the HUD, and the battery reaches Betaflight so its OSD shows
real voltage and warnings. The world (`worlds/flat.toml`: launch pad, gates, buildings, the pilot's spot and a parked
quad's VTX) is a data file the simulator loads; its analog video link degrades the picture and the OSD together, like
real goggles. A USB radio in joystick mode, a gamepad or the keyboard
flies it; the F2 screen sets the bindings, and they persist to `user://controls.json` (Godot's per-user data dir,
`%APPDATA%\Godot\app_userdata\Open FPV Sim\` on Windows, `~/.local/share/godot/app_userdata/Open FPV Sim/` on Linux).

    cargo build -p ofs-sim -p ofs-godot
    godot --path godot                             # Godot 4.7; see docs/dev-setup.md for the binary

Keys: **P** pause/resume, **R** reload the quad (retry after an error), **C** FPV/chase camera, **D** record the
FPV view (DVR), **K** cut the radio link (failsafe test), **H** show/hide the HUD, **F1** help, **F2** controls
setup, **F11** fullscreen, **Esc** closes the screens. Arm with the aux 1 switch; aux 2 selects Angle mode.

Settings: built-in defaults < the `[client]` section of `user://ofs_client.cfg` < `OFS_*` environment variables <
command-line flags after `--`. By default the game starts `ofs-sim` itself (from `target/release`, else
`target/debug`, else `PATH`) on `127.0.0.1:50051` and loads `quads/opendrone-5f-freestyle.toml` in
`worlds/flat.toml` in real time.
Overrides:

| Setting | Environment | Flag |
|---|---|---|
| server binary | `OFS_SIM_BIN` | `--sim=` |
| server address | `OFS_SERVER_ADDR` | `--server=` |
| quad file | `OFS_QUAD` | `--quad=` |
| data dir | `OFS_DATA_DIR` | `--data-dir=` |
| open loop (no firmware) | `OFS_OPEN_LOOP` | `--open-loop` |
| SITL launch command | `OFS_SITL_LAUNCH` | `--sitl-launch=` |
| world file | `OFS_WORLD` | `--world=` |
| video effects (on/off) | `OFS_VIDEO_EFFECTS` | `--video-effects=` |
| FPV aspect (4:3/16:9/window) | `OFS_FPV_ASPECT` | `--fpv-aspect=` |

    godot --path godot -- --open-loop              # fly the model without Betaflight

With `OFS_SITL_LAUNCH` set (see "With real Betaflight" above), the game starts Betaflight and arms it through the
simulated radio link.

## Component library

The quad file is schema 5: it references parts and keeps only its own wiring, and the hardware lives in
`library/` — one TOML per part, every parameter's provenance recorded.

    library/
      frames/    motors/   props/   escs/   batteries/   vtx/
    quads/       <quad>.toml + <quad>.betaflight.diff   (as today)

A part's id is its file stem, and the quad references it — `motors = ["opendrone-openmotor-2207"]` in
`quads/opendrone-5f-freestyle.toml` fits four copies of the one motor part. The library is resolved against
`<quad dir>/../library`; set `OFS_LIBRARY` to fly another part set without touching `quads/`. Every parameter
of a part must carry a `[provenance]` entry or the part is refused at load:

    [provenance."motor.kv_rpm_per_v"]
    source = "estimated"     # datasheet | measured | estimated
    note = "M1 estimate; the OpenMotor datasheet value replaces this when transcribed"

Every shipped value is an estimate recorded in the open — the ledger is
`docs/research/component-library.md`, and `library/README.md` describes adding a part. Schema-4 quad files
(which carried the `[frame]`, `[ground]`, `[motor]`, `[prop]`, `[esc]`, `[battery]` and `[vtx]` hardware
inline) are refused at load with a message naming the move; the three shipped quads
(`quads/opendrone-5f-freestyle.toml`, `quads/opendrone-5f-tramp.toml`, `quads/opendrone-5f-weak-link.toml`)
are the worked example of the migration.

License: GPL-3.0-or-later.
