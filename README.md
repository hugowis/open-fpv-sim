# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M2a — radio link and real time.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator should connect to the running simulator (the manual check with the desktop app is pending). A reboot sent to its port makes the simulator relaunch SITL from its EEPROM (verified live).
- Next is M2b: the Godot pilot client.

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

License: GPL-3.0-or-later.
