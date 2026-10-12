import ofs
sim = ofs.launch(headless=True)
sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
sim.play_inputs("flips.csv")
sim.run(seconds=10)
df = sim.log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
