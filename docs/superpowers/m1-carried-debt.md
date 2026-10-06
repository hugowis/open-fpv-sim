# M1 carried debt

These findings came out of the M1 task reviews and the final whole-branch review. Each was deliberately left out of M1. They are listed so the M2 planning can pick them up.

## Server and session lifecycle
- **Resolved in M2a:**
  - graceful `ofs-sim` shutdown on Ctrl-C/SIGTERM;
  - `Run` stops within 50 ms when its client goes away;
  - a Python client's session ends when the client disconnects (unless `keep_alive`);
  - firmware directories are keyed on the quad file's path.
- **Non-loopback `--listen` (still open).** `Load` reads any path and executes that quad file's `fc.launch` argv, and TOML parse errors echo file contents back. Refuse or warn on non-loopback binds unless an explicit flag is given, before M2b exposes the server to more clients.

## SITL bridge
- **Stray second reply after a resend.** If SITL received both copies of a resent first datagram, the second reply could arrive after the next drain and leave a one-tick lag. This is now rare: SITL ignores packets until it is ready and the bridge waits for its ready line, so resends only happen on lost datagrams. A settle-and-drain after a resent first exchange would close it fully.
- **Reply sender not checked.** The bridge does not check that a reply comes from the expected SITL address.
- **Not-found hint only.** The `FcError::Launch` hint covers only "program not found". A wrong `.elf` path behind `wsl.exe` surfaces as a startup error, without the hint.
- **`net::run()` has no timeout.** A hung `wsl.exe` stalls the vehicle build.
- **`OFS_SITL_LAUNCH` paths cannot contain spaces.** The value is split on whitespace.
- **`assert!(motor_count <= 4)` panics** instead of returning a config error. Config validation enforces 4 motors, so this is unreachable today.

## Models and config
- **NaN or inf in array config fields passes validation.** This covers motor positions, contact points, IMU biases and the initial pose. It surfaces later as a `NonFinite` simulator error instead of a config error.
- **Table preconditions are enforced only by config validation.** Prop and battery tables must be non-empty and sorted. The models themselves trust them.
- **The scheduler is unusable after an error.** The server poisons the session in that case.
- **`run_for` has no upper bound and rounds silently.** The scheduler also does not reject duplicate model names.
- **The non-finite check scans the full bus every tick.** This is a performance hotspot for later.
- **Physics simplifications:**
  - no rotor gyroscopic coupling;
  - friction is viscous below 5 cm/s;
  - `BODY_ACCEL_NED` is coordinate acceleration (it includes gravity);
  - the published motor `omega_dot` is the pre-clamp derivative.
- **ISA pressure is NaN above ~44 km.** The barometer noise path is untested.
- **Test gaps:**
  - digest Vec3/Quat and non-finite Scalar/Quat cases;
  - rigid-body contact torque, friction and damping terms;
  - the propeller model's name and rate divisor;
  - the gRPC `run_before_load` status code;
  - the gRPC tests share one temp dir.

## CI and tooling
- **The CI workflow has not run yet.** That covers the core, sitl and windows jobs; there is no remote.
- **No cargo/pip caching.** The SITL job rebuilds Betaflight on every run.
- **14 pre-existing clippy `result_large_err` warnings** in `crates/ofs-sim/src/server.rs`. Clippy is not run in CI.
- **The native-Linux Python hover path is unverified.** WSL has no grpc Python module; the Rust live bridge test does pass natively on Linux.

## M0 open items still open
- Betaflight Configurator connection. Real-time mode now exists (M2a); the manual check is pending the user, see `docs/dev-setup.md` and `docs/research/sitl-interface.md` §8.
- SmartAudio reply.
- Heading/quaternion mapping for the compass.
- One intermittent, unexplained MSP timeout.
