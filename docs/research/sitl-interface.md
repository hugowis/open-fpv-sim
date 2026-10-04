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
<filled in Task 2>

## 3. Sign conventions
<filled in Task 3>

## 4. Timing and lockstep
<filled in Task 4>

## 5. Feature build (CRSF, ESC sensor, OSD, VTX, Blackbox)
<filled in Task 5>

## 6. Windows / WSL2
<filled in Task 6>

## 7. Answers to spec §10 risks and recommendations
<filled in Task 7>
