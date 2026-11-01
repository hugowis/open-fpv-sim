# Roadmap and known limitations

v1 — milestones M0 to M4, `open-fpv-sim` 0.1.0 on PyPI (2026-10-13) — is complete: see the parent spec
(`docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`) for what it set out to do and the evidence that it does.
Phases 2 and 3 (M5 to M8c: swappable radio, accuracy and validation, hardening, race timing and scripted flight) are
release 0.2.0 (`CHANGELOG.md`).
This file is what the simulator deliberately does not do, and what comes next. It replaces the per-milestone carried-debt
files (kept as history in `docs/superpowers/history/`); the v1 close-out's fixes are in `CHANGELOG.md`.

## Known limitations of v1

### Physics (`docs/research/collision.md`)
- `body.accel_ned` is coordinate acceleration (it includes gravity); a motor's published `omega_dot` is the
  derivative before clamping.
- The IMU's analytic vibration rate term (the gyro disturbance from prop imbalance) still ignores rotor gyroscopic
  coupling, which M6a added to the rigid-body dynamics only.
- Two contact mechanisms, one per job: hits get impulses, resting weight gets the spring-dampers, so a hard
  touchdown both bounces and compresses the legs. The landing points hold on a slope by static friction (a
  millimetre of tangential sag, not a rigid hold) but the spheres' impulse friction creeps `g·sinθ·Δt` (0.4 mm/s on
  a 20° slope); the position correction translates the body out of every contact but never turns it.
- The ground is the plane d = 0 unless the world has a `[terrain]` heightfield (#153); boxes are axis-aligned, and objects never collide with each other.
- Triangle meshes (`[[meshes]]`, `.glb`): the quad's spheres and landing points collide with the surface (no inside, so a
  mesh need not be watertight); a foot's force on a mesh is capped at `k × 5 mm` by its skin, and a mesh collides with
  nothing but the quad. The Godot drawing of meshes was verified headless only. The caps (8 meshes, 400 000 triangles a
  world, 64 MiB a file) are estimates; the sim and the Godot client must share a filesystem; only `heading_deg = 0` meshes
  are claimed bit-identical across operating systems (`docs/research/collision.md`).

### Electrical
- The thermal model has no airflow dependence (the thermal resistance is a constant) and nothing derates on
  overtemperature; with the estimated constants the ESC warms only a couple of kelvin at this model's phase currents
  (`docs/research/battery-and-thermal.md`).

### Radio (`docs/research/elrs-link.md`)
- A mesh costs a path the loss of every surface it crosses (`[[materials]]` `rf_loss_db`), the same at every frequency: no
  diffraction around its edges, no reflection and no thickness (one crossing, one loss).
- One frequency (2440 MHz) stands for the whole hop sequence; the body shadow is the video link's smooth lobe, not
  a measured 2.4 GHz pattern; the handset transmits on the antenna it receives best on.
- Telemetry slots default to 1:4 of the downlink (`[radio] telemetry_slot_divisor`, 4) but are configurable; one
  ratio covers every band and packet rate, and the shared RC/telemetry UART has no contention model.
- The `RadioLoss` fault acts on the uplink; `DownlinkLoss` cuts the downlink telemetry while leaving control untouched.
- Every constant is an estimate or a published figure; none is measured. The 900 MHz band's carrier, bandwidth,
  noise figure and downlink power are estimates (its sensitivities are ExpressLRS's published ones).
- The radio is three swappable parts (`docs/writing-a-radio-model.md`), but only one handset (`joystick`), one
  receivers (`crsf`, `ibus`) and two links (`elrs`, `fixed`) ship; a new one is a Rust module, a match arm and a config enum
  variant, in-tree.

### Inputs recording (`docs/superpowers/specs/2026-10-15-m5-radio-components-design.md` §6)
- A replay is not the real-time flight: real-time timing, the SITL schedule and the link's seeded draws differ. The
  CSV's optional `tx` and `link_loss` columns carry the transmitter state and a **K** cut; a file written before
  0.3.0 has neither column and still plays.
- `Load` and `Unload` stop a running inputs recording and publish `inputs_recording_stopped`. A session that vanishes
  (its last watcher left, the server shut down) finishes the file without an event, so a client that tracks the
  recording state still resets it on a lost session, as the Godot game does.
- `PlayInputs` is refused while an inputs recording runs (playback writes the bus directly, so a scripted segment
  would be missing from the file): stop the recording first.

### Video (`docs/research/video-link.md`)
- One knife edge per object, losses of several objects added; reflections off the ground only, and the reflected
  ray is not obstructed separately; one body-shadow lobe of up to 8 dB.
- Every video constant is an estimate; the receiver's thresholds are world data (`[video]`) but none is measured.
- DVR (`docs/dev-setup.md`): after a window resize or an aspect switch the frames are letterboxed to the size the
  recording started with, so the bars stay until it is restarted; the chase view still stops it. The clean feed needs
  a real renderer, and its first frame appears one render after the recording starts (black, then repeated, until
  then). H.264 export needs an ffmpeg with libx264 and an AAC encoder on `PATH`, and an export still running when
  the game quits is lost. The audio leads the picture by the goggles' latency (40 ms) and has no VTX buzzer (the sim
  has no buzzer model).
- Flight audio (#158, `docs/superpowers/specs/2026-10-31-flight-audio-design.md`), played live in the client:
  airspeed is ground speed (the simulation has no wind yet), a hard landing and an object hit sound the same
  (one thud), and the tone is the blade-pass fundamental plus 2x and 3x harmonics, which alias above about
  7.35 kHz blade-pass. The samples are bit-identical on one platform only (`sin`/`exp`/`tanh` come from the
  platform libm), so #159's DVR-audio goldens must not expect byte equality across OSes.

### Flight controller and SITL (`docs/research/sitl-interface.md`)
- Several SITLs per machine run side by side, one simulator process each, in slots of ten ports
  (`docs/dev-setup.md`, Ports and parallel sessions): at most 16, claimed per operating system (a Windows simulator and
  one inside WSL do not see each other's claims), and the Rust live tests still run one at a time.
- macOS is supported best-effort on Apple silicon (#68, `docs/research/macos.md`): the SITL builds with a one-hunk patch,
  the Rust, Python and Godot suites pass (the radio goldens are recorded per architecture), the slot cleanup waits for
  its victims and escalates to SIGKILL as it does on Linux, and CI has a non-blocking job for it. Not done: anything on
  a physical Mac (window, rendering, gamepad), Intel Macs beyond the manual survey (their real-time Godot checks are
  paced by the wall clock and flaked once), and promoting the macOS job into `publish.needs` after a record of clean
  `ci.yml` runs.
- SITL runs natively on Windows, without WSL, from an MSYS2 build (`scripts/build-sitl-windows.sh`, #67,
  `docs/research/sitl-native-windows.md`). It is experimental: the port needs a stub file for hardware-only symbols
  (`--gc-sections` removes nothing on PE), the executable needs the two Cygwin runtime DLLs (`msys-2.0.dll`,
  `msys-gcc_s-seh-1.dll`) beside it, and it has not been compared with the Linux SITL bit for bit. The `windows-sitl`
  CI job builds it (MSYS2's msys environment, cached), live-tests the `.exe` from the runner's plain PowerShell, and
  gates `publish` since 2026-10-31, after 13 consecutive green runs. A DLL-free native Win32 (`ucrt64`) port, which
  this build is not, is sized at several days and not started (#105).
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
- Opening a recording while it is still being written: the file has no summary until `stop()`, so indexed
  readers refuse it and a streaming reader gets only the flushed chunks (`docs/research/mcap-determinism.md`).
  Foxglove itself was not installed on the evidence machine, so its behavior on a live file remains to be checked.

### Race timing (`docs/race-timing.md`)
- A gate is a trigger: a pass needs the quad's bounding sphere inside the opening (a clipped pass is a `gate_hit`),
  and a gate can be tilted (`pitch_deg`, `roll_deg`). A world can cap the race (`[race] laps`), `ResetRace` starts a
  fresh one without a `Load`, and Godot highlights the next gate. A world can name several courses over its gates and a
  race mode each (laps or time attack, #132); the client picks the course. There is no ghost or best-lap replay.

### Python
- The live view follows the state stream (up to 240 Hz); the 8 kHz signals are only in the recording.
- The reader is pure Python. On a one-minute open-loop recording (27.6 MB, 148 signals) it takes 0.34 s for three
  patterns and 2.2 s for every signal; the `mcap` iterator it replaced took 3.3 s and 9.8 s
  (`scripts/bench_reader.py`). The fast path needs message indexes (the server's recordings have them) and zstd or
  uncompressed chunks; any other file is read the slow way.
- Open-loop sessions fly a scripted path only with the opt-in stabilizer (`stabilize=True`, `docs/scripted-flight.md`), a
  test stand-in: angle mode on the simulator's truth, no filters, arming, rates or I-term, gains tuned for the shipped
  quad. Real tuning needs Betaflight SITL, which needs WSL on Windows (or the experimental native build).

### Performance
- Headless lockstep runs at 7.4× real time with Betaflight SITL (WSL 2) and 84–296× open loop, on an AMD Ryzen 9
  9900X3D (`docs/dev-setup.md`, Performance), 10.0–11.8× Betaflight on CI's Linux runner (`scripts/benchmark-baseline.json`): above the 2× target. CI fails below 1×, and when a row falls under 60 % of the
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
- Non-CRSF receivers: iBUS is done (`docs/research/ibus-receiver.md`, #8); SBUS, FrSky and DSMX remain, a `Receiver` each,
  with the matching Betaflight config and a `#define USE_SERIALRX_<protocol>` in the SITL patch.
- Plugin loading for radio models from outside the tree.

### Modelling
- Terrain: the physics ground is a heightfield (#153); the radio and video line of sight over it (#154) and the Godot mesh (#155) are done. Rotated boxes; a frequency-dependent `rf_loss_db`; collisions between objects; crash damage.
- ExpressLRS dynamic power and frequency hopping; 2.4 GHz interference.
- DShot extended telemetry and MSP over CRSF.
- Feed Betaflight SITL a measured magnetometer (#31 added the model and left the firmware on its fixed virtual compass): a reserved block in the state datagram like eRPM's, and a SITL patch for `virtualMagSet`, so noise, hard iron and the world's declination reach heading hold and GPS rescue. A datasheet magnetometer part and a `fault.mag.*` interference seam.

### Video and OSD
- The full HD/digital OSD element set (#32): the `OSD_SYS_*` elements and the 50 × 18 canvas that Betaflight sends only
  when built with `USE_OSD_HD`, which needs an HD SITL build, a decoder for the system-element sub-command and an HD font
  with a verified licence. Warnings and criticals (font pages 2–3) are drawn as the bold glyphs in amber and red.
- Digital video systems (parent spec §2).

### Sandbox and component library
- (C) plugin models (parent spec §1.5) and a stable plugin API.
- The repro bundle as a single archive (it is a folder today, `docs/repro-bundle.md`).
- `fcs/`, `receivers/` and `cameras/` part kinds.

### Tooling
- The site generator (`tools/sitegen`, `tools/sitegen/README.md`) is piece 2 of the realistic Chernobyl map: a recipe and an
  OpenStreetMap snapshot become a `.glb` and a world file, deterministically. Piece 1 (mesh worlds, protocol 17) and piece 2
  are done; piece 3, the real map (`docs/chernobyl-map.md`), is in two stages. Stage A is done: `worlds/chernobyl.toml`, the
  New Safe Confinement alone from published dimensions with one door, an empty snapshot, and the generator's multipolygon
  relations, extent rule, breaches and the `WorldConfig` mesh cache. Stage B is pending and waits for the owner to confirm the
  licence note (`docs/research/sitegen-licensing.md`, status unconfirmed): the real snapshot, units 3 and 4, the turbine hall
  and a breach in its wall. Not built, deliberately: pitched roofs, a cooling-tower builder, a DEM ground, textures, multipolygon
  holes (courtyards are filled), `col_` collision proxies; a stack is open at both ends.
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
  is not stepped over; objects standing on the ground are rooted below it (on the terrain's surface in a world with `[terrain]`). The terrain has its own sampled search because its clearance is not convex along a ray.
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
