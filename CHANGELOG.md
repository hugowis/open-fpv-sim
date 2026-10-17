# Changelog

All notable changes, by release of the Python package `open-fpv-sim` (the simulator is built from this
repository). The format follows Keep a Changelog.

## Unreleased

Nothing yet.

## 0.2.0 — 2026-10-17 (phases 2 and 3)
Milestones M5 to M8c. Phase 2 made the models more accurate and the simulator verifiable; phase 3 hardened it, added
race timing and made flights scriptable.
- **A swappable radio (M5):** the radio is three parts (`Handset`, `Link`, `Receiver`) with an ExpressLRS link on 2.4
  GHz or 900 MHz and a geometry-free `fixed` link; a live flight's sticks can be recorded to a `play_inputs` CSV (the
  game's **I** key, `sim.record_inputs()`).
- **More accurate physics and electrics (M6a, M6b):** rotor gyroscopic coupling, windmilling, a soft FC mount, a
  per-cell battery with ageing, a thermal model for the ESCs, motors and VTX, and the video receiver's thresholds as
  world data.
- **Verifiable (M6c):** a failed session writes a repro bundle that `ofs-sim --replay` and `ofs.repro.replay_bundle`
  run again; `ofs-validate` is a harness for community measurements (`docs/validation.md`).
- **Hardened (M6d, M6e, M7a):** stricter config checks, validated and clamped sticks, an inputs recording that always
  finishes, a Blackbox log that is finished even when a session ends armed, a Betaflight reboot that no longer wedges
  when the client hangs up, clearer errors from the recording reader, and two radio fixes (a burst-loss window edge,
  the 2.4 GHz 50 Hz sensitivity).
- **Race timing (M8a):** `[[gates]]` in the world file, split and lap events, a lap clock in the state and on the
  game's HUD (`docs/race-timing.md`).
- **Scripted flight (M8b):** an opt-in angle-mode stabilizer for open-loop sessions, a four-gate circuit
  (`worlds/circuit.toml`) and a pilot script that flies a lap on it and on real Betaflight
  (`docs/scripted-flight.md`).
- **Tooling (M8c):** a CI gate against a recorded benchmark baseline, and a recording reader 4 to 10 times faster.

**Upgrading from 0.1.0**
- The gRPC protocol is version 12 (0.1.0 spoke 9): a 0.1.0 client or server is refused at the handshake, so install the
  Python package and build `ofs-sim` from the same release.
- Rebuild Betaflight SITL with `scripts/build-sitl.sh`: the patch changed (a reboot no longer waits on a reply nobody
  reads).
- Rust API: `Vehicle::set_sticks` returns a `Result` (a non-finite stick is refused; the rest is clamped).
- Recordings differ from 0.1.0: new channels (`prop.{i}.inflow_mps`, `battery.cell.{k}.*`, `esc.{i}.temp_c`,
  `motor.{i}.temp_c`, `vtx.temp_c`, and `race.*` in a world with gates), the pattern `battery.*` matches 16 signals on
  the 6S pack instead of 4, and the temperatures are recorded at 100 Hz. Flights whose clockwise and counter-clockwise
  rotors are unbalanced (a yaw input, a failed motor) differ from 0.1.0 because of gyroscopic coupling; balanced
  flights are bit-identical. Scripts that count channels or compare recordings byte for byte need a look.
- The loader refuses a prop part with `windmill_j0` between 0 and 0.2 and a battery whose resistance grows more than
  tenfold.
- Every session now keeps a 10 s tail and a journal in memory for the repro bundle (about 15 MB for the shipped quad,
  about 6 % of the open-loop real-time factor).

The detail by milestone follows.

### M8c: a performance gate and a faster reader
#### Added
- **A benchmark regression gate (#61).** CI's `scripts/benchmark.py` step compares every measured row with a baseline
  recorded for its runner class in `scripts/benchmark-baseline.json` (the median of five green runs on `ubuntu-24.04`:
  open loop 47.9× with recording and 131.9× without, Betaflight 10.0× and 11.8×) and fails a row under 60 % of it, on
  top of the old 1× floor. Each row is the best of two runs (`--repeat 2`) so the 4 % noise between CI runs cannot
  trip it; skipped rows and rows without a baseline are never gated. A deliberate slowdown is accepted by updating the
  baseline in the PR or by setting the repository variable `OFS_BENCH_ACCEPT_SLOWER` to a reason (the table then says
  `ACCEPTED: <reason>`). `--print-baseline` prints the measured rows as baseline JSON.
- `scripts/bench_reader.py`: times the reader against the previous implementation on a fresh recording.
#### Changed
- **`ofs.reader.read` is 4 to 10 times faster (#45)**, with the same signature, result and error messages. On a
  one-minute open-loop recording (27.6 MB, 148 signals) three patterns take 0.34 s instead of 3.3 s and every signal
  2.2 s instead of 9.8 s. It cuts only the selected signals' records out of each chunk using the chunk's message
  indexes. Recordings without message indexes or with lz4 chunks (not what the server writes) are read the old way.

### M8b: scripted flight
#### Added
- **An opt-in stabilizer for open-loop sessions (#66).** `load(..., open_loop_fc=True, stabilize=True)` (Rust:
  `FcKind::OpenLoopStabilized`, or `[fc] kind = "open_loop_stabilized"` in a quad file) adds a minimal angle-mode
  controller, so the roll and pitch sticks set the lean (up to 60 degrees), the yaw stick the turn rate (up to 200 degrees
  per second) and a script can fly a path without Betaflight. It is a test stand-in, not Betaflight: it reads the
  simulator's truth and has no arming, rates, I-term or filters, and real tuning needs SITL. `open_loop_fc` alone is
  unchanged (throttle to every motor). `docs/scripted-flight.md`.
- **A scripted circuit (#53).** `worlds/circuit.toml` (four gates on a 40 m square) and `python/examples/circuit.py`, a
  pilot that steers with the sticks from the quad's state and flies a lap through the gates. `python/tests/test_circuit.py`
  asserts the lap on the stabilizer in every CI run; `python/tests/test_sitl_circuit.py` asserts it on real Betaflight in
  the `sitl` job.
#### Changed
- **Breaking (protocol):** the protocol is version 12 (`LoadRequest.stabilize`), so a client of protocol 11 is refused by
  the handshake. `stabilize` without `open_loop_fc` is refused with `invalid_argument`. A repro bundle records
  `stabilize` (a bundle without the key was never stabilized).

### M8a: gates and lap timing
#### Added
- **Race timing (#71).** A world file declares an ordered course with `[[gates]]` (`name`, `center_ned_m`, `heading_deg`,
  `width_m`, `height_m`; at most 64; gate 0 is the start and finish). The simulator counts the gates the quad flies
  through in order, interpolating each crossing inside the tick, and times the splits and laps (`docs/race-timing.md`).
  `worlds/flat.toml` has three gates.
- Events `gate_passed` and `lap_completed` (messages such as `Gate1 (gate 1) split 8.412 s` and `lap 1: 31.204 s (best)`),
  `State.race` (`present`, `running`, `next_gate`, `gate_count`, `laps`, `current_lap_s`, `last_lap_s`, `best_lap_s`),
  `World.gates`, and `race.*` signals in a recording. A world without gates has none of them.
- The Godot client draws each opening as a translucent rectangle labelled START or with its number, shows a lap line on
  the HUD and toasts the events. Python: `ofs.Gate`, `ofs.Race`, `World.gates`, `State.race`.
#### Changed
- **Breaking (protocol):** the protocol is version 11, so a client of protocol 10 is refused by the handshake. The
  Rust and Python clients in this repository speak it.

### M7a: hardening sweep
#### Changed
- **Breaking (Rust API):** `Vehicle::set_sticks` returns a `Result`: a non-finite stick is refused with an `InvalidArgument` error and
  nothing is applied; what is accepted is clamped to the transmitter's range (sticks to [-1, 1], throttle to [0, 1])
  before the bus, an inputs recording and the journal see it (#54). The gRPC `SetSticks` already refused non-finite
  values; out-of-range ones are now clamped on the bus too (the `rc.*` channels of a recording show the clamped values).
- The inputs recorder writes no row for a change that came back to the written row within the same tick, and always
  writes a first row (#54).
- `Load` and `Unload` stop a running inputs recording, so its file is complete, and publish `inputs_recording_stopped`
  (#55).
- A session on Betaflight that ends while the quad is armed disarms it first (the link and the radio faults are
  restored on the bus so the disarm reaches it, then it steps until the motors read disarmed, 3 simulated seconds at
  most), so its Blackbox log is finished; it used to be cut short, notably when the pilot had left first, which is the
  Godot client's quit order. A running MCAP recording sees those steps; the journal and an inputs recording do not
  (#58). A session that is not armed, or has failed, does not pay for it.
- Reading a recording the Python reader cannot use now says why instead of an mcap record error (#59): `unfinished`
  (no footer: a write error such as a full disk, or a killed server), `damaged` (a half-written last chunk) or
  `not an MCAP file`, each pointing at the `recording_stopped` event where relevant. The writer finishes the file
  best-effort after a write error (a test holds that), but the record being written can be left half written.
- Radio: a packet exactly on a burst-loss window edge belongs to the window that starts there, for the ELRS and the
  fixed link alike (#13), and the 2.4 GHz
  50 Hz sensitivity is -115 dBm, ExpressLRS's published figure, instead of -117 (#12). Both change seeded runs and the
  ELRS golden digests (regenerated on both OSes, in their own commits).
#### Fixed
- The patched Betaflight SITL no longer wedges a reboot when the MSP client hangs up right after `MSP_REBOOT` (#57):
  rebuild SITL with `scripts/build-sitl.sh`.
- The SITL port-bind flake (#14) was fixed in the patch on 2026-10-15 (`6ccbaef`, `ac97cad`); no recurrence since.

### M6e: the rest of the M6 minors
#### Changed
- The default recording table records the ESC and motor temperatures (`esc.{i}.temp_c`, `motor.{i}.temp_c`) at 100 Hz,
  the thermal model's own rate, instead of 240 Hz (they only change at 100 Hz). Recordings and the radio-flight
  golden digests change accordingly (both OS files regenerated); the simulation itself does not. A Python selection
  such as `to_dataframe(["esc.*"])` now mixes two rates (242 Hz and 100 Hz), so its columns have gaps on the union time
  grid: `.dropna()` a column before plotting it, as for any mixed-rate selection.
- ESC telemetry and the VTX built without a thermal model (test rigs) read the default 25 °C ambient, not 0 °C.
  `ofs_core::Bus::signal_or` and `ofs_core::consts::DEFAULT_AMBIENT_C` are new.

### M6d: deferred minors
#### Changed
- The loader refuses what it used to accept: a prop part with `windmill_j0` between 0 and 0.2 (0 still turns windmilling
  off; a small value is a runaway rotor), and a battery whose `resistance_growth_per_cycle × cycles` makes the pack more
  than ten times as resistive. A non-finite `[video]` threshold is reported on its own field.
- `ofs-validate`: a `submission.toml` error names its line; `--max-error-pct` now fails (exit 1) when a row never
  settled; the settle test is one part in a million per second as documented (it was four times looser), so a slow
  rotor settles a little later and closer to its steady state. Without the flag the exit code is unchanged.
- A repro bundle's `journal.txt` must open with its `# ofs-journal 1` header (a byte-order mark is accepted), and
  `ofs.repro.find_bundles` skips a folder that has no `bundle.toml` yet (still being written). `replay_bundle` reads
  `ofs-sim`'s output as UTF-8, so accented paths and Windows error texts come back readable.
#### Fixed
- Stale comments and docs: the cell voltages are the battery model's own, not equal shares.

### M6c: validation and the repro bundle
#### Added
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
#### Changed
- The `sim_error` event's text now ends with `(repro bundle: <folder>)` (or why there is none).
- `ofs-sim --data-dir` has no default shown in `--help` any more (it is still `.ofs-data`); with `--replay` it defaults
  to a temporary directory.

### M6b: battery, thermal and receiver
#### Added
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
#### Changed
- The weak-cell fault lands on the cell it names, and the state stream's cell voltages are the battery model's own
  (they used to be equal shares of the pack voltage).
- The recording pattern `battery.*` now also matches the per-cell channels (16 signals on the 6S pack instead of 4); a
  script that counted them, or that relied on `battery.voltage`'s near-miss suggestions, needs a narrower pattern.
- The KISS telemetry frames carry each ESC's simulated temperature and Tramp's status reply the VTX's, instead of a
  constant 25 °C (which is what they still report with no heating, at the default ambient).
- The radio goldens' digests were regenerated for the new recorded channels.

### M6a: rotor and frame physics
#### Added
- Rotor gyroscopic coupling: the spinning rotors' angular momentum now pushes back on a turning airframe
  (`docs/research/rotor-dynamics.md`).
- Windmilling: a propeller's torque now depends on the air coming up through its disc, so an unpowered rotor in a fast
  descent keeps turning. The prop part's new optional `windmill_j0` sets it (0 = off), and recordings gain the
  `prop.{i}.inflow_mps` channel.
- A flight-controller mount: an optional `[frame.fc_mount]` (`natural_hz`, `damping_ratio`) in a frame part passes the
  motors' vibration through a second-order transmissibility before the IMU feels it, and the library part
  `opendrone-5f-softmount` is a soft-mounted example. A frame without it is rigid, as before.
#### Changed
- Flights where the clockwise and counter-clockwise rotors do not stay balanced (a yaw input, a failed or damaged
  motor) differ from 0.1.0; balanced flights, such as the radio goldens' roll and pitch steps, are bit-identical.
- Recordings carry one more channel per motor (`prop.{i}.inflow_mps`), so the radio goldens' digests were regenerated.
  A descent now lowers a prop's torque by the advance-ratio correction (`prop.windmill_j0` on the shipped prop).

### M5: swappable radio components
M5 was specified as a real radio over USB serial; the capture found no CRSF stream on the TX15's port
(`docs/research/serial-radio.md`), so the joystick stays the input and the milestone became the radio's structure and
recording a live flight.
#### Added
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
#### Changed
- `link_stats_interval_packets` moved from the link to the receiver internally; quad files are unchanged.
- The protocol is version 10: a client built before M5 is refused at the handshake.

### the v1 close-out
#### Fixed
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
#### Added
- `scripts/benchmark.py`: the real-time factor of headless runs, in CI's job summary (fails below 1×).
- Tests: the ELRS link test runs in CI; a PID saved over MSP persists through a reboot and a new session; a real
  tap overflow counts Betaflight's lost bytes; same-seed lockstep runs with latency and field drops write identical
  recordings; the DVR end to end in the Godot e2e and its JPEG encode; the radio fault cannot move the model's own
  random stream; MSP replies split across reads; the 50 km test reads Betaflight's own RX-loss flag.
- `docs/roadmap.md`, `docs/architecture.md`, `CONTRIBUTING.md`, this changelog.
#### Changed
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
