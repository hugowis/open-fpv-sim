# Open FPV Sim — Architecture & v1 Design

- **Date:** 2026-10-04
- **Status:** Approved 2026-10-04
- **Short name:** `ofs` (crate/package prefix)

## 1. Intent

### 1.1 What we are building
An open-source FPV drone simulator that models the whole aircraft — hardware, electronics, radio and video links, and flight physics — and runs the **real flight-controller firmware** (Betaflight) against those models. It replicates real protocols (ExpressLRS/CRSF, SmartAudio/Tramp, MSP) so that real tools (Betaflight Configurator, Blackbox Explorer) work against it unchanged, and it is a **sandbox**: components, scenarios and faults are data and scripts, so people can test and develop without risking hardware.

### 1.2 Inspiration and reference hardware
Inspired by the [OpenDrone initiative](https://opendrone.be/) ([GitHub](https://github.com/OpenDrone-hw)), which designs fully open drone parts (KiCad, CERN-OHL-S hardware, GPL/MIT firmware). OpenDrone parts are the **first reference component library**:

| Part | Hardware | Firmware |
|---|---|---|
| OpenFC Lite / Lite Mini | RP2354B / RP2354A flight controller | Betaflight |
| OpenESC 30x30 / 20x20 | 4-in-1 ESC | AM32 |
| OpenRX Lite / Mono / Gemini | SX128x / LR1121 receivers | ExpressLRS |
| OpenVTX (in progress) | RTC6705 + STM32G4 analog VTX with software OSD | custom |
| OpenMotor, OpenFrame 3F/5F | motors, frames | — |

The simulator is not tied to OpenDrone: any component can be described in the same format.

### 1.3 Audience, in priority order
1. **Developers and tinkerers** — test firmware changes, PID tunes, protocols and custom hardware. Accuracy, determinism, inspectability and scriptability come first.
2. **Pilots** — an open alternative for practice; flight feel and visuals matter.
3. **Education** — explaining how each layer of a drone works.

### 1.4 v1 success criteria
- A developer flies a simulated OpenDrone 5" quad running **real Betaflight SITL**, with a joystick, through a simulated **ELRS/CRSF** link, seeing an FPV view with **Betaflight's real OSD** and **analog video degradation**.
- The real **Betaflight Configurator** connects to the simulated FC; changed PIDs/rates take effect and persist.
- A Python script can run the same quad **headless and deterministically**, inject faults, and get every signal back as a DataFrame.

### 1.5 Decisions made during brainstorming
- **Fidelity:** every component is swappable between fidelity levels. v1 ships **level 1** (behavioral models + host-compiled Betaflight SITL); **level 2** (AM32 and ExpressLRS as host-compiled firmware-in-the-loop) follows. **Level 3** (full MCU emulation of real target binaries, e.g. via Renode) is out of scope but the interfaces must not preclude it.
- **Stack:** Rust core, Godot 4 front-end via godot-rust (gdext).
- **Architecture:** standalone headless sim server owning the clock; Godot and Python are clients (Approach 2).
- **Pilot input:** layered — all inputs pass through the ELRS/CRSF stack; Betaflight always sees a CRSF receiver on a UART. Joystick first; real-radio CRSF-over-serial later.
- **Sandbox v1:** (A) data-defined components, (B) scripting API, (D) logging/inspection. (C) plugin models later.
- **Video transmission is in scope:** VTX, 5.8 GHz link model, real OSD, visual degradation.

## 2. Non-goals for v1
- Level 2/3 firmware fidelity (AM32/ELRS firmware-in-the-loop, MCU emulation).
- Real radio input via CRSF serial; telemetry back to a real radio.
- Stable plugin API for third-party models.
- Digital video systems (HDZero, OpenHD/Ruby-style links).
- GPS, Remote ID, INAV/ArduPilot.
- Detailed environments, multiplayer, macOS support.

## 3. Architecture overview

```
┌───────────────────────────── ofs-sim (headless Rust server) ─────────────────────────────┐
│  ofs-core: clock · multi-rate scheduler · Model trait · signal bus                       │
│                                                                                          │
│  ofs-physics   ofs-electrical   ofs-sensors   ofs-radio   ofs-video   ofs-log           │
│        └──────────────┴──────────────┴─────────────┴──────────┴─────────┘               │
│                                   │                                                      │
│  ofs-fc (Betaflight SITL bridge) ─┼── UDP: sensor state in / motor outputs out           │
│                                   └── TCP: SITL UARTs (CRSF, SmartAudio, MSP DisplayPort)│
│  ofs-config (TOML library + quad assembly)                                               │
│  gRPC API (tonic)                                                                        │
└────────────────┬──────────────────────────────┬──────────────────────────────────────────┘
                 │ gRPC                         │ gRPC
        ofs-godot (GDExtension client)     python/ofs (scripting client)
        + Godot 4 project                   
                                      Betaflight Configurator ──TCP──► SITL MSP UART (direct)
```

Betaflight SITL runs as a child process supervised by `ofs-fc`. The Configurator connects directly to the SITL MSP UART; `ofs-sim` only reports the port.

## 4. Components

### 4.1 The `Model` trait (in `ofs-core`)
Every simulated part implements `Model`: typed input ports, typed output ports, a fixed step rate (an integer divisor of the base tick), and `step(dt)`. Models perform no I/O outside their ports. The one exception is bridge models (`ofs-fc`), which are explicitly marked as external and form the only non-deterministic boundary.

The trait is the fidelity seam: a behavioral ESC model and a future AM32 firmware-in-the-loop ESC expose identical ports (throttle/DShot command in, phase power/RPM/telemetry out), so quads switch fidelity per component in config.

### 4.2 Crates

| Crate | Responsibility |
|---|---|
| `ofs-core` | Simulation clock, multi-rate scheduler, `Model` trait, signal bus, seeded RNG streams. No I/O; fully deterministic. |
| `ofs-physics` | 6-DOF rigid body; prop thrust and torque from coefficient tables vs RPM (and advance ratio where data exists); body drag; ground contact and simple collisions; wind and turbulence. |
| `ofs-electrical` | Battery (Thevenin equivalent circuit: cells, capacity, SoC-dependent open-circuit voltage, internal resistance, sag); ESC (throttle→duty, current limit, response delay, signal-loss behavior); brushless motor (KV, winding resistance, no-load current, rotor inertia, pole count, back-EMF → torque and current). |
| `ofs-sensors` | IMU (gyro/accelerometer noise, bias, scale, motor-induced vibration from imbalance), barometer. |
| `ofs-radio` | CRSF frame encoder/decoder (real bytes, CRC); ELRS link model (packet rate, link quality, RSSI, loss, burst loss, latency, failsafe); joystick-to-RC adapter. |
| `ofs-video` | VTX model (band, channel, power, pit mode) driven by real SmartAudio/Tramp bytes; 5.8 GHz analog link model (free-space path loss, antenna gain/polarization, body and terrain occlusion, multipath, adjacent-channel interference) producing per-frame signal quality (SNR, sync stability); OSD capture from MSP DisplayPort as a character grid. |
| `ofs-fc` | Betaflight SITL bridge: build discovery, process launch and supervision, sensor-state packets in, motor outputs out, CRSF into a SITL UART, SmartAudio and MSP DisplayPort from SITL UARTs, per-quad EEPROM directory, Blackbox capture. |
| `ofs-config` | TOML component library, schema versioning, validation, quad assembly, Betaflight `diff` application on first boot. |
| `ofs-log` | Signal recording to MCAP with per-signal rate limits; optional live streaming to Rerun; DataFrame-friendly export. |
| `ofs-sim` (bin) | Headless server: loads a quad, runs the scheduler, serves the gRPC API. |

### 4.3 Clients
- **`ofs-godot`** — Rust GDExtension (godot-rust/gdext) acting as gRPC client; launches `ofs-sim` if not running.
- **Godot 4 project** — world rendering (grey-box world in v1), FPV camera (FOV, lens distortion, uptilt, exposure), analog video degradation shader, OSD layer, HUD (sim status, overruns, link stats), joystick capture.
- **`python/ofs`** — Python client package (gRPC), `pip`-installable, with `launch`, `load`, `run`, `set_sticks`, `play_inputs`, `inject`, and log export.

### 4.4 API
A single gRPC service (`tonic` on the server):
- **Control:** handshake (protocol version), load quad/world, set mode, start/pause/step, set sticks, play input file, inject/clear fault, query state.
- **Streams:** vehicle state at client rate, telemetry, OSD frames, video signal quality, events (failsafe, faults, overruns, errors).

## 5. Data flow and timing

### 5.1 Clock and run modes
`ofs-sim` owns simulated time.
- **Real-time** — paced to wall clock for piloting. Overruns are counted and reported; policy `warn` (continue, mark log segments unreliable) or `slow` (stretch time, stay correct).
- **Lockstep** — as fast as possible or at a speed factor; for scripts and CI.
- **Paused / single-step** — for debugging.

### 5.2 Default rates (configurable per quad)

| Rate | Models |
|---|---|
| 8 kHz base tick | rigid body, motors, ESCs, IMU sampling |
| 1 kHz | battery, temperatures |
| ELRS packet rate (e.g. 500 Hz) | radio link, CRSF |
| video frame rate (~60 Hz) | VTX link quality, OSD frame |
| ≤ 240 Hz | client state stream |

All model rates are integer divisors of the base tick.

### 5.3 Control loop
1. Joystick (Godot) → gRPC → `ofs-radio`: sticks become ELRS packets subject to packet rate, loss and latency → CRSF frames → TCP → SITL UART configured as serial RX (CRSF).
2. Betaflight SITL runs PID and mixer → motor outputs over UDP → `ofs-fc`.
3. `ofs-electrical`: ESC → motor current, torque, RPM; battery supplies voltage and integrates current.
4. `ofs-physics`: props → thrust/torque → 6-DOF integration → pose and velocity.
5. `ofs-sensors`: IMU + baro → UDP sensor-state packet → SITL for its next loop.

### 5.4 Return paths
- CRSF telemetry (battery, LQ, RSSI) from SITL → `ofs-radio` → HUD.
- MSP DisplayPort from SITL → OSD grid → Godot.
- SmartAudio/Tramp from SITL → VTX model → video link model → signal quality → Godot degradation shader.

### 5.5 Determinism
Same quad + same seed + same inputs + lockstep mode ⇒ identical MCAP output. Each model draws randomness from its own seeded stream so adding a model does not perturb others.

## 6. Sandbox

### 6.1 (A) Data-defined components
```
library/
  frames/ motors/ props/ escs/ fcs/ receivers/ vtx/ cameras/ batteries/
quads/    <quad>.toml + <quad>.betaflight.diff
worlds/   <world>.toml
```
- One TOML file per part; quads reference parts by ID.
- Every parameter records its **provenance** (`datasheet`, `measured`, `estimated`) and an optional source link.
- Units are explicit; files carry a `schema_version`; validation collects all errors with file and field.
- Each quad has its own SITL EEPROM directory; the `diff` is applied on first boot only, so Configurator changes persist.
- The fidelity level of each component is selected in the quad file (v1: only level 1 available).

### 6.2 (B) Scripting
Python first, Rust also supported (same gRPC). Example:
```python
import ofs
sim = ofs.launch(headless=True)
sim.load("quads/opendrone-5f-freestyle.toml", world="flat", seed=42, mode="lockstep")
sim.inject(ofs.faults.MotorFailure(motor=2, at=3.0))
sim.play_inputs("flips.csv")
sim.run(seconds=10)
df = sim.log.to_dataframe(["gyro.*", "motor.*.rpm", "battery.voltage"])
```
**v1 fault catalog:** motor failure/degradation; prop damage (thrust loss + imbalance vibration); weak battery cell; packet loss and burst loss (→ failsafe); gyro noise and bias; VTX interference.

### 6.3 (D) Logging and inspection
- Any signal-bus port can be recorded to MCAP, with per-signal rate limits.
- Live Rerun view: 3D pose, motor/battery plots, link statistics.
- Betaflight's own Blackbox log captured from SITL for Blackbox Explorer.
- Export to pandas/polars, CSV, Parquet.

## 7. Error handling

**Principle:** simulated failures (failsafe, dead motor, lost video) are behavior — logged as events, the drone reacts realistically. Simulator failures (SITL crash, NaN, bad config) stop the sim loudly and never masquerade as drone behavior.

| Situation | Handling |
|---|---|
| Invalid config | Collect all validation errors (file, field, expected); refuse to start. Unknown `schema_version` fails explicitly. |
| SITL missing/unbuildable | Startup error with platform-specific instructions. |
| SITL crash or hang | Watchdog (no motor output within timeout in lockstep; stalled UART). Pause, capture stderr, notify clients with reason, offer restart. Never continue physics on stale outputs. |
| NaN/inf or implausible state | Per-step check; pause and write state snapshot. |
| Real-time overrun | Count, show in HUD and log; apply `warn` or `slow` policy. |
| Pilot client disconnect | Treated as radio signal loss: ELRS stops sending, Betaflight failsafes; sim keeps running for reconnect. |
| Script client disconnect | End session unless `keep_alive`. |
| API version mismatch | Handshake fails with a clear message. Python raises typed exceptions (`ConfigError`, `FirmwareCrashed`, …). |

**Repro bundle:** every simulator error writes a folder with quad config, seed, recorded inputs, SITL EEPROM and the last N seconds of MCAP.

## 8. Testing

1. **Model unit tests vs analytic results** — free fall; energy/angular-momentum conservation without torque; motor steady-state RPM ≈ KV·(V − I·R) and stall current; battery sag = I·R; free-space path loss.
2. **Protocol tests** — CRSF round-trip and CRC; golden frames from the CRSF/ELRS spec and real captures; `proptest` fuzzing of every parser (malformed bytes never crash the sim).
3. **CI integration tests (Linux, headless, real Betaflight SITL)** — arm and hover; angle mode self-levels; radio cut → failsafe within expected timing; MSP over TCP returns API version; SmartAudio channel change reaches the VTX model.
4. **Determinism test** — two lockstep runs with the same seed produce identical MCAP hashes.
5. **Validation reports** (tracked, not gating initially) — simulated vs measured thrust-stand data for OpenMotor/prop combinations; simulated vs real Blackbox step responses for the same quad and tune. Community measurements feed these.
6. **Performance benchmark** — real-time factor tracked; target ≥ 2× real-time headless on a mid-range PC.
7. **Godot smoke test** — boots and renders one frame headlessly; flight feel tested manually.

## 9. Milestones

| # | Milestone | Exit criteria |
|---|---|---|
| **M0** | Spike (throwaway code) | Written answers to the risks in §10, with a recommended approach for each. |
| **M1** | Headless core | Python script arms and hovers the quad in lockstep with physics, electrical and sensor models against real Betaflight SITL; determinism test passes. |
| **M2** | Radio + pilot | Fly with a joystick in Godot (grey-box world, FPV camera) through the ELRS/CRSF stack; failsafe works; Configurator connects. |
| **M3** | Video | VTX model controlled via SmartAudio; real Betaflight OSD rendered; analog degradation driven by link model. |
| **M4** | Sandbox | OpenDrone component library with provenance; v1 fault catalog; MCAP + Rerun; installable Python package. |

Each milestone gets its own implementation plan. The first plan covers M0 and M1.

## 10. Risks to resolve in M0

1. **SITL lockstep** — whether Betaflight SITL's option to sync its gyro/PID loop to incoming sensor packets works reliably. Fallback: maintain a small, upstreamable SITL patch for externally controlled time. Real-time mode works regardless.
2. **Battery voltage, current and motor RPM into SITL** — the stock sensor-state packet may not carry them (RPM is needed for RPM filtering). Fallback: small SITL patch.
3. **CRSF over a SITL UART** — confirm SITL's TCP-backed UARTs accept CRSF as serial RX with correct timing. Fallback: SITL's UDP RC input for M1 only.
4. **MSP DisplayPort and SmartAudio on SITL UARTs** — confirm both work in SITL builds.
5. **Blackbox capture from SITL** — confirm the mechanism (file device or MSP).
6. **Windows support** — confirm Betaflight SITL builds and runs natively on Windows; fallback: run SITL under WSL2 with networking to the Windows-hosted sim.

## 11. Project conventions
- **Platforms (v1):** Windows and Linux.
- **Toolchain:** Rust stable (Cargo workspace), Godot 4.x with godot-rust/gdext, Python ≥ 3.10.
- **License:** GPL-3.0-or-later for code (matches Betaflight and OpenDrone's copyleft spirit); component library data under CC-BY-SA-4.0.
- **Repository layout:** `crates/` (Rust workspace), `godot/` (Godot project), `python/` (client package), `library/`, `quads/`, `worlds/`, `docs/`.
