# Open FPV Sim — Phase 2 design (M5–M6): real gear, trusted numbers

- **Date:** 2026-10-13
- **Status:** approved in conversation 2026-10-13; this file is the written design.
- **Parent:** `2026-10-04-open-fpv-sim-design.md` (v1: M0–M4, feature-complete; close-out in flight on `v1-closeout`).
- **Backlog source:** the "Roadmap" dispositions of `2026-10-13-v1-closeout-design.md` §3 and the parent spec's v1
  non-goals (§2). Phase 2 is a selection from that backlog, not new invention: every item below is a deferral the
  project already wrote down.

## 1. Intent

v1 proved the architecture: real Betaflight SITL, real protocols end to end, a deterministic sandbox, a package on
PyPI. Phase 2 connects it to the two things that make people stay:

- **M5 — the real-radio bridge.** Fly the simulator with your own radio: CRSF over USB serial from an ELRS
  handset, and the simulator's CRSF telemetry flowing back to the handset's screen. The parent spec deferred this
  from day one ("joystick first; real-radio CRSF-over-serial later").
- **M6 — accuracy and validation.** The carried modelling upgrades the close-out pushed to the roadmap, each with
  unit tests against analytic results, plus the validation-report framework the parent spec §8.5 left "tracked, not
  gating" — the harness and data format exist, ready for community measurements.

Both serve the audience order of §1.3: the bridge serves developers (test ELRS-side behaviour, handset setups) and
pilots (practice with your own radio) at once; validation serves developers first and education second.

**Success:** the maintainer flies the Godot client with a real RadioMaster handset through the simulated ELRS link
with telemetry on the handset; every M6 model upgrade has an analytic test and recorded provenance; a validation
report runs in CI; the repro bundle exists.

## 2. Rulings made while brainstorming

| Question | Ruling |
|---|---|
| M5 scope | Input **and** telemetry back to the handset — the full "it's my real radio" feel, not input only. |
| Validation data | None exists yet: M6 ships the harness, a defined data-submission format and estimated baselines; real thrust-stand/Blackbox comparisons arrive with community data (provenance `estimated` until then). |
| Ground-truth radio | The maintainer's RadioMaster EdgeTX handset (internal ELRS module). An early M5 task captures what it actually puts on the wire and pins the capture as golden frames. |
| Level-2 firmware-in-the-loop | Deliberately deferred — the leading candidate for phase 3, informed by M6's validation harness. |
| Phase close-out | No separate close-out milestone: two milestones, each plan absorbing its own carried debt; M6's final task writes the short phase-close docs. |
| Link policy for a serial radio | Real CRSF frames feed the existing ELRS link model — loss, latency and failsafe still apply (the point of a simulated RF) — and that is the default; a pass-through option bypasses the link model for pure input use. M5's spec names the option. |

## 3. M5 — the real-radio bridge

ELRS TX firmware (including RadioMaster internal modules) has a USB mode that speaks CRSF over serial — the same
path commercial simulators use for real radios. The bridge adds a serial input source beside the joystick adapter:

- **Input.** CRSF channel frames from the serial port enter `ofs-radio` at the ELRS model's uplink seam, through
  the simulated link (ruling above). The one-stick-owner ruling of M4c extends: a serial radio, a pilot and a
  playing input file exclude each other.
- **Telemetry home.** The CRSF downlink M3d already carries (battery voltage/current, flight mode, link statistics)
  is encoded to the handset, rate-limited as a real link would be; EdgeTX's telemetry screen shows it and dashes
  out on link loss.
- **Clients.** Godot: a serial-port picker in the F2 controls screen and the radio's LQ on the HUD; keyboard and
  joystick stay. Python/gRPC: a call to attach a serial radio to a session (surface named in M5's spec).
- **Determinism.** Real serial input is non-deterministic by nature; a real-radio session can record its stick
  stream and replay it as scripted inputs (M4c's `play_inputs`), producing an identical MCAP in lockstep. Fly it
  for real, then replay it deterministically.
- **Ground truth early.** One of the first tasks captures the handset's actual wire traffic (baud, framing, frame
  set, telemetry polling) and pins it as golden frames; the bridge is built against those, not against assumed
  protocol details.
- **CI without hardware.** A fake software peer (a pty pair on Linux CI) speaks CRSF at the bridge, so every path
  is tested without a radio attached. Platforms: Windows and Linux (`serialport` crate).
- **Not real RF.** A real *receiver* hearing actual radio waves is level-2 territory; the bridge is serial CRSF
  only.

**Exit criteria:** the maintainer flies the Godot client with the handset through the simulated ELRS link — real
sticks and arm switch, failsafe on radio-off behaving like a receiver going silent; the handset displays
Betaflight's telemetry and dashes it on link loss; a recorded real-flight stick log replays deterministically
(identical MCAP); the CRSF golden-frame tests and the fake-peer CI test pass.

## 4. M6 — accuracy and validation

The roadmap's modelling rows, each landing as: model upgrade, unit test against an analytic result, provenance on
every new constant, and the research doc updated.

| Upgrade | Source row | Notes |
|---|---|---|
| Rotor gyroscopic coupling | M1 limitations | Prop angular momentum × body rate torque in the 6-DOF model. |
| Windmilling drag on a dead motor | M4b roadmap | The `MotorFailure`/`PropDamage` coast-down gains real aero drag. |
| Vibration transmissibility | M4b roadmap | Motor imbalance reaches the IMU through a frame response, not rigidly. |
| Per-cell battery + capacity fade | M4b roadmap | Cell-level resistance and SoC; `WeakCell` becomes expressible as a real cell; fade over cycles. |
| Thermal model: ESC and VTX | M3a/M3e roadmap | Simple RC thermal networks fed by losses; reaches Tramp's temperature field and Betaflight's ESC telemetry. Constants are estimates with provenance. |
| Configurable receiver thresholds | M3b roadmap | The video link's sensitivity edge as world data, not a constant. |
| Validation framework | Parent §8.5 | Harness in CI (tracked, not gating): simulated vs thrust-stand curves and vs real Blackbox step responses, a defined submission format for community data, one worked report with estimated baselines. |
| Repro bundle | M4c roadmap / parent §7 | Every simulator error writes its folder: quad config, seed, recorded inputs, SITL EEPROM, last N seconds of MCAP. |

**Exit criteria:** every upgrade has an analytic unit test; signals Betaflight reads (battery, ESC telemetry)
change continuously across the model swap — its OSD must not blink; the validation harness runs in CI tracked with
one worked report; the repro bundle fires on a real error.

## 5. Non-goals for phase 2

Level-2/3 firmware fidelity (AM32/ELRS firmware-in-the-loop, MCU emulation); real RF (a receiver hearing radio
waves); GPS, Remote ID, INAV/ArduPilot; digital video (HDZero, OpenHD/Ruby); non-flat terrain and rotated boxes;
race timing and lap counting; audio; multiplayer; macOS; native Windows SITL; stable plugin API; fast (non-Python)
reader; new PyPI-breaking API changes beyond what M5's surface adds.

## 6. Risks

| Risk | Answer |
|---|---|
| ELRS sim-mode serial details differ per handset/firmware | The capture-first task pins the maintainer's handset as ground truth; the fake peer in CI replays the capture, so drift is caught. |
| Telemetry slot scheduling against EdgeTX | Same capture task records the handset's polling; the bridge answers what was asked, in CRSF's slot discipline. |
| Thermal and per-cell constants are pure estimates | Provenance discipline (`estimated`, with notes) keeps them honest; the validation framework exists precisely to replace them with measurements later. |
| Validation harness over-builds without data | YAGNI: define the submission format, ship exactly one worked report, stop. |
| Serial quirks on Windows (driver naming, CDC baud) | The ground-truth radio is flown on Windows from the first task; port naming is part of the captured evidence. |

## 7. Structure

M5 first, then M6 — each with its own spec (brainstormed against this doc), an implementation plan
(superpowers:writing-plans), subagent-driven execution and its own merge, exactly as M3/M4 ran. Small sandbox items
slot in as filler tasks where they fit (the repro bundle in M6; the rest stay on the close-out's roadmap). The
close-out's `docs/roadmap.md` (W6) remains the backlog ledger; when M5/M6 land, their rows move from backlog to
done.
