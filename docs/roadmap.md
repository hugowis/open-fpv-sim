# Roadmap and known limitations

v1 — milestones M0 to M4, `open-fpv-sim` 0.1.0 on PyPI (2026-10-13) — is complete: see the parent spec
(`docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`) for what it set out to do and the evidence that it does.
This file is what v1 deliberately does not do, and what comes next. It replaces the per-milestone carried-debt files
(kept as history in `docs/superpowers/history/`); the v1 close-out's fixes are in `CHANGELOG.md`.

## Known limitations of v1

### Physics (`docs/research/collision.md`)
- No rotor gyroscopic coupling; friction is viscous below 5 cm/s; `body.accel_ned` is coordinate acceleration (it
  includes gravity); a motor's published `omega_dot` is the derivative before clamping.
- Two contact mechanisms, one per job: hits get impulses, resting weight gets the spring-dampers, so a hard
  touchdown both bounces and compresses the legs. Position correction moves only the deepest contact out.
- The ground is the plane d = 0, boxes are axis-aligned, and objects never collide with each other.
- Vibration is rigid-body and below resonance; on an open-loop build with mixed motor pole pairs the rate
  disturbance ignores gyroscopic coupling.

### Electrical
- The battery is one Thevenin pack: a weak cell is an offset on it, with no per-cell model and no capacity fade.
- ESC and VTX temperatures are a constant 25 °C (the KISS telemetry frames, Tramp's status).
- A dead motor freewheels through its prop's drag only.

### Radio (`docs/research/elrs-link.md`)
- One frequency (2440 MHz) stands for the whole hop sequence; the body shadow is the video link's smooth lobe, not
  a measured 2.4 GHz pattern; the handset transmits on the antenna it receives best on.
- Telemetry slots are a fixed 1:4 ratio of the downlink (`TELEMETRY_SLOT_DIVISOR`), and the shared RC/telemetry
  UART has no contention model.
- The `RadioLoss` fault acts on the uplink only.
- Every constant is an estimate or a published figure; none is measured.

### Video (`docs/research/video-link.md`)
- One knife edge per object, losses of several objects added; reflections off the ground only, and the reflected
  ray is not obstructed separately; one body-shadow lobe of up to 8 dB.
- The receiver's thresholds are named constants, not world-file fields; every constant is an estimate.
- The DVR stops when the picture's size changes mid-recording, and it records what the pilot sees (no clean feed).

### Flight controller and SITL (`docs/research/sitl-interface.md`)
- One SITL per machine: its ports are fixed (UDP 9001–9004, TCP 5760+).
- The motor protocol is PWM; Betaflight's RPM filter is fed by the simulator's eRPM block, not a DShot driver.
- A session whose scheduler failed is poisoned by design; a new `Load` starts a fresh one.
- A Blackbox log of a session that ends armed is cut short: disarm first.
- A client that closes its MSP connection right after `MSP_REBOOT` leaves Betaflight waiting to send the reply, so
  it never reboots and the session fails; keep the connection open until the relaunch
  (`docs/superpowers/history/closeout-evidence.md`) — the Configurator's own Save does not do this (checked by hand, 2026-10-14).
- Betaflight SITL aborted once with heap corruption while resetting (glibc `tcache` check), so a reboot failed as a
  firmware crash; not seen again in 38 reboots. Inside SITL's teardown, upstream of the simulator.
- The port-bind flake: fixed (2026-10-14). The cleanup's `pkill` only signalled its victim, whose sockets close
  one by one during teardown, so a launch immediately after could hit a port still closing. The pre-launch
  cleanup now waits for its victims to exit (SIGKILL after a grace), under WSL inside the VM; should it recur,
  the error names the port and who held it at the moment of the bind
  (`docs/superpowers/history/closeout-evidence.md`).

### Logging (`docs/research/mcap-determinism.md`)
- Recording rates are whole divisors of the tick: 240 Hz on an 8 kHz quad records at 242.4 Hz (the channel's
  `rate_hz` metadata says so).
- A recording stopped by a write error has no summary section, so indexed readers refuse it; the
  `recording_stopped` event names the error.
- Opening a recording in Foxglove while it is still being written is untested.

### Python
- The live view follows the state stream (up to 240 Hz); the 8 kHz signals are only in the recording.
- The reader decodes in pure Python: about 3.7 s for three patterns of a one-minute recording, 11.7 s for all of it.
- Open-loop sessions have no stabilizer, so they cannot fly a scripted path; that needs Betaflight.

### Performance
- Headless lockstep runs at 7.4× real time with Betaflight SITL (WSL 2) and 84–296× open loop, on an AMD Ryzen 9
  9900X3D (`docs/dev-setup.md`, Performance), 12.4× on CI's Linux runner: above the 2× target. CI fails only below 1×.

## Post-v1 backlog

### Fidelity (parent spec §1.5, §2)
- Level 2: AM32 and ExpressLRS as host-compiled firmware in the loop. Level 3 (MCU emulation) stays out of scope,
  but the interfaces must not preclude it.
- Real radio input over CRSF serial, and telemetry back to a real radio.

### Modelling
- A thermal model (ESCs, motors, VTX); windmilling drag on a dead motor; a per-cell battery with capacity fade;
  vibration transmissibility (a structural model); `RadioLoss` on the downlink.
- Non-flat terrain and rotated boxes; a frequency-dependent `rf_loss_db`; collisions between objects; crash damage.
- 900 MHz ExpressLRS, dynamic power and frequency hopping; 2.4 GHz interference.
- Configurable receiver thresholds; a configurable telemetry slot ratio; DShot extended telemetry and MSP over CRSF.
- A compass, and the heading mapping it needs.

### Video and OSD
- OSD font pages 2–3 (warnings, criticals) and the full HD/digital OSD element set.
- Digital video systems (parent spec §2).
- A DVR that survives a resize; a clean-feed DVR; audio; image-sequence and ffmpeg export; a DVR toggle on the F2
  controls screen (v1 ships the **D** key).

### Sandbox and component library
- (C) plugin models (parent spec §1.5) and a stable plugin API.
- The repro bundle (parent spec §7): one file that replays a session.
- `fcs/`, `receivers/` and `cameras/` part kinds; the IMU and barometer as parts; provenance exposed over the API;
  `OFS_LIBRARY` as a search path.

### Tooling
- A faster reader (a columnar decode, or the Rust reader behind a binding).
- `mcap` is pinned to an upstream git revision (00e47a1b): switch to the crates.io release that carries
  foxglove/mcap#1838 once it ships, and re-run `crates/ofs-log/tests/determinism.rs`.
- `rerun-sdk` is pinned to 0.39.x by the `rerun` extra: bump it deliberately (its Python API changes between minors).

### Validation (parent spec §8.5)
- Thrust-stand comparisons for OpenMotor/prop combinations, and simulated against real Blackbox step responses.
- A spectral proof that the RPM filter's notches track the motors, from a Blackbox log.
- A scripted circuit through the gates against real Betaflight.

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
