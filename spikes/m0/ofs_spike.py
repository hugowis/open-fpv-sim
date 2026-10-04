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
GYRO_SIGN = [1.0, 1.0, 1.0]    # applied to FRD body rates before sending (Task 3 result)
ACCEL_SIGN = [-1.0, 1.0, 1.0]  # applied to FRD specific force before sending (Task 3 result)


def fdm_legacy(t, gyro_frd, accel_frd, q_ned, vel_ned=(0.0, 0.0, 0.0),
               alt_m=0.0, pressure_pa=101325.0, lat=50.85, lon=4.35):
    """fdm_packet for SITL built with -DENABLE_GAZEBO_BRIDGE=0."""
    gyro = tuple(s * g for s, g in zip(GYRO_SIGN, gyro_frd))
    accel_frd = tuple(s * a for s, a in zip(ACCEL_SIGN, accel_frd))
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
    """MSP v1 client. With external SITL time, Betaflight only services MSP while simulated time
    advances, so pass `pump` (e.g. Flight.hold) to keep sending state packets during a request."""

    def __init__(self, port=5761, host="127.0.0.1", timeout=2.0, pump=None):
        self.s = socket.create_connection((host, port), timeout=timeout)
        self.pump = pump

    def request(self, cmd, payload=b""):
        n = len(payload)
        ck = n ^ cmd
        for b in payload:
            ck ^= b
        self.s.sendall(b"$M<" + bytes([n, cmd]) + payload + bytes([ck]))
        if self.pump:
            self.pump()
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


ARMING_FLAGS = ["NO_GYRO", "FAILSAFE", "RX_FAILSAFE", "NOT_DISARMED", "BOXFAILSAFE", "RUNAWAY_TAKEOFF",
                "CRASH_DETECTED", "THROTTLE", "ANGLE", "BOOT_GRACE_TIME", "NOPREARM", "LOAD", "CALIBRATING",
                "CLI", "CMS_MENU", "BST", "MSP", "PARALYZE", "GPS", "RESC", "DSHOT_TELEM", "REBOOT_REQUIRED",
                "DSHOT_BITBANG", "ACC_CALIBRATION", "MOTOR_PROTOCOL", "CRASHFLIP", "ALTHOLD", "POSHOLD",
                "AUTOPILOT", "ARM_SWITCH"]


def msp_arming_disabled(msp):
    """Names of active arming-disable flags, from MSP_STATUS (layout per src/main/msp/msp.c)."""
    d = msp.request(MSP_STATUS)
    off = 16 + d[15]
    count = d[off]
    flags = struct.unpack_from("<I", d, off + 1)[0]
    return [ARMING_FLAGS[i] if i < len(ARMING_FLAGS) else f"bit{i}" for i in range(count) if flags >> i & 1]


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
        self._kw = {}

    def hold(self, seconds=0.05):
        """Keep sending the most recent state for `seconds` (used to pump MSP)."""
        return self.run(seconds, **self._kw)

    def run(self, seconds, q=None, gyro=(0.0, 0.0, 0.0), sticks=None, timeout=0.5):
        q = q or attitude_ned()
        sticks = sticks or {}
        self._kw = dict(q=q, gyro=gyro, sticks=sticks)
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
