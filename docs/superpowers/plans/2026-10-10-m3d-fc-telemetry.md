# M3d: Flight-Controller Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Betaflight's own CRSF telemetry reaches the handset over the simulated ELRS downlink (the HUD battery and flight mode show Betaflight's values and dash out when the link drops), and the simulator's per-motor eRPM reaches Betaflight's RPM filter through a reserved state-datagram block — with the motor protocol staying PWM.

**Architecture:** The M3d spike (`docs/research/sitl-interface.md` §10, probes in `spikes/m3d/`) proved both chains on live SITL. Task 1–2 transcribe the spike's SITL edits into generator scripts and regenerate `third_party/betaflight/ofs-sitl.patch`: an atomic-block shim brings `telemetry/crsf.c` back into the build (its frames ride the reply datagram's UART2 trailer), and a `sitl.c` stub satisfies the `USE_DSHOT_TELEMETRY`/`USE_RPM_FILTER` API without the DShot driver, fed per-motor eRPM from a reserved block `[250][len][4 × u16]` in the state datagram. On the simulator side the bridge composes that block from the `motor_omega` bus signals (Task 3), routes Betaflight's UART2 output to a new telemetry ingress on `ElrsLink` (Task 4–5), whose downlink carries one telemetry frame every 4th downlink packet from a bounded drop-oldest FIFO and publishes the decoded battery/attitude/flight mode through a shared handle (the `OsdHandle` pattern). Protocol 6 (`PROTOCOL_VERSION = 6`) carries `State.fc_telemetry` to the Python client, `ofs-client` and the Godot HUD (Task 6–7). Docs and carried debt close the milestone (Task 8).

**Tech Stack:** Rust 1.85+ (workspace: glam, rand_chacha, prost/tonic, serde/toml), Python 3.10+ client (grpcio), Godot 4.7.2 + godot-rust, Betaflight SITL 2026.6.2 (pinned patch, WSL2).

**Spec:** `docs/superpowers/specs/2026-10-10-m3d-fc-telemetry-design.md` (read it first; this plan implements it). Spike evidence: `docs/research/sitl-interface.md` §10 and `spikes/m3d/` (merged as f1d2dde). Predecessor: `docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md` (merged as 4b0fb8f — this plan is written against that merged tree). Parent: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.

## Global Constraints

- Rust edition 2021, `rust-version = "1.85"`: no language or std features newer than 1.85 in any crate except `ofs-godot` (the msrv CI job runs `cargo check --workspace --all-targets --locked --exclude ofs-godot`).
- Python >= 3.10. Godot **4.7.2** only (GL Compatibility). License GPL-3.0-or-later.
- Determinism: same quad + world + seed + inputs in lockstep give identical output. M3d adds **no new random draws**: the telemetry FIFO and slot counter are pure functions of the frames offered and the packets flown; `ElrsLink` keeps drawing exactly what M3c drew (the uplink fades, the uplink loss draw, the downlink fades, the downlink draw — in that order).
- Protocol: `PROTOCOL_VERSION = 6` in `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` together (a test compares them). Quad schema stays **4**, world schema stays **1** — M3d changes no config file format and no Betaflight diff.
- Frames: NED metres for world positions, FRD for body vectors; only `ofs-client` converts to Godot's frame.
- The ELRS link runs at the packet rate (50/150/250/500 Hz); the eRPM block rides every state datagram (1 kHz at the shipped config).
- Every text file ends with a newline. Commit the `.uid` files Godot creates next to new `.gd` files. Never stage `.superpowers/`, `graphify-out/`, `shots/` or `build/`.
- Commit messages end with the trailer `Co-Authored-By: <the model of the session executing the task> <noreply@anthropic.com>`.
- Never use `pkill -f` (use `pkill -x betaflight_SITL`); never run `wsl --shutdown`; do not touch Windows Firewall settings (ask the user).
- **Never trust an incremental SITL build** (§10 "Build notes": an incremental `make` linked stale objects that behaved as before). Every SITL build in this plan runs `bash scripts/build-sitl.sh`, which builds clean.
- Files that contain backslashes must be written with the editor tools (Write/Edit), not Bash heredocs (heredocs collapse `\\`).
- Every edit below is "In `file`, replace: ... with: ...": the old text appears exactly once in the file at that point of the plan. Apply the edits of a step in the order given. C code blocks in generator scripts are Python string literals: backslashes are doubled there by the generator's own raw strings, never by hand.

## Rulings and facts verified in advance

Every code block below was written against the current tree (4b0fb8f + f1d2dde) with these facts in hand; where the spec was silent or self-contradictory, the ruling is listed here and the change is one config value or one function if the human disagrees. All SITL-side facts were verified live by the spike (§10).

- **Ruling: `USE_DSHOT_TELEMETRY_STATS` is defined** (the spec asks for it). Verified 2026-10-10: with both stats and telemetry defines and no `USE_DSHOT`, the SITL links. With the stats on, `MSP_MOTOR_TELEMETRY`'s `invalidPct` comes from the stub's `getDshotTelemetryMotorInvalidPercent` (0.00 %) instead of the hardcoded 100.00 % — the honest value, since the sim has no telemetry-loss source.
- **Ruling: the FC telemetry crosses to clients through a shared handle, not bus signals** — the flight-mode field is a string and the bus carries numbers. `ElrsLink` owns an `FcTelemetryHandle` (the `OsdHandle` pattern: `Arc<Mutex<…>>`, `crates/ofs-video/src/osd.rs:195`); `Vehicle::state()` snapshots it. Consequence: the FC telemetry is not part of `Vehicle::digest()`; lockstep determinism is pinned by the ELRS byte-stream test (Task 4) and the live lockstep run (Task 6), like the OSD's.
- **Ruling: the HUD shows the flight mode on the battery line** (`12.40 V   3.2 A   ACRO`), not as its own label — the HUD's label grid is fixed-place and one line carries both spec fields. The whole line shows dashes when the telemetry is staler than 1 s.
- **Ruling: Betaflight's flight-mode strings pass through verbatim.** `crsfFrameFlightMode` (`telemetry/crsf.c:609`) sends a fixed set (`"ACRO"`, `"!FS!"`, `"RTH"`, `"PASS"`, `"ANGLE"`, `"HORIZON"`, `"AIR *"`…); the sim does not re-map them.
- **Ruling: `fc_telemetry` is a `State` field, not a `RadioLink` field.** The spec's §6 header says "`RadioLink` gains `fc_telemetry`" but its Python row says `state.fc_telemetry.*`; a top-level `State.fc_telemetry` message satisfies the Python text, keeps `RadioLink` to the radio's own measurements, and maps one `VehicleState` field. The Rust/Python/Godot surfaces all use `fc_telemetry`.
- **Ruling: the eRPM block is per-datagram.** The spec's "no block (a legacy simulator) leaves the stub at zero" is honoured per datagram: `extErpmValid` resets at every `extInjectSerial` call (which runs once per taken tick, even with no blocks), so a datagram without the block feeds zeros. The shipped bridge sends the block in every datagram after the first, so nothing changes in practice.
- **Ruling: the live tests also re-pin RC (spec §7's "`MSP_RC` still tracks sticks").** Task 1's `t_a4` control run proved RC survives telemetry at the spike level; Task 6's live test asserts it again through the client path.
- **Ruling: telemetry slots exist only while the receiver transmits.** M3c computes `downlink_received` only when `transmitting`; the slot counter advances with downlink packets, so a link that never comes up (or drops) simply stops producing slots — "link down = telemetry stops" needs no extra rule. Frames Betaflight offered meanwhile wait in the FIFO (bounded, drop-oldest).
- **Ruling: the eRPM block counts against `EXT_SERIAL_MAX` (512).** At the shipped config the serial section carries ~32 bytes of CRSF RC, so ~43 of 512 bytes are used; `state_datagram_with_serial`'s assert guards overflow. A first (resendable) datagram carries no eRPM block, like its serial blocks.
- **Ruling: the stub reports zeros until the first eRPM block arrives**, so a legacy simulator (one that sends no block) degrades cleanly: `isDshotMotorTelemetryActive` false, the RPM filter fed zeros (harmless). The spike's synthetic feed (throttle × 120 000 × (1 + 0.25 i)) is Task 1's proof only and is replaced in Task 2.
- **Ruling: the RPM filter's spectral proof (a notch at motor frequency in the gyro) is not implemented** — the spec leaves it to the plan and it costs a Blackbox-decode harness for a value the structural tests already pin (the filter's gate is on, its input is the per-motor Hz). Recorded in `m3d-carried-debt.md`.
- **Ruling: `MspMotorTelemetry` battery fields pass through Betaflight verbatim**, including the ESC-sensor quirks (consumption summed over motor slots, so ×4 on a 4-motor quad; §9). The HUD shows Betaflight's values as Betaflight computes them.
- **Fact: Betaflight's CRSF payload encodings** (pinned against `telemetry/crsf.c:450–625`): `BATTERY_SENSOR` (0x08) payload = u16 BE voltage in **0.1 V** units (`getLegacyBatteryVoltage()`, not the CRSF spec's 0.01 V), u16 BE current in **0.1 A** units (`getAmperage()/10`), u24 BE mAh drawn, u8 remaining %; `ATTITUDE` (0x1E) payload = 3 × i16 BE **radians × 10 000**, order pitch, roll, yaw, yaw wrapped to ±180°; `FLIGHT_MODE` (0x21) payload = a NUL-terminated string.
- **Fact: `ElrsLink::new(params, rate_divisor, seed, uart, bus)`** (`crates/ofs-radio/src/elrs.rs:159`); its `step` computes `received`, `downlink_received`, LQ windows and writes LINK_STATISTICS on the uplink UART; `Frame::Other { address, frame_type, payload }` is the decoder's passthrough (`crates/ofs-radio/src/crsf.rs:143`). The test rig `rig()` in `crates/ofs-radio/tests/elrs.rs:13` constructs the link against a 1 MiB `Wire` and steps whole packets with `packets()`.
- **Fact: the bridge serial plumbing** (`crates/ofs-fc/src/sitl/`): `BridgeConfig { …, serial: Vec<SerialLink>, taps: Vec<SerialTap> }`; `SerialLink { uart_index: u8, rx: Wire }` carries bytes into SITL (composed per datagram by `take_serial_blocks`, ≤ `SERIAL_SECTION_MAX` = 512); `SerialTap { uart_index: u8, tx: Wire }` receives the reply's UART blocks via `route_reply`. `state_datagram_with_serial` appends `[index][len u16 LE][bytes]` blocks and asserts the 512 cap; `parse_reply_serial` walks the reply trailer after `ServoPacket::SIZE` (16) + a dropped-bytes `u16`.
- **Fact: the vehicle wiring** (`crates/ofs-sim/src/vehicle.rs`): the receiver UART is built at line 585, `ElrsLink::new` is pushed at 587, the Sitl arm composes `serial` (616) and `taps: video.taps` (628); `Handles::register` reads the bus into `VehicleState` (243, 724); the open-loop arm never constructs a bridge. `names::motor_omega(i)` is the per-motor angular velocity (rad/s); `ofs_fc::sitl::esc_telemetry::POLE_PAIRS = 7.0`.
- **Fact: the generator style** (`third_party/betaflight/tools/add_serial_in_datagram.py`): an `edit(rel, replacements)` helper asserts each anchor occurs exactly once; the docstring carries the regenerate procedure (build clean from the committed patch → run generators → `git diff > ofs-sitl.patch` → rebuild clean). Generators are **not** idempotent.
- **Fact: the M3c ELRS tests** pin the rig's behaviour (one RC frame per received packet, LQ 100 on an ideal link, same-seed byte streams) and keep passing through Tasks 4–5 unchanged.
- **Fact: the live-test recipe** (Windows host, Git Bash): `wsl_run() { wsl.exe -d Ubuntu -e bash -lc "cd /mnt$(pwd) && export CARGO_TARGET_DIR=\$HOME/ofs/target OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf && $*"; }`; Python live tests use the `sim` fixture from `python/tests/conftest.py` and are skipped without `OFS_SITL_LAUNCH`. CI's `sitl` job runs the SITL pytest list at `.github/workflows/ci.yml:100`.
- **Fact: the Python stubs are generated** by `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` (`docs/dev-setup.md:17`; `grpcio-tools` is a dev dependency).

## File Structure

- `third_party/betaflight/tools/add_fc_telemetry.py` — generator: the atomic-block shim, the telemetry defines, the DShot-telemetry stub, the OSD NULL guards (Task 1).
- `third_party/betaflight/tools/add_erpm_block.py` — generator: the eRPM block intercept and the stub's feed swap (Task 2).
- `third_party/betaflight/ofs-sitl.patch` — regenerated by Tasks 1 and 2.
- `spikes/m3d/t_b2_erpm_block.py` — throwaway probe: the eRPM block end to end against real SITL (Task 2).
- `crates/ofs-fc/src/sitl/codec.rs` — `EXT_ERPM_BLOCK_ID`, `erpm_block_payload` (Task 3).
- `crates/ofs-fc/src/sitl/bridge.rs` — the bridge composes the eRPM block (Task 3).
- `crates/ofs-radio/src/crsf.rs` — the three telemetry frame-type constants (Task 4).
- `crates/ofs-radio/src/elrs.rs` — `FcTelemetry`, `FcTelemetryHandle`, the telemetry FIFO, slots and decoders; the telemetry ingress on `ElrsLink` (Task 4).
- `crates/ofs-radio/tests/elrs.rs` — the scheduler unit tests (Task 4).
- `crates/ofs-fc/tests/sitl_codec.rs` — the eRPM block codec tests (Task 3).
- `crates/ofs-sim/src/vehicle.rs` — the radio tap, the handle storage, `FcTelemetrySnapshot` in `VehicleState` (Tasks 3, 5).
- `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs` — protocol 6 (Task 6).
- `crates/ofs-sim/src/session.rs` — `State.fc_telemetry` (Task 6).
- `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` — regenerated (Task 6).
- `python/ofs/client.py` — `FcTelemetry`, `State.fc_telemetry`, `PROTOCOL_VERSION = 6` (Task 6).
- `python/tests/test_sitl_telemetry.py` — the live SITL tests (Task 6).
- `.github/workflows/ci.yml` — the new test file in the sitl job (Task 6).
- `crates/ofs-client/src/model.rs`, `crates/ofs-godot/src/lib.rs`, `godot/ui/hud.gd` — the client chain (Task 7).
- `godot/tests/test_hud.gd`, `godot/tests/e2e_betaflight.gd` — the HUD and e2e tests (Task 7).
- `docs/research/elrs-link.md`, `docs/dev-setup.md`, `README.md`, `docs/superpowers/m3d-carried-debt.md`, `docs/superpowers/m3a-carried-debt.md` — docs (Task 8).

---

### Task 1: The SITL patch — CRSF telemetry shim and the DShot-telemetry stub

**Files:**
- Create: `third_party/betaflight/tools/add_fc_telemetry.py`
- Modify: `third_party/betaflight/ofs-sitl.patch` (regenerated)
- Test (throwaway, re-run): `spikes/m3d/t_a5_crsf_telemetry.py`, `spikes/m3d/t_b_dshot_telemetry.py` (committed in f1d2dde)

**Interfaces:**
- Consumes: the merged `ofs-sitl.patch` (M0–M3a edits: external time, serial in/out datagrams, the `USE_ESC_SENSOR` stub with `useDshotTelemetry`/`initDshotTelemetry`/`erpmToRpm`).
- Produces: a SITL binary that streams Betaflight's CRSF telemetry in the reply datagram's UART2 (index 1) blocks, arms and flies with RC unaffected, and answers `MSP_MOTOR_TELEMETRY` with the stub's synthetic per-motor eRPM (replaced by the datagram in Task 2). Tasks 2–7 build on this binary.

- [ ] **Step 1: Write the generator**

Create `third_party/betaflight/tools/add_fc_telemetry.py` (LF endings; the Write tool guarantees them):

```python
"""Adds flight-controller telemetry to a Betaflight tree that already has ofs-sitl.patch applied:
Betaflight's CRSF telemetry (an atomic-block shim; telemetry/crsf.c compiles again), and per-motor
eRPM for the RPM filter through an emulated DShot telemetry (no USE_DSHOT, no DShot driver). The
eRPM itself arrives in Task 2's state-datagram block; until then the stub feeds synthetic values so
the chain is provable standalone. See docs/research/sitl-interface.md §10 and the M3d spec.

The base tree is the committed ofs-sitl.patch applied to the pinned Betaflight checkout. The
generators are NOT idempotent: running one on a tree that already has the current patch inserts its
changes a second time.

Usage (Linux or WSL), from the repository root:
    bash scripts/build-sitl.sh                                   # pinned commit + the committed patch
    python3 third_party/betaflight/tools/add_fc_telemetry.py ~/ofs/betaflight
    git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
    bash scripts/build-sitl.sh                                   # rebuild from the regenerated patch
"""
import sys

if len(sys.argv) != 2:
    sys.exit(f"usage: python3 {sys.argv[0]} <betaflight checkout>  (see the docstring above)")
ROOT = sys.argv[1]


def edit(rel, replacements):
    path = f"{ROOT}/{rel}"
    with open(path, newline="") as f:
        text = f.read()
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: anchor not found exactly once:\n{old}"
        text = text.replace(old, new, 1)
    with open(path, "w", newline="") as f:
        f.write(text)


# 1. atomic.h: the software-BASEPRI branch (UNIT_TEST's) also covers simulator builds. Upstreamable.
edit("src/main/build/atomic.h", [
    (
        "#if !defined(UNIT_TEST)\n// BASEPRI manipulation functions",
        "#if !defined(UNIT_TEST) && !defined(SIMULATOR_BUILD)\n// BASEPRI manipulation functions",
    ),
    (
        "#if defined(UNIT_TEST)\n// atomic related functions for unittest.",
        "#if defined(UNIT_TEST) || defined(SIMULATOR_BUILD)\n"
        "// atomic related functions for unittest and simulator builds (software BASEPRI).",
    ),
])

# 2. SITL.mk: telemetry/crsf.c compiles again (its ATOMIC_BLOCK is emulated above).
edit("src/platform/SIMULATOR/mk/SITL.mk", [
    (
        "        telemetry/crsf.c \\\n        telemetry/ghst.c \\",
        "        telemetry/ghst.c \\",
    ),
])

# 3. target.h: CRSF telemetry (the master USE_TELEMETRY was never defined for SITL) and the emulated
#    DShot telemetry (no USE_DSHOT: dshot.c stays out, the sitl.c stub owns the API).
edit("src/platform/SIMULATOR/target/SITL/target.h", [
    (
        "#undef USE_TELEMETRY_CRSF\n",
        "// M3d: CRSF telemetry enabled; atomic.h emulates BASEPRI in software for SIMULATOR_BUILD.\n"
        "#define USE_TELEMETRY\n"
        "#define USE_TELEMETRY_CRSF\n",
    ),
    (
        "// USE_TELEMETRY_CRSF stays undefined: telemetry/crsf.c needs an ATOMIC_BLOCK shim for SITL (future work)\n",
        "",
    ),
    (
        "#define USE_PWM_OUTPUT\n",
        "#define USE_PWM_OUTPUT\n"
        "// M3d: DShot telemetry emulated in sitl.c (no DShot driver), so the RPM filter builds.\n"
        "#define USE_DSHOT_TELEMETRY\n"
        "#define USE_DSHOT_TELEMETRY_STATS\n"
        "#define USE_RPM_FILTER\n",
    ),
])

# 4. common_post.h: keep USE_DSHOT_TELEMETRY (and with it USE_RPM_FILTER) without USE_DSHOT under
#    SIMULATOR_BUILD — the same exception USE_ESC_SENSOR already has.
edit("src/main/target/common_post.h", [
    (
        "#ifndef USE_DSHOT\n#undef USE_DSHOT_TELEMETRY\n#undef USE_DSHOT_BITBANG\n#endif",
        "#if !defined(USE_DSHOT) && !defined(SIMULATOR_BUILD)  // SITL: DShot telemetry is emulated without the driver\n"
        "#undef USE_DSHOT_TELEMETRY\n"
        "#undef USE_DSHOT_BITBANG\n"
        "#endif",
    ),
])

# 5. sitl.c: the atomic_BASEPRI storage (atomic.c is UNIT_TEST-only), the includes the stub needs,
#    the DShot-telemetry stub (synthetic per-motor eRPM until Task 2's datagram block lands), and the
#    feed hook in pwmCompleteMotorUpdate.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        '#include "drivers/pwm_output_impl.h"\n',
        '#include "drivers/pwm_output_impl.h"\n'
        '#include "drivers/dshot.h"\n'
        '#include "sensors/esc_sensor.h"\n',
    ),
    (
        "uint32_t microsISR(void)  // used by serial RX drivers (e.g. CRSF frame timing)\n{",
        "uint8_t atomic_BASEPRI;  // software BASEPRI for ATOMIC_BLOCK (atomic.h); atomic.c is UNIT_TEST-only\n"
        "\n"
        "uint32_t microsISR(void)  // used by serial RX drivers (e.g. CRSF frame timing)\n{",
    ),
    (
        "#if defined(USE_ESC_SENSOR) && !defined(USE_DSHOT)\n"
        "// ESC telemetry without DShot: the simulator streams KISS telemetry frames to the ESC sensor UART.\n"
        "// These stand in for the helpers in drivers/dshot.c (compiled only with USE_DSHOT); same math.\n"
        "bool useDshotTelemetry = false;\n"
        "static float sitlErpmToHz = 0.0f;\n"
        "\n"
        "void initDshotTelemetry(const timeUs_t looptimeUs)\n"
        "{\n"
        "    UNUSED(looptimeUs);\n"
        "    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100\n"
        "}\n"
        "\n"
        "float erpmToRpm(uint32_t erpm)\n"
        "{\n"
        "    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;\n"
        "}\n"
        "#endif",
        "#if defined(USE_ESC_SENSOR) && !defined(USE_DSHOT)\n"
        "// ESC telemetry without DShot: the simulator streams KISS telemetry frames to the ESC sensor UART.\n"
        "// These stand in for the helpers in drivers/dshot.c (compiled only with USE_DSHOT); same math.\n"
        "static float sitlErpmToHz = 0.0f;\n"
        "\n"
        "void initDshotTelemetry(const timeUs_t looptimeUs)\n"
        "{\n"
        "    UNUSED(looptimeUs);\n"
        "    sitlErpmToHz = 100.0f / SECONDS_PER_MINUTE / (motorConfig()->motorPoleCount / 2.0f);  // ERPM_PER_LSB = 100\n"
        "}\n"
        "\n"
        "float erpmToRpm(uint32_t erpm)\n"
        "{\n"
        "    return erpm * sitlErpmToHz * SECONDS_PER_MINUTE;\n"
        "}\n"
        "\n"
        "#ifdef USE_DSHOT_TELEMETRY\n"
        "// SITL DShot-telemetry emulation (dshot.c compiles only with USE_DSHOT): per-motor eRPM provided by\n"
        "// the simulator. Task 1 feeds synthetic values so the chain is provable standalone; Task 2's state-\n"
        "// datagram block replaces them with the simulator's motor models.\n"
        "bool useDshotTelemetry = true;\n"
        "dshotTelemetryState_t dshotTelemetryState;\n"
        "dshotTelemetryCycleCounters_t dshotDMAHandlerCycleCounters;\n"
        "static uint32_t sitlDshotErpm[MAX_SUPPORTED_MOTORS];\n"
        "static float sitlMotorFrequencyHz[MAX_SUPPORTED_MOTORS];\n"
        "\n"
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // servo_packet carries 4 motor values\n"
        "        const float throttle = pwmPkt.motor_speed[i];  // throttle 0..1 ((us - 1000) / 1000)\n"
        "        const uint32_t erpm = (uint32_t)(throttle * 120000.0f * (1.0f + 0.25f * i));\n"
        "        sitlDshotErpm[i] = erpm;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(erpm / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}\n"
        "\n"
        "uint16_t getDshotErpm(uint8_t motorIndex) { return (uint16_t)(sitlDshotErpm[motorIndex] / 100U); }  // ERPM_PER_LSB\n"
        "float getDshotRpm(uint8_t motorIndex) { return erpmToRpm(sitlDshotErpm[motorIndex] / 100U); }\n"
        "float getDshotRpmAverage(void)\n"
        "{\n"
        "    float sum = 0.0f;\n"
        "    for (int i = 0; i < 4; i++) {\n"
        "        sum += getDshotRpm(i);\n"
        "    }\n"
        "    return sum / 4.0f;\n"
        "}\n"
        "float getMotorFrequencyHz(uint8_t motorIndex) { return sitlMotorFrequencyHz[motorIndex]; }\n"
        "float getMinMotorFrequencyHz(void)\n"
        "{\n"
        "    float m = sitlMotorFrequencyHz[0];\n"
        "    for (int i = 1; i < 4; i++) {\n"
        "        if (sitlMotorFrequencyHz[i] < m) m = sitlMotorFrequencyHz[i];\n"
        "    }\n"
        "    return m;\n"
        "}\n"
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { return sitlDshotErpm[motorIndex] > 0; }\n"
        "bool isDshotTelemetryActive(void) { return true; }\n"
        "void dshotCleanTelemetryData(void) { memset(sitlDshotErpm, 0, sizeof(sitlDshotErpm)); }\n"
        "int16_t getDshotTelemetryMotorInvalidPercent(uint8_t motorIndex) { UNUSED(motorIndex); return 0; }\n"
        "void updateDshotTelemetry(void) {}\n"
        "bool getDshotSensorData(escSensorData_t *dest, int motorIndex)\n"
        "{\n"
        "    memset(dest, 0, sizeof(*dest));\n"
        "    dest->dataAge = 0;\n"
        "    dest->temperature = 25;\n"
        "    dest->rpm = (int16_t)constrain(sitlDshotErpm[motorIndex] / 100U, 0, 32767);  // 0.01 erpm units (escSensorData_t contract)\n"
        "    return true;\n"
        "}\n"
        "#else\n"
        "bool useDshotTelemetry = false;\n"
        "#endif\n"
        "#endif",
    ),
    (
        "    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.\n"
        "    return;",
        "    // pwmPkt now holds the latest motor values; simulatorSchedulerIdle() sends one reply per tick.\n"
        "#if defined(USE_ESC_SENSOR) && defined(USE_DSHOT_TELEMETRY) && !defined(USE_DSHOT)\n"
        "    sitlDshotTelemetryFeed();  // M3d: per-motor eRPM (synthetic until Task 2's datagram block)\n"
        "#endif\n"
        "    return;",
    ),
])

# 6. Two upstream NULL-deref bugs: osdEscDataCombined is assigned only in osdProcessStats2 (osd.c),
#    but with USE_ESC_SENSOR and USE_DSHOT_TELEMETRY both compiled it is read during flight — at the
#    ESC-temperature warning blink and in the post-flight stats. Un-guarded, the first arm SIGSEGVs
#    (kernel trap IP + objdump; §10). Worth sending upstream with the storageTotal fix (M0 §5).
edit("src/main/osd/osd_elements.c", [
    (
        "        blink = osdConfig()->esc_temp_alarm != ESC_TEMP_ALARM_OFF && osdEscDataCombined->temperature >= osdConfig()->esc_temp_alarm;",
        "        blink = osdEscDataCombined != NULL && osdConfig()->esc_temp_alarm != ESC_TEMP_ALARM_OFF && osdEscDataCombined->temperature >= osdConfig()->esc_temp_alarm;",
    ),
])
edit("src/main/osd/osd.c", [
    (
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        return lrintf(erpmToRpm(osdEscDataCombined->rpm));\n"
        "    }",
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        if (osdEscDataCombined == NULL) return 0;  // OFS: assigned only in osdProcessStats2\n"
        "        return lrintf(erpmToRpm(osdEscDataCombined->rpm));\n"
        "    }",
    ),
    (
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        value = osdEscDataCombined->temperature;\n"
        "        if (stats.max_esc_temp < value) {\n"
        "            stats.max_esc_temp = value;\n"
        "        }\n"
        "    } else",
        "    if (featureIsEnabled(FEATURE_ESC_SENSOR)) {\n"
        "        if (osdEscDataCombined != NULL) {  // OFS: assigned only in osdProcessStats2\n"
        "            value = osdEscDataCombined->temperature;\n"
        "            if (stats.max_esc_temp < value) {\n"
        "                stats.max_esc_temp = value;\n"
        "            }\n"
        "        }\n"
        "    } else",
    ),
])

print("add_fc_telemetry: edits applied")
```

- [ ] **Step 2: Apply the generator and regenerate the patch**

The WSL tree still carries the spike's uncommitted edits; discard them first (they are disposable — `build-sitl.sh` resets and re-applies the committed patch).

```bash
wsl.exe -d Ubuntu -e bash -c "cd ~/ofs/betaflight && git reset --hard && git clean -fd src/"
cd "C:\dev\open-fpv-sim" && wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim && bash scripts/build-sitl.sh 2>&1 | tail -3 && python3 third_party/betaflight/tools/add_fc_telemetry.py ~/ofs/betaflight && git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch && git -C ~/ofs/betaflight reset --hard"
```

Expected: the build succeeds, `add_fc_telemetry: edits applied`, and `third_party/betaflight/ofs-sitl.patch` grows by roughly +150 lines. Verify the new patch builds from clean:

```bash
wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim && bash scripts/build-sitl.sh 2>&1 | tail -3"
```

Expected: `Linking SITL` and the final `ls -l` line of the script, with no errors. If a link error names a `dshot`/`telemetry` symbol, stop and record it — the stub's API surface needs the missing function (do not silently extend it; bring it to the reviewer).

- [ ] **Step 3: Re-run the spike probes as live proof**

```bash
wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim/spikes/m3d && OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf OFS_SITL_CLEANUP='pkill -x betaflight_SITL' python3 t_a5_crsf_telemetry.py 2>&1 | tail -8"
wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim/spikes/m3d && rm -rf runs/ta5 runs/tb && OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf OFS_SITL_CLEANUP='pkill -x betaflight_SITL' python3 t_b_dshot_telemetry.py 2>&1 | tail -12"
```

Expected, exactly as recorded in §10: the first prints `375 CRC-valid frames` (±10 %), the frame census 0x07/0x08/0x09/0x11/0x12/0x1E/0x21 and `FLIGHT_MODE: b'AIR*'`; the second prints `DISTINCT PER-MOTOR RPM: YES` and `RPM SCALES WITH THROTTLE: YES` with no `SIGSEGV` (the OSD guards hold at first arm).

- [ ] **Step 4: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add third_party/betaflight/tools/add_fc_telemetry.py third_party/betaflight/ofs-sitl.patch && git commit -m "feat(sitl): CRSF telemetry in (atomic-block shim) and an emulated DShot telemetry for the RPM filter

Two upstream osdEscDataCombined NULL-deref bugs guarded (first arm SIGSEGV'd with
USE_ESC_SENSOR + USE_DSHOT_TELEMETRY compiled). Probes re-run clean.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 2: Per-motor eRPM in the state datagram (SITL side)

**Files:**
- Create: `third_party/betaflight/tools/add_erpm_block.py`
- Modify: `third_party/betaflight/ofs-sitl.patch` (regenerated)
- Create: `spikes/m3d/t_b2_erpm_block.py` (throwaway probe)

**Interfaces:**
- Consumes: Task 1's SITL (the stub with `sitlDshotTelemetryFeed`).
- Produces: a SITL that accepts the reserved state-datagram block `[250][len u16 LE][8 bytes: motor 0..3's eRPM/100, u16 LE]`, stages it with the packet, and feeds it to the stub each PID loop (replacing the synthetic feed). Task 3's bridge composes exactly this block.

- [ ] **Step 1: Write the generator**

Create `third_party/betaflight/tools/add_erpm_block.py`:

```python
"""Adds the simulator's per-motor eRPM block to a Betaflight tree that already has ofs-sitl.patch
and add_fc_telemetry.py applied. The state datagram may carry one reserved block after the UART
blocks — the same [index][len lo][len hi][bytes] format with index 250 — carrying motor 0..3's
eRPM/100 as u16 little-endian. SITL stages it with the packet and the DShot-telemetry stub feeds it
to the RPM filter each PID loop, replacing Task 1's synthetic feed. See the M3d spec §3.

Run AFTER add_fc_telemetry.py; the generators are NOT idempotent.
Usage: python3 third_party/betaflight/tools/add_erpm_block.py ~/ofs/betaflight
"""
import sys

if len(sys.argv) != 2:
    sys.exit(f"usage: python3 {sys.argv[0]} <betaflight checkout>  (see the docstring above)")
ROOT = sys.argv[1]


def edit(rel, replacements):
    path = f"{ROOT}/{rel}"
    with open(path, newline="") as f:
        text = f.read()
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: anchor not found exactly once:\n{old}"
        text = text.replace(old, new, 1)
    with open(path, "w", newline="") as f:
        f.write(text)


# 1. The staging statics, next to the serial staging.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static uint8_t extSerialPending[EXT_SERIAL_MAX];\nstatic int extSerialPendingLen = 0;",
        "static uint8_t extSerialPending[EXT_SERIAL_MAX];\nstatic int extSerialPendingLen = 0;\n"
        "// The simulator's per-motor eRPM block (index 250), staged with the packet like the serial\n"
        "// bytes. Main thread only (extInjectSerial runs there, as does the consumer in pwmCompleteMotorUpdate).\n"
        "static uint16_t extErpm[4];\nstatic bool extErpmValid = false;",
    ),
])

# 2. extInjectSerial: validity is per datagram (a datagram without the block leaves the stub at
#    zero, per the spec), and the reserved index is intercepted before the UART lookup (a legacy
#    UART id of 250 does not exist; the block must not be dropped as "no such open UART").
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static void extInjectSerial(const uint8_t *p, int len)\n{\n    int i = 0;",
        "static void extInjectSerial(const uint8_t *p, int len)\n{\n    int i = 0;\n"
        "    extErpmValid = false;  // the eRPM block is per datagram; the bridge sends it every datagram",
    ),
    (
        "        if (n > len - i) {\n"
        "            printf(\"[SITL] malformed serial block for UART%u: %d bytes announced, %d left; rest dropped\\n\", uart + 1, n, len - i);\n"
        "            return;\n"
        "        }\n"
        "        if (!tcpSerialInject(uart, &p[i], n)) {",
        "        if (n > len - i) {\n"
        "            printf(\"[SITL] malformed serial block for UART%u: %d bytes announced, %d left; rest dropped\\n\", uart + 1, n, len - i);\n"
        "            return;\n"
        "        }\n"
        "        if (uart == 250) {  // M3d: the simulator's per-motor eRPM block\n"
        "            if (n != 8) {\n"
        "                printf(\"[SITL] malformed eRPM block: %d bytes, expected 8; dropped\\n\", n);\n"
        "            } else {\n"
        "                memcpy(extErpm, &p[i], sizeof(extErpm));\n"
        "                extErpmValid = true;\n"
        "            }\n"
        "            i += n;\n"
        "            continue;\n"
        "        }\n"
        "        if (!tcpSerialInject(uart, &p[i], n)) {",
    ),
])

# 3. The stub consumes the staged values instead of the synthetic feed.
edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // servo_packet carries 4 motor values\n"
        "        const float throttle = pwmPkt.motor_speed[i];  // throttle 0..1 ((us - 1000) / 1000)\n"
        "        const uint32_t erpm = (uint32_t)(throttle * 120000.0f * (1.0f + 0.25f * i));\n"
        "        sitlDshotErpm[i] = erpm;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(erpm / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}",
        "static void sitlDshotTelemetryFeed(void)\n"
        "{\n"
        "    for (int i = 0; i < 4; i++) {  // the datagram block's eRPM/100 (zeros until one arrives)\n"
        "        sitlDshotErpm[i] = extErpmValid ? (uint32_t)extErpm[i] * 100U : 0U;\n"
        "        sitlMotorFrequencyHz[i] = erpmToRpm(sitlDshotErpm[i] / 100U) / 60.0f;\n"
        "    }\n"
        "    dshotTelemetryState.readCount++;\n"
        "}",
    ),
    (
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { return sitlDshotErpm[motorIndex] > 0; }",
        "bool isDshotMotorTelemetryActive(uint8_t motorIndex) { UNUSED(motorIndex); return extErpmValid; }",
    ),
])

print("add_erpm_block: edits applied")
```

- [ ] **Step 2: Write the probe**

Create `spikes/m3d/t_b2_erpm_block.py` (the M0 harness pattern; the datagram is `fdm_packet` + `rc_packet` + one `[250][8][0][8 bytes]` block):

```python
"""THROWAWAY M3d Task 2: the state datagram's eRPM block reaches Betaflight's DShot-telemetry stub.
Sends motor i at eRPM = 100_000 * (1 + i) (u16/100 = 1000 * (1 + i)) while disarmed and hovering;
MSP_MOTOR_TELEMETRY must report four distinct rpm values matching those eRPMs (pole pairs 7)."""
import os
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0"))
from ofs_spike import *  # noqa: F401,F403

POLE_PAIRS = 7.0

work = os.path.abspath("runs/tb2")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0", "feature.diff"))
proc = launch_sitl(work)
link = SitlLink()
socks = {}


def drained(port):
    s = socket.create_connection(("127.0.0.1", port))
    socks[port] = s

    def drain():
        try:
            while True:
                if not s.recv(4096):
                    return
        except OSError:
            pass
    threading.Thread(target=drain, daemon=True).start()


class ErpmFlight(Flight):
    """The state datagram gains one reserved block: [250][len lo][len hi][4 x u16 LE eRPM/100].
    Sends fdm+rc+block as ONE datagram to the state port (never a bare fdm: SITL must see exactly
    one packet per tick, and a first datagram with a block would stage bytes twice on resend — this
    flight only runs after SITL has answered)."""

    def __init__(self, link, erpm):
        super().__init__(link)
        self.erpm = erpm

    def _datagram(self):
        block = bytes([250, 8, 0]) + b"".join(struct.pack("<H", e // 100) for e in self.erpm)
        return (fdm_legacy(self.t, (0.0, 0.0, 0.0), specific_force_frd(attitude_ned()), attitude_ned())
                + rc(self.t) + block)

    def run(self, seconds, **kw):
        n = int(round(seconds / self.dt))
        for _ in range(n):
            self.t += self.dt
            self.link.tx.sendto(self._datagram(), ("127.0.0.1", PORT_STATE))
            m = self.link.recv_motors(0.5)
            if m is not None:
                self.replies += 1
        return self.last


def motor_telemetry(msp):
    d = msp.request(MSP_MOTOR_TELEMETRY)
    out = []
    for i in range(d[0]):
        off = 1 + i * 13
        rpm, inv, temp, v, a, mah = struct.unpack_from("<IHBHHH", d, off)
        out.append((rpm, inv, temp, v, a, mah))
    return out


try:
    for port in (5762, 5763):
        drained(port)
    time.sleep(0.5)
    f = ErpmFlight(link, [100_000 * (1 + i) for i in range(4)])
    f.run(2.0)
    msp = Msp(pump=f.hold)
    tel = motor_telemetry(msp)
    ok = True
    for i, row in enumerate(tel):
        expect = 100_000 * (1 + i) / POLE_PAIRS
        good = abs(row[0] - expect) < max(50.0, 0.01 * expect)
        ok &= good
        print(f"motor {i}: rpm {row[0]:6.0f} (expects ~{expect:.0f}, eRPM {100_000 * (1 + i):>7}) "
              f"invalid% {row[1] / 100:.2f} -> {'OK' if good else 'MISMATCH'}")
    print("eRPM BLOCK REACHES THE STUB:", "YES" if ok and len(tel) == 4 else "NO")
except Exception as e:
    print("FAIL:", repr(e), " poll:", proc.poll())
    with open(os.path.join(work, "sitl.log")) as fh:
        print("".join(fh.readlines()[-15:]))
    raise
finally:
    link.close()
    stop_sitl(proc)
```

Note: the probe bypasses `Flight.run`'s send (which would double-send) and sends one combined datagram per tick. If `Msp(pump=…)` trips over the custom `run` signature, pump with `lambda seconds=0.05: f.run(seconds)`.

- [ ] **Step 3: Apply, regenerate, rebuild, probe**

```bash
wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim && bash scripts/build-sitl.sh 2>&1 | tail -2 && python3 third_party/betaflight/tools/add_erpm_block.py ~/ofs/betaflight && git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch && git -C ~/ofs/betaflight reset --hard && bash scripts/build-sitl.sh 2>&1 | tail -2"
wsl.exe -d Ubuntu -e bash -lc "cd /mnt/c/dev/open-fpv-sim/spikes/m3d && rm -rf runs/tb2 && OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf OFS_SITL_CLEANUP='pkill -x betaflight_SITL' python3 t_b2_erpm_block.py 2>&1 | tail -6"
```

Expected: `add_erpm_block: edits applied`, a clean rebuild, and the probe printing four `OK` rows — eRPM 100 000/200 000/300 000/400 000 at 7 pole pairs, so `MSP_MOTOR_TELEMETRY` rpm ≈ 14 286 / 28 571 / 42 857 / 57 143 — and `eRPM BLOCK REACHES THE STUB: YES`. Re-run `t_a5` once more if anything about the serial path is in doubt (the block must not disturb the UART blocks).

- [ ] **Step 4: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add third_party/betaflight/tools/add_erpm_block.py third_party/betaflight/ofs-sitl.patch spikes/m3d/t_b2_erpm_block.py && git commit -m "feat(sitl): the simulator's per-motor eRPM rides a reserved state-datagram block

Block [250][8][4 x u16 eRPM/100] is staged with the packet and feeds the DShot-telemetry
stub each PID loop, replacing the synthetic feed.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 3: The eRPM block on the simulator side (codec, bridge, vehicle)

**Files:**
- Modify: `crates/ofs-fc/src/sitl/codec.rs`
- Modify: `crates/ofs-fc/src/sitl/bridge.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Test: `crates/ofs-fc/tests/sitl_codec.rs`

**Interfaces:**
- Consumes: Task 2's SITL block format; `names::motor_omega(i)` (`Signal<f64>`, rad/s); `ofs_fc::sitl::esc_telemetry::POLE_PAIRS = 7.0`.
- Produces: `codec::EXT_ERPM_BLOCK_ID: u8 = 250`, `codec::erpm_block_payload(motor_omega_radps: &[f64]) -> [u8; 8]`; `BridgeConfig.erpm: Vec<Signal<f64>>` (Task 5's vehicle wiring passes the motor-omega signals; every datagram after the first gains the block).

- [ ] **Step 1: Write the failing codec tests**

In `crates/ofs-fc/tests/sitl_codec.rs`, append (the file's existing imports cover `codec::{…}` items — extend the `use` at the top of the new module instead of touching existing ones; the file's tests live in one flat module, so add a `use` only if missing):

```rust
mod erpm_block {
    use ofs_fc::sitl::codec::{erpm_block_payload, state_datagram_with_serial, EXT_ERPM_BLOCK_ID,
                              FdmPacket, RcPacket, SERIAL_SECTION_MAX, STATE_DATAGRAM_SIZE};

    #[test]
    fn omega_becomes_erpm_over_one_hundred_little_endian() {
        // 1000 mechanical rpm = 104.719755 rad/s; x 7 pole pairs = 7000 eRPM; /100 = 70.
        let bytes = erpm_block_payload(&[104.71975511965977, 0.0, -1.0, 1.0e9]);
        assert_eq!(bytes, [70, 0, 0, 0, 0, 0, 255, 255], "motor 0 = 70; rest clamp and floor");
    }

    #[test]
    fn fewer_than_four_motors_pad_with_zeros() {
        assert_eq!(erpm_block_payload(&[]), [0u8; 8]);
        assert_eq!(erpm_block_payload(&[104.71975511965977]), [70, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn the_erpm_block_rides_the_datagram_like_a_serial_block() {
        let fdm = FdmPacket { timestamp_s: 1.0, gyro_rpy_radps: [0.0; 3], accel_xyz_mps2: [0.0; 3],
                              quat_wxyz: [1.0, 0.0, 0.0, 0.0], velocity_xyz_mps: [0.0; 3],
                              position_xyz: [0.0; 3], pressure_pa: 0.0 };
        let rc = RcPacket { timestamp_s: 1.0, channels: [0; 16] };
        let datagram = state_datagram_with_serial(&fdm, &rc, &[(EXT_ERPM_BLOCK_ID, erpm_block_payload(&[0.0; 4]).to_vec())]);
        assert_eq!(datagram.len(), STATE_DATAGRAM_SIZE + 3 + 8);
        let block = &datagram[STATE_DATAGRAM_SIZE..];
        assert_eq!((block[0], u16::from_le_bytes([block[1], block[2]])), (250, 8));
        // The whole serial section, eRPM block included, stays within SITL's EXT_SERIAL_MAX.
        let full = state_datagram_with_serial(&fdm, &rc, &[(1, vec![0xAA; 400]), (EXT_ERPM_BLOCK_ID, erpm_block_payload(&[0.0; 4]).to_vec())]);
        assert!(full.len() - STATE_DATAGRAM_SIZE <= SERIAL_SECTION_MAX);
    }
}
```

Check first how `sitl_codec.rs` is organised (`mod` blocks vs flat tests) and match it; if it is flat, drop the `mod erpm_block { … }` wrapper and merge the `use` into the file's existing one.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-fc --test sitl_codec --locked`
Expected: FAIL to compile — `erpm_block_payload` and `EXT_ERPM_BLOCK_ID` not found.

- [ ] **Step 3: Implement the codec**

In `crates/ofs-fc/src/sitl/codec.rs`, after `state_datagram_with_serial` (line 88), add:

```rust
/// The reserved block id of the per-motor eRPM block in the state datagram, above any real UART index
/// (SITL intercepts it in `extInjectSerial`; see third_party/betaflight/tools/add_erpm_block.py).
pub const EXT_ERPM_BLOCK_ID: u8 = 250;

/// The eRPM block's payload: motor 0..3's eRPM/100 as u16 little-endian, saturating (u16 covers
/// 6.55 M eRPM). eRPM = mechanical rpm x pole pairs (the ESC-sensor shim's constant: the same
/// convention as Betaflight's `motor_poles` default of 14 on the shipped quad).
pub fn erpm_block_payload(motor_omega_radps: &[f64]) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, omega) in motor_omega_radps.iter().take(4).enumerate() {
        let erpm = omega * 60.0 / (2.0 * std::f64::consts::PI) * crate::sitl::esc_telemetry::POLE_PAIRS / 100.0;
        let value = erpm.round().clamp(0.0, 65535.0) as u16;  // round: the f64 chain is 69.999... for 1000 rpm
        out[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    out
}
```

Check `crates/ofs-fc/src/sitl/mod.rs` exposes `esc_telemetry` publicly (the vehicle already imports `ofs_fc::sitl::esc_telemetry::EscTelemetry`, so it does).

- [ ] **Step 4: Run the codec tests**

Run: `cargo test -p ofs-fc --test sitl_codec --locked`
Expected: PASS (all, including the pre-existing tests).

- [ ] **Step 5: The bridge composes the block**

In `crates/ofs-fc/src/sitl/bridge.rs`:

1. Extend the imports: the `use super::codec::{ … }` list gains `EXT_ERPM_BLOCK_ID, erpm_block_payload` (alphabetical order in that list).
2. In `BridgeConfig`, after `pub serial: Vec<SerialLink>,` add:

```rust
    /// The motor-omega signals for the per-motor eRPM block (rad/s; empty for an open-loop quad —
    /// but the bridge only runs under SITL). One value per motor, at most 4.
    pub erpm: Vec<Signal<f64>>,
```

3. In `SitlBridge::start`, copy the signals out before `cfg` moves into `Self { cfg, … }`: immediately before the `Ok(Self {` literal add

```rust
        let erpm = cfg.erpm.clone();
```

and into the literal, after `cfg,`, add `erpm,`.

4. In `Model for SitlBridge::step`, replace:

```rust
        let serial = if self.answered { take_serial_blocks(&self.cfg.serial) } else { Vec::new() };
        let datagram = state_datagram_with_serial(&fdm, &rc, &serial);
```

with:

```rust
        let mut serial = if self.answered { take_serial_blocks(&self.cfg.serial) } else { Vec::new() };
        // The per-motor eRPM block, after the UART blocks (a first, resendable datagram carries none).
        if self.answered {
            let omega: Vec<f64> = self.erpm.iter().map(|s| bus.get(*s)).collect();
            serial.push((EXT_ERPM_BLOCK_ID, erpm_block_payload(&omega).to_vec()));
        }
        let datagram = state_datagram_with_serial(&fdm, &rc, &serial);
```

- [ ] **Step 6: The vehicle passes the motor-omega signals**

In `crates/ofs-sim/src/vehicle.rs`, in `build`'s Sitl arm, into the `BridgeConfig { … }` literal after `serial,` add:

```rust
                    erpm: (0..n).map(|i| bus.signal(&names::motor_omega(i))).collect(),
```

(`bus` is `&mut Bus`; `bus.signal` registers-or-returns the signal — the same call `Handles::register` makes later, which is fine: `Signal` handles are interchangeable.)

- [ ] **Step 7: Run the workspace Rust tests**

Run: `cargo test --workspace --locked`
Expected: PASS. (`ofs-sim`'s open-loop tests never construct a bridge; the fake-SITL tests in `ofs-fc` construct `BridgeConfig` — the new field breaks their literals, so update every construction site the compiler names, passing `erpm: Vec::new()` except where the test wants a block.)

- [ ] **Step 8: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add crates/ofs-fc/src/sitl/codec.rs crates/ofs-fc/src/sitl/bridge.rs crates/ofs-sim/src/vehicle.rs crates/ofs-fc/tests/sitl_codec.rs && git commit -m "feat(fc): the bridge sends the per-motor eRPM block in every state datagram

erpm_block_payload converts motor omega to eRPM/100 (saturating u16); the block rides
after the UART blocks, never in a resendable first datagram.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 4: The ELRS downlink telemetry scheduler (`ofs-radio`)

**Files:**
- Modify: `crates/ofs-radio/src/crsf.rs`
- Modify: `crates/ofs-radio/src/elrs.rs`
- Test: `crates/ofs-radio/tests/elrs.rs`

**Interfaces:**
- Consumes: `crsf::Decoder` (`push(&[u8]) -> Vec<Frame>`, `Frame::Other { frame_type, payload, .. }`); M3c's per-packet `downlink_received`.
- Produces: `crsf::FRAMETYPE_BATTERY_SENSOR/ATTITUDE/FLIGHT_MODE` (u8 consts); `elrs::FcTelemetry` (Debug, Clone, PartialEq, Default: `battery_v/battery_a/battery_mah/roll_deg/pitch_deg/yaw_deg: f64`, `flight_mode: String`, `last_frame_s: f64`, `frame_count: u64`); `elrs::FcTelemetryHandle` (`new()`, `latest() -> FcTelemetry`, `store(FcTelemetry)`); `ElrsLink::new(params, div, seed, uart, telemetry_in: Wire, bus)` and `ElrsLink::fc_telemetry_handle() -> FcTelemetryHandle`. Tasks 5–7 consume these.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-radio/tests/elrs.rs`: change the `rig` helper to build the link with a telemetry ingress wire and return both the wire (the tests feed frames into it) and the handle (the tests assert on it):

```rust
fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire, Wire, ElrsHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let div = BASE_HZ / params.packet_rate_hz;
    let link = ElrsLink::new(params, div, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = link.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(link));
    (s, uart, telemetry_in, handle)
}

type ElrsHandle = ofs_radio::elrs::FcTelemetryHandle;
```

Update every existing `rig(...)` call site (`let (mut s, uart) = rig(...)` → `let (mut s, uart, _telemetry_in, _fc) = rig(...)`; several tests destructure two names), and add the new tests:

```rust
mod fc_telemetry {
    use super::*;
    use ofs_radio::crsf::{FRAMETYPE_ATTITUDE, FRAMETYPE_BATTERY_SENSOR, FRAMETYPE_FLIGHT_MODE};
    use ofs_radio::elrs::FcTelemetry;

    /// One CRC-valid CRSF frame: [address 0xC8][len][type][payload][crc8 poly 0xD5].
    fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0xC8, (payload.len() + 2) as u8, frame_type];
        f.extend_from_slice(payload);
        f.push(ofs_radio::crsf::crc8_dvb_s2(&f[2..]));
        f
    }

    fn battery_frame(voltage_v: f64, current_a: f64, mah: u32, index: u8) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&((voltage_v * 10.0) as u16).to_be_bytes()); // 0.1 V units
        p.extend_from_slice(&((current_a * 10.0) as u16).to_be_bytes()); // 0.1 A units
        p.extend_from_slice(&[0, 0, index]); // u24 mAh: the index rides in the low byte
        p.push(100);
        frame(FRAMETYPE_BATTERY_SENSOR, &p)
    }

    fn attitude_frame(pitch_deg: f64, roll_deg: f64, yaw_deg: f64) -> Vec<u8> {
        let rad = |d: f64| (d.to_radians() * 10_000.0).round() as i16;
        let mut p = Vec::new();
        for v in [rad(pitch_deg), rad(roll_deg), rad(yaw_deg)] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        frame(FRAMETYPE_ATTITUDE, &p)
    }

    #[test]
    fn a_frame_flies_every_fourth_downlink_packet_and_updates_the_handle() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 7));
        packets(&mut s, 3);
        assert_eq!(fc.latest().frame_count, 0, "no slot before the 4th packet");
        packets(&mut s, 1);
        let t = fc.latest();
        assert_eq!(t.frame_count, 1);
        assert!((t.battery_v - 24.6).abs() < 1e-9, "{:?}", t);
        assert!((t.battery_a - 3.2).abs() < 1e-9, "{:?}", t);
        assert_eq!(t.battery_mah, 7.0, "the u24's low byte rode through: {:?}", t);
        packets(&mut s, 4);
        assert_eq!(fc.latest().frame_count, 1, "the FIFO is empty: later slots fly idle");
    }

    #[test]
    fn attitude_and_flight_mode_decode() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&attitude_frame(-4.5, 12.3, 90.0));
        telemetry_in.write(&frame(FRAMETYPE_FLIGHT_MODE, b"ACRO\0"));
        packets(&mut s, 8);
        let t = fc.latest();
        assert!((t.pitch_deg + 4.5).abs() < 0.01 && (t.roll_deg - 12.3).abs() < 0.01, "{:?}", t);
        assert!((t.yaw_deg - 90.0).abs() < 0.01, "yaw wrapped to ±180 by Betaflight decodes back: {:?}", t);
        assert_eq!(t.flight_mode, "ACRO");
        assert_eq!(t.frame_count, 2);
    }

    #[test]
    fn a_full_fifo_drops_the_oldest_frame() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        for i in 0..20u32 {
            telemetry_in.write(&battery_frame(20.0 + f64::from(i), 0.0, 0, i as u8));
        }
        packets(&mut s, 4); // the first slot: 16 kept of 20 -> the 5th offered frame (index 4) flies
        let t = fc.latest();
        assert_eq!(t.frame_count, 1);
        assert!((t.battery_v - 24.0).abs() < 1e-9, "oldest dropped: {:?}", t);
    }

    #[test]
    fn no_downlink_packets_means_no_telemetry() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        s.run_for(0.1).unwrap();
        let after_connected = fc.latest().frame_count;
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
        set(&mut s, names::RADIO_TX_ENABLED, 0.0);
        s.run_for(0.25).unwrap(); // 125 packets of silence drain the 100-packet LQ window
        assert_eq!(fc.latest().frame_count, after_connected, "the receiver is silent: no slots");
        assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    }

    #[test]
    fn corrupt_bytes_are_skipped_and_the_stream_stays_aligned() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&[0xDE, 0xAD, 0xC8, 0xFF]);
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
        packets(&mut s, 4);
        assert_eq!(fc.latest().frame_count, 1, "the garbage never reached the handle");
    }

    #[test]
    fn the_telemetry_path_draws_no_numbers() {
        // The downlink byte stream is identical whether or not frames were offered: the scheduler
        // consumes no randomness.
        let run = |offer: bool| {
            let (mut s, uart, telemetry_in, _fc) = rig(
                LinkParams { fading: true, ground_bounce: true, ..LinkParams::ideal(500) },
                5,
            );
            if offer {
                telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
            }
            set_pos(&mut s, DVec3::new(30.0, 0.0, -1.7));
            s.run_for(0.2).unwrap();
            uart.take(usize::MAX)
        };
        assert_eq!(run(true), run(false), "the uplink bytes must not depend on the telemetry offered");
    }

    #[test]
    fn an_open_link_reports_no_telemetry_until_frames_arrive() {
        let (mut s, _uart, _telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        s.run_for(0.05).unwrap();
        assert_eq!(fc.latest(), FcTelemetry::default(), "frame_count 0, everything zero, no mode");
    }
}
```

Also update every pre-existing `rig` call site (`let (mut s, uart) = rig(...)` → `let (mut s, uart, _telemetry_in, _fc) = rig(...)`; several tests destructure two names).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-radio --test elrs --locked`
Expected: FAIL to compile — `ElrsLink::new` takes 5 arguments, `FcTelemetryHandle`/`FRAMETYPE_*` not found.

- [ ] **Step 3: Implement**

1. In `crates/ofs-radio/src/crsf.rs`, next to the existing `FRAMETYPE_*` consts (line 8–12), add:

```rust
pub const FRAMETYPE_BATTERY_SENSOR: u8 = 0x08;
pub const FRAMETYPE_ATTITUDE: u8 = 0x1E;
pub const FRAMETYPE_FLIGHT_MODE: u8 = 0x21;
```

2. In `crates/ofs-radio/src/elrs.rs`:

a. Extend the `use` of `crate::crsf` to `use crate::crsf::{self, Decoder, Frame, LinkStatistics, CHANNEL_COUNT};` and add `use std::sync::{Arc, Mutex};` at the top (alphabetical with the existing `std::collections::VecDeque`).

b. After the existing consts, add:

```rust
/// Every this-many downlink packets carries one telemetry frame (an ExpressLRS telemetry-ratio
/// estimate: 125 slots/s at the 500 Hz packet rate).
pub const TELEMETRY_SLOT_DIVISOR: u32 = 4;
/// How many offered telemetry frames wait for a slot at most; the oldest is dropped when full
/// (stale data yields to fresh, like a radio that cannot retransmit).
pub const TELEMETRY_FIFO_LEN: usize = 16;
```

c. After the `LinkParams` impl block, add the handle and the decoded telemetry:

```rust
/// What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink
/// slot. Units are Betaflight's own CRSF encodings (see docs/research/elrs-link.md): the battery
/// voltage in 0.1 V and current in 0.1 A (`getLegacyBatteryVoltage()`/`getAmperage()/10`), attitude
/// in radians x 10 000 with yaw wrapped to ±180°, the flight mode verbatim.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FcTelemetry {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    /// Simulated time of the last frame that flew a slot (0 until one arrives).
    pub last_frame_s: f64,
    /// Frames that flew a slot so far (a monotonic "anything arrived yet" marker).
    pub frame_count: u64,
}

/// The latest [`FcTelemetry`], shared with the server's state snapshot (the `OsdHandle` pattern).
#[derive(Debug, Clone, Default)]
pub struct FcTelemetryHandle(Arc<Mutex<FcTelemetry>>);

impl FcTelemetryHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> FcTelemetry {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn store(&self, t: FcTelemetry) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
}
```

d. Extend `ElrsLink`'s struct and constructor. New fields after `last_pos: Option<DVec3>,`:

```rust
    telemetry_in: Wire,
    decoder: Decoder,
    telemetry_fifo: VecDeque<(u8, Vec<u8>)>,
    downlink_packets: u32,
    fc_telemetry: FcTelemetryHandle,
```

`new` gains the `telemetry_in: Wire` parameter between `uart: Wire` and `bus`, and initialises `telemetry_in`, `decoder: Decoder::default()`, `telemetry_fifo: VecDeque::new()`, `downlink_packets: 0`, `fc_telemetry: FcTelemetryHandle::new()`. Add the accessor:

```rust
    /// The handset's view of Betaflight's telemetry, updated as frames fly downlink slots.
    pub fn fc_telemetry_handle(&self) -> FcTelemetryHandle {
        self.fc_telemetry.clone()
    }
```

e. In `Model for ElrsLink`, rename the step parameter `_ctx` to `ctx` (the slot timestamp needs it) and, after the downlink block that ends with `let downlink_received = transmitting && …` (line 304), add:

```rust
        // Telemetry ingress: whatever Betaflight wrote this tick joins the FIFO (oldest dropped when
        // full). No new random draws: the queue is a pure function of the frames offered and the
        // slots flown.
        for frame in self.decoder.push(&self.telemetry_in.take(usize::MAX)) {
            if let Frame::Other { frame_type, payload, .. } = frame {
                if self.telemetry_fifo.len() >= TELEMETRY_FIFO_LEN {
                    self.telemetry_fifo.pop_front();
                }
                self.telemetry_fifo.push_back((frame_type, payload));
            }
        }
        if transmitting {
            self.downlink_packets += 1;
            if downlink_received && self.downlink_packets % TELEMETRY_SLOT_DIVISOR == 0 {
                if let Some((frame_type, payload)) = self.telemetry_fifo.pop_front() {
                    self.apply_telemetry_frame(frame_type, &payload, ctx.time_s);
                }
            }
        }
```

f. In the `impl ElrsLink` block (next to `sample`/`lq_of`), the frame decoder:

```rust
    /// Decodes one telemetry frame that flew a downlink slot into the shared handle. Frame types we
    /// do not expose still consume their slot (the radio forwarded them; nothing more is modelled).
    fn apply_telemetry_frame(&mut self, frame_type: u8, payload: &[u8], now_s: f64) {
        let mut t = self.fc_telemetry.latest();
        match frame_type {
            crsf::FRAMETYPE_BATTERY_SENSOR if payload.len() >= 7 => {
                t.battery_v = f64::from(u16::from_be_bytes([payload[0], payload[1]])) / 10.0;
                t.battery_a = f64::from(u16::from_be_bytes([payload[2], payload[3]])) / 10.0;
                t.battery_mah = f64::from(u32::from_be_bytes([0, payload[4], payload[5], payload[6]]));
            }
            crsf::FRAMETYPE_ATTITUDE if payload.len() >= 6 => {
                let deg = |raw: [u8; 2]| f64::from(i16::from_be_bytes(raw)) * 180.0 / (10_000.0 * std::f64::consts::PI);
                t.pitch_deg = deg([payload[0], payload[1]]);
                t.roll_deg = deg([payload[2], payload[3]]);
                t.yaw_deg = deg([payload[4], payload[5]]);
            }
            crsf::FRAMETYPE_FLIGHT_MODE => {
                t.flight_mode = String::from_utf8_lossy(payload).split('\0').next().unwrap_or("").to_owned();
            }
            _ => {}
        }
        t.frame_count += 1;
        t.last_frame_s = now_s;
        self.fc_telemetry.store(t);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-radio --locked`
Expected: PASS — the new scheduler tests and every M3c ELRS test (the byte-stream tests prove the telemetry path draws no numbers).

- [ ] **Step 5: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add crates/ofs-radio/src/crsf.rs crates/ofs-radio/src/elrs.rs crates/ofs-radio/tests/elrs.rs && git commit -m "feat(radio): the ELRS downlink carries Betaflight's CRSF telemetry on every 4th packet

A bounded drop-oldest FIFO holds the frames offered on the receiver UART; slots fly only
while the link transmits, so a down link stops telemetry. Decoded battery/attitude/flight
mode publish through an FcTelemetryHandle (the OsdHandle pattern). No new draws.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 5: Vehicle wiring — the radio tap and `VehicleState.fc_telemetry`

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs`

**Interfaces:**
- Consumes: Task 4's `ElrsLink::new(…, telemetry_in, bus)` / `fc_telemetry_handle()`; `SerialTap { uart_index, tx }`.
- Produces: `VehicleState.fc_telemetry: FcTelemetrySnapshot` — `battery_v, battery_a, battery_mah, roll_deg, pitch_deg, yaw_deg: f64`, `flight_mode: String`, `frame_count: u64`, `age_s: f64` (INFINITY until the first frame). Task 6's server fills protocol 6 from it.

- [ ] **Step 1: Wire the tap and the handle**

In `crates/ofs-sim/src/vehicle.rs`:

1. Extend the elrs import: `use ofs_radio::elrs::{ElrsLink, FcTelemetry, FcTelemetryHandle, LinkParams};`.
2. In `build`, replace (lines 585–587):

```rust
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    models.push(Box::new(ElrsLink::new(link, base_hz / r.packet_rate_hz, opts.seed, receiver_uart.clone(), &mut bus)));
```

with:

```rust
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    let radio_tap = Wire::new(VIDEO_TAP_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    let elrs = ElrsLink::new(link, base_hz / r.packet_rate_hz, opts.seed, receiver_uart.clone(), radio_tap.clone(), &mut bus);
    let fc_telemetry = elrs.fc_telemetry_handle();
    models.push(Box::new(elrs));
```

3. In the Sitl arm, replace `let mut serial = vec![SerialLink { uart_index: r.uart - 1, rx: receiver_uart }];` and the `taps: video.taps,` line:

```rust
            let mut serial = vec![SerialLink { uart_index: r.uart - 1, rx: receiver_uart }];
            serial.extend(video.serial);
```

stays, and `taps: video.taps,` becomes:

```rust
                    taps: {
                        let mut taps = vec![SerialTap { uart_index: r.uart - 1, tx: radio_tap.clone() }];
                        taps.extend(video.taps);
                        taps
                    },
```

(`ElrsLink::new` was passed `radio_tap.clone()`, so the original is still alive here for the tap's `tx` end — the Wire is a shared handle: the link reads from the same FIFO the tap receives into.)

4. Store the handle on `Vehicle`: the struct gains `fc_telemetry: FcTelemetryHandle,` after `osd: Option<OsdHandle>,`; the construction at the end of `build` gains `fc_telemetry,` — note `Vehicle { … }` is built after `let h = Handles::register(...)`, so `fc_telemetry` (the local from step 2) is in scope.

- [ ] **Step 2: The state snapshot**

In `crates/ofs-sim/src/vehicle.rs`:

1. After the `RadioState` impl block (line ~72), add:

```rust
/// Betaflight's telemetry as the handset received it over the ELRS downlink (`age_s` = INFINITY until
/// the first frame arrives; always INFINITY without SITL).
#[derive(Debug, Clone, PartialEq)]
pub struct FcTelemetrySnapshot {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    pub frame_count: u64,
    pub age_s: f64,
}

impl FcTelemetrySnapshot {
    fn new(t: FcTelemetry, now_s: f64) -> Self {
        let age_s = if t.frame_count == 0 { f64::INFINITY } else { now_s - t.last_frame_s };
        Self {
            battery_v: t.battery_v,
            battery_a: t.battery_a,
            battery_mah: t.battery_mah,
            roll_deg: t.roll_deg,
            pitch_deg: t.pitch_deg,
            yaw_deg: t.yaw_deg,
            flight_mode: t.flight_mode,
            frame_count: t.frame_count,
            age_s,
        }
    }
}
```

2. `VehicleState` gains `pub fc_telemetry: FcTelemetrySnapshot,` after `pub radio: RadioState,`.
3. In `Vehicle::state()`, after the `radio: { … }` block add:

```rust
            fc_telemetry: FcTelemetrySnapshot::new(self.fc_telemetry.latest(), self.scheduler.time_s()),
```

- [ ] **Step 3: Unit tests**

In `crates/ofs-sim/src/vehicle.rs`'s `mod tests`, add (the existing `open_loop`/`shipped_quad` helpers apply):

```rust
    #[test]
    fn an_open_loop_quad_reports_no_fc_telemetry() {
        let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
        vehicle.run_for(0.1).unwrap();
        let fc = vehicle.state().fc_telemetry;
        assert_eq!(fc.frame_count, 0);
        assert_eq!(fc.age_s, f64::INFINITY);
        assert!(fc.flight_mode.is_empty());
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --locked`
Expected: PASS (the new test plus the existing vehicle tests).

- [ ] **Step 5: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add crates/ofs-sim/src/vehicle.rs && git commit -m "feat(sim): Betaflight's UART2 telemetry reaches the ELRS model; the state carries fc_telemetry

The radio's UART gains a SerialTap; the vehicle snapshots the FcTelemetryHandle with an
age that is INFINITY until the first frame (always INFINITY open loop).

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 6: Protocol 6, the server, Python, and the live SITL tests

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`
- Modify: `crates/ofs-sim/src/session.rs`
- Regenerate: `python/ofs/v1/sim_pb2.py`, `python/ofs/v1/sim_pb2.pyi`, `python/ofs/v1/sim_pb2_grpc.py`
- Modify: `python/ofs/client.py`
- Create: `python/tests/test_sitl_telemetry.py`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Task 5's `FcTelemetrySnapshot`.
- Produces: `PROTOCOL_VERSION = 6`; `State.fc_telemetry` (`pb::FcTelemetry`); Python `state.fc_telemetry.*`. Task 7 consumes the proto.

- [ ] **Step 1: The proto**

In `proto/ofs/v1/sim.proto`:

1. Replace the comment `// Simulator control, protocol version 5: protocol 4 plus the radio link's SNR, active antenna and downlink,` — read the file's full comment line first; it continues onto the next line(s). Replace the `version 5` wording with `version 6: protocol 5 plus the flight controller's telemetry over the ELRS downlink,` keeping the rest of the comment.
2. After the `RadioLink` message (line 113), add:

```proto
// Betaflight's own telemetry as the handset received it over the ELRS downlink. age is INFINITY
// until the first frame arrives (and always INFINITY without SITL): the HUD dashes.
message FcTelemetry {
  double battery_v = 1;    // Betaflight's filtered pack voltage
  double battery_a = 2;
  double battery_mah = 3;  // drawn so far (Betaflight's ESC-sensor value)
  double roll_deg = 4;     // Betaflight's attitude estimate, not the simulator's truth
  double pitch_deg = 5;
  double yaw_deg = 6;
  string flight_mode = 7;  // Betaflight's mode string ("ACRO", "AIR *", "!FS!", ...)
  uint64 frame_count = 8;
  double telemetry_age_s = 9;
}
```

3. In `State`, after `double collision_speed_mps = 17;` add `FcTelemetry fc_telemetry = 18;`.

- [ ] **Step 2: The Rust protocol version, test, and server**

1. `crates/ofs-proto/src/lib.rs`: `pub const PROTOCOL_VERSION: u32 = 5;` → `6`, and the doc comment above it stays.
2. `crates/ofs-proto/tests/messages.rs`: `assert_eq!(PROTOCOL_VERSION, 5);` → `6`.
3. `crates/ofs-sim/src/session.rs`, in `state_msg` after `collision_speed_mps: s.collision_speed_mps,` add:

```rust
            fc_telemetry: Some(pb::FcTelemetry {
                battery_v: s.fc_telemetry.battery_v,
                battery_a: s.fc_telemetry.battery_a,
                battery_mah: s.fc_telemetry.battery_mah,
                roll_deg: s.fc_telemetry.roll_deg,
                pitch_deg: s.fc_telemetry.pitch_deg,
                yaw_deg: s.fc_telemetry.yaw_deg,
                flight_mode: s.fc_telemetry.flight_mode.clone(),
                frame_count: s.fc_telemetry.frame_count,
                telemetry_age_s: s.fc_telemetry.age_s,
            }),
```

- [ ] **Step 3: Regenerate the Python stubs**

Run (from the repo root, Windows Python is fine):

```bash
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
```

Also update the "protocol version 5" docstrings in `python/ofs/v1/sim_pb2_grpc.py` to "protocol version 6" (three occurrences, plain text replace) — or leave them; they are regenerated text. Leave them: the generator owns the file, and the next regeneration rewrites them; instead update the proto's service comment (step 1) so future regenerations carry the right wording.

- [ ] **Step 4: The Python client**

In `python/ofs/client.py`:

1. `PROTOCOL_VERSION = 5` → `PROTOCOL_VERSION = 6` (wherever the module defines it — the Rust test greps this exact line).
2. After the `RadioLink` dataclass, add:

```python
@dataclass(frozen=True)
class FcTelemetry:
    """Betaflight's own telemetry as the handset received it over the ELRS downlink.

    telemetry_age_s is INFINITY until the first frame arrives (and always INFINITY without SITL).
    """
    battery_v: float = 0.0
    battery_a: float = 0.0
    battery_mah: float = 0.0
    roll_deg: float = 0.0
    pitch_deg: float = 0.0
    yaw_deg: float = 0.0
    flight_mode: str = ""
    frame_count: int = 0
    telemetry_age_s: float = float("inf")
```

3. In the `State` dataclass, after `radio: RadioLink = field(default_factory=RadioLink)` add `fc_telemetry: FcTelemetry = field(default_factory=FcTelemetry)`.
4. In `_state(m)`, after the `radio=RadioLink(...)` argument add:

```python
        fc_telemetry=FcTelemetry(m.fc_telemetry.battery_v, m.fc_telemetry.battery_a, m.fc_telemetry.battery_mah,
                                 m.fc_telemetry.roll_deg, m.fc_telemetry.pitch_deg, m.fc_telemetry.yaw_deg,
                                 m.fc_telemetry.flight_mode, m.fc_telemetry.frame_count,
                                 m.fc_telemetry.telemetry_age_s),
```

- [ ] **Step 5: Unit checks**

Run: `cargo test -p ofs-proto --locked` and `cd "C:\dev\open-fpv-sim" && python -m pytest python/tests/test_client.py -q`
Expected: PASS (the version test now expects 6 and finds the Python client at 6; `test_client.py` exercises `_state` against the regenerated pb2 — if it builds sample states from raw dicts it may need the new field; follow the compiler/test failures, passing `fc_telemetry=FcTelemetry()` defaults).

Also `cargo build --workspace --locked` — `session.rs` compiles against the new pb::FcTelemetry.

- [ ] **Step 6: The live SITL tests**

Create `python/tests/test_sitl_telemetry.py`:

```python
"""The FC telemetry chain end to end against real Betaflight (M3d): per-motor eRPM reaches the RPM
filter's source (MSP_MOTOR_TELEMETRY shows four distinct values matching the sim's motors), the ELRS
downlink carries Betaflight's telemetry to the client (battery tracks the sim's, a mode string
arrives), and a radio cut ages the telemetry out while the sim's own battery lives on. Needs
OFS_SITL_LAUNCH."""
import os
import socket
import struct

import pytest

from conftest import QUAD
from ofs.faults import RadioLinkLoss

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM = (1.0, 1.0, -1.0, -1.0)
POLE_PAIRS = 7.0
MSP_MOTOR_TELEMETRY = 139
MSP_RC = 105


def _msp_requester(sim):
    """Minimal MSP v1 client over the Configurator TCP port, pumped by sim time."""
    address = sim.configurator_address  # an attribute set by load(): "tcp://127.0.0.1:5761"
    port = int(address.rsplit(":", 1)[1])
    s = socket.create_connection(("127.0.0.1", port), timeout=2.0)
    s.settimeout(2.0)

    def request(cmd):
        ck = cmd
        s.sendall(b"$M<" + bytes([0, cmd]) + bytes([ck]))
        while True:
            sim.run(0.02)  # MSP is serviced only while simulated time advances
            b = s.recv(4096)
            if b"$M>" in b:
                i = b.index(b"$M>")
                n, got = b[i + 3], b[i + 4]
                payload = b[i + 5:i + 5 + n]
                if got == cmd:
                    return payload

    return request


def test_per_motor_erpm_reaches_betaflight(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)
    sim.set_sticks(throttle=0.0, aux=ARM)
    sim.run(1.0)  # armed, idle
    sim.set_sticks(throttle=0.5, aux=ARM)
    s = sim.run(1.0)  # ~half throttle: motors spin up
    request = _msp_requester(sim)
    rc = request(MSP_RC)  # MSP order: roll, pitch, yaw, throttle
    assert struct.unpack_from("<4H", rc, 0) == (1500, 1500, 1500, 1500), \
        "telemetry did not disturb RC: the sticks read back as sent"
    d = request(MSP_MOTOR_TELEMETRY)
    rpm = [struct.unpack_from("<I", d, 1 + i * 13)[0] for i in range(d[0])]
    assert len(rpm) == 4 and len(set(rpm)) > 1, f"expected distinct per-motor RPM, got {rpm}"
    sim_rpm = s.motor_rpm
    for bf, ours in zip(rpm, sim_rpm):
        # MSP_MOTOR_TELEMETRY's value is already mechanical rpm (the patch's getDshotRpm
        # divides eRPM by the 7 pole pairs), so the comparison is like-for-like.
        assert abs(bf - ours) <= max(500.0, 0.05 * ours), \
            f"Betaflight reports {bf} rpm, the sim models {ours} rpm: {rpm} vs {sim_rpm}"

def test_battery_and_mode_arrive_over_the_downlink(sim):
    sim.load(QUAD, seed=1)
    s = sim.run(6.0)
    assert s.fc_telemetry.frame_count > 0, "no telemetry frame flew a downlink slot"
    assert abs(s.fc_telemetry.battery_v - s.battery_voltage_v) <= 0.5, \
        f"Betaflight {s.fc_telemetry.battery_v} V vs the sim {s.battery_voltage_v} V"
    assert s.fc_telemetry.flight_mode != "", s.fc_telemetry
    assert s.fc_telemetry.telemetry_age_s < 1.0, s.fc_telemetry


def test_a_radio_cut_ages_the_telemetry_out(sim):
    sim.load(QUAD, seed=1)
    sim.run(6.0)
    assert sim.run(0.1).fc_telemetry.frame_count > 0
    sim.inject(RadioLinkLoss())
    s = sim.run(2.0)
    assert not s.radio.link_up
    assert s.fc_telemetry.telemetry_age_s > 1.0, "the telemetry went stale with the link"
    assert s.battery_voltage_v > 0.0, "the simulator's own battery model lives on"
    sim.clear_faults()
```

The API above is the real one (`python/ofs/client.py`): `sim.configurator_address` is an attribute set by `load`, `set_sticks(roll, pitch, yaw, throttle, aux)` takes kwargs, `run(seconds)` returns the newest `State`, and faults go through `sim.inject(RadioLinkLoss())` / `sim.clear_faults()`. MSP framing note: the `request` loop above resynchronises on `$M>`; if the Configurator port is busy (another client), the test fails with a connect error — close other sessions first.

- [ ] **Step 7: Run the live tests (WSL recipe)**

```bash
wsl_run() { wsl.exe -d Ubuntu -e bash -lc "cd /mnt$(pwd) && export CARGO_TARGET_DIR=\$HOME/ofs/target OFS_SITL_LAUNCH=\$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf && $*"; }
wsl_run cargo build -p ofs-sim --locked
wsl_run python -m pytest python/tests/test_sitl_telemetry.py -v
```

Expected: 3 passed. (The MSP helper may need one iteration on framing; the `recv` loop above resynchronises on `$M>`.)

- [ ] **Step 8: CI picks the test up**

In `.github/workflows/ci.yml`, replace:

```yaml
      - run: python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py python/tests/test_sitl_video.py -v
```

with:

```yaml
      - run: python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py python/tests/test_sitl_video.py python/tests/test_sitl_telemetry.py -v
```

(The `sitl-` cache key hashes `third_party/betaflight/**`, so the patch change rebuilds SITL in CI automatically.)

- [ ] **Step 9: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add proto/ofs/v1/sim.proto crates/ofs-proto crates/ofs-sim/src/session.rs python/ofs python/tests/test_sitl_telemetry.py .github/workflows/ci.yml && git commit -m "feat(proto)!: protocol 6 - the flight controller's telemetry over the ELRS downlink

State.fc_telemetry carries Betaflight's battery, attitude estimate and flight mode with a
staleness age; live tests prove per-motor eRPM in MSP_MOTOR_TELEMETRY, the battery tracking
the sim's within 0.5 V, and the telemetry going stale on a radio cut.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 7: The clients — ofs-client, ofs-godot, and the HUD

**Files:**
- Modify: `crates/ofs-client/src/model.rs`
- Modify: `crates/ofs-godot/src/lib.rs`
- Modify: `godot/ui/hud.gd`
- Test: `godot/tests/test_hud.gd`
- Test: `godot/tests/e2e_betaflight.gd`

**Interfaces:**
- Consumes: protocol 6's `pb::FcTelemetry`.
- Produces: `Telemetry.fc_telemetry: FcTelemetry` (ofs-client); the Godot telemetry dict keys `fc_battery_v`, `fc_battery_a`, `fc_battery_mah`, `fc_roll_deg`, `fc_pitch_deg`, `fc_yaw_deg`, `fc_flight_mode`, `fc_frame_count`, `fc_telemetry_age_s`; the HUD battery line switching on `fc_telemetry_age_s < 1.0`.

- [ ] **Step 1: ofs-client**

In `crates/ofs-client/src/model.rs`, before `pub struct Telemetry`:

```rust
/// Betaflight's own telemetry as the handset received it over the ELRS downlink (`telemetry_age_s`
/// is INFINITY until the first frame arrives; always INFINITY without SITL).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FcTelemetry {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    pub frame_count: u64,
    pub telemetry_age_s: f64,
}

impl FcTelemetry {
    fn from_pb(f: pb::FcTelemetry) -> FcTelemetry {
        FcTelemetry {
            battery_v: f.battery_v,
            battery_a: f.battery_a,
            battery_mah: f.battery_mah,
            roll_deg: f.roll_deg,
            pitch_deg: f.pitch_deg,
            yaw_deg: f.yaw_deg,
            flight_mode: f.flight_mode,
            frame_count: f.frame_count,
            telemetry_age_s: f.telemetry_age_s,
        }
    }
}
```

`Telemetry` gains `pub fc_telemetry: FcTelemetry,` after `pub downlink_lq_pct: f64,`; `from_pb` gains `fc_telemetry: s.fc_telemetry.as_ref().map(FcTelemetry::from_pb).unwrap_or_default(),` after the `downlink_lq_pct:` line.

- [ ] **Step 2: ofs-godot's telemetry dict**

In `crates/ofs-godot/src/lib.rs`, extend the `get_telemetry` doc comment's key list (after `downlink_lq_pct,`) with `fc_battery_v, fc_battery_a, fc_battery_mah, fc_roll_deg, fc_pitch_deg, fc_yaw_deg, fc_flight_mode, fc_frame_count, fc_telemetry_age_s,` and, after the `d.set("downlink_lq_pct", …)` line, add:

```rust
        let fc = &t.fc_telemetry;
        d.set("fc_battery_v", fc.battery_v);
        d.set("fc_battery_a", fc.battery_a);
        d.set("fc_battery_mah", fc.battery_mah);
        d.set("fc_roll_deg", fc.roll_deg);
        d.set("fc_pitch_deg", fc.pitch_deg);
        d.set("fc_yaw_deg", fc.yaw_deg);
        d.set("fc_flight_mode", &GString::from(fc.flight_mode.as_str()));
        d.set("fc_frame_count", fc.frame_count as i64);
        d.set("fc_telemetry_age_s", fc.telemetry_age_s);
```

- [ ] **Step 3: The HUD battery line**

In `godot/ui/hud.gd`, add a constant near the top (after the preload/var block, before `_ready`):

```gdscript
## Betaflight's telemetry is shown while younger than this; past it the line dashes out (goggles
## behaviour: when the link drops, the pilot loses the FC's numbers too).
const FC_TELEMETRY_FRESH_S := 1.0
```

Replace (line 212):

```gdscript
		_battery.text = "%.2f V   %.1f A" % [t["battery_voltage_v"], t["battery_current_a"]]
```

with:

```gdscript
		var fc_age: float = t.get("fc_telemetry_age_s", INF)
		if fc_age < FC_TELEMETRY_FRESH_S:
			_battery.text = "%.2f V   %.1f A   %s" % [t["fc_battery_v"], t["fc_battery_a"], t["fc_flight_mode"]]
		else:
			_battery.text = "-- V   -- A   --"
```

- [ ] **Step 4: The HUD tests**

In `godot/tests/test_hud.gd`, extend the `_telemetry` fixture dict (after `"downlink_lq_pct": 100.0,`) with `"fc_battery_v": 24.6, "fc_battery_a": 3.2, "fc_battery_mah": 120.0, "fc_roll_deg": 1.0, "fc_pitch_deg": -2.0, "fc_yaw_deg": 90.0, "fc_flight_mode": "ACRO", "fc_frame_count": 120, "fc_telemetry_age_s": 0.05,` and add:

```gdscript
func test_the_battery_line_shows_betaflight_while_fresh_and_dashes_when_stale() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.battery_text(), "24.60 V   3.2 A   ACRO", "fresh: Betaflight's values and its mode")
	hud.update_view(_view({"telemetry": _telemetry({"fc_telemetry_age_s": 1.5})}))
	eq(hud.battery_text(), "-- V   -- A   --", "stale: dashes")
	hud.update_view(_view({"telemetry": _telemetry({"fc_telemetry_age_s": INF})}))
	eq(hud.battery_text(), "-- V   -- A   --", "never arrived (open loop): dashes")
	hud.queue_free()
```

`hud.gd` needs the accessor the tests use — add next to `link_text()` (line ~229):

```gdscript
## The battery line (for the tests).
func battery_text() -> String:
	return _battery.text
```

- [ ] **Step 5: The e2e check**

In `godot/tests/e2e_betaflight.gd`, after the `armed` check block (`_check(armed, …)`, line ~70), add:

```gdscript
	var fc_fresh: bool = await _wait_for(func():
		var t: Dictionary = app.client.get_telemetry()
		return t.get("fc_telemetry_age_s", INF) < 1.0 and t.get("fc_battery_v", 0.0) > 0.0, 10.0)
	_check(fc_fresh, "Betaflight's telemetry arrives over the downlink: %s" % str(app.client.get_telemetry().get("fc_battery_v")))
```

and, inside the radio-cut section after `_check(disarmed, …)` (line ~95), add:

```gdscript
	var fc_stale: bool = await _wait_for(func(): return app.client.get_telemetry().get("fc_telemetry_age_s", 0.0) > 1.0, 6.0)
	_check(fc_stale, "the FC telemetry goes stale with the link: the HUD dashes")
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p ofs-client -p ofs-godot --locked
bash scripts/run-godot-tests.sh unit   # test_hud.gd runs here (check the script's argument names)
```

The Godot e2e against Betaflight runs in CI's sitl job (`bash scripts/run-godot-tests.sh e2e`); locally run it only with OFS_SITL_LAUNCH set (the script handles WSL) — expected: the two new checks pass.

- [ ] **Step 7: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add crates/ofs-client/src/model.rs crates/ofs-godot/src/lib.rs godot/ui/hud.gd godot/ui/hud.gd.uid godot/tests/test_hud.gd godot/tests/test_hud.gd.uid godot/tests/e2e_betaflight.gd godot/tests/e2e_betaflight.gd.uid && git commit -m "feat(client): the HUD battery line shows Betaflight's telemetry and dashes at link loss

Protocol 6's fc_telemetry reaches Godot; the line switches on a 1 s freshness constant.

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

### Task 8: Docs and carried debt

**Files:**
- Modify: `docs/research/elrs-link.md`, `docs/dev-setup.md`, `README.md`
- Create: `docs/superpowers/m3d-carried-debt.md`
- Modify: `docs/superpowers/m3a-carried-debt.md`

**Interfaces:** none — docs only.

- [ ] **Step 1: `docs/research/elrs-link.md`** gains a "Downlink telemetry" section: the 1:4 slot ratio (named constant `TELEMETRY_SLOT_DIVISOR = 4` in `crates/ofs-radio/src/elrs.rs` — an ExpressLRS telemetry-ratio **estimate**, not a measured figure), the 16-frame drop-oldest FIFO (`TELEMETRY_FIFO_LEN`), the pinned Betaflight payload encodings (battery 0.1 V / 0.1 A from `getLegacyBatteryVoltage()`/`getAmperage()/10`; attitude radians × 10 000, yaw ±180°; flight mode verbatim), and the unit quirks (`mAh` = the ESC sensor's motor-summed consumption; `report_cell_voltage` off on the shipped diff).

- [ ] **Step 2: `docs/dev-setup.md`** gains: a note that **updating the repo requires rebuilding SITL** (`bash scripts/build-sitl.sh`) — a stale binary refuses the eRPM block with a `[SITL] … no such open UART` line per datagram; and the manual check: fly, watch the HUD battery line show Betaflight's values sagging with punch-outs (mode appended), cut the link (`set_fault`/the UI's radio cut) and watch it dash out while the picture stays, and open the Configurator's Motors tab to see four distinct per-motor RPMs tracking the throttle.

- [ ] **Step 3: `README.md`** — the features list gains one line: the FC's telemetry over the simulated link (HUD battery/mode are Betaflight's own) and per-motor eRPM feeding Betaflight's RPM filter.

- [ ] **Step 4: `docs/superpowers/m3d-carried-debt.md`**, in the M3b/M3c style:

```markdown
# M3d carried debt

These items were deliberately left out of M3d, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **The RPM filter's spectral proof.** The filter is structurally on (the stub's `useDshotTelemetry` gate, per-motor Hz fed from the datagram block); a Blackbox-based proof that the gyro's motor-frequency notch actually appears was skipped as too costly for the value (the M3d spec's §7 left it to the plan).
- **DShot extended telemetry and MSP-over-CRSF.** The CRSF telemetry carries battery, attitude, mode, baro, mag and vario; per-ESC voltage/current/temperature (EDT) and MSP-over-CRSF stay out with the DShot driver itself.
- **The telemetry slot ratio is config-less.** `TELEMETRY_SLOT_DIVISOR = 4` is a named constant with its estimate recorded in docs/research/elrs-link.md; expose it in the quad file only if a user misses it.

## Modelling simplifications (estimates)
- The telemetry slots are an estimated 1:4 ratio of the downlink; real ExpressLRS negotiates the ratio dynamically.
- The RC/telemetry UART is shared with no contention model: Betaflight's own scheduler interleaves its telemetry with the receiver's traffic, and the sim captures whatever it wrote.

## Rulings made while planning
- **The motor protocol stays PWM** (M3d spec decision 2): the spike proved the RPM filter needs no DShot driver, so the M3c milestone table's "motor-protocol change" note is retired.
- **The FC telemetry is not part of the lockstep digest.** It crosses to clients through a shared handle (the OSD pattern); determinism is pinned by the ELRS byte-stream test and the live runs.
- **Battery values pass through Betaflight verbatim**, including the ESC-sensor consumption summed over motor slots (x4 on a 4-motor quad) and `getLegacyBatteryVoltage()`'s 0.1 V units.
- **A stale SITL binary is refused loudly but harmlessly**: it prints one `[SITL] … no such open UART` line per datagram for the eRPM block and flies without motor telemetry; `dev-setup.md` says to rebuild.
```

- [ ] **Step 5: `docs/superpowers/m3a-carried-debt.md`** — mark the two closed items: "The RPM filter and per-motor eRPM" and "CRSF telemetry back to the radio" gain "(closed in M3d — `docs/superpowers/m3d-carried-debt.md`'s predecessor work; see the M3d spec)" — match the file's existing convention for closed items (check how M0's open item was closed in `docs/research/sitl-interface.md` §9 for the wording pattern).

- [ ] **Step 6: Commit**

```bash
cd "C:\dev\open-fpv-sim" && git add docs README.md && git commit -m "docs: M3d - the telemetry model, the rebuild-SITL note, and the carried debt

Co-Authored-By: <model> <noreply@anthropic.com>"
```

---

## Final verification

- [ ] `cargo test --workspace --locked` green; `cargo check --workspace --all-targets --locked --exclude ofs-godot` green (msrv).
- [ ] `python -m pytest python/tests/ -q` (unit tests, no OFS_SITL_LAUNCH) green.
- [ ] Live, in WSL: `wsl_run python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py python/tests/test_sitl_video.py python/tests/test_sitl_telemetry.py -v` green.
- [ ] The spike probes still pass (`t_a5`, `t_b2`) against the final patched SITL.
- [ ] Two lockstep runs with telemetry flowing give identical digests (extend `ofs-sim`'s existing same-seed determinism test only if the FC telemetry were bus signals — it is not; the ELRS byte-stream test and the live runs carry this).
- [ ] Docs: every constant added by M3d has its provenance line; `m3d-carried-debt.md` exists.
