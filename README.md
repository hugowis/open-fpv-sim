# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

**Status: v1 complete** (2026-10-13; `pip install open-fpv-sim`). v1 does what its design set out to
(`docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md` §1.4, with the evidence for each point):
a developer flies a simulated OpenDrone 5" quad running real Betaflight SITL with a joystick, through a simulated
ExpressLRS/CRSF link, watching Betaflight's real OSD on a degrading analog picture; the real Betaflight Configurator
connects, and settings saved over its port persist; and a Python script runs the same quad headless and deterministically, injects
faults and gets every signal back as a DataFrame. What it does not do — level-2/3 firmware fidelity, real radios,
digital video, GPS, INAV/ArduPilot, macOS — and what comes next are in `docs/roadmap.md`.

What is in it:
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator connects to the running simulator and a PID saved through it persists (checked by hand
  2026-10-14, and live over MSP through a reboot and a new session), and a reboot makes the simulator relaunch
  SITL from its EEPROM.
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
- The sandbox: parts are data files with recorded provenance (M4a), scripts break the aircraft with the v1 fault
  catalog (M4b), and sessions are recordable and replayable — the signal bus to MCAP (two identical lockstep runs
  write byte-identical files, Betaflight included), pandas/CSV/Parquet export, a live Rerun view, scripted stick
  inputs, faults at a simulated time, and Betaflight's own Blackbox logs (M4c, `docs/research/mcap-determinism.md`).
- The radio is three swappable parts — a handset, a link and a receiver — so other links can be tried: ExpressLRS on
  2.4 GHz or 900 MHz, or a geometry-free `fixed` link, flown through the same chain; and a live flight's sticks record
  to a CSV that `play_inputs` replays in lockstep (M5, `docs/writing-a-radio-model.md`).

Documentation:
- `docs/dev-setup.md` — building, running and testing on Windows and Linux (and the performance numbers).
- `docs/architecture.md` — how the crates, the server and the clients fit together, and how to add a part, a model,
  a fault or a recorded signal.
- `docs/roadmap.md` — v1's known limitations and what comes next.
- `docs/writing-a-radio-model.md` — the radio's three parts and how to write a link.
- `CONTRIBUTING.md` — the test tiers, CI and the release steps. `CHANGELOG.md` — what changed, by release.
- `docs/research/` — the models' equations and every constant's provenance; `docs/superpowers/specs/` — the designs.

## Install the Python package

    pip install open-fpv-sim[log,rerun]

The package is the client (`import ofs`); it drives the `ofs-sim` server you build here (`cargo build -p ofs-sim`,
then `OFS_SIM_BIN` or `PATH`). The `log` extra reads recordings (`mcap`, `pandas`; Parquet export also needs
`pyarrow`), the `rerun` extra adds the live view. From a checkout, `python -m pip install -e "python[dev]"` installs
it editable with every extra and the test tools.

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
FPV view (DVR), **K** cut the radio link (failsafe test), **I** record the sticks to a replayable CSV, **H** show/hide the HUD, **F1** help, **F2** controls
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

The radio between the handset and Betaflight is three swappable parts (a handset, a link, a receiver): the quad file's
`[radio]` section picks them, and `docs/writing-a-radio-model.md` shows how to write another. Press **I** to record
the sticks of a flight to a CSV; `sim.play_inputs(path)` in a lockstep Python session replays them.

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
inline) are refused at load with a message naming the move; the shipped quads
(`quads/opendrone-5f-freestyle.toml`, `quads/opendrone-5f-tramp.toml`, `quads/opendrone-5f-weak-link.toml`,
`quads/opendrone-5f-900mhz.toml`, `quads/opendrone-5f-fixed-link.toml`)
are the worked example of the migration.

## The fault catalog

Scripts can break the aircraft in the ways real aircraft break: eight faults, injectable mid-flight from Python
or gRPC, each raising `fault_injected` / `fault_cleared` events and all cleared by `clear_faults()`. Fault
randomness draws from its own stream, so a fault-inactive run stays bit-identical to a fault-free one. The
mechanisms, constants and their estimate status are in `docs/research/faults.md`.

| Fault | Meaning |
|---|---|
| `RadioLinkLoss()` | every uplink packet is lost: the receiver goes silent, Betaflight fails safe |
| `RadioLoss(fraction)` | an extra per-packet uplink loss probability on top of the link model's own |
| `BurstLoss(on_ms, off_ms)` | the uplink dies for `on_ms`, delivers for `off_ms`, repeating from session start |
| `MotorFailure(motor, scale)` | motor `motor` makes `scale` of its torque and current; 0 = dead, coasting down through prop drag |
| `PropDamage(prop, thrust_scale, imbalance_kgm)` | the prop loses thrust and torque and gains `imbalance_kgm` of airframe vibration |
| `WeakCell(cell, r_offset_ohm, v_offset_v)` | one cell sags deeper under load; Betaflight's own telemetry reads it |
| `GyroFault(bias_radps, noise_scale)` | the gyro reads an extra bias and a multiple of its normal noise |
| `VtxInterference(freq_mhz, power_mw, position_ned_m)` | a transmitter next door breaks up the picture |

Every fault takes a keyword-only `at=`: the simulated time it activates at (on that tick, however the script cuts its
`run` calls); without it, now. `clear_faults()` also drops faults still waiting for their time. The same faults fly
from the game: the **K** key injects and clears `RadioLinkLoss`.

## Scripting, recording and the live view

The spec's sandbox example (`python/examples/sandbox.py`): run it from the repository root with `OFS_SIM_BIN` and
`OFS_SITL_LAUNCH` set and `python/examples/flips.csv` copied beside it (`cp python/examples/flips.csv .`):

    import ofs
    sim = ofs.launch(headless=True)
    sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
    sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
    sim.play_inputs("flips.csv")
    sim.run(seconds=10)
    df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])

- **Recording.** A lockstep `load` records the whole signal bus at the default rates (body 240 Hz, IMU 1 kHz,
  motors/ESCs/props 240 Hz, battery 10 Hz, everything else 50 Hz) into `<data_dir>/logs/<quad>-<n>.mcap`;
  `load(record=False)` does not, and real-time sessions record only when asked. `sim.recording.start(["imu.*",
  ("motor.*.omega_radps", 1000)])` picks signals and rates, `sim.recording.stop()` finishes the file. Recordings
  open in Foxglove unchanged. Two lockstep runs with the same seed and inputs write byte-identical files.
- **Reading.** `sim.log` stops the running recording and returns it; `.to_dataframe(patterns)` gives one row per
  recorded time and one column per signal component (`imu.gyro_frd_radps.x`), indexed by `time_s`, NaN where a
  slower signal has no record. `.to_csv(path)` and `.to_parquet(path)` export; `ofs.reader.to_dataframe(path)`
  reads any recording. Patterns: `*` matches any run of characters, `?` one; a pattern matching nothing names
  the signals it probably meant.
- **Scripted sticks.** `sim.play_inputs("flips.csv")` plays a CSV with a `t_s` column (seconds from the call) and any
  of `roll`, `pitch`, `yaw`, `throttle`, `aux1`..`aux4`, sample-and-hold on the simulator's ticks; `set_sticks`
  is refused while it plays, and a malformed file raises `ConfigError` naming each problem's line.
- **Betaflight's Blackbox.** The shipped quads log to SITL's virtual Blackbox; when a session ends its logs are
  copied into the logs directory (`sim.blackbox_files()`), ready for Blackbox Explorer. Disarm before the session
  ends for a complete log. (Enabling it changed the shipped diffs: an EEPROM made before M4c is refused at load
  until you delete it, see `docs/dev-setup.md`.)
- **Live view.** `ofs.rerun.view(sim)` opens the Rerun viewer on a running session: the quad's pose in the world,
  battery, motor, link and video curves, the events as text. It follows the state stream (up to 240 Hz); the 8 kHz
  signals are in the recording.

## Licence
Open FPV Sim is GPL-3.0-or-later (`LICENSE`). One file is GPL-3.0 only: the OSD font atlas `godot/ui/osd_font.png`,
converted from Betaflight Configurator's fonts (`godot/ui/OSD_FONT_LICENSE.md`). The two licences are compatible,
so the combined work is distributed under GPL-3.0. Part data files in `library/` are CC-BY-SA-4.0.
