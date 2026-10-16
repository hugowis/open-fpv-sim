# Changelog

All notable changes, by release of the Python package `open-fpv-sim` (the simulator is built from this
repository). The format follows Keep a Changelog.

## Unreleased — M5: swappable radio components
M5 was specified as a real radio over USB serial; the capture found no CRSF stream on the TX15's port
(`docs/research/serial-radio.md`), so the joystick stays the input and the milestone became the radio's structure and
recording a live flight.
### Added
- The radio as three swappable parts, `Handset`, `Link` and `Receiver`, stepped by `RadioChain`
  (`crates/ofs-radio`): the `joystick` handset, the `elrs` and `fixed` links and the `crsf` receiver.
- ExpressLRS on 900 MHz: a `band` key (`"2g4"`, the default, or `"900"`) on the `elrs` link, and the quad
  `quads/opendrone-5f-900mhz.toml`. The 900 MHz band's carrier, bandwidth, noise figure and downlink power are
  estimates.
- The `fixed` link (a geometry-free loss fraction, bursts and latency) and its `[radio.fixed]` table, with the quad
  `quads/opendrone-5f-fixed-link.toml`.
- `[radio]` keys `handset` and `receiver` (each defaults to the only variant it has), and a test that swaps a link
  defined inside a test file into a flying chain.
- Recording a live flight's sticks to a `play_inputs` CSV: protocol 10 (`StartInputsRecording`,
  `StopInputsRecording` returning `{path, rows, duration_s}`, the events `inputs_recording_started` and
  `inputs_recording_stopped`), Python `sim.record_inputs()` and `sim.stop_inputs_recording()`, and the game's **I**
  key with a `REC STICKS` indicator. Two lockstep replays of one recording write byte-identical MCAP.
- Golden tests that pin the ELRS link's bytes and a recorded flight's digests, one digest file per OS (the libm
  differs between Windows and Linux).
- `docs/writing-a-radio-model.md` and `docs/research/serial-radio.md`.
### Changed
- `link_stats_interval_packets` moved from the link to the receiver internally; quad files are unchanged.
- The protocol is version 10: a client built before M5 is refused at the handshake.

## Unreleased — the v1 close-out
### Fixed
- The first boot's diff apply is judged on the config run's exit and its whole output: a run that failed no longer
  passes because SITL had created `eeprom.bin`, and a good run is no longer refused when its last line was still in
  the pipe.
- A SITL that cannot bind its ports now says which port failed and who held it at the moment of the bind.
- A DVR start on an existing file fails instead of truncating the earlier recording.
- The DVR keys its frames on an integer field index, so a long flight can no longer stretch a recording by a frame;
  the video field rate is defined once (`ofs-core`).
- The goggles' latency is reported without a transmitting VTX.
- The burst-loss validation message states its units (seconds).
- The pre-launch SITL cleanup waits for its victims to exit before the next launch binds (the port-bind flake
  caught by CI); under WSL the wait runs in the VM, and a victim that ignores SIGTERM is SIGKILLed after a grace.
### Added
- `scripts/benchmark.py`: the real-time factor of headless runs, in CI's job summary (fails below 1×).
- Tests: the ELRS link test runs in CI; a PID saved over MSP persists through a reboot and a new session; a real
  tap overflow counts Betaflight's lost bytes; same-seed lockstep runs with latency and field drops write identical
  recordings; the DVR end to end in the Godot e2e and its JPEG encode; the radio fault cannot move the model's own
  random stream; MSP replies split across reads; the 50 km test reads Betaflight's own RX-loss flag.
- `docs/roadmap.md`, `docs/architecture.md`, `CONTRIBUTING.md`, this changelog.
### Changed
- The per-milestone carried-debt files moved to `docs/superpowers/history/`; the roadmap replaces them.
- The OSD font atlas's GPL-3.0-only licence is accepted and documented (README, Licence).

## 0.1.0 — 2026-10-13 (v1)
The first release: milestones M0–M4 of `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.
- **Headless core (M0, M1):** real Betaflight SITL in exact lockstep with 6-DOF physics, battery, ESC and motor,
  IMU and barometer models; bit-identical runs with the same seed.
- **Radio and pilot (M2):** the ExpressLRS/CRSF link with failsafe, real-time pacing, the firmware lifecycle, the
  Betaflight Configurator over TCP, and the Godot pilot client with joystick input and an FPV camera.
- **Video (M3):** Betaflight's own OSD over MSP DisplayPort; SmartAudio and Tramp VTX control; a 5.8 GHz analog
  link model that breaks the picture up with distance, attitude, buildings and interference; solid world objects;
  a geometric ELRS link; the FC's own telemetry home; video latency, field drops and a DVR.
- **Sandbox (M4):** a component library with recorded provenance; the v1 fault catalog (motor, prop, battery cell,
  radio loss and bursts, gyro, VTX interference), injectable at a simulated time; MCAP recording (byte-identical
  across identical lockstep runs), pandas/CSV/Parquet export, scripted stick inputs, Betaflight's Blackbox logs, a
  live Rerun view; the `open-fpv-sim` package on PyPI.
