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
