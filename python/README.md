# open-fpv-sim

The Python client of [Open FPV Sim](https://github.com/hugowis/open-fpv-sim), an open-source FPV drone simulator
that runs **real Betaflight** (SITL) against physics, electrical, sensor, radio and video models.

    pip install open-fpv-sim[log,rerun]

The package is the client (`import ofs`). It drives an `ofs-sim` server, which you build from the repository
(`cargo build -p ofs-sim`) and point at with `OFS_SIM_BIN`; flying real Betaflight also needs the SITL build
described in the repository's `docs/dev-setup.md`.

    import ofs

    sim = ofs.launch(headless=True)
    sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
    sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
    sim.play_inputs("flips.csv")
    sim.run(seconds=10)
    df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])

If a session fails (a Betaflight crash, a non-finite value), the server writes a repro bundle, and the `sim_error`
event names it. `ofs.repro.find_bundles(sim.logs_dir)` lists the bundles and `ofs.repro.replay_bundle(path)` runs one
again and says whether the failure came back (`docs/repro-bundle.md` in the repository).

A world with `[[gates]]` has a race: `sim.get_world().gates` lists them, `sim.run(...).race` is the lap clock
(`present`, `running`, `next_gate`, `laps`, `current_lap_s`, `last_lap_s`, `best_lap_s`) and the `gate_passed` and
`lap_completed` events carry the splits and lap times (`docs/race-timing.md` in the repository).

To fly a path from a script without Betaflight, load with `open_loop_fc=True, stabilize=True`: a minimal angle-mode
stabilizer (a test stand-in, not Betaflight) so the sticks set the lean and the turn rate. `python/examples/circuit.py`
flies a lap through four gates this way, and on Betaflight SITL too (`docs/scripted-flight.md` in the repository).

Extras: `log` reads the session recordings (`mcap`, `pandas`; Parquet export also needs `pyarrow`), `rerun` adds the
live view (`ofs.rerun.view(sim)`).
