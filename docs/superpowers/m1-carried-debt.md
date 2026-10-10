# M1 carried debt

These findings came out of the M1 task reviews and the final whole-branch review. Each was deliberately left out of M1 and listed for later milestones. The minors cleanup after M3b (branch `minors-cleanup`) closed most of them; what remains is listed under "Still open".

## Resolved
- **Server and session lifecycle (M2a):** graceful `ofs-sim` shutdown on Ctrl-C/SIGTERM; `Run` stops within one 50 ms chunk when its client goes away or the server shuts down; a Python client's session ends when it disconnects (unless `keep_alive`); firmware directories are keyed on the quad file's path.
- **Non-loopback `--listen` (M2b):** refused unless `--allow-remote` is given.
- **SITL bridge (minors cleanup):**
  - after a resent first datagram, late duplicate replies are drained, and a first datagram carries no UART bytes, so none reach Betaflight twice;
  - replies from any address other than SITL's are ignored (and logged);
  - a wrong program path behind `wsl.exe` gets a hint;
  - the `wsl.exe` queries time out after 10 s;
  - `OFS_SITL_LAUNCH` accepts quoted paths with spaces;
  - more than 4 motors is a config error, not a panic.
- **Models and config (minors cleanup):**
  - NaN or infinity in vector config fields (motor positions, contact points, IMU biases, the initial pose, home altitude) is a config error;
  - the propeller and battery models check their tables when built;
  - the scheduler refuses duplicate model names and runs longer than a simulated day; `run_for` documents its rounding to whole ticks;
  - the per-tick non-finite check scans the contiguous signal arenas, and searches by name only when one is found;
  - ISA pressure is 0 above ~44 km instead of NaN; the barometer's noise path is tested;
  - the test gaps (digest and non-finite cases, rigid-body contact torque, friction and damping, the propeller's name and rate, the gRPC `run_before_load` code, one data directory per gRPC test server) are covered.
- **CI and tooling:** the CI workflow runs (core, msrv, windows, godot, sitl), with cargo, pip, SITL and Godot caches; clippy runs with `-D warnings` (the `result_large_err` warnings are allowed where tonic's `Status` is fixed by the gRPC traits); the native-Linux Python hover test runs in the `sitl` job.
- **M0 open items:** the Betaflight Configurator connection (checked by the user, 2026-10-08) and the SmartAudio reply (M3a).

## Still open (by design or not reproducible)
- **The scheduler is unusable after an error.** By design: the server poisons the session, and a new Load starts a fresh one.
- **Physics simplifications** (features, not defects): no rotor gyroscopic coupling; friction is viscous below 5 cm/s; `BODY_ACCEL_NED` is coordinate acceleration (it includes gravity); the published motor `omega_dot` is the pre-clamp derivative.
- **Heading/quaternion mapping for the compass (M0).** A feature: the shipped quad has no compass.
- **One intermittent, unexplained MSP timeout (M0).** Not seen since M1; the MSP client now rejects stale replies and `$M!` errors, which would have shown a mismatch.
