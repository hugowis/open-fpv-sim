# M3d: flight-controller telemetry — design

Status: design approved in conversation 2026-10-10, awaiting written-spec review. Parent spec: `2026-10-04-open-fpv-sim-design.md` (§4, §5 — "CRSF telemetry … from SITL → `ofs-radio` → HUD", §9). Predecessor: `2026-10-10-m3c-world-collision-elrs-design.md` (approved; implementation under way in `m3c-world-collision`). This spec builds on M3c's merged output — the rewritten `ofs-radio::elrs` on `ofs-rf`, protocol 5, quad schema 4 — and its implementation plan is written after M3c merges. Carried debt it closes: "The RPM filter and per-motor eRPM" and "CRSF telemetry back to the radio" in `docs/superpowers/m3a-carried-debt.md`. Every SITL claim below was verified live by the M3d spike: `docs/research/sitl-interface.md` §10 and `spikes/m3d/`.

## 1. Purpose and scope

The last of the three M3 follow-ups (M3c's milestone table): Betaflight's own CRSF telemetry reaches the handset over the simulated ELRS downlink, and the simulator's per-motor eRPM reaches Betaflight's RPM filter.

**Success looks like:** flying the Godot client, the HUD's battery readout and the flight-mode line show **Betaflight's own values**, arriving over the simulated link; cut the link and they dash out while the simulator-internal values stay available to Python and logs. In the Configurator's Motors tab (or `MSP_MOTOR_TELEMETRY`) four **distinct** per-motor RPM values track the simulator's motor models, and Betaflight's RPM filter runs on them. Two lockstep runs with telemetry flowing are bit-identical.

### Decisions made in the design conversation

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | Betaflight's CRSF telemetry frames **ride the ELRS downlink** to the handset (scheduled onto downlink slots, lost when the link is down); clients read Betaflight's values there. | The sim decodes the frames off the reply datagram and exposes them directly (telemetry would survive link loss; the milestone's "back to the radio" only half met). |
| 2 | **The motor protocol stays PWM.** The spike proved the RPM filter needs no DShot driver: the `sitl.c` stub plus `USE_DSHOT_TELEMETRY`/`USE_RPM_FILTER` deliver per-motor eRPM. This supersedes the M3c milestone table's "a motor-protocol change" note. | `motor_pwm_protocol = DSHOT600` for command-path realism (unverified SITL path, bridge and ESC-model rework, no fidelity gain the behavioural ESC can show). |
| 3 | The **battery stays on the KISS ESC-sensor UART** (M3a); DShot telemetry carries only eRPM. The spike proved the two coexist, `msp.c` preferring DShot RPM. | Moving voltage/current onto DShot extended telemetry (untested EDT path; retires a proven M3a chain for nothing visible). |
| 4 | The **HUD switches source**: Betaflight's values while telemetry is fresh, dashes when it is not — goggles behaviour. Sim-internal battery remains on the bus for Python and logs. | Showing both, or falling back to simulator values (a real pilot loses telemetry when the link drops). |
| 5 | The eRPM feed rides the **state datagram** as a reserved block, staged per packet like the M2 UART bytes. | TCP (arrival tick not reproducible) or computing eRPM inside SITL from motor outputs (duplicates the motor model). |

### Out of scope

MSP-over-CRSF and the CRSF V3 device negotiation; DShot extended telemetry (per-ESC voltage/current/temperature); the DShot protocol switch (decision 2); GPS telemetry (`feature -GPS` stays off in the diff); ELRS dynamic telemetry ratio; moving motor pole pairs into config; M3e's video extras.

## 2. The SITL patch

The spike's edits, transcribed into generator scripts under `third_party/betaflight/tools/` with `ofs-sitl.patch` regenerated (M0 discipline: apply to a clean tree, `git diff`). `scripts/build-sitl.sh` is unchanged — it already builds clean, which §10 shows is required (an incremental `make` linked stale objects that behaved as before).

**CRSF telemetry (§10 "CRSF telemetry: yes, with a 4-edit shim"):** `atomic.h` takes its software-BASEPRI branch for `SIMULATOR_BUILD` (upstreamable, same trick as `UNIT_TEST`); `sitl.c` defines `atomic_BASEPRI`; `target.h` defines `USE_TELEMETRY` and `USE_TELEMETRY_CRSF` (the runtime `FEATURE_TELEMETRY` is default-on; no diff line needed); `mk/SITL.mk` stops excluding `telemetry/crsf.c`.

**DShot telemetry (§10 "DShot telemetry: yes, without USE_DSHOT"):** `common_post.h` keeps `USE_DSHOT_TELEMETRY` (and so `USE_RPM_FILTER`) under `SIMULATOR_BUILD` without `USE_DSHOT`, the pattern of the existing `USE_ESC_SENSOR` exception; `target.h` defines both plus `USE_DSHOT_TELEMETRY_STATS` (without it `MSP_MOTOR_TELEMETRY` hardcodes `invalidPct` 100.00 %); `sitl.c`'s stub grows the ~70-line API that those defines reference (`useDshotTelemetry = true`, `dshotTelemetryState`, `dshotDMAHandlerCycleCounters`, the `getDshot*`/`getMotorFrequencyHz`/`isDshot*`/`dshotCleanTelemetryData`/`updateDshotTelemetry`/`getDshotSensorData` set). eRPM units follow the spike: the feed stores true eRPM, `getDshotErpm` returns eRPM/100 (ERPM_PER_LSB), `getDshotRpm` converts through `erpmToRpm` (pole pairs from `motor_poles`, default 14 = the shipped quad).

**Upstream guards (§10 "Two upstream Betaflight bugs"):** NULL checks at the three `osdEscDataCombined` deref sites (`osd_elements.c` warning blink; `osd.c` `getAverageEscRpm` and the max-ESC-temp stats update) — with `USE_ESC_SENSOR` and `USE_DSHOT_TELEMETRY` both compiled and the ESC-sensor feature on, the pointer is read before its only assignment (`osdProcessStats2`); unguarded it SIGSEGVs at first arm. Worth sending upstream with the §5 `storageTotal` fix.

**The feed hook:** `pwmCompleteMotorUpdate` (per PID loop, main thread) calls the stub's feed, which replaces the spike's synthetic values with the datagram block's per-motor eRPM (§3), converts each to Hz through `erpmToRpm(erpm/100)/60`, and stores it for `getMotorFrequencyHz` — the RPM filter's input.

## 3. The eRPM datagram block

After the M2 UART blocks, the state datagram may carry one reserved block — the format of the UART blocks with a reserved index:

- `[250][8][0][4 × u16 LE]` — index 250 (`EXT_ERPM_BLOCK_ID`, above any UART index), length 8, then motor 0..3's eRPM/100 (u16 covers 6.55 M eRPM; the shipped quad's 1950 KV motor on a full 6S tops out near 0.35 M).
- Staged with its datagram like UART bytes and applied at that tick; a second block in one datagram replaces the first, a malformed block is dropped with a `[SITL]` line, and no block (a legacy simulator) leaves the stub at zero — `isDshotMotorTelemetryActive` false, the RPM filter harmless.
- The bridge appends the block when composing each state datagram, reading the four `motor_omega` bus signals: eRPM = ω × 60/2π × pole pairs. Pole pairs is the named constant 7 the KISS shim already carries (`ofs-fc::sitl::esc_telemetry::POLE_PAIRS`); it moves to config only if a motor ever disagrees (carried debt).
- The block is sent once per state datagram (1 kHz); the hook applies the latest values each PID loop, so the filter sees values at most one exchange old.

## 4. CRSF telemetry through the sim

The reply datagram's UART2 (index 1) trailer blocks are Betaflight's telemetry frames (§10: ~9 Hz per type — VARIO 0x07, BATTERY 0x08, BARO_ALTITUDE 0x09, BARO 0x11, MAG 0x12, ATTITUDE 0x1E, FLIGHT_MODE 0x21; up to 530-byte replies while flowing). The bridge routes those bytes to the ELRS model's new telemetry ingress; `ofs-radio::crsf::Decoder` (streaming, CRC-checking, `Frame::Other` passthrough) turns them into frames. CRC failures count, never panic — the decoder's existing contract. Unit quirks pinned during implementation against `sendBattery` (§10: the voltage word read 246 for 24.6 V).

## 5. The ELRS downlink (`ofs-radio::elrs`)

On M3c's rewritten link, the downlink gains telemetry slots:

- **Every 4th downlink packet** carries one telemetry frame (named constant, an ExpressLRS telemetry-ratio estimate: 125 slots/s at the 500 Hz packet rate — the provenance note in `docs/research/elrs-link.md` records it as an estimate). M3c's LINK_STATISTICS downlink keeps its own interval; the two interleave.
- Frames wait in a bounded FIFO (16, named constant). When it is full the **oldest frame is dropped**: stale data yields to fresh, matching a radio that cannot retransmit.
- Link down → no downlink packets → no telemetry (M3c behaviour, unchanged). Uplink untouched. Open-loop quads (no Betaflight) produce no frames: slots idle, the HUD dashes.
- **Determinism:** no new random draws; the queue is a pure function of the frames offered and the slots flown, so lockstep runs stay bit-identical.

## 6. Protocol and clients

**Protocol 6** (`PROTOCOL_VERSION = 6`; M3c ships 5; all clients move together). `RadioLink` gains `fc_telemetry`:

- `battery_v`, `battery_a`, `battery_mah` (Betaflight's filtered values), `attitude_roll_deg`/`_pitch_deg`/`_yaw_deg` (Betaflight's estimator, not the sim's truth), `flight_mode` (string, ≤ 16 bytes), `telemetry_age_s` (time since the last frame; infinity until one arrives).

**Python:** `state.fc_telemetry.*` (the sim-internal `state.battery_*` and attitude stay as they are).

**`ofs-client` and the `OfsClient` node:** telemetry gains the same fields.

**Godot HUD:** the battery readout and a flight-mode line switch to Betaflight's values while `telemetry_age_s < 1` (named constant), else `—`. The link line's existing M3c fields are unchanged.

## 7. Order of work, failure handling and tests

**Order, riskiest first:**
1. The patch generators and the rebuilt SITL; the spike's probes re-run as live proof (CRSF frames on the reply datagram, RC coexisting, per-motor eRPM in `MSP_MOTOR_TELEMETRY`).
2. The eRPM datagram block (bridge, SITL staging and feed hook).
3. The telemetry ingress and the downlink scheduler.
4. Protocol 6, Python.
5. The Godot HUD.
6. Docs, carried debt, CI.

**Failure handling:** a malformed or oversized eRPM block, or one naming an unknown motor count, is dropped with a `[SITL]` line naming it, as the UART blocks are. Telemetry CRC errors are counted, not errors. A missing block or a silent Betaflight degrades to zeros/dashes — drone behaviour, never errors. The two upstream OSD guards are load-bearing: without them the first arm SIGSEGVs (§10).

**Tests:**
- **Unit (`ofs-radio`):** the downlink scheduler — frame order, FIFO drop-oldest, loss behaviour at link-down, LINK_STATISTICS interleaving, same seed ⇒ same sequence; the CRSF decoder on the seven frame types and a corrupt byte stream.
- **Unit (`ofs-fc`):** the eRPM block codec — round-trip, ω→eRPM arithmetic (1950 KV-class motor at full throttle stays under u16/100), malformed blocks dropped.
- **Simulator, open loop:** with an SITL quad, `fc_telemetry` reaches the client snapshot and ages out; two lockstep runs bit-identical with telemetry flowing.
- **Live SITL (`python/tests/test_sitl_telemetry.py`):** armed and throttling, `MSP_MOTOR_TELEMETRY` shows four distinct RPMs matching the sim's motor models within quantization (compare through the Python client); `MSP_RC` still tracks sticks (telemetry did not disturb RC); the client's `fc_telemetry.battery_v` tracks the sim's battery within 0.5 V (the ESC-sensor value is exact; the margin is Betaflight's own voltage filtering); a forced link loss dashes the telemetry while `state.battery_*` lives on.
- **Godot e2e:** the HUD battery reads Betaflight's value and dashes when the link is cut.
- **Stretch (the plan decides):** a Blackbox-based proof that the RPM filter notches motor-frequency gyro noise; skipped if it costs more than a task.
- **CI:** the `sitl` job grows the live test.

**Docs:** `docs/research/elrs-link.md` gains the telemetry slot/ratio model with provenance; `dev-setup.md` (no config change to document — the manual check: fly, watch the HUD battery sag with punch-outs, cut the link and watch telemetry dash out while the picture stays, see four distinct RPMs in the Configurator's Motors tab); `README.md`; `docs/superpowers/m3d-carried-debt.md`.

## 8. Risks

| Risk | Mitigation |
|---|---|
| The BATTERY_SENSOR payload's units differ from the CRSF spec (§10 read 246 for 24.6 V). | The decoder pins units against `sendBattery` in `telemetry/crsf.c`; a live test compares against the sim's battery. |
| The 1:4 telemetry ratio is an estimate. | A named constant with its provenance recorded; config exposure only if a user misses it (carried debt). |
| The 125 Hz slot rate outruns Betaflight's ~9 Hz-per-type production; the FIFO sits empty. | Correct behaviour (slots idle); the scheduler test pins it. |
| The RPM filter's effect is asserted structurally (gate + per-motor Hz) but not spectrally. | The stretch Blackbox test; the HUD/Python show the per-motor RPMs regardless. |
| M3c's merged `elrs` shape differs from its spec when this plan is written. | The plan is written against merged code and re-argues §5's seams then; this spec pins only the behaviour. |
| Incremental SITL builds lie (§10 "Build notes"). | The build script already builds clean; never trust an incremental build during this work. |
