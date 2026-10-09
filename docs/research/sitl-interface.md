# Betaflight SITL Interface — M0 Findings

## 1. Pinned version and build
- Tag: 2026.6.2, commit: e0b7bb01b17b21351057e9ead2d1ab39dd44fa16
- Build host: WSL2 Ubuntu 24.04, gcc 13.3, GNU make 4.3
- Build command: `bash scripts/build-sitl.sh`. It applies `third_party/betaflight/ofs-sitl.patch`, builds clean, and runs `make TARGET=SITL EXTRA_FLAGS="-DENABLE_GAZEBO_BRIDGE=0 -DENABLE_SIMULATOR_EXTERNAL_TIME=1" -j$(nproc)`. It refuses to build if the tag doesn't resolve to the pinned commit (`BF_SHA`). `OFS_SITL_PATCH=none` builds stock SITL; with the patch but without `-DENABLE_SIMULATOR_EXTERNAL_TIME=1`, it builds and behaves like stock timing.
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
- Configurator: **open item, not tested in M0** (needs the user; see §7). Expected implications on the patched build:
  - MSP is serviced only while simulated time advances (§4), so the Configurator needs the sim stepping (M2 real-time mode).
  - "Save" makes Betaflight reboot, and in SITL that means `systemReset` → `exit(0)`: the simulator must treat that exit as a firmware restart, not a crash.
  - The web Betaflight App can't open raw TCP and would need a WebSocket↔TCP bridge.

## 3. Sign conventions
Verified on the external-time build (§4) with the legacy bridge (`-DENABLE_GAZEBO_BRIDGE=0`), by motor responses (`spikes/m0/t3_conventions.py`): **8/8 checks pass.** Every later run passed 8/8 too: more than ten, including on the final build. One early run showed 7/8, and which check failed wasn't recorded. Its cause is unknown; a race of at most one tick between RC and state packets seems too small to flip a 30-tick pulse, so that earlier attribution was dropped.

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
**Stock SITL cannot run in lockstep. `GYROPID_SYNC` alone doesn't fix it. A patch (`third_party/betaflight/ofs-sitl.patch`, enabled with `-DENABLE_SIMULATOR_EXTERNAL_TIME=1`) makes it exact:** every state packet gets exactly one motor reply, at 6–7× real time, and two runs with the same inputs produce bit-identical motor outputs.

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

### Changes after the code review
- **Hardening (minor findings):**
  - the idle wait uses a `CLOCK_MONOTONIC` condition variable, so WSL2 clock steps after host sleep can't stretch or skip it;
  - the hook prototypes moved into the SITL `target.h`;
  - a liveness cap (`simulatorSchedulerBusy`): if tasks are still being selected after 100 000 scheduler passes at one frozen instant, SITL replies anyway and warns once;
  - interrupt-driven TCP serial RX is delivered on the main thread at each tick (§5).
- **Time and RC are now staged, not published, by the state thread.** Before, `updateState()` wrote the new time (and applied RC) before counting the tick. A scheduler pass waking from its 1 ms wait inside that window could see the new instant before its tick. The state thread now only stages time and RC under `extPacketMutex`. The main thread applies them in `simulatorTakeGyroTick()`, then publishes `extTimeValid` with release semantics. The race couldn't be reproduced, even with a debug build that widens the window to 300 µs plus 0–3 ms sender jitter (3000/3000 identical before and after). It was fixed on code reasoning; the widened-window run is kept as a regression check.
- **One reply per tick, whether or not PID ran.** Before, the reply came from the PID motor update. With `pid_process_denom = 2`, SITL answered only every second packet (reply ratio 0.50, reproduced), which would be fatal in M1. Now every taken tick gets exactly one reply carrying the latest motor values. After the fix: `pid_process_denom = 2` gives reply ratio 1.00 and 3000/3000 identical steps at 6.4–6.6× real time. The plain combined-mode check is still 3000/3000 at 6.7–6.8×, conventions 8/8, the feature probe unchanged, and the stall probe 6000/6000.
- **Runs that had been left out of the table above** (all from `spikes/m0/runs/t4/results.jsonl`):
  - one ext5 run had one lost reply (ratio 0.99967, real-time factor 3.0);
  - one combined-mode run had 2993/3000 identical steps, first difference at step 2912;
  - §6 lost 8 replies during boot with separate RC packets.

  All were on builds before the review fixes. The harness doesn't pair replies with requests: a reply arriving after its 0.5 s timeout is taken as the *next* packet's reply, which plausibly explains a one-step, 0.001-sized divergence after a loss. Hence the M1 recommendations in §7: generous first-reply timeout, drain before send, optional timestamp echo.

### Consequences for M1–M3
- **Build:** `bash scripts/build-sitl.sh` now builds the patched lockstep binary by default (`OFS_SITL_PATCH=none` builds stock).
- **Exchange rate must equal Betaflight's gyro rate:** one packet = one gyro sample. The virtual gyro defaults to 1 kHz (`VIRTUAL_GYRO_SAMPLE_RATE_HZ`). M1's `exchange_hz = 1000` matches the default build; an 8 kHz build is possible but about 8× more exchanges.
- **Protocol:** send one 184-byte datagram (`fdm_packet` ‖ `rc_packet`) to UDP 9003 per exchange; read one 16-byte `servo_packet` from 9002. Don't send to 9004.
- **Determinism with SITL is achievable:** the spec's "identical logs" can include firmware runs on the patched build (simulator side must be deterministic too).
- **MSP is only serviced while simulated time advances.** Harnesses and the Configurator need the simulation stepping (real-time mode in M2) while they talk to it. A paused simulation freezes MSP.
- **Arming config:** unchanged (§3). Boot grace clears by the second simulated status line after the first packet.
- The first status line after the first packet reads `t=10001ms` because of the fixed 10 s time base.

## 5. Feature build (CRSF, ESC sensor, OSD, VTX, Blackbox)

The single patch `third_party/betaflight/ofs-sitl.patch` (8 files, +346/−14 after the review and minor fixes) now contains the lockstep changes (§4) and the feature changes below. `scripts/build-sitl.sh` builds it by default, always from a clean object directory: Betaflight's make doesn't reliably rebuild objects when `EXTRA_FLAGS` change; `target.h` edits are suspected too. Stale objects briefly confused these results, and building clean covers both. Generator scripts: `spikes/m0/patch_ext_time.py`, `spikes/m0/patch_features.py` (apply to a clean tree, then `git diff`). Re-verified on the final build: §3 conventions 8/8, §4 reproducibility 3000/3000 bit-identical at 6.4–6.7× real time.

### Results (`spikes/m0/t5_features.py`, clean build)

| Feature | Answer | Evidence |
|---|---|---|
| CRSF receiver | **Yes** | CRSF RC frames (500 Hz) written to UART2 (tcp:5762) → `MSP_RC` = 1600 / 1400 / 1500 / 1000 (MSP order roll, pitch, yaw, throttle) for sent AETR 1600 / 1400 / 1000 / 1500; arming via CRSF AUX1 works (no arming-disable flags, and the ARM box is active in `MSP_STATUS`). |
| CRSF telemetry | **No** | `telemetry/crsf.c` uses ARM-only `ATOMIC_BLOCK`/`BASEPRI`; SITL excludes it in `SITL.mk`. Needs a SITL atomic-block shim → M2 work if the HUD should show Betaflight's CRSF telemetry. Battery/RPM/link data are available to the simulator anyway (it produces them). |
| ESC sensor (battery V/I, RPM) | **Partial** | KISS 10-byte frames (CRC-8 poly 0x07) on UART3 (tcp:5763) → `MSP_BATTERY_STATE` 24.59 V for 24.6 V sent, 6 cells detected; `MSP_MOTOR_TELEMETRY` RPM 1714 = 12 000 eRPM / 7 pole pairs. Caveats (from review): `esc_sensor.c` **sums current and consumption across motors** (only voltage and RPM are averaged), so the simulator must send per-motor current, not pack current (current was not checked here). Each frame is stored under the motor the FC is currently polling, which the simulator can't observe without DShot telemetry requests, so **per-motor values can't be attributed** (all frames here were identical). **RPM never reaches the RPM filter**, which requires `useDshotTelemetry` (hard-set false). |
| MSP DisplayPort (OSD) | **Yes** | UART4 (tcp:5764) with `serial 3 131073` (VTX_MSP + MSP) and `osd_displayport_device = MSP` → 456 checksum-valid `MSP_DISPLAYPORT` (182) frames in ~1.5 s, subcommands 0 (heartbeat), 2 (clear), 3 (write string), 4 (draw); counted by parsing frames (`msp_frames`). Needs `USE_CMS` + `USE_OSD_OVER_MSP_DISPLAYPORT` (re-enabled). |
| SmartAudio | **Partial** | UART5 (tcp:5765) receives Betaflight's SmartAudio `GET_SETTINGS` requests (`aa 55 03 00 9f`, repeated); `MSP_VTX_CONFIG` reports a SmartAudio device. Replying with a SmartAudio settings frame was not attempted (M3). |
| Blackbox | **Yes** | `set blackbox_device = VIRTUAL` writes `LOG00001.BFL` in SITL's working directory when armed (20 KB for ~2 s), standard header (`H Product:Blackbox flight data recorder…`). **Decodes with Betaflight's `blackbox_decode`** (blackbox-tools `f832acf`): 451 frames, with the usual `loopIteration, time (us), axisP[0] …` columns. |

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
TCP UART bytes arrive on SITL's TCP thread asynchronously to the lockstep tick. On the lockstep build they are now buffered and delivered to the drivers' RX callbacks on the **main thread** at the next tick (`tcpSerialDispatchRx`), so drivers never run concurrently with the main loop, as with a UART interrupt. *Which* tick bytes land on still depends on wall-clock arrival, so CRSF, ESC-sensor and SmartAudio traffic over TCP isn't bit-reproducible. M1 doesn't use them: RC rides in the state packet and the battery comes from the simulator's own models. When M2 moves RC onto CRSF, either carry UART bytes inside the state datagram (applied on the state thread before the tick, the same technique as RC in §4) or accept non-bit-exact runs in that mode.

## 6. Windows / WSL2
Host: Windows 11, WSL 2.6.3, kernel 6.6.87.2, Ubuntu 24.04. `.wslconfig` has **no `networkingMode`**, so WSL uses the default NAT networking. Nothing was changed on the machine.

**Default NAT networking, Windows-side harness** (reproducible with `spikes/m0/t6_windows.py`, run from Windows):
- TCP `127.0.0.1:5761` from Windows reaches SITL inside WSL (WSL localhost forwarding), so a Windows-hosted Configurator works.
- **UDP is not forwarded.** Sending to `127.0.0.1:9003` from Windows gets no replies, because SITL's replies go to `127.0.0.1:9002` *inside* WSL.
- **Works with explicit addresses:**
  - Send state packets to the **WSL VM IP** (`wsl -e hostname -I`, first field; here `172.28.26.115`).
  - Launch SITL with `--ip <Windows host IP as seen from WSL>` (`wsl -e sh -c "ip route | awk '/default/ {print $3}'"`; here `172.28.16.1`).
  - Bind the motor socket on `0.0.0.0:9002`.
  
  Result with combined 184-byte packets: **5000/5000 replies, p50 0.21 ms, p99 0.35 ms, 4.6× real time**. Rerun on the final build with the committed script, which binds the host-side vEthernet IP: 5000/5000, p50 0.20 ms, p99 0.29 ms, 4.9×. No firewall prompt. With separate RC packets, 8 replies were lost during boot.
- The WSL VM IP can change after `wsl --shutdown`/reboot, so it must be discovered at launch, not configured.

**Mirrored networking** (`[wsl2] networkingMode=mirrored`) would make `127.0.0.1` work in both directions, but it changes networking for every WSL distribution. It's not needed, so it was not enabled. It's a reasonable opt-in for users who want zero configuration.

**Process lifecycle (corrected after review):**
- Killing or terminating the `wsl.exe` launcher *process* ends SITL inside WSL (`pgrep` shows nothing afterwards).
- **That is not enough when the host process dies uncleanly.** Windows does not kill child processes, so `wsl.exe` and SITL keep running. This happened during M0: stopping a background shell left the WSL-side harness *and* SITL alive, holding UDP 9002–9004.
- A stale SITL is dangerous because a new one doesn't fail cleanly:
  - `udplink.c` uses `SO_REUSEADDR`, so the new instance binds 9003/9004 alongside the old one;
  - its TCP 5761 bind fails, but it only prints `bind port 5761 for UART1 failed!!` and keeps running;
  - a readiness probe on 5761 then connects to the *old* instance;
  - under NAT, Windows-side port probes can't see ports inside WSL at all.

  The simulator would silently talk to the wrong SITL, with the wrong config.
- **Cleanup is therefore required on Windows:** `OFS_SITL_CLEANUP="wsl.exe -d Ubuntu -e pkill -x betaflight_SITL"`, run before every launch. Match the process name exactly (`-x`), never with `-f`: the command line of the launching shell often contains the binary path, and `pkill -f` then kills the shell itself (this bit M0 twice). It kills every SITL on the machine, which is acceptable under the one-session-per-machine rule.
- **Also treat any `bind port … failed` line in SITL's output as a startup error.**

**Launch values (Windows):**
```
OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"
# plus, under NAT (default): append "--ip <host IP from WSL>" and send UDP to the WSL VM IP
OFS_SITL_CLEANUP="wsl.exe -d Ubuntu -e pkill -x betaflight_SITL"   # required (see Process lifecycle)
```

**Native Windows build:** not attempted. No MinGW/MSYS2/clang toolchain is installed, and installing one is a machine change. The SITL code is POSIX (pthreads, `clock_gettime`, `nanosleep`, BSD sockets; dyad has a `_WIN32` path), so an MSYS2/Cygwin port is plausible future work. **WSL2 is the supported Windows path for v1.**

Caveats:
- The 9p-mounted `/mnt/c` working directories work for `eeprom.bin`, logs and blackbox files.
- In a CRLF-converting Windows checkout, the build script and patches need LF endings; enforced via `.gitattributes`.

## 7. Answers to spec §10 risks and recommendations

| # | Risk | Answer | Evidence | Recommendation |
|---|---|---|---|---|
| 1 | SITL lockstep | **Yes, with our patch** (stock: no) | §4: stock clock collapses (0.2× real time, diverges from step 1); `GYROPID_SYNC` alone still paced by wall time; patched build gives exactly one reply per packet, 6.0–6.7× real time, **bit-identical** runs | Use `third_party/betaflight/ofs-sitl.patch` with `-DENABLE_SIMULATOR_EXTERNAL_TIME=1` (build script default). Exchange rate = virtual gyro rate (1 kHz). Keep the patch small and upstreamable; the `osd.c` fix is upstream-worthy on its own. |
| 2 | Battery V/I and RPM into SITL | **Partial:** voltage yes; current only as per-motor values; per-motor RPM and the RPM filter **no** | §5: simulated KISS ESC telemetry → `MSP_BATTERY_STATE` 24.59 V; current is summed per motor; frames can't be attributed to motors; the RPM filter needs DShot telemetry | M1 doesn't need it (the battery affects physics only). For battery OSD/warnings, add the ESC-telemetry UART model (async, see the §5 caveat). For the RPM filter, the original reason for this risk, emulate bidirectional DShot telemetry in SITL (feed per-motor eRPM each loop), or add a SITL hook that exposes the polled motor index. Future work. |
| 3 | CRSF over a SITL UART | **Yes** (RX); **No** (telemetry) | §5: CRSF RC frames on tcp:5762 drive `MSP_RC`, arming works; CRSF telemetry needs an `ATOMIC_BLOCK` shim | M1: RC rides in the state datagram (deterministic). M2: CRSF on UART2 for protocol realism; carry UART bytes in the state datagram if bit-exact runs are wanted; add the telemetry shim if the HUD should show Betaflight's CRSF telemetry. |
| 4 | MSP DisplayPort and SmartAudio | **DisplayPort yes; SmartAudio partial** | §5: 457 DisplayPort frames; SmartAudio requests seen, replies not yet emulated | M3 implements the SmartAudio reply side (VTX model) and decodes DisplayPort into the OSD grid. |
| 5 | Blackbox capture | **Yes** | §5: `blackbox_device = VIRTUAL` writes `LOGnnnnn.BFL` in SITL's working directory with a valid header | M4 copies the log into the session's recording. |
| 6 | Windows support | **Yes via WSL2**; native not attempted | §6: default NAT works with WSL-IP + `--ip`, 4.6× real time; killing `wsl.exe` ends SITL | Support WSL2 in v1; discover addresses at launch; mirrored networking optional. |

**Open items:**
- **Configurator connection** (plan Task 2 Step 5) was not run; it needs the user. Implications are listed in §2.
- **SmartAudio reply side** was not emulated (closed in M3a, see §9).
- **Heading/quaternion** is unverified.
- **Intermittent MSP timeout:** after the review fixes, one of six Task 3 runs timed out on the trailing pumped `MSP_ATTITUDE` read-back. The motor checks still passed 8/8, and five reruns, three of them right after Task 4 runs, were clean, with no `bind … failed` lines. The cause is unknown; that run's SITL log was overwritten. Watch for it when M2 uses MSP and the Configurator.

Two cross-cutting findings that also affect M2:
- **MSP is serviced only while simulated time advances.** Real-time mode (M2) keeps the Configurator working; a paused sim freezes it.
- **SITL stdout is line-buffered** in the patched build, so a supervising process sees logs live.

### M1 plan assumptions

| Assumption | Status | Change needed in M1 |
|---|---|---|
| **A1** build script, `ENABLE_GAZEBO_BRIDGE=0`, binary path, `--config` | **Confirmed** (extended) | None to the steps. The script now builds the patched lockstep binary by default (`-DENABLE_SIMULATOR_EXTERNAL_TIME=1`). |
| **A2** ports 9002/9003/9004, packet sizes, `motor_speed` ∈ [0, 1], 0 disarmed | **Confirmed** | Armed idle is 0.055. **Send one 184-byte datagram (`fdm_packet` ‖ `rc_packet`) to 9003 and don't use 9004** (Task 11 bridge). |
| **A3** gyro `(x, y, −z)`, accel = FRD specific force | **Refuted** | Task 10: `GYRO_SIGN = [1, 1, 1]` and accel sent as `(−fx, fy, fz)`; update the test vectors (`gyro_rpy_radps == [1, 2, 3]`, `accel_xyz_mps2 == [−0.1, 0.2, −9.8]`). The quaternion mapping is **unverified**: it only feeds the virtual compass, which wasn't fused, and roll/pitch come from the accel-driven estimator. No M1 impact (acro/angle); verify before GPS/compass modes. |
| **A4** ≤ 1 reply per state packet within 500 ms | **Confirmed on the patched build** (exactly 1); stock refuted | None beyond using the patched build. Optional: add a SITL determinism test (same seed → identical motor trace), now feasible. |
| **A5** arming diff (ARM AUX1, ANGLE AUX2, PWM, small_angle 180) | **Confirmed** unchanged | None. |
| **A6** Windows via `wsl.exe` with mirrored networking | **Refuted** (mirrored not enabled and not required) | Task 11: under default NAT the bridge must (a) discover the WSL VM IP and send state packets there, (b) append `--ip <host IP from WSL>` to the launch argv, and (c) bind the motor socket on the host-side vEthernet IP (the `--ip` value) or `0.0.0.0:9002`. Detect WSL when `launch[0]` is `wsl.exe`; allow overrides (`OFS_SITL_HOST`, `OFS_SITL_REPLY_IP`). **`OFS_SITL_CLEANUP` is required on Windows** (§6) and should default to the `pkill` above when launching via `wsl.exe`. Fail startup on a `bind port … failed` line. |

**Further M1 changes from the review:**
- **First exchange:** TCP 5761 accepting does not mean the main loop is running. Allow a generous *first-reply* timeout (seconds), or retry the first packet, before applying the per-exchange timeout.
- **Reply pairing:** a reply that arrives after a timeout would be taken as the next packet's reply. M1 treats a timeout as fatal, so that's safe, but drain the socket before each send. Optionally have SITL echo the FDM timestamp (24-byte reply) so mis-pairing is detectable.
- **`pid_process_denom`:** fixed in the patch (one reply per tick regardless), but keep `pid_process_denom = 1` in quad configs so that one exchange is one PID update.
- **One SITL per simulation-time origin:** SITL's external clock never runs backwards, so restarting simulation time at 0 needs a fresh SITL process. M1 already relaunches on every `Load`; keep it that way.

## 8. M2 changes (radio link, real time, reboots)

### UART bytes in the state datagram
- **Format.** After the 184-byte datagram the simulator appends blocks of `[uart index (0-based)][len lo][len hi][bytes]`, at most 512 bytes in total (`EXT_SERIAL_MAX`). SITL's FDM thread stages them with the packet. `simulatorTakeGyroTick()` hands them to `tcpSerialInject()` → the port's RX buffer → the driver at that tick's `tcpSerialDispatchRx()`. The receiver's CRSF output therefore reaches Betaflight on a deterministic tick. M0 §5's caveat for TCP UARTs still applies to real tools connected over TCP.
- **Generator:** `third_party/betaflight/tools/add_serial_in_datagram.py`.
- **Verified:** CRSF sticks roll 0.2, pitch −0.2, yaw 0, throttle 0 read back over MSP_RC as exactly 1600 / 1400 / 1500 / 1000 µs.
- **UDP RC:** the `rc_packet` channels are sent as zeros. With `serialrx_provider = CRSF`, Betaflight ignores them. A stale EEPROM still on the UDP receiver sees invalid pulses and fails safe.

### Deterministic boot
- **Symptom.** With CRSF, about 40 % of compared lockstep flights diverged, always at the same instant. Motor 0 read 0.245 vs 0.246 two seconds after arming. The M1 path (RC inside the datagram) stayed deterministic on the same binary.
- **Ruled out.** The divergence was independent of packet rate (500 Hz / 1 kHz) and of link-statistics frames. `eeprom.bin` was identical before every flight.
- **Located.** A Blackbox recorded from boot (`blackbox_mode = ALWAYS`) showed the attitude estimate already differing by 1–2 counts 0.3 s after the first packet. The recording also started 1 ms apart.
- **Cause.** SITL's UDP thread starts early in `systemInit()`, so a state packet could land while init still ran. Init read the fake sensors before or after the packet's values arrived. CRSF's longer init moved which side of that race boots landed on. Separately, the scheduler ran tasks on wall time until the first packet.
- **Fix** (`third_party/betaflight/tools/deterministic_boot.py`):
  - state packets are ignored until the scheduler runs;
  - SITL prints `[SITL] ready for the simulator` at that moment;
  - the scheduler runs no task before the first packet;
  - the bridge waits for that line instead of probing TCP 5761 and sleeping 300 ms.
- **Result:** 8/8 compared flights bit-identical; the M1-path determinism test still passes.

### Reboots
- **What SITL does.** `systemReset()` prints `[system]Reset!`, joins its threads and calls `exit(0)`. Before that, `motorShutdown()` sleeps 501.5 ms of real time (PWM protocol), longer than the 500 ms reply timeout.
- **What the bridge does.** After a missed reply it waits up to 3 s for "exited 0 and the reset line seen". It then drops the old process (its `pkill` cleanup would kill a new instance), relaunches from the same EEPROM with the same argv, and resends the current datagram with the first-exchange resend logic. SITL's clock restarts from the next packet (fixed 10 s base).
- **Live test:** `a_betaflight_reboot_is_a_firmware_restart_not_a_crash`.

### Real time and MSP
- In a real-time session on Windows + WSL2, SITL answered `MSP_API_VERSION` (API 1.48) in about 16 ms without the client stepping anything.
- An 8 s real-time run had 0 overruns.
- **Configurator check: pending (needs the user).** Run `python python/examples/serve_realtime.py` (with `OFS_SITL_LAUNCH` set) and connect Betaflight Configurator to `tcp://127.0.0.1:5761`. Then change one PID value and press Save, reconnect and confirm the value persisted, and note `betaflight restarts=1` in the script's output. Record here: Configurator version, pass/fail, observations.

### Failsafe timing
- **Measured.** With the radio link cut while armed at idle, Betaflight disarmed 1.4–2.2 s later, consistent with its code:
  - frames stop;
  - `failsafe_delay` defaults to 1.5 s (`failsafeOnValidDataFailed`);
  - then `JUST_DISARM` at low throttle, or `DROP`.
- **Live test:** `a_radio_cut_fails_safe_on_betaflight_timing`.

## 9. M3a changes (OSD, VTX, battery telemetry)

M3a wired three more live links, all riding the state/reply datagrams (never TCP, so the traffic is deterministic in lockstep): the battery as KISS ESC telemetry on UART3, Betaflight's OSD over MSP DisplayPort on UART4, and a SmartAudio v2.1 VTX on UART5. Design: `docs/superpowers/specs/2026-10-08-m3a-osd-vtx-design.md`.

### The reply datagram

After the 16-byte `servo_packet`, a reply on UDP 9002 carries a `u16` bytes-dropped count, then blocks of `[uart index (0-based)][len lo][len hi][bytes]` for the UARTs SITL wrote to — at most 512 bytes of blocks per reply (headers included; `REPLY_MAX` = 530 on the simulator side). SITL buffers each UART's TX bytes in a 4096-byte capture buffer (`TX_CAPTURE_SIZE`, added by `third_party/betaflight/tools/add_serial_out_datagram.py`) and drains them across exchanges; bytes that don't fit are dropped and counted in the `u16`. UART1 (MSP, tcp:5761, Configurator) is never captured (the capture branch checks the port id). A reply without a trailer is a SITL built without the capture and is treated as a legacy reply; a malformed trailer is a firmware error that stops the simulator. Betaflight reboots relaunch SITL and the taps keep working (Task 11's e2e).

### Probe results

- **DisplayPort capture (Task 3):** 176 checksum-valid `MSP_DISPLAYPORT` frames, 1,669 bytes, in 3 s of simulated time on UART4, 0 bytes dropped — the plan's binding numbers, reproduced on the first attempt.
- **ESC telemetry → battery (Task 5):** a fixed 24.6 V / 8 A / 120 mAh battery encoded as KISS frames on UART3 read back over `MSP_BATTERY_STATE` as `cells 6, 120 mAh, 8.00 A, 24.59 V` (24.59 V for 24.6 V sent, 6 cells detected).
- **SmartAudio VTX (Task 7):** with the `VtxModel` answering, `MSP_VTX_CONFIG` reported `VTXDEV_SMARTAUDIO` with the device ready (byte 7 = 1) at band 5 (R), channel 1, power index 2, frequency 5658 MHz — the shipped diff's band, channel and frequency. `MSP_SET_VTX_CONFIG` for R3 / level 3 moved the model to 5732 MHz / 600 mW, and pit mode switched on and off. Over one ~10 s scenario Betaflight sent 23 clean requests (18 GET_SETTINGS burst-reads every 200 ms until the first v2.1 reply, 3 SET_POWER, 1 SET_CHANNEL, 1 SET_MODE), never GET_PIT (v2.1 devices don't get it), 0 bad frames.
- **Ruling (Tasks 4 and 7): the diff's `set vtx_power = 2` against the quad's `default_power_index = 1` is not a defect** — it is the power-numbering offset below, confirmed live (`MSP_VTX_CONFIG` reports power index 2 for the model's index 1).
- Reboot survival (the OSD returns `present`, the VTX keeps its state) and OSD determinism (two identical lockstep loads draw identical frames) are e2e-tested against real Betaflight (Task 11, `python/tests/test_sitl_video.py`).

### Betaflight facts the design rests on

- **The craft name setting is `craft_name`** (not `name`); the shipped diff sets `craft_name = OpenFPV`.
- **The patch defines `USE_OSD_SD`** next to `USE_OSD`. Without it, `displayPortMspInit()` runs both of its fallback blocks and always leaves `vcd_video_system` at AUTO, so the OSD is a 13-row NTSC grid whatever the diff says (OSD elements at rows 14 and 15 piled up on row 12). With it defined, the diff's `vcd_video_system = PAL` takes effect and the grid is 30 x 16, which the shipped OSD element positions are laid out for.
- **No `vtxtable` in SITL**: the build has no `USE_VTX_TABLE`, and `MSP_VTX_CONFIG` reports "table available" = 0. Betaflight uses the factory bands (A, B, E, F, R, L — the same frequencies as `ofs-video`) and builds its power list from the dBm values the VTX reports in its settings reply, so the diff has no `vtxtable` lines.
- **A diff that enables SmartAudio without a VTX answering crashes Betaflight SITL** (SIGSEGV) when the OSD draws the VTX channel element — seen about 13 s into a run, at radio loss. The quad's config check rejects a diff that enables SmartAudio without a `[vtx]` section at load; with the model answering, long runs are stable.
- **The SmartAudio response CRC excludes the preamble**: a response is `AA 55 <code> <len> <payload> <crc8 over <code> <len> <payload>>`, polynomial 0xD5, initial 0, while a request's CRC covers all bytes before the CRC (from `vtx_smartaudio.c`).
- **The v2.1 power table skips the leading zero level**: Betaflight reads `saSupportedPowerValues[i] = buf[9 + i + 1]` ("+ 1 to skip the first power level"), so its power indexes are 1-based over the VTX's 0-based list — the shipped quad's `default_power_index = 1` (200 mW) is the diff's `vtx_power = 2`.
- **The ESC sensor sums current and consumption** across motors (`esc_sensor.c`; only voltage and RPM are averaged), so the simulator sends four identical frames that each carry a quarter of the pack current and consumed charge, not the pack current in one frame (the §5 caveat, addressed by the wiring).

### M0 open item

"SmartAudio reply side was not emulated" (§7) is now closed: the `VtxModel` answers, Betaflight accepts the replies, and MSP changes flow down into the model (Task 7's probe, Task 11's e2e).
