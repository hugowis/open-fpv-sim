# Changelog

All notable changes, by release of the Python package `open-fpv-sim` (the simulator is built from this
repository). The format follows Keep a Changelog.

## Unreleased — M6d: deferred minors
### Changed
- The loader refuses what it used to accept: a prop part with `windmill_j0` between 0 and 0.2 (0 still turns windmilling
  off; a small value is a runaway rotor), and a battery whose `resistance_growth_per_cycle × cycles` makes the pack more
  than ten times as resistive. A non-finite `[video]` threshold is reported on its own field.
- `ofs-validate`: a `submission.toml` error names its line; `--max-error-pct` now fails (exit 1) when a row never
  settled; the settle test is one part in a million per second as documented (it was four times looser), so a slow
  rotor settles a little later and closer to its steady state. Without the flag the exit code is unchanged.
- A repro bundle's `journal.txt` must open with its `# ofs-journal 1` header (a byte-order mark is accepted), and
  `ofs.repro.find_bundles` skips a folder that has no `bundle.toml` yet (still being written). `replay_bundle` reads
  `ofs-sim`'s output as UTF-8, so accented paths and Windows error texts come back readable.
### Fixed
- Stale comments and docs: the cell voltages are the battery model's own, not equal shares.

## Unreleased — M6c: validation and the repro bundle
### Added
- A failed session writes a **repro bundle**: a folder under the logs directory (`<quad>-<n>.repro`) with the quad,
  the library and world it used, the seed, a journal of everything that steered the run, the firmware's state from
  `Load` and the last 10 seconds of its channels. `ofs-sim --replay <folder>` and `ofs.repro.replay_bundle` run it again
  and say whether the failure came back (`docs/repro-bundle.md`). Every session now keeps that 10 s tail in memory (about 15 MB
  for the shipped quad; it costs about 6 % of the open-loop real-time factor, 275x to 257x) and a journal of its
  steering calls.
- `ofs-validate`, the validation harness: a documented submission format for community measurements (`thrust_stand`,
  and `blackbox_step` as a checked format), a checker that reports every problem with its file and line, and a
  thrust-stand comparator that runs the simulator's own ESC, motor and propeller to steady state at each measured row.
  A synthetic worked example and its generated report are under `validation/`; CI prints the report in the job summary
  (tracked, not gating). See `docs/validation.md`.
- `ofs.repro` (`find_bundles`, `replay_bundle`), and `ofs_config::load_with_library_env`.
### Changed
- The `sim_error` event's text now ends with `(repro bundle: <folder>)` (or why there is none).
- `ofs-sim --data-dir` has no default shown in `--help` any more (it is still `.ofs-data`); with `--replay` it defaults
  to a temporary directory.

## Unreleased — M6b: battery, thermal and receiver
### Added
- The video receiver's thresholds are world data: an optional `[video]` table in a world file sets the SNR
  thresholds, field counts and drop probability (`docs/research/video-link.md`). Without it nothing changes.
- A per-cell battery: the pack is a series string of cells with their own resistance, state of charge and capacity;
  battery parts may set a per-cell spread and ageing (`cycles`, capacity fade, resistance growth), the library has an
  aged example pack, and recordings gain `battery.cell.{k}.voltage_v` and `.soc` channels
  (`docs/research/battery-and-thermal.md`).
- A thermal model: one RC node per ESC, motor and VTX, heated by the losses already on the bus, with the ambient
  set by an optional `[environment] ambient_c` in the world file. Parts carry optional `thermal` sections (the library
  parts ship estimated constants), and recordings gain `esc.{i}.temp_c`, `motor.{i}.temp_c` and `vtx.temp_c`
  (`docs/research/battery-and-thermal.md`). Nothing derates.
### Changed
- The weak-cell fault lands on the cell it names, and the state stream's cell voltages are the battery model's own
  (they used to be equal shares of the pack voltage).
- The recording pattern `battery.*` now also matches the per-cell channels (16 signals on the 6S pack instead of 4); a
  script that counted them, or that relied on `battery.voltage`'s near-miss suggestions, needs a narrower pattern.
- The KISS telemetry frames carry each ESC's simulated temperature and Tramp's status reply the VTX's, instead of a
  constant 25 °C (which is what they still report with no heating, at the default ambient).
- The radio goldens' digests were regenerated for the new recorded channels.

## Unreleased — M6a: rotor and frame physics
### Added
- Rotor gyroscopic coupling: the spinning rotors' angular momentum now pushes back on a turning airframe
  (`docs/research/rotor-dynamics.md`).
- Windmilling: a propeller's torque now depends on the air coming up through its disc, so an unpowered rotor in a fast
  descent keeps turning. The prop part's new optional `windmill_j0` sets it (0 = off), and recordings gain the
  `prop.{i}.inflow_mps` channel.
- A flight-controller mount: an optional `[frame.fc_mount]` (`natural_hz`, `damping_ratio`) in a frame part passes the
  motors' vibration through a second-order transmissibility before the IMU feels it, and the library part
  `opendrone-5f-softmount` is a soft-mounted example. A frame without it is rigid, as before.
### Changed
- Flights where the clockwise and counter-clockwise rotors do not stay balanced (a yaw input, a failed or damaged
  motor) differ from 0.1.0; balanced flights, such as the radio goldens' roll and pitch steps, are bit-identical.
- Recordings carry one more channel per motor (`prop.{i}.inflow_mps`), so the radio goldens' digests were regenerated.
  A descent now lowers a prop's torque by the advance-ratio correction (`prop.windmill_j0` on the shipped prop).

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
