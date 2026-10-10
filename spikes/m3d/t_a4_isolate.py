"""THROWAWAY M3d spike A, take 4: does enabling CRSF telemetry break the CRSF RC path?
Run A: feature.diff as-is (FEATURE_TELEMETRY default on). Run B: same diff + feature -TELEMETRY.
Both: 2 s of CRSF RC over TCP, then MSP_RC / arming / battery."""
import os
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0"))
from ofs_spike import *  # noqa: F401,F403


def run_variant(name, extra_lines):
    work = os.path.abspath(f"runs/ta4-{name}")
    diff = os.path.join(work, "variant.diff")
    os.makedirs(work, exist_ok=True)
    base = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0", "feature.diff")
    with open(base) as fh:
        lines = fh.read().splitlines()
    # insert our lines before the first `serial` line so serial setup stays last
    first_serial = next(i for i, l in enumerate(lines) if l.startswith("serial"))
    with open(diff, "w", newline="\n") as fh:
        fh.write("\n".join(lines[:first_serial] + extra_lines + lines[first_serial:]) + "\n")
    if not os.path.exists(os.path.join(work, "eeprom.bin")):
        apply_config(work, diff)
    proc = launch_sitl(work)
    link = SitlLink()
    socks = {}
    for port in (5762, 5763):
        socks[port] = socket.create_connection(("127.0.0.1", port))

        def drain(s=socks[port]):
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

    try:
        time.sleep(0.5)
        f = F(link, [1500, 1500, 1000, 1500, 1000, 1000, 1500, 1500])
        f.run(2.0)
        f.channels = [1600, 1400, 1000, 1500, 1000, 1000, 1500, 1500]
        f.run(0.5)
        msp = Msp(pump=f.hold)
        print(f"[{name}] MSP_RC (sent 1600 1400 1000 1500):", msp_rc(msp)[:8])
        print(f"[{name}] arming disabled:", msp_arming_disabled(msp))
        bat = msp.request(123)
        v = struct.unpack_from("<H", bat, 9)[0] if len(bat) >= 11 else -1
        print(f"[{name}] MSP_BATTERY_STATE voltage: {v / 100 if v >= 0 else v} V (KISS 24.6)")
        return proc.poll()
    finally:
        link.close()
        stop_sitl(proc)


print("run A: telemetry feature ON (default)  -> poll:", run_variant("tel-on", []))
print("run B: telemetry feature OFF           -> poll:", run_variant("tel-off", ["feature -TELEMETRY"]))
