# open-fpv-sim

The Python client of [Open FPV Sim](https://github.com/hugowis/open-fpv-sim), an open-source FPV drone simulator
that runs **real Betaflight** (SITL) against physics, electrical, sensor, radio and video models.

    pip install open-fpv-sim[log]

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

The `log` extra reads the session recordings (`mcap`, `pandas`; Parquet export also needs `pyarrow`).
