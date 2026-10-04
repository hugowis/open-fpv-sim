"""THROWAWAY M0 Task 5: prove each re-enabled feature works at runtime on the patched lockstep SITL.

UART mapping: UART2 tcp:5762 CRSF RX, UART3 tcp:5763 ESC sensor (KISS), UART4 tcp:5764 MSP DisplayPort,
UART5 tcp:5765 SmartAudio. MSP queries on UART1 are pumped (time only advances with state packets).
"""
import os
import socket
import struct
import threading
import time
from ofs_spike import *

work = os.path.abspath("runs/t5")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, "feature.diff")
proc = launch_sitl(work)
link = SitlLink()
captured = {}
socks = {}


def reader(port):
    s = socket.create_connection(("127.0.0.1", port))
    socks[port] = s
    captured[port] = bytearray()
    while True:
        try:
            d = s.recv(4096)
        except OSError:
            return
        if not d:
            return
        captured[port].extend(d)


class FeatureFlight(Flight):
    """Sends CRSF RC frames (every 2nd tick) and KISS ESC telemetry (every tick) alongside state packets."""

    def __init__(self, link, channels):
        super().__init__(link)
        self.channels, self.k = channels, 0

    def run(self, seconds, **kw):
        n = int(round(seconds / self.dt))
        for _ in range(n):
            if self.k % 2 == 0 and not os.environ.get("NO_CRSF"):
                socks[5762].sendall(crsf_rc_frame(self.channels))
            if not os.environ.get("NO_ESC"):
                socks[5763].sendall(kiss_frame(35, 24.6, 3.2, 120, 12000))
            self.k += 1
            super().run(self.dt, **kw)
        return self.last


try:
    for port in (5762, 5763, 5764, 5765):
        threading.Thread(target=reader, args=(port,), daemon=True).start()
    time.sleep(0.5)
    disarmed = [1500, 1500, 1000, 1500, 1000, 1000, 1500, 1500]
    f = FeatureFlight(link, disarmed)
    f.run(6.0)
    msp = Msp(pump=f.hold)
    print("CRSF -> MSP_RC (expect 1500 1500 1000 1500 1000 1000):", msp_rc(msp)[:8])
    f.channels = [1600, 1400, 1000, 1500, 1000, 1000, 1500, 1500]
    f.run(0.5)
    print("CRSF -> MSP_RC (expect ~1600 1400 1000 1500):", msp_rc(msp)[:8])
    print("arming disabled:", msp_arming_disabled(msp))
    tel = bytes(captured[5762])
    print(f"CRSF telemetry back on 5762: {len(tel)} bytes, frame types:",
          sorted({tel[i + 2] for i in range(len(tel) - 2) if tel[i] in (0xC8, 0xEA, 0xEE) and 2 <= tel[i + 1] <= 62}))
    bat = msp.request(MSP_BATTERY_STATE)
    print("ESC sensor -> MSP_BATTERY_STATE:", bat.hex(), "voltage(0.01V)=", struct.unpack_from("<H", bat, 9)[0] if len(bat) >= 11 else None)
    print("ESC sensor -> MSP_MOTOR_TELEMETRY:", msp.request(MSP_MOTOR_TELEMETRY).hex()[:80])
    feat = struct.unpack("<I", msp.request(36)[:4])[0]  # MSP_FEATURE_CONFIG
    print(f"features bitmask 0x{feat:08x}: TELEMETRY={'on' if feat >> 10 & 1 else 'off'} OSD={'on' if feat >> 18 & 1 else 'off'}")
    # Behave like an HD VTX on UART4: send MSP requests, then see whether DisplayPort output starts.
    for _ in range(5):
        socks[5764].sendall(b"$M<" + bytes([0, 2, 2]))  # MSP_FC_VARIANT, checksum = 0 ^ 2
        f.run(0.2)
    dp = bytes(captured[5764])
    print(f"DisplayPort on 5764: {len(dp)} bytes, '$M>' frames: {dp.count(b'$M>')}, MSP 182 frames: {dp.count(bytes([182]))}",
          dp[:40].hex())
    sa = bytes(captured[5765])
    print(f"SmartAudio on 5765: {len(sa)} bytes, 'aa 55' count: {sa.count(bytes([0xAA, 0x55]))}", sa[:24].hex())
    print("MSP_VTX_CONFIG:", msp.request(MSP_VTX_CONFIG).hex())
    # Blackbox: arm for 2 s (CRSF AUX1 high), then disarm.
    f.channels = [1500, 1500, 1000, 1500, 2000, 1000, 1500, 1500]
    f.run(2.0)
    print("arming disabled while arm switch on:", msp_arming_disabled(msp))
    f.channels = disarmed
    f.run(1.0)
except BrokenPipeError:
    time.sleep(0.5)
    print("SITL returncode at failure:", proc.poll())
    raise
finally:
    link.close()
    stop_sitl(proc)
print("work dir:", sorted(os.listdir(work)))
