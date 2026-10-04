# M0 — Betaflight SITL Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Answer the six risks in spec §10 with evidence, and leave behind a findings document, a reproducible SITL build script, and (if needed) a Betaflight patch that M1–M3 build on.

**Architecture:** This is a spike. All harness code is **throwaway** Python under `spikes/m0/` (committed for reference, never imported by product code). The only durable outputs are `scripts/build-sitl.sh`, `third_party/betaflight/ofs-sitl.patch` (if needed), and `docs/research/sitl-interface.md`. The harness talks to a real Betaflight SITL over its native interfaces: UDP 9003 (state in), UDP 9002 (motors out), UDP 9004 (RC in), TCP 5760+n (UARTs).

**Tech Stack:** Betaflight 2026.6.2 (SITL target), GCC/make (Linux or WSL2 Ubuntu), Python ≥ 3.10 standard library only.

**Spec:** `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`

## Global Constraints

- License: GPL-3.0-or-later for code.
- Platforms (v1): Windows and Linux. On Windows, SITL runs under WSL2 unless Task 6 proves a native build works.
- Pinned firmware: Betaflight tag `2026.6.2`. Record the exact commit SHA in the findings.
- Spike code is throwaway: no tests, no packaging, never imported by `crates/` or `python/`.
- Any change to the user's machine configuration (installing WSL distributions, editing `%UserProfile%\.wslconfig`, `sudo apt install`) is done **by the user** after you ask; you give them the exact command.
- Conventions used throughout (from the spec and M1): world frame **NED**, body frame **FRD**, quaternions **body→world**. Betaflight Quad-X motor order: M1 rear-right, M2 front-right, M3 rear-left, M4 front-left; default spin M1 CW, M2 CCW, M3 CCW, M4 CW (seen from above). `servo_packet.motor_speed[0..3]` = M1..M4.

## Review Focus

- **SITL not reachable on Windows** (WSL NAT networking): harness must fail with a clear "no reply on UDP 9002" message, not hang. Covered by `recv_motors` timeouts in Task 2.
- **Stale SITL process holding ports** after a crash: Task 6 must establish a reliable cleanup command, and the findings must state it.
- **Gyro calibration never completes** because the harness sends noisy/moving data at boot: arming checks in Task 3 boot with perfectly still data first.
- **Sign convention silently wrong on one axis**: Task 3 tests each axis independently through motor responses, never only through displayed attitude.
- **Patched feature compiles but is dead at runtime** (e.g. CRSF parser never fed by TCP UART): Task 5 verifies every feature by reading the effect back over MSP, not by successful compilation.

---

### Task 1: Pinned SITL build

**Files:**
- Create: `.gitignore`
- Create: `scripts/build-sitl.sh`
- Create: `docs/research/sitl-interface.md` (skeleton; filled in later tasks)

**Interfaces:**
- Produces: `scripts/build-sitl.sh` — env `BF_TAG` (default `2026.6.2`), `BF_DIR` (default `$HOME/ofs/betaflight`), `OFS_SITL_FLAGS` (default `-DENABLE_GAZEBO_BRIDGE=0`), `OFS_SITL_PATCH` (optional absolute path to a patch). Output: `$BF_DIR/obj/main/betaflight_SITL.elf`.

- [ ] **Step 1: Check the build environment**

On Windows run in PowerShell: `wsl -l -v`. If there is no Ubuntu distribution, STOP and ask the user to run `wsl --install -d Ubuntu-24.04` themselves. Inside Linux/WSL run: `gcc --version; make --version; git --version; python3 --version`. If anything is missing, ask the user to run `sudo apt update && sudo apt install -y build-essential git python3`.

- [ ] **Step 2: Create `.gitignore`**

```gitignore
/target/
/.ofs-data/
/spikes/m0/runs/
__pycache__/
*.egg-info/
.venv/
```

- [ ] **Step 3: Write `scripts/build-sitl.sh`**

```bash
#!/usr/bin/env bash
# Builds the pinned Betaflight SITL used by Open FPV Sim.
# Usage: scripts/build-sitl.sh   (env: BF_TAG, BF_DIR, OFS_SITL_FLAGS, OFS_SITL_PATCH)
set -euo pipefail
BF_TAG="${BF_TAG:-2026.6.2}"
BF_DIR="${BF_DIR:-$HOME/ofs/betaflight}"
FLAGS="${OFS_SITL_FLAGS:--DENABLE_GAZEBO_BRIDGE=0}"

if [ ! -d "$BF_DIR/.git" ]; then
  git clone --branch "$BF_TAG" --depth 1 https://github.com/betaflight/betaflight "$BF_DIR"
fi
cd "$BF_DIR"
git fetch --depth 1 origin "refs/tags/$BF_TAG:refs/tags/$BF_TAG" 2>/dev/null || true
git checkout -q "$BF_TAG"
git checkout -q -- .            # drop any previously applied patch
if [ -n "${OFS_SITL_PATCH:-}" ]; then
  git apply "$OFS_SITL_PATCH"
fi
make TARGET=SITL EXTRA_FLAGS="$FLAGS" -j"$(nproc)"
echo "commit: $(git rev-parse HEAD)"
ls -l obj/main/betaflight_SITL.elf
```

- [ ] **Step 4: Run it**

Run (Linux/WSL, from the repo root; on Windows the repo is at `/mnt/c/dev/open-fpv-sim`): `bash scripts/build-sitl.sh`
Expected: ends with `commit: <sha>` and a listing of `obj/main/betaflight_SITL.elf`.
If `make TARGET=SITL` reports an unknown target, try `make SITL EXTRA_FLAGS=...` and update the script. If `EXTRA_FLAGS` is ignored (check with `grep -n EXTRA_FLAGS Makefile mk/*.mk`), record that; Task 4/5 will move the flags into the patch instead.

- [ ] **Step 5: Confirm the source matches the assumptions**

Run inside `$BF_DIR`:
```bash
grep -n "PORT_PWM\|PORT_STATE\|PORT_RC\|--config\|--ip" src/platform/SIMULATOR/sitl.c
grep -n "ENABLE_GAZEBO_BRIDGE\|SITL_ATTITUDE_DIRECT\|GYROPID_SYNC\|EEPROM_FILENAME\|#undef USE_" src/platform/SIMULATOR/target/SITL/target.h
grep -n -A12 "typedef struct" src/platform/SIMULATOR/target/SITL/target.h
cat src/platform/SIMULATOR/sitl_gyro.h | sed -n '/static inline/,/^}/p'
```
Expected: ports 9001–9004, `--config` and `--ip` options, `fdm_packet` = 18 doubles, `servo_packet` = 4 floats, `rc_packet` = double + 16×u16, legacy gyro map `(x, -y, -z)`.

- [ ] **Step 6: Start the findings document**

Create `docs/research/sitl-interface.md`:
```markdown
# Betaflight SITL Interface — M0 Findings

## 1. Pinned version and build
- Tag: 2026.6.2, commit: <sha from Step 4>
- Build command: <exact command that worked>
- Binary: obj/main/betaflight_SITL.elf
- Deviations from assumptions (Step 5): <none / list>

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
```

- [ ] **Step 7: Commit**

```bash
git add .gitignore scripts/build-sitl.sh docs/research/sitl-interface.md
git commit -m "spike(m0): pinned Betaflight SITL build script and findings skeleton"
```

---

### Task 2: Spike harness and smoke test

**Files:**
- Create: `spikes/m0/ofs_spike.py`
- Create: `spikes/m0/spike.diff`
- Create: `spikes/m0/t2_smoke.py`
- Modify: `docs/research/sitl-interface.md` (§2)

**Interfaces:**
- Consumes: `betaflight_SITL.elf` from Task 1; env `OFS_SITL_LAUNCH` = launch command line (Linux: `/home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`; Windows: `wsl.exe -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`).
- Produces: helpers used by Tasks 3–6: `SitlLink`, `Msp`, `fdm_legacy`, `rc`, `attitude_ned`, `specific_force_frd`, `launch_sitl`, `apply_config`, `crsf_rc_frame`, `kiss_frame`, MSP command constants.

- [ ] **Step 1: Write the harness helpers**

`spikes/m0/ofs_spike.py`:
```python
"""THROWAWAY M0 spike helpers: SITL UDP packets, MSP v1 over TCP, CRSF and KISS frames.

Conventions: world NED, body FRD, quaternions (w, x, y, z) body->world.
"""
import math
import os
import shlex
import shutil
import socket
import struct
import subprocess
import time

PORT_PWM_RAW, PORT_PWM, PORT_STATE, PORT_RC = 9001, 9002, 9003, 9004
FDM = struct.Struct("<d3d3d4d3d3dd")  # 144 bytes: fdm_packet
RC = struct.Struct("<d16H")           # 40 bytes: rc_packet
SERVO = struct.Struct("<4f")          # 16 bytes: servo_packet
G = 9.80665


# ---------- quaternion helpers ----------
def qmul(a, b):
    aw, ax, ay, az = a
    bw, bx, by, bz = b
    return (aw * bw - ax * bx - ay * by - az * bz,
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw)


def qconj(q):
    return (q[0], -q[1], -q[2], -q[3])


def qaxis(axis, angle):
    s = math.sin(angle / 2)
    return (math.cos(angle / 2), axis[0] * s, axis[1] * s, axis[2] * s)


def qrot(q, v):
    return qmul(qmul(q, (0.0, *v)), qconj(q))[1:]


QX180 = qaxis((1, 0, 0), math.pi)


def attitude_ned(roll_deg=0.0, pitch_deg=0.0, yaw_deg=0.0):
    """FRD body -> NED world, Z-Y-X (yaw, pitch, roll) order."""
    qz = qaxis((0, 0, 1), math.radians(yaw_deg))
    qy = qaxis((0, 1, 0), math.radians(pitch_deg))
    qx = qaxis((1, 0, 0), math.radians(roll_deg))
    return qmul(qmul(qz, qy), qx)


def specific_force_frd(q_ned, accel_ned=(0.0, 0.0, 0.0)):
    """What an accelerometer measures: R^T (a - g), g = (0, 0, +G) in NED."""
    return qrot(qconj(q_ned), (accel_ned[0], accel_ned[1], accel_ned[2] - G))


# ---------- SITL packets (candidate legacy-bridge mapping; Task 3 verifies) ----------
GYRO_SIGN = [1.0, 1.0, -1.0]  # applied to FRD body rates before sending


def fdm_legacy(t, gyro_frd, accel_frd, q_ned, vel_ned=(0.0, 0.0, 0.0),
               alt_m=0.0, pressure_pa=101325.0, lat=50.85, lon=4.35):
    """fdm_packet for SITL built with -DENABLE_GAZEBO_BRIDGE=0."""
    gyro = tuple(s * g for s, g in zip(GYRO_SIGN, gyro_frd))
    q = qmul(qmul(QX180, q_ned), QX180)  # FRD->NED  ==>  FLU->NWU
    if q[0] < 0:
        q = tuple(-c for c in q)
    vel_enu = (vel_ned[1], vel_ned[0], -vel_ned[2])
    return FDM.pack(t, *gyro, *accel_frd, *q, *vel_enu, lon, lat, alt_m, pressure_pa)


def rc(t, roll=0.0, pitch=0.0, throttle=0.0, yaw=0.0, aux=(-1.0, -1.0, -1.0, -1.0)):
    """rc_packet, channel order AETR + AUX1..4. Sticks in [-1, 1], throttle in [0, 1]."""
    def us(x):
        return int(round(1500 + 500 * max(-1.0, min(1.0, x))))
    ch = [us(roll), us(pitch), int(round(1000 + 1000 * max(0.0, min(1.0, throttle)))), us(yaw)]
    ch += [us(a) for a in aux]
    ch += [1500] * (16 - len(ch))
    return RC.pack(t, *ch[:16])


class SitlLink:
    def __init__(self, host="127.0.0.1"):
        self.host = host
        self.pwm = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.pwm.bind(("127.0.0.1", PORT_PWM))
        self.tx = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

    def send(self, fdm_pkt, rc_pkt):
        for pkt, port in ((fdm_pkt, PORT_STATE), (rc_pkt, PORT_RC)):
            try:
                self.tx.sendto(pkt, (self.host, port))
            except ConnectionResetError:  # Windows reports earlier ICMP errors here
                pass

    def recv_motors(self, timeout=0.5):
        self.pwm.settimeout(timeout)
        try:
            data, _ = self.pwm.recvfrom(64)
        except (socket.timeout, ConnectionResetError):
            return None
        return SERVO.unpack(data[:16]) if len(data) >= 16 else None

    def close(self):
        self.pwm.close()
        self.tx.close()


# ---------- MSP v1 over TCP ----------
MSP_API_VERSION, MSP_STATUS, MSP_RAW_IMU, MSP_RC = 1, 101, 102, 105
MSP_ATTITUDE, MSP_ANALOG, MSP_VTX_CONFIG = 108, 110, 88
MSP_BATTERY_STATE, MSP_MOTOR_TELEMETRY, MSP_DISPLAYPORT = 130, 139, 182


class Msp:
    def __init__(self, port=5761, host="127.0.0.1", timeout=2.0):
        self.s = socket.create_connection((host, port), timeout=timeout)

    def request(self, cmd, payload=b""):
        n = len(payload)
        ck = n ^ cmd
        for b in payload:
            ck ^= b
        self.s.sendall(b"$M<" + bytes([n, cmd]) + payload + bytes([ck]))
        return self.read_frame(expect=cmd)[1]

    def read_frame(self, expect=None):
        while True:
            if self._read(1) != b"$":
                continue
            if self._read(2) not in (b"M>", b"M!"):
                continue
            n, cmd = self._read(2)
            payload = self._read(n)
            self._read(1)
            if expect is None or cmd == expect:
                return cmd, payload

    def _read(self, n):
        out = b""
        while len(out) < n:
            chunk = self.s.recv(n - len(out))
            if not chunk:
                raise ConnectionError("MSP connection closed")
            out += chunk
        return out


def msp_attitude(msp):
    r, p, y = struct.unpack("<hhh", msp.request(MSP_ATTITUDE)[:6])
    return r / 10.0, p / 10.0, float(y)


def msp_raw_imu(msp):
    v = struct.unpack("<9h", msp.request(MSP_RAW_IMU)[:18])
    return v[0:3], v[3:6]


def msp_rc(msp):
    d = msp.request(MSP_RC)
    return struct.unpack(f"<{len(d) // 2}H", d)


# ---------- CRSF ----------
def crc8(data, poly):
    crc = 0
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = ((crc << 1) ^ poly) & 0xFF if crc & 0x80 else (crc << 1) & 0xFF
    return crc


def crsf_rc_frame(channels_us):
    vals = [int(round((us - 1500) * 8 / 5 + 992)) for us in channels_us]
    vals += [992] * (16 - len(vals))
    bits = 0
    for i, v in enumerate(vals[:16]):
        bits |= (v & 0x7FF) << (11 * i)
    body = bytes([0x16]) + bits.to_bytes(22, "little")
    return bytes([0xC8, len(body) + 1]) + body + bytes([crc8(body, 0xD5)])


# ---------- KISS ESC telemetry (10 bytes, big-endian, CRC-8 poly 0x07) ----------
def kiss_frame(temp_c, voltage_v, current_a, consumption_mah, erpm):
    body = struct.pack(">BHHHH", int(temp_c), int(voltage_v * 100), int(current_a * 100),
                       int(consumption_mah), int(erpm / 100))
    return body + bytes([crc8(body, 0x07)])


# ---------- process control ----------
def _launch_argv(extra=()):
    return shlex.split(os.environ["OFS_SITL_LAUNCH"]) + list(extra)


def apply_config(workdir, diff_path, timeout=30):
    """First boot: `betaflight_SITL.elf --config betaflight.diff` writes eeprom.bin and exits."""
    os.makedirs(workdir, exist_ok=True)
    shutil.copy(diff_path, os.path.join(workdir, "betaflight.diff"))
    out = subprocess.run(_launch_argv(["--config", "betaflight.diff"]), cwd=workdir,
                         capture_output=True, text=True, timeout=timeout)
    print(out.stdout[-2000:], out.stderr[-2000:])
    assert os.path.exists(os.path.join(workdir, "eeprom.bin")), "eeprom.bin was not written"


def launch_sitl(workdir, extra=()):
    log = open(os.path.join(workdir, "sitl.log"), "w")
    proc = subprocess.Popen(_launch_argv(extra), cwd=workdir, stdout=log, stderr=subprocess.STDOUT)
    deadline = time.time() + 15
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"SITL exited with {proc.returncode}; see {workdir}/sitl.log")
        try:
            socket.create_connection(("127.0.0.1", 5761), timeout=0.5).close()
            return proc
        except OSError:
            time.sleep(0.2)
    proc.kill()
    raise RuntimeError(f"SITL did not open tcp:5761 within 15 s; see {workdir}/sitl.log")


def stop_sitl(proc):
    proc.terminate()
    try:
        proc.wait(5)
    except subprocess.TimeoutExpired:
        proc.kill()
    cleanup = os.environ.get("OFS_SITL_CLEANUP")
    if cleanup:
        subprocess.run(shlex.split(cleanup), capture_output=True)


class Flight:
    """Steps a still or scripted vehicle against SITL at a fixed exchange rate."""

    def __init__(self, link, dt=0.001):
        self.link, self.dt, self.t = link, dt, 0.0
        self.replies = self.sent = self.misses = 0
        self.last = (0.0, 0.0, 0.0, 0.0)

    def run(self, seconds, q=None, gyro=(0.0, 0.0, 0.0), sticks=None, timeout=0.5):
        q = q or attitude_ned()
        sticks = sticks or {}
        for _ in range(int(round(seconds / self.dt))):
            self.t += self.dt
            self.link.send(fdm_legacy(self.t, gyro, specific_force_frd(q), q), rc(self.t, **sticks))
            self.sent += 1
            m = self.link.recv_motors(timeout)
            if m is None:
                self.misses += 1
                if self.misses >= 20:
                    raise RuntimeError("no reply on UDP 9002 for 20 consecutive packets: is SITL running "
                                       "and sending to 127.0.0.1:9002? See sitl.log in the run directory")
                continue
            self.misses = 0
            self.replies += 1
            self.last = m
        return self.last
```

- [ ] **Step 2: Write the spike Betaflight config**

`spikes/m0/spike.diff`:
```
# M0 spike config, applied once with --config
feature -GPS
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
```
(`aux 0 0 0 …` = ARM on AUX1 high; `aux 1 1 1 …` = ANGLE on AUX2 high.)

- [ ] **Step 3: Write the smoke test**

`spikes/m0/t2_smoke.py`:
```python
"""THROWAWAY M0 Task 2: SITL boots, answers MSP over TCP, replies to FDM packets."""
import os
import time
from ofs_spike import *

work = os.path.abspath("runs/t2")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "spike.diff")
proc = launch_sitl(work)
link = SitlLink()
try:
    f = Flight(link)
    t0 = time.time()
    f.run(3.0)
    wall = time.time() - t0
    print(f"replies {f.replies}/{f.sent}, sim 3.0 s in {wall:.2f} s wall")
    msp = Msp()
    print("MSP_API_VERSION:", msp.request(MSP_API_VERSION).hex())
    print("attitude (roll, pitch, yaw):", msp_attitude(msp))
finally:
    link.close()
    stop_sitl(proc)
```

- [ ] **Step 4: Run it**

Run (from `spikes/m0`, with `OFS_SITL_LAUNCH` set): `python t2_smoke.py`
Expected: replies close to 3000/3000, an MSP API version hex string such as `00012f`, attitude near `(0.0, 0.0, 0.0)`.
If replies are 0, check `runs/t2/sitl.log` for the UDP bind lines and confirm SITL sends to 127.0.0.1:9002.

- [ ] **Step 5: Configurator check (manual, user)**

Ask the user to start SITL (`python -c "from ofs_spike import *; import time; p=launch_sitl('runs/t2'); time.sleep(600)"`), open Betaflight Configurator (desktop) and connect to `tcp://127.0.0.1:5761`. Record whether it connects, and also whether the web Betaflight App can connect (expected: it cannot use raw TCP; if so, note that a WebSocket↔TCP bridge is needed for it in M2).

- [ ] **Step 6: Record and commit**

Fill §2 of the findings: ports, packet sizes, reply ratio, wall time, Configurator result. Then:
```bash
git add spikes/m0/ofs_spike.py spikes/m0/spike.diff spikes/m0/t2_smoke.py docs/research/sitl-interface.md
git commit -m "spike(m0): SITL harness and smoke test"
```

---

### Task 3: Sign conventions via motor responses

**Files:**
- Create: `spikes/m0/t3_conventions.py`
- Modify: `spikes/m0/ofs_spike.py` (`GYRO_SIGN`, accel mapping, only if a check fails)
- Modify: `docs/research/sitl-interface.md` (§3)

**Interfaces:**
- Consumes: Task 2 helpers.
- Produces: the confirmed FRD→`fdm_packet` mapping table that M1 Task 10 implements, and the minimal arming diff that M1 Task 8 ships.

- [ ] **Step 1: Write the convention test**

`spikes/m0/t3_conventions.py`:
```python
"""THROWAWAY M0 Task 3: verify gyro/accel/attitude conventions through motor responses.

Ground truth is physics, not displays: a rate on an axis must make the PID push
back against it, and a stick must push the matching motors.
Motor index: 0=M1 RR, 1=M2 FR, 2=M3 RL, 3=M4 FL. CW props: M1, M4.
"""
import os
from ofs_spike import *

RIGHT, LEFT, REAR, FRONT, CW, CCW = (0, 1), (2, 3), (0, 2), (1, 3), (0, 3), (1, 2)


def bias(m, group):
    other = [i for i in range(4) if i not in group]
    return sum(m[i] for i in group) - sum(m[i] for i in other)


def check(name, base, m, group):
    d = bias(m, group) - bias(base, group)
    ok = d > 0.01
    print(f"{'PASS' if ok else 'FAIL'}  {name:42s} delta={d:+.4f}  motors={['%.3f' % x for x in m]}")
    return ok


work = os.path.abspath("runs/t3")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "spike.diff")
proc = launch_sitl(work)
link = SitlLink()
results = []
try:
    f = Flight(link)
    disarmed = dict(throttle=0.0, aux=(-1, -1, -1, -1))
    acro = dict(throttle=0.5, aux=(1, -1, -1, -1))
    angle = dict(throttle=0.5, aux=(1, 1, -1, -1))
    f.run(5.0, sticks=disarmed)                       # boot + gyro calibration, perfectly still
    f.run(1.0, sticks=dict(throttle=0.0, aux=(1, -1, -1, -1)))  # arm
    base = f.run(1.0, sticks=acro)
    print("armed baseline:", base, " (all zero => not armed: read runs/t3/sitl.log 'Arming disabled')")

    # Rate disturbances: PID must oppose them. Short pulses so I-term stays small.
    results.append(check("gyro +x FRD (rolling right) -> roll left", base, f.run(0.03, gyro=(1, 0, 0), sticks=acro), RIGHT))
    f.run(0.5, sticks=acro)
    results.append(check("gyro +y FRD (nose rising) -> pitch down", base, f.run(0.03, gyro=(0, 1, 0), sticks=acro), REAR))
    f.run(0.5, sticks=acro)
    results.append(check("gyro +z FRD (yawing right) -> yaw left", base, f.run(0.03, gyro=(0, 0, 1), sticks=acro), CW))
    f.run(0.5, sticks=acro)

    # Sticks.
    results.append(check("roll stick right -> left motors up", base, f.run(0.03, sticks=dict(acro, roll=0.3)), LEFT))
    f.run(0.5, sticks=acro)
    results.append(check("pitch stick forward -> rear motors up", base, f.run(0.03, sticks=dict(acro, pitch=0.3)), REAR))
    f.run(0.5, sticks=acro)
    results.append(check("yaw stick right -> CCW props up", base, f.run(0.03, sticks=dict(acro, yaw=0.3)), CCW))
    f.run(0.5, sticks=acro)

    # Angle mode: estimator must read tilt from accel and level the craft.
    base_angle = f.run(2.0, sticks=angle)
    results.append(check("angle: rolled right 20 deg -> roll left", base_angle,
                         f.run(1.5, q=attitude_ned(roll_deg=20), sticks=angle), RIGHT))
    f.run(2.0, sticks=angle)
    results.append(check("angle: nose up 20 deg -> pitch down", base_angle,
                         f.run(1.5, q=attitude_ned(pitch_deg=20), sticks=angle), REAR))

    msp = Msp()
    f.run(2.0, q=attitude_ned(roll_deg=20, pitch_deg=10), sticks=disarmed)
    print("MSP_ATTITUDE for FRD roll=+20 (right), pitch=+10 (nose up):", msp_attitude(msp))
    print("MSP_RAW_IMU (acc, gyro):", msp_raw_imu(msp))
finally:
    link.close()
    stop_sitl(proc)
print(f"{sum(results)}/{len(results)} checks passed")
```

- [ ] **Step 2: Run it**

Run: `python t3_conventions.py`
Expected: `8/8 checks passed`.
If the baseline motors are all zero, the quad did not arm: read the `Arming disabled:` flags in `runs/t3/sitl.log`, add the CLI setting that clears each flag to `spike.diff`, delete `runs/t3/eeprom.bin`, and rerun. Record every setting you had to add.
If a `gyro` check fails for one axis, flip that axis in `GYRO_SIGN` and rerun. If an angle check fails while the gyro checks pass, the accel mapping is wrong: try negating the failing axis of `accel_frd` inside `fdm_legacy` and rerun.

- [ ] **Step 3: Record and commit**

Fill §3 of the findings with: the final `GYRO_SIGN`, the accel mapping, the quaternion mapping (`q_send = Rx(π)·q_ned·Rx(π)`, w ≥ 0), the 8 check results, the MSP_ATTITUDE readout and Betaflight's sign convention it implies, and the **final minimal arming diff** (full text). Then:
```bash
git add spikes/m0/ docs/research/sitl-interface.md
git commit -m "spike(m0): verify SITL sign conventions through motor responses"
```

---

### Task 4: Timing and lockstep

**Files:**
- Create: `spikes/m0/t4_lockstep.py`
- Modify: `docs/research/sitl-interface.md` (§4)

**Interfaces:**
- Consumes: Task 2–3 helpers; two SITL builds.
- Produces: recommendation for M1 Task 11: build flags, `exchange_hz` default, reply timeout, and whether runs are bit-reproducible.

- [ ] **Step 1: Build the GYROPID_SYNC variant**

Run: `BF_DIR=$HOME/ofs/betaflight-sync OFS_SITL_FLAGS="-DENABLE_GAZEBO_BRIDGE=0 -DENABLE_SIMULATOR_GYROPID_SYNC=1" bash scripts/build-sitl.sh`
Expected: a second binary at `$HOME/ofs/betaflight-sync/obj/main/betaflight_SITL.elf`. If `EXTRA_FLAGS` was found to be ignored in Task 1, instead uncomment `//#define ENABLE_SIMULATOR_GYROPID_SYNC 1` in that tree's `target.h` and rebuild with `make TARGET=SITL`.

- [ ] **Step 2: Write the timing probe**

`spikes/m0/t4_lockstep.py`:
```python
"""THROWAWAY M0 Task 4: reply ratio, real-time factor, and reproducibility at several exchange rates.

Usage: python t4_lockstep.py <label>   (OFS_SITL_LAUNCH selects the build under test)
"""
import json
import math
import os
import shutil
import sys
import time
from ofs_spike import *

label = sys.argv[1]
seed_dir = os.path.abspath("runs/t3")  # eeprom.bin with the working arming config
results = {}
for hz in (500, 1000, 2000, 4000):
    traces = []
    for run in range(2):
        work = os.path.abspath(f"runs/t4/{label}-{hz}-{run}")
        shutil.rmtree(work, ignore_errors=True)
        os.makedirs(work)
        shutil.copy(os.path.join(seed_dir, "eeprom.bin"), work)
        proc = launch_sitl(work)
        link = SitlLink()
        try:
            f = Flight(link, dt=1.0 / hz)
            f.run(4.0, sticks=dict(throttle=0.0, aux=(-1, -1, -1, -1)))
            f.run(1.0, sticks=dict(throttle=0.0, aux=(1, -1, -1, -1)))
            trace = []
            t0, sent0, rep0 = time.time(), f.sent, f.replies
            for k in range(int(3.0 * hz)):
                g = 0.5 * math.sin(2 * math.pi * 3.0 * k / hz)
                f.run(1.0 / hz, gyro=(g, 0.0, 0.0), sticks=dict(throttle=0.5, aux=(1, -1, -1, -1)))
                trace.append(list(f.last))
            wall = time.time() - t0
            traces.append(trace)
            results.setdefault(hz, []).append({
                "reply_ratio": (f.replies - rep0) / (f.sent - sent0),
                "real_time_factor": 3.0 / wall,
            })
        finally:
            link.close()
            stop_sitl(proc)
    identical = sum(a == b for a, b in zip(*traces))
    max_diff = max(max(abs(x - y) for x, y in zip(a, b)) for a, b in zip(*traces))
    results[hz].append({"identical_steps": identical, "steps": len(traces[0]), "max_abs_diff": max_diff})
    print(hz, json.dumps(results[hz]))
os.makedirs("runs/t4", exist_ok=True)
json.dump(results, open(f"runs/t4/{label}.json", "w"), indent=2)
```

- [ ] **Step 3: Run both builds**

Run: `OFS_SITL_LAUNCH=<stock binary> python t4_lockstep.py stock` then `OFS_SITL_LAUNCH=<sync binary> python t4_lockstep.py sync`
Expected: per rate, a reply ratio, a real-time factor, and identical-step counts. No expected values: these are measurements.

- [ ] **Step 4: Record and commit**

Fill §4 of the findings with a table (build × rate → reply ratio, real-time factor, identical steps / steps, max diff) and a recommendation: which build M1 uses, the default `exchange_hz`, and whether the spec's "identical logs" determinism can include SITL (if not, state that M1's determinism test covers the Rust core only and that SITL runs are reproducible within the measured max diff). Then:
```bash
git add spikes/m0/t4_lockstep.py docs/research/sitl-interface.md
git commit -m "spike(m0): measure SITL lockstep timing and reproducibility"
```

---

### Task 5: Feature build — CRSF, ESC sensor, MSP DisplayPort, VTX, Blackbox

**Files:**
- Create: `third_party/betaflight/ofs-sitl.patch`
- Create: `spikes/m0/feature.diff`
- Create: `spikes/m0/t5_features.py`
- Modify: `docs/research/sitl-interface.md` (§5)

**Interfaces:**
- Consumes: Task 1 build script (`OFS_SITL_PATCH`), Task 2 helpers.
- Produces: the patch M2–M3 build on, and a yes/no answer per feature.

- [ ] **Step 1: Re-enable the features in a scratch tree**

In `$HOME/ofs/betaflight-feat` (clone via `BF_DIR=$HOME/ofs/betaflight-feat bash scripts/build-sitl.sh` first), edit `src/platform/SIMULATOR/target/SITL/target.h` and delete these `#undef` lines: `USE_SERIALRX`, `USE_SERIALRX_CRSF`, `USE_TELEMETRY_CRSF`, `USE_OSD`, `USE_VTX_COMMON`, `USE_VTX_CONTROL`, `USE_VTX_SMARTAUDIO`, `USE_VTX_TRAMP`. Then add, after the `#undef` block:
```c
#define USE_ESC_SENSOR
#define USE_MSP_DISPLAYPORT
#define USE_OSD
```
Build: `make TARGET=SITL EXTRA_FLAGS="-DENABLE_GAZEBO_BRIDGE=0"`. Fix compile errors with the smallest change in SITL-specific files only (`src/platform/SIMULATOR/**`); time-box this step to one working day. Record each error and fix. If a feature cannot be made to compile in the time-box, put its `#undef` back and record it as **No**.

- [ ] **Step 2: Save the patch**

Run in `$HOME/ofs/betaflight-feat`: `git diff > /mnt/c/dev/open-fpv-sim/third_party/betaflight/ofs-sitl.patch` (Linux: the repo path). Verify it applies cleanly to a fresh tree: `BF_DIR=$HOME/ofs/betaflight-check OFS_SITL_PATCH=$PWD/../../third_party/betaflight/ofs-sitl.patch bash scripts/build-sitl.sh` (run from the repo root with an absolute patch path).

- [ ] **Step 3: Write the feature config**

`spikes/m0/feature.diff` (append the final arming settings from Task 3):
```
feature -GPS
feature RX_SERIAL
feature ESC_SENSOR
feature OSD
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
serial 1 64 115200 57600 0 115200
serial 2 1024 115200 57600 0 115200
serial 3 1 115200 57600 0 115200
serial 4 2048 115200 57600 0 115200
set serialrx_provider = CRSF
set battery_meter = ESC
set current_meter = ESC
set osd_displayport_device = MSP
set displayport_msp_serial = 3
set blackbox_device = VIRTUAL
```
UART mapping: `serial 1` = UART2 = tcp:5762 (CRSF RX), `serial 2` = UART3 = tcp:5763 (ESC sensor), `serial 3` = UART4 = tcp:5764 (MSP DisplayPort), `serial 4` = UART5 = tcp:5765 (SmartAudio). If a setting name is rejected by `--config`, check `get <partial name>` in the CLI and record the correct name.

- [ ] **Step 4: Write the feature probe**

`spikes/m0/t5_features.py`:
```python
"""THROWAWAY M0 Task 5: prove each re-enabled feature works at runtime."""
import os
import socket
import threading
import time
from ofs_spike import *

work = os.path.abspath("runs/t5")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "feature.diff")
proc = launch_sitl(work)
link = SitlLink()
captured = {5764: bytearray(), 5765: bytearray(), 5762: bytearray()}
socks = {}


def reader(port):
    s = socket.create_connection(("127.0.0.1", port))
    socks[port] = s
    while True:
        try:
            d = s.recv(4096)
        except OSError:
            return
        if not d:
            return
        captured.setdefault(port, bytearray()).extend(d)


try:
    for port in (5762, 5764, 5765):
        threading.Thread(target=reader, args=(port,), daemon=True).start()
    esc = socket.create_connection(("127.0.0.1", 5763))
    time.sleep(0.5)
    f = Flight(link)
    for i in range(3000):  # 3 s: CRSF at 500 Hz, KISS telemetry at 1 kHz round-robin
        f.run(0.001)
        if i % 2 == 0:
            socks[5762].sendall(crsf_rc_frame([1600, 1400, 1100, 1500, 1000, 1000, 1500, 1500]))
        esc.sendall(kiss_frame(35, 24.6, 3.2, 120, 12000))
    msp = Msp()
    print("CRSF -> MSP_RC (expect ~1600 1400 1100 1500 ...):", msp_rc(msp)[:8])
    print("CRSF telemetry bytes back on 5762:", len(captured[5762]), captured[5762][:32].hex())
    print("ESC sensor -> MSP_BATTERY_STATE:", msp.request(MSP_BATTERY_STATE).hex())
    print("ESC sensor -> MSP_MOTOR_TELEMETRY:", msp.request(MSP_MOTOR_TELEMETRY).hex())
    dp = bytes(captured[5764])
    print("DisplayPort bytes:", len(dp), "MSP 182 frames:", dp.count(b"$M>") , dp[:48].hex())
    sa = bytes(captured[5765])
    print("SmartAudio bytes:", len(sa), sa[:32].hex(), "(expect frames starting 00? aa 55)")
    print("MSP_VTX_CONFIG:", msp.request(MSP_VTX_CONFIG).hex())
finally:
    link.close()
    stop_sitl(proc)
print("Blackbox: look for new files in", work, "->", os.listdir(work))
```

- [ ] **Step 5: Run and interpret**

Run (with `OFS_SITL_LAUNCH` pointing at the patched build): `python t5_features.py`
Expected per feature, record **Yes / No / Partial** with the evidence line:
- CRSF RX: MSP_RC shows ~1600/1400/1100/1500.
- CRSF telemetry: bytes starting `c8` (or `ea`) appear on 5762.
- ESC sensor: battery voltage ≈ 24.6 V in MSP_BATTERY_STATE (bytes 3–4 are voltage in 0.01 V on current firmware; decode per `src/main/msp/msp.c` `MSP_BATTERY_STATE`), and RPM in MSP_MOTOR_TELEMETRY.
- MSP DisplayPort: `$M>` frames with command byte `0xb6` (182).
- SmartAudio: frames containing `aa 55`. If seen, write one SmartAudio v2 GET_SETTINGS response by hand (per the SmartAudio spec) and check MSP_VTX_CONFIG changes; if not attempted, record "requests seen, response untested".
- Blackbox: a log file appears in the work dir after arming (arm for 2 s by adding a `Flight.run` with AUX1 high); open it with Blackbox Explorer or `blackbox_decode`.

- [ ] **Step 6: Record and commit**

Fill §5 of the findings (per-feature answer, evidence, compile fixes, exact CLI settings that worked). Then:
```bash
git add third_party/betaflight/ofs-sitl.patch spikes/m0/feature.diff spikes/m0/t5_features.py docs/research/sitl-interface.md
git commit -m "spike(m0): patched SITL feature build and runtime checks"
```

---

### Task 6: Windows / WSL2 networking and process lifecycle

**Files:**
- Modify: `docs/research/sitl-interface.md` (§6)

**Interfaces:**
- Produces: the exact `OFS_SITL_LAUNCH` and `OFS_SITL_CLEANUP` values for Windows that M1 Task 11 documents.

- [ ] **Step 1: Check networking mode**

Run in PowerShell: `Get-Content "$env:UserProfile\.wslconfig" -ErrorAction SilentlyContinue`. If it does not contain `networkingMode=mirrored` under `[wsl2]`, ask the user whether they agree to add it (it changes how all their WSL distributions network), and if so ask them to add:
```ini
[wsl2]
networkingMode=mirrored
```
then run `wsl --shutdown`.

- [ ] **Step 2: Run the smoke test from Windows Python**

With `OFS_SITL_LAUNCH="wsl.exe -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"`, run `python t2_smoke.py` from Windows (not WSL).
Expected: same results as Task 2. If replies are 0 under NAT networking, try adding `--ip <Windows host IP as seen from WSL>` (from `wsl -e sh -c "ip route | awk '/default/ {print \$3}'"`) to the launch command and record whether that works.

- [ ] **Step 3: Check that stopping wsl.exe stops SITL**

Run: `python -c "from ofs_spike import *; import os; p=launch_sitl(os.path.abspath('runs/t2')); p.kill(); p.wait()"` then `wsl -e pgrep -fa betaflight_SITL`.
Expected: no output. If a process is still listed, set `OFS_SITL_CLEANUP="wsl.exe -e pkill -f betaflight_SITL"`, verify `stop_sitl` leaves no process, and record it as required.

- [ ] **Step 4: Native Windows build attempt (time-boxed to 1 hour)**

Check whether the SITL target builds under MSYS2/MinGW (it uses pthreads, `clock_gettime`, `nanosleep`, BSD sockets). Record the result; WSL2 remains the supported path if it fails.

- [ ] **Step 5: Record and commit**

Fill §6 (networking mode, launch and cleanup values, `--ip` needs, native build result).
```bash
git add docs/research/sitl-interface.md
git commit -m "spike(m0): Windows WSL2 networking and SITL lifecycle findings"
```

---

### Task 7: Answers, recommendations, and checkpoint

**Files:**
- Modify: `docs/research/sitl-interface.md` (§7)
- Modify: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md` (§10)

- [ ] **Step 1: Write §7**

For each spec §10 risk (1–6): **Answer** (Yes / No / Partial), **Evidence** (pointer to §2–§6), **Recommendation** (what M1/M2/M3 do). Also list the M1 plan assumptions A1–A6 (from `docs/superpowers/plans/2026-10-04-m1-headless-core.md`, section "Assumptions from M0") with **Confirmed** or **Refuted → change needed**.

- [ ] **Step 2: Update the spec**

Under each item in spec §10, add a line `**M0 result:** <one sentence> (see docs/research/sitl-interface.md §N)`.

- [ ] **Step 3: Commit**

```bash
git add docs/research/sitl-interface.md docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md
git commit -m "spike(m0): findings and answers to spec risks"
```

- [ ] **Step 4: Checkpoint with the user**

STOP. Present the §7 summary to the user. If any M1 assumption was refuted, revise the affected M1 tasks (10–13) before they are executed.
