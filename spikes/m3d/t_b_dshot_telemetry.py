"""THROWAWAY M3d spike B: per-motor eRPM through the emulated DShot telemetry reaches Betaflight.

Build: USE_DSHOT_TELEMETRY + USE_RPM_FILTER without USE_DSHOT; sitl.c stub owns the dshot.c API and
feeds synthetic eRPM = throttle x 120000 x (1 + 0.25 i) per motor in pwmCompleteMotorUpdate.
Proof: MSP_MOTOR_TELEMETRY shows four DISTINCT rpm values tracking throttle; arming and motors
still work (the RPM filter now also initializes, gated on the stub's useDshotTelemetry = true).
"""
import os
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0"))
from ofs_spike import *  # noqa: F401,F403

POLE_PAIRS = 7.0  # motor_poles default 14
ERPM_TO_RPM = 1.0 / POLE_PAIRS

work = os.path.abspath("runs/tb")
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


class F(Flight):
    def __init__(self, link, channels):
        super().__init__(link)
        self.channels, self.k = channels, 0

    def run(self, seconds, **kw):
        n = int(round(seconds / self.dt))
        for _ in range(n):
            if self.k % 2 == 0:
                socks[5762].sendall(crsf_rc_frame(self.channels))
            socks[5763].sendall(kiss_frame(35, 24.6, 3.2, 120, 12000))
            self.k += 1
            super().run(self.dt, **kw)
        return self.last


def motor_telemetry(msp):
    """MSP_MOTOR_TELEMETRY: u8 count, then per motor u32 rpm, u16 invalidPct, u8 temp, u16 V, u16 A, u16 mAh."""
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
    disarmed = [1500, 1500, 1000, 1500, 1000, 1000, 1500, 1500]
    f = F(link, disarmed)
    f.run(2.0)
    msp = Msp(pump=f.hold)
    print("boot ok, arming disabled:", msp_arming_disabled(msp)[:3])

    # Arm with throttle LOW (AUX1 high; CRSF AETR index 2 is throttle), then raise to ~50 %.
    f.channels = [1500, 1500, 1000, 1500, 2000, 1000, 1500, 1500]
    f.run(1.5)
    print("armed:", msp_armed(msp), " arming disabled:", msp_arming_disabled(msp)[:3])
    f.channels = [1500, 1500, 1500, 1500, 2000, 1000, 1500, 1500]
    f.run(1.5)
    print("armed:", msp_armed(msp), " arming disabled:", msp_arming_disabled(msp)[:3])
    print("MSP_RC:", msp_rc(msp)[:8], " (roll pitch yaw throttle aux1)")
    print("motor outputs (MSP_MOTOR):", [round(x, 1) for x in struct.unpack("<8H", msp.request(104)[:16])[:4]])

    tel = motor_telemetry(msp)
    print("MSP_MOTOR_TELEMETRY (rpm, invalidPct, tempC, V, A, mAh) per motor:")
    for i, row in enumerate(tel):
        expect = 0.5 * 120000 * (1 + 0.25 * i) * ERPM_TO_RPM
        print(f"  motor {i}: rpm {row[0]:6d} (synthetic expects ~{expect:.0f}), invalid% {row[1] / 100:.2f}, "
              f"temp {row[2]}, {row[3] / 100:.2f} V, {row[4] / 100:.2f} A, {row[5]} mAh")
    distinct = len({row[0] for row in tel}) == 4 and tel[0][0] > 0
    print("DISTINCT PER-MOTOR RPM:", "YES" if distinct else "NO")

    # Throttle to ~75 %: all four rpm values must scale up together.
    f.channels = [1500, 1500, 1750, 1500, 2000, 1000, 1500, 1500]
    f.run(1.0)
    print("motor outputs at 75%:", [round(x, 1) for x in struct.unpack("<8H", msp.request(104)[:16])[:4]])
    tel2 = motor_telemetry(msp)
    print("at 75% throttle:", [row[0] for row in tel2])
    scaled = all(abs(t2[0] - t1[0] * 1.5) < max(60.0, 0.02 * t2[0]) for t1, t2 in zip(tel, tel2))
    print("RPM SCALES WITH THROTTLE:", "YES" if scaled else "NO")

    f.channels = disarmed
    f.run(1.0)
    print("disarmed again:", msp_armed(msp), " poll:", proc.poll())
except Exception as e:
    print("FAIL:", repr(e), " poll:", proc.poll())
    with open(os.path.join(work, "sitl.log")) as fh:
        print("".join(fh.readlines()[-15:]))
    raise
finally:
    link.close()
    stop_sitl(proc)
