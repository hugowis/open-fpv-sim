# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical and sensor models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M1 — headless core.** Lockstep physics at 8 kHz, Betaflight SITL bridge, Python scripting over gRPC.

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
- SITL interface findings: `docs/research/sitl-interface.md`
- Developer setup: `docs/dev-setup.md`

## Quick start (open loop, no firmware)

    cargo build -p ofs-sim
    python -m pip install -e "python[dev]"
    python -c "import ofs; s = ofs.launch(binary='target/debug/ofs-sim'); s.load('quads/opendrone-5f-freestyle.toml', open_loop_fc=True); s.set_sticks(throttle=0.6); print(s.run(1.0)); s.close()"

## Hover with real Betaflight

    bash scripts/build-sitl.sh                      # Linux or WSL2
    export OFS_SIM_BIN=target/debug/ofs-sim
    export OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf
    python python/examples/hover.py

License: GPL-3.0-or-later.
