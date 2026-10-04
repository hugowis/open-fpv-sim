# Betaflight SITL Interface — M0 Findings

## 1. Pinned version and build
- Tag: 2026.6.2, commit: e0b7bb01b17b21351057e9ead2d1ab39dd44fa16
- Build host: WSL2 Ubuntu 24.04, gcc 13.3, GNU make 4.3
- Build command: `bash scripts/build-sitl.sh` → `make TARGET=SITL EXTRA_FLAGS="-DENABLE_GAZEBO_BRIDGE=0" -j$(nproc)`
- Binary: `obj/main/betaflight_SITL.elf` (also copied to `obj/betaflight_2026.6.2_SITL`)
- `EXTRA_FLAGS` is honoured (Makefile line 398 appends it to CFLAGS), so compile-time switches that are tested with `#ifndef`/`#if` (e.g. `ENABLE_GAZEBO_BRIDGE`, `ENABLE_SIMULATOR_GYROPID_SYNC`) can be set without patching. Features that `target.h` explicitly `#undef`s cannot.
- Deviations from assumptions: none in the source. One build-script fix: the Makefile initialises git submodules on demand, which races on `.git/config` under `-j`; the script now runs `git submodule update --init --recursive --depth 1` first.
- Confirmed in source: UDP ports 9001 (raw PWM out), 9002 (motors out), 9003 (state in), 9004 (RC in); `--config <file>` and `--ip <addr>` options; legacy-bridge gyro map `(x, −y, −z)` in `sitl_gyro.h`; `EEPROM_FILENAME "eeprom.bin"` (relative to the working directory); `ENABLE_SIMULATOR_GYROPID_SYNC` commented out by default; real attitude estimator (`USE_IMU_CALC`) on unless `-DSITL_ATTITUDE_DIRECT`.
- `target.h` `#undef`s (relevant to later milestones): `USE_ADC`, `USE_OSD`, `USE_SERIALRX`, `USE_SERIALRX_CRSF`, `USE_TELEMETRY_CRSF`, `USE_VTX_COMMON`, `USE_VTX_CONTROL`, `USE_VTX_SMARTAUDIO`, `USE_VTX_TRAMP`, `USE_VTX_MSP`, `USE_CMS`.

## 2. Interface facts
- Packet sizes accepted as assumed: SITL logs `new fdm 144` and `new rc 40` on the first packets; motor replies are 16-byte `servo_packet`s on UDP 9002 (sent to `127.0.0.1` by default, `--ip` changes it).
- UART1 binds `tcp://127.0.0.1:5761` at startup ("bind port 5761 for UART1"); MSP over it works: `MSP_API_VERSION` → `00 01 30` (MSP protocol 0, API 1.48); `MSP_ATTITUDE` → (0.0, 0.0, 0) for a level, still FDM feed.
- `--config <file>` works: applies the CLI file, prints `[CONFIG] Config file processed, EEPROM saved`, writes `eeprom.bin` in the working directory and exits.
- Stock build (no `GYROPID_SYNC`), harness doing send→wait-for-reply at a 1 kHz simulated rate, inside WSL: **2969/3000 replies, 3.0 s simulated in 57.4 s wall** (31 timeouts of 0.5 s each account for ~15 s; the rest is ~14 ms per exchange).
- **SITL's clock does not track the FDM timestamps.** Its `micros()` integrates wall time × `simRate`, where `simRate` is re-estimated from packet spacing. Under request/response pacing this collapses: after 3.0 s of simulated time SITL's clock read ≈ 0.66 s (its 1 Hz-real-time status line advanced only 3–5 ms of SITL time per print). Consequence: Betaflight's loop timing, boot grace and RX timeouts run on a different timebase from the physics. Task 4 evaluates `GYROPID_SYNC` and an external-time patch.
- Boot arming flags seen with the spike diff while disarmed: `RXLOSS ANGLE BOOTGRACE` → `RXLOSS BOOTGRACE` (expected during boot; arming is exercised in Task 3).
- Configurator: pending user check (desktop Configurator to `tcp://127.0.0.1:5761`).

## 3. Sign conventions
Verified on the external-time build (§4) with the legacy bridge (`-DENABLE_GAZEBO_BRIDGE=0`), by motor responses (`spikes/m0/t3_conventions.py`): **8/8 checks pass** (and 8/8 on two repeat runs; one run in between showed 7/8, see §4 on RC/FDM thread ordering).

**Mapping to send** (simulator state in NED world / FRD body):

| `fdm_packet` field | Send | Notes |
|---|---|---|
| `imu_angular_velocity_rpy` | `(ωx, ωy, ωz)` FRD, **unchanged** | SITL applies `(x, −y, −z)` → Betaflight gets FLU rates. The plan's candidate `(x, y, −z)` was wrong on yaw. |
| `imu_linear_acceleration_xyz` | `(−fx, fy, fz)` FRD specific force | SITL negates all axes → Betaflight gets `(fx, −fy, −fz)` = FLU specific force. Level at rest: send `(0, 0, −9.80665)`. |
| `imu_orientation_quat` | `Rx(π)·q_ned·Rx(π)` (FLU→NWU), w ≥ 0 | Only feeds the virtual compass (attitude comes from Betaflight's own estimator). Heading not verified: with default settings the virtual compass is not fused (yaw stayed at 2–4° for a 90° heading). Irrelevant for acro/angle; revisit for GPS modes. |
| `velocity_xyz` | ENU `(vE, vN, vU)` | virtual GPS only |
| `position_xyz` | `(lon°, lat°, alt m)` | virtual GPS only |
| `pressure` | Pa | used directly by the legacy bridge |

In other words, Betaflight's internal body frame is FLU; gyro and accel must arrive consistently in it.

**Check results** (motor index 0..3 = M1 RR, M2 FR, M3 RL, M4 FL; armed, 50 % throttle):

| Check | Expected group up | Result |
|---|---|---|
| gyro +x FRD (rolling right) | right (M1, M2) | PASS Δ=+0.44 |
| gyro +y FRD (nose rising) | rear (M1, M3) | PASS Δ=+0.46 |
| gyro +z FRD (yawing right) | CW props (M1, M4) | PASS Δ=+0.62 |
| roll stick right | left (M3, M4) | PASS Δ=+0.40 |
| pitch stick forward | rear (M1, M3) | PASS Δ=+0.81 |
| yaw stick right | CCW props (M2, M3) | PASS Δ=+0.47 |
| angle mode, rolled right 20° | right (M1, M2) | PASS Δ=+1.62 |
| angle mode, nose up 20° | rear (M1, M3) | PASS Δ=+0.76 |

This also confirms the Betaflight Quad-X motor order and default spin directions in the plan (M1 CW, M2 CCW, M3 CCW, M4 CW), and the RC channel order AETR + AUX1..4.

**Estimator read-back over MSP** (static attitudes, 6 s each): roll right 20° → `MSP_ATTITUDE` roll +19.9; nose up 10° → pitch **−9.9** (Betaflight's MSP pitch is positive nose-down); combined 20°/10° → (20.0, −9.9). `MSP_RAW_IMU` acc matches the sent values to ±1 count (1 g = 256).

**Motor output:** `servo_packet.motor_speed` is 0 when disarmed, 0.055 at armed idle, 0.50 at 50 % throttle hover-trim, saturating at 1.0.

**Minimal arming config** (the spike diff worked unchanged; boot flags clear by 5 s simulated: `RXLOSS BOOTGRACE` → `BOOTGRACE` → none):
```
feature -GPS
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
```

## 4. Timing and lockstep

### Summary
**Stock SITL cannot run in lockstep. `GYROPID_SYNC` alone doesn't fix it. A ~170-line patch (`third_party/betaflight/ofs-sitl.patch`, enabled with `-DENABLE_SIMULATOR_EXTERNAL_TIME=1`) makes it exact:** every state packet gets exactly one motor reply, at 6–7× real time, and two runs with the same inputs produce bit-identical motor outputs.

### Measurements (`spikes/m0/t4_lockstep.py`)
Each run boots for 5.5 s and arms for 1 s. It then records 3 s of motor outputs under a 3 Hz gyro sinusoid plus a 1 Hz roll-stick sinusoid at 50 % throttle. Two fresh runs from the same `eeprom.bin` are compared. WSL2, 24 threads, Python harness.

| Build | Exchange | RC transport | Reply ratio | Real-time factor | Identical steps | Max diff |
|---|---|---|---|---|---|---|
| stock | 1 kHz | separate (9004) | 1.0 / 0.999 | 0.20–0.26 | 1 / 3000 | 0.361 |
| `GYROPID_SYNC` only | 1 kHz | separate | ~0.98 | ~0.05 (p50 12 ms/exchange) | not measured | — |
| **patched** (final) | 1 kHz | **in state packet** | 1.0 | **6.0–6.7** | **3000 / 3000** | **0** |
| patched (final) | 1 kHz | separate (9004) | 1.0 | 6.2–6.7 | 2124 / 3000 | 0.004 |
| patched, `-DVIRTUAL_GYRO_SAMPLE_RATE_HZ=8000` | 8 kHz | in state packet | 1.0 | 0.80 (harness-bound) | 24000 / 24000 | 0 |

Steady disarmed run on the patched build: 40 s simulated, 0 misses, round trip p50 0.15 ms, p99 0.25 ms.

### Root causes found (each fixed in the patch)
1. **SITL's clock is wall time × `simRate`**, and `simRate` is re-estimated from packet spacing. Under request/response pacing it collapses: 3.0 s simulated left SITL's clock at about 0.66 s. → After the first packet, `micros()`/`millis()` follow the packet timestamps, starting from a **fixed** 10 s base (a wall-derived offset made task phases differ between runs). `delay()`/`delayMicroseconds()` sleep in real time, so they can never wait on frozen time.
2. **The scheduler waits for a cycle-counter target** before running gyro/filter/PID. With frozen time, a target that lands inside the next packet interval is never reached: a deadlock whose onset depends on phase. It was root-caused with a `SIGUSR1` backtrace; the main thread was idling in `run()`. The same frozen budget starved the RX and MSP tasks (`RXLOSS`, dropped MSP connections). → In external-time mode, gyro, filter and PID run **exactly once per state packet**, and the non-realtime tasks always get a full gyro period of budget.
3. **The motor reply was sent from inside the PID task**, before the tick's other tasks (RX and so on) had run. The simulator's next packet could then advance time mid-tick, shifting when stick input took effect by a few ticks. → The reply is queued and sent from a **scheduler-idle hook**, once every task due at the current instant has run. The idle hook then waits on a condition variable (1 ms bound) for the next packet, and `RUN_LOOP_DELAY_US` is 0, so there's no per-pass sleep.
4. **RC arrives on its own UDP thread** and races the state packet by up to a tick. → The state port also accepts **`fdm_packet` + `rc_packet` in one 184-byte datagram**; the RC is applied on the state thread before the gyro tick. Separate RC on 9004 still works but isn't bit-reproducible.
5. **stdout is block-buffered** when piped, so a supervisor sees stale logs and loses them on kill. → `setvbuf(stdout, _IOLBF)` at startup in external-time mode.

### Consequences for M1–M3
- **Build:** `bash scripts/build-sitl.sh` now builds the patched lockstep binary by default (`OFS_SITL_PATCH=none` builds stock).
- **Exchange rate must equal Betaflight's gyro rate:** one packet = one gyro sample. The virtual gyro defaults to 1 kHz (`VIRTUAL_GYRO_SAMPLE_RATE_HZ`). M1's `exchange_hz = 1000` matches the default build; an 8 kHz build is possible but about 8× more exchanges.
- **Protocol:** send one 184-byte datagram (`fdm_packet` ‖ `rc_packet`) to UDP 9003 per exchange; read one 16-byte `servo_packet` from 9002. Don't send to 9004.
- **Determinism with SITL is achievable:** the spec's "identical logs" can include firmware runs on the patched build (simulator side must be deterministic too).
- **MSP is only serviced while simulated time advances.** Harnesses and the Configurator need the simulation stepping (real-time mode in M2) while they talk to it. A paused simulation freezes MSP.
- **Arming config:** unchanged (§3). Boot grace clears by the second simulated status line after the first packet.
- The first status line after the first packet reads `t=10001ms` because of the fixed 10 s time base.

## 5. Feature build (CRSF, ESC sensor, OSD, VTX, Blackbox)

The single patch `third_party/betaflight/ofs-sitl.patch` (7 files, +235/−14) now contains the lockstep changes (§4) and the feature changes below. `scripts/build-sitl.sh` builds it by default, always from a clean object directory: Betaflight's make does **not** rebuild objects after `target.h` changes, and stale objects briefly confused these results. Generator scripts: `spikes/m0/patch_ext_time.py`, `spikes/m0/patch_features.py`. Re-verified on the final build: §3 conventions 8/8, §4 reproducibility 3000/3000 bit-identical at 6.4–6.7× real time.

### Results (`spikes/m0/t5_features.py`, clean build)

| Feature | Answer | Evidence |
|---|---|---|
| CRSF receiver | **Yes** | CRSF RC frames (500 Hz) written to UART2 (tcp:5762) → `MSP_RC` = 1600 / 1400 / 1500 / 1000 (MSP order roll, pitch, yaw, throttle) for sent AETR 1600 / 1400 / 1000 / 1500; arming via CRSF AUX1 works (no arming-disable flags). |
| CRSF telemetry | **No** | `telemetry/crsf.c` uses ARM-only `ATOMIC_BLOCK`/`BASEPRI`; SITL excludes it in `SITL.mk`. Needs a SITL atomic-block shim → M2 work if the HUD should show Betaflight's CRSF telemetry. Battery/RPM/link data are available to the simulator anyway (it produces them). |
| ESC sensor (battery V/I, RPM) | **Yes** | KISS 10-byte frames (CRC-8 poly 0x07) on UART3 (tcp:5763) → `MSP_BATTERY_STATE` 24.59 V for 24.6 V sent, 6 cells detected; `MSP_MOTOR_TELEMETRY` RPM 1714 = 12 000 eRPM / 7 pole pairs. **This answers spec risk 2 without a sensor-packet patch.** |
| MSP DisplayPort (OSD) | **Yes** | UART4 (tcp:5764) with `serial 3 131073` (VTX_MSP + MSP) and `osd_displayport_device = MSP` → 457 `MSP_DISPLAYPORT` (182) frames in ~1.5 s (clear, write-string, draw subcommands). Needs `USE_CMS` + `USE_OSD_OVER_MSP_DISPLAYPORT` (re-enabled). |
| SmartAudio | **Partial** | UART5 (tcp:5765) receives Betaflight's SmartAudio `GET_SETTINGS` requests (`aa 55 03 00 9f`, repeated); `MSP_VTX_CONFIG` reports a SmartAudio device. Replying with a SmartAudio settings frame was not attempted (M3). |
| Blackbox | **Yes** | `set blackbox_device = VIRTUAL` writes `LOG00001.BFL` in SITL's working directory when armed (20 KB for ~2 s), standard header (`H Product:Blackbox flight data recorder…`), so it should open in Blackbox Explorer. |

### Changes the feature build needed (all in the patch)
- `target.h`: stop `#undef`-ing `USE_SERIALRX`, `USE_SERIALRX_CRSF`, `USE_OSD`, `USE_CMS`, `USE_VTX_COMMON`, `USE_VTX_CONTROL`, `USE_VTX_SMARTAUDIO`, `USE_VTX_TRAMP`; define `USE_ESC_SENSOR`, `USE_MSP_DISPLAYPORT`, `USE_OSD_OVER_MSP_DISPLAYPORT`.
- `common_post.h`: keep `USE_ESC_SENSOR` without `USE_DSHOT` in `SIMULATOR_BUILD`. `sitl.c` provides `erpmToRpm`, `useDshotTelemetry`, `initDshotTelemetry` (same math as `drivers/dshot.c`), plus `microsISR()`.
- `serial_tcp.c`:
  - **Interrupt-driven RX drivers never saw TCP bytes.** `tcpDataIn` only filled the buffer, but CRSF and the ESC sensor register an `rxCallback`, which is now called per byte as a UART ISR would.
  - `serialSetBaudRate`/`setMode` were NULL, and **SmartAudio crashed SITL** calling them; they're now no-ops.
- `osd.c`: **upstream bug.** `osdGetBlackboxStatusString` divides by `storageTotal`, which is 0 for the VIRTUAL device. The compiler emits a trap, so SITL died with SIGILL on the post-disarm stats screen. Guarded. Found via the kernel's trap IP and `objdump -l`; worth sending upstream.

### Working CLI config (`spikes/m0/feature.diff`)
```
feature -GPS
feature RX_SERIAL
feature ESC_SENSOR
feature OSD
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
serial 1 64 115200 57600 0 115200        # UART2 tcp:5762  CRSF receiver
serial 2 1024 115200 57600 0 115200      # UART3 tcp:5763  ESC sensor (KISS)
serial 3 131073 115200 57600 0 115200    # UART4 tcp:5764  MSP + VTX_MSP (DisplayPort)
serial 4 2048 115200 57600 0 115200      # UART5 tcp:5765  SmartAudio
set serialrx_provider = CRSF
set battery_meter = ESC
set current_meter = ESC
set osd_displayport_device = MSP
set blackbox_device = VIRTUAL
```
(`displayport_msp_serial` no longer exists in 2026.6; the DisplayPort port is the first one with both VTX_MSP and MSP functions.)

### Caveat for determinism (M2/M3)
TCP UART bytes arrive on SITL's TCP thread asynchronously to the lockstep tick, so CRSF, ESC-sensor and SmartAudio traffic over TCP isn't bit-reproducible. M1 doesn't use them: RC rides in the state packet and the battery comes from the simulator's own models. When M2 moves RC onto CRSF, either carry UART bytes inside the state datagram (applied on the state thread before the tick, the same technique as RC in §4) or accept non-bit-exact runs in that mode.

## 6. Windows / WSL2
<filled in Task 6>

## 7. Answers to spec §10 risks and recommendations
<filled in Task 7>
