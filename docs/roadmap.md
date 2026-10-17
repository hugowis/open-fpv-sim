# Roadmap and known limitations

v1 — milestones M0 to M4, `open-fpv-sim` 0.1.0 on PyPI (2026-10-13) — is complete: see the parent spec
(`docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`) for what it set out to do and the evidence that it does.
Phases 2 and 3 (M5 to M8c: swappable radio, accuracy and validation, hardening, race timing and scripted flight) are
release 0.2.0 (`CHANGELOG.md`).
This file is what the simulator deliberately does not do, and what comes next. It replaces the per-milestone carried-debt
files (kept as history in `docs/superpowers/history/`); the v1 close-out's fixes are in `CHANGELOG.md`.

## Known limitations of v1

### Physics (`docs/research/collision.md`)
- Friction is viscous below 5 cm/s; `body.accel_ned` is coordinate acceleration (it
  includes gravity); a motor's published `omega_dot` is the derivative before clamping.
- The IMU's analytic vibration rate term (the gyro disturbance from prop imbalance) still ignores rotor gyroscopic
  coupling, which M6a added to the rigid-body dynamics only.
- Two contact mechanisms, one per job: hits get impulses, resting weight gets the spring-dampers, so a hard
  touchdown both bounces and compresses the legs. Position correction moves only the deepest contact out.
- The ground is the plane d = 0, boxes are axis-aligned, and objects never collide with each other.

### Electrical
- The thermal model has no airflow dependence (the thermal resistance is a constant) and nothing derates on
  overtemperature; with the estimated constants the ESC warms only a couple of kelvin at this model's phase currents
  (`docs/research/battery-and-thermal.md`).

### Radio (`docs/research/elrs-link.md`)
- One frequency (2440 MHz) stands for the whole hop sequence; the body shadow is the video link's smooth lobe, not
  a measured 2.4 GHz pattern; the handset transmits on the antenna it receives best on.
- Telemetry slots are a fixed 1:4 ratio of the downlink (`TELEMETRY_SLOT_DIVISOR`), and the shared RC/telemetry
  UART has no contention model.
- The `RadioLoss` fault acts on the uplink only.
- Every constant is an estimate or a published figure; none is measured. The 900 MHz band's carrier, bandwidth,
  noise figure and downlink power are estimates (its sensitivities are ExpressLRS's published ones).
- The radio is three swappable parts (`docs/writing-a-radio-model.md`), but only one handset (`joystick`), one
  receiver (`crsf`) and two links (`elrs`, `fixed`) ship; a new one is a Rust module, a match arm and a config enum
  variant, in-tree.

### Inputs recording (`docs/superpowers/specs/2026-10-15-m5-radio-components-design.md` §6)
- A replay is not the real-time flight: real-time timing, the SITL schedule and the link's seeded draws differ. The
  CSV has no transmitter column, so a link cut (**K**) is not captured.
- `Load` and `Unload` stop a running inputs recording and publish `inputs_recording_stopped`. A session that vanishes
  (its last watcher left, the server shut down) finishes the file without an event, so a client that tracks the
  recording state still resets it on a lost session, as the Godot game does.
- `PlayInputs` is refused while an inputs recording runs (playback writes the bus directly, so a scripted segment
  would be missing from the file): stop the recording first.

### Video (`docs/research/video-link.md`)
- One knife edge per object, losses of several objects added; reflections off the ground only, and the reflected
  ray is not obstructed separately; one body-shadow lobe of up to 8 dB.
- Every video constant is an estimate; the receiver's thresholds are world data (`[video]`) but none is measured.
- The DVR stops when the picture's size changes mid-recording, and it records what the pilot sees (no clean feed).

### Flight controller and SITL (`docs/research/sitl-interface.md`)
- One SITL per machine: its ports are fixed (UDP 9001–9004, TCP 5760+).
- The motor protocol is PWM; Betaflight's RPM filter is fed by the simulator's eRPM block, not a DShot driver.
- A session whose scheduler failed is poisoned by design; a new `Load` starts a fresh one.
- A session that ends while the quad is armed disarms Betaflight first (restoring the link, stepping until the motors
  read disarmed, 3 simulated seconds at most), so its Blackbox log is finished; those steps are not journaled, but a
  running MCAP recording sees them.
- A client may close its MSP connection right after `MSP_REBOOT`: the patched SITL drops the reply nobody is
  listening for, so the reboot completes (it used to wedge, `docs/superpowers/history/closeout-evidence.md`).
- Betaflight SITL aborted once with heap corruption while resetting (glibc `tcache` check), so a reboot failed as a
  firmware crash; not seen again in 38 reboots. Inside SITL's teardown, upstream of the simulator.
- The port-bind flake (a SITL port failing to bind at launch) had two causes, both fixed in
  `third_party/betaflight/ofs-sitl.patch`: a racing `dyad_update` against stream setup (`6ccbaef`) and a
  single-attempt serial bind (`ac97cad`, now a bounded retry of 20 tries at 25 ms). The pre-launch cleanup also waits
  for its victims to exit, and a launch reports whether one existed and who holds a port that fails to bind
  (`docs/superpowers/history/closeout-evidence.md`). It has not recurred: the `sitl` job passed in all 12 CI runs from
  2026-10-16 to 2026-10-17 (the one failed run failed in `core`).

### Logging (`docs/research/mcap-determinism.md`)
- Recording rates are whole divisors of the tick: 240 Hz on an 8 kHz quad records at 242.4 Hz (the channel's
  `rate_hz` metadata says so).
- A recording stopped by a write error has no summary section, so indexed readers refuse it; the
  `recording_stopped` event names the error.
- Opening a recording in Foxglove while it is still being written is untested.

### Race timing (`docs/race-timing.md`)
- A gate is a trigger on the quad's centre: no margin for its size, no tilt. There is no lap limit and no reset (a
  new `Load` starts a fresh race), and Godot does not highlight the next gate.

### Python
- The live view follows the state stream (up to 240 Hz); the 8 kHz signals are only in the recording.
- The reader is pure Python. On a one-minute open-loop recording (27.6 MB, 148 signals) it takes 0.34 s for three
  patterns and 2.2 s for every signal; the `mcap` iterator it replaced took 3.3 s and 9.8 s
  (`scripts/bench_reader.py`). The fast path needs message indexes (the server's recordings have them) and zstd or
  uncompressed chunks; any other file is read the slow way.
- Open-loop sessions fly a scripted path only with the opt-in stabilizer (`stabilize=True`, `docs/scripted-flight.md`), a
  test stand-in: angle mode on the simulator's truth, no filters, arming, rates or I-term, gains tuned for the shipped
  quad. Real tuning needs Betaflight SITL, which also needs WSL on Windows.

### Performance
- Headless lockstep runs at 7.4× real time with Betaflight SITL (WSL 2) and 84–296× open loop, on an AMD Ryzen 9
  9900X3D (`docs/dev-setup.md`, Performance), 12.4× on CI's Linux runner: above the 2× target. CI fails below 1×, and when a row falls under 60 % of the
  baseline recorded for CI's runner (`scripts/benchmark-baseline.json`).

## Post-v1 backlog

Phase 2 (M5 swappable radio components, M6 accuracy and validation) is done. M6 delivered the rotor and frame physics
(M6a: gyroscopic coupling, windmilling, the flight controller's mount response; `docs/research/rotor-dynamics.md`), the
per-cell battery, the thermal model and the receiver thresholds as world data (M6b;
`docs/research/battery-and-thermal.md`), and the validation harness and the repro bundle (M6c; `docs/validation.md`,
`docs/repro-bundle.md`). What is left of the epic "physics, electrical and RF modelling" is the list below; replacing the
`estimated` constants with measurements is what the validation harness is for.

### Fidelity (parent spec §1.5, §2)
- Level 2: AM32 and ExpressLRS as host-compiled firmware in the loop. Level 3 (MCU emulation) stays out of scope,
  but the interfaces must not preclude it.
- A real radio over serial: the TX15's USB-VCP does not stream CRSF (`docs/research/serial-radio.md`); the untested
  routes are the external module bay with a USB-UART adapter, and a Lua script over `serialWrite`. **Blocked on a
  verified route.**
- Crossfire and Tracer links: a profile or a `Link` each now that the seams exist; they need sourced figures.
- Non-CRSF receivers (SBUS, iBUS, FrSky, DSMX): a `Receiver` each, with the matching Betaflight config.
- Plugin loading for radio models from outside the tree.

### Modelling
- `RadioLoss` on the downlink.
- Non-flat terrain and rotated boxes; a frequency-dependent `rf_loss_db`; collisions between objects; crash damage.
- ExpressLRS dynamic power and frequency hopping; 2.4 GHz interference.
- A configurable telemetry slot ratio; DShot extended telemetry and MSP over CRSF.
- A compass, and the heading mapping it needs.

### Video and OSD
- OSD font pages 2–3 (warnings, criticals) and the full HD/digital OSD element set.
- Digital video systems (parent spec §2).
- A DVR that survives a resize; a clean-feed DVR; audio; image-sequence and ffmpeg export; a DVR toggle on the F2
  controls screen (v1 ships the **D** key).

### Sandbox and component library
- (C) plugin models (parent spec §1.5) and a stable plugin API.
- The repro bundle as a single archive (it is a folder today, `docs/repro-bundle.md`), and a Configurator's EEPROM
  changes made after `Load`.
- `fcs/`, `receivers/` and `cameras/` part kinds; the IMU and barometer as parts; provenance exposed over the API;
  `OFS_LIBRARY` as a search path.

### Tooling
- `mcap` is pinned to an upstream git revision (00e47a1b): switch to the crates.io release that carries
  foxglove/mcap#1838 once it ships, and re-run `crates/ofs-log/tests/determinism.rs`.
- `rerun-sdk` is pinned to 0.39.x by the `rerun` extra: bump it deliberately (its Python API changes between minors).

### Validation (parent spec §8.5)
- Real measurements: the harness and the submission format exist (`docs/validation.md`, with a synthetic worked example),
  so every `estimated` constant in the library waits only for data. Thrust-stand submissions are compared today.
- The step-response comparator: simulated against real Blackbox step responses (the `blackbox_step` format is defined and
  checked; the comparison needs SITL and a real tune).
- A spectral proof that the RPM filter's notches track the motors, from a Blackbox log.

## Rulings worth remembering
These explain behaviour that would otherwise look like a bug.

- **The VTX transmits in open loop**: a powered VTX transmits whether or not a flight controller talks to it.
- **Obstruction is found by a golden-section search** on the shape's signed distance, so a thin post on a long path
  is not stepped over; objects standing on the ground are rooted below it.
- **Emitters may be 5300–6000 MHz** (Lowband is one of the VTX's own bands).
- **The sync-loss test is at 4 km and 25 mW**: at 2 km the shipped diversity goggles sit at the edge.
- **Prop spheres are 2.0 cm**, not 1.5: the larger radius keeps a 12 cm post from passing between them.
- **`collision_speed_mps` is the last collision's speed**: a rebound over 1 m/s raises a second event.
- **A link that starts down raises no `LINK_DOWN`**: the event marks an up-to-down transition.
- **The motor protocol stays PWM**; **the FC telemetry is outside the lockstep digest** (determinism is pinned by
  the ELRS byte-stream test and the live runs); **battery values pass through Betaflight verbatim**, including its
  ESC-sensor consumption summed over motor slots and its 0.1 V units.
- **A stale SITL binary is refused loudly but harmlessly**: it prints one `no such open UART` line per datagram.
- **SITL quads need uniform motor pole pairs** (one `motor_poles` setting), and the diff's `motor_poles` must equal
  2 × `pole_pairs` (checked at load); rotor inertia is per motor.
- **The diff is applied once, on first boot**, and only a run that says `EEPROM saved` and exits cleanly counts
  (SITL creates `eeprom.bin` at boot, so the file alone proves nothing).
- **Fault randomness has its own stream** (`elrs-faults`): a fault changes only loss decisions, never the model's
  own draws.
- **The failsafe decode reads Betaflight's arming-disable flags** (RX_FAILSAFE, FAILSAFE), not the BOXFAILSAFE
  mode bit, which a real RX loss never sets.
- **`fault.battery.weak_cell` carries the cell index + 1** (0 means none).
- **The video interference cache is keyed on frequency and the injected emitter**.
- **The goggles' latency is theirs**: it is reported whether or not a VTX transmits.
- **Records are JSON objects** (`{"v"}`, `{"x","y","z"}`, `{"w","x","y","z"}`) with a JSON schema per shape:
  Foxglove's message paths need object roots.
- **MCAP message indexes stay on**: Foxglove's reader needs them, though they are most of a file's size.
- **A repro bundle replays what steers a run, not Betaflight's timing**: a lockstep session with an intact journal is
  exact; a real-time session or one whose journal outgrew its cap replays approximately and says so
  (`exact = false`). The 10 s tail costs about 15 MB per session and about 6 % of the open-loop real-time factor.
- **The thermal and cell constants stay estimates** until a validation submission replaces them (`docs/validation.md`).
- **A record is stamped with the time after its tick**, `(tick + 1) / base_hz`: what `GetState` reports then.
- **Scheduled faults and inputs apply right after the step that reaches their time**.
- **Inputs rows are relative to the `PlayInputs` call**; columns a file leaves out hold their value; the last row
  holds after playback; the CSV travels in the request (the server reads no client paths).
- **Requests up to 64 MiB**; **sticks have one owner** (a playing file and `SetSticks` exclude each other).
- **Python records lockstep sessions by default**, and `sim.log` only ever returns this session's recording.
- **A world name** (`world="flat"`) is `<quad dir>/../worlds/<name>.toml`.
- **The Blackbox copy runs last** as a session ends (it is the `Session` field declared last).
- **A launched server stops exactly once**: at `close()`, when its `Sim` is collected, or at interpreter exit.
- **The `world.has("handset_position")` guard is kept** (`godot/world/world.gd`): it serves the empty-dictionary
  clear path (`build({})`), where the key is legitimately absent; the HUD shows `SNR –` while the link is down.
- **The m3c VTX timeout was a one-off**: `no MSP reply to command 89` once (run 38078545752, 2026-10-10); every
  ci.yml run from 2026-10-10 to 2026-10-13 searched — 7 runs, all with logs — no recurrence
  (`docs/superpowers/history/closeout-evidence.md`).
- **A DVR start never overwrites a file**, and its frames are keyed on an integer field index.
