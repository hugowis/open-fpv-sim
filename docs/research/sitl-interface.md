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
<filled in Task 4>

## 5. Feature build (CRSF, ESC sensor, OSD, VTX, Blackbox)
<filled in Task 5>

## 6. Windows / WSL2
<filled in Task 6>

## 7. Answers to spec §10 risks and recommendations
<filled in Task 7>
