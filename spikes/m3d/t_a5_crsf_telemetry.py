"""THROWAWAY M3d spike A, final: correct frame names (2026.6 crsf_protocol.h), drained TCP sockets,
6 s flight with a known attitude; census + decoded ATTITUDE/BATTERY vs known inputs."""
import os
import socket
import struct
import sys
import threading
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0"))
from ofs_spike import *  # noqa: F401,F403

NAMES = {0x02: "GPS", 0x07: "VARIO_SENSOR", 0x08: "BATTERY_SENSOR", 0x09: "BARO_ALTITUDE",
         0x0B: "HEARTBEAT", 0x11: "BARO", 0x12: "MAG", 0x14: "LINK_STATISTICS",
         0x1E: "ATTITUDE", 0x21: "FLIGHT_MODE", 0x29: "DEVICE_INFO"}

work = os.path.abspath("runs/ta5")
if not os.path.exists(os.path.join(work, "eeprom.bin")):
    apply_config(work, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m0", "feature.diff"))
proc = launch_sitl(work)
link = SitlLink()
socks = {}
uart_tx = bytearray()
raw_sizes = []


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

    def recv_reply(self):
        self.link.pwm.settimeout(0.5)
        try:
            data, _ = self.link.pwm.recvfrom(2048)
        except (socket.timeout, ConnectionResetError):
            return
        raw_sizes.append(len(data))
        i = 18
        while i + 3 <= len(data):
            idx, ln = data[i], data[i + 1] | (data[i + 2] << 8)
            if i + 3 + ln > len(data):
                break
            if idx == 1:
                uart_tx.extend(data[i + 3:i + 3 + ln])
            i += 3 + ln


def walk(buf):
    i = 0
    while i < len(buf) - 3:
        if buf[i] not in (0xC8, 0xEA, 0xEC, 0xEE):
            i += 1
            continue
        ln = buf[i + 1]
        if not 1 <= ln <= 62 or i + 2 + ln > len(buf):
            i += 1
            continue
        body = buf[i + 2:i + 2 + ln]
        if crc8(body[:-1], 0xD5) == body[-1]:
            yield body[0], body[1:-1]
            i += 2 + ln
        else:
            i += 1


try:
    for port in (5762, 5763):
        drained(port)
    time.sleep(0.5)
    f = F(link, [1500, 1500, 1000, 1500, 1000, 1000, 1500, 1500])
    q = attitude_ned(roll_deg=12.3, pitch_deg=-4.5, yaw_deg=90.0)
    # patched exchange loop: collect reply trailers
    f.hold  # noqa
    n = int(6.0 / f.dt)
    for _ in range(n):
        if f.k % 2 == 0:
            socks[5762].sendall(crsf_rc_frame(f.channels))
        socks[5763].sendall(kiss_frame(35, 24.6, 3.2, 120, 12000))
        f.k += 1
        f.t += f.dt
        f.link.send(fdm_legacy(f.t, (0.0, 0.0, 0.0), specific_force_frd(q), q), rc(f.t))
        f.recv_reply()
    frames = list(walk(bytes(uart_tx)))
    types = {}
    for t, p in frames:
        types.setdefault(t, []).append(p)
    print(f"6 s flight: {len(uart_tx)} UART2 bytes in reply trailers, reply sizes "
          f"{min(raw_sizes)}..{max(raw_sizes)} B, {len(frames)} CRC-valid frames, SITL poll: {proc.poll()}")
    for t in sorted(types):
        print(f"  0x{t:02X} {NAMES.get(t, '?'):15s} x{len(types[t]):4d}  first: {types[t][0].hex()}")
    if 0x1E in types:
        pit, roll, yaw = struct.unpack(">hhh", types[0x1E][-1][:6])
        print(f"ATTITUDE: pitch {pit / 100:+.1f} roll {roll / 100:+.1f} yaw {yaw / 100:+.1f} deg (sent -4.5 / 12.3 / 90)")
    if 0x08 in types:
        v, a = struct.unpack(">HH", types[0x08][-1][:4])
        mah = int.from_bytes(types[0x08][-1][4:7], "big")
        print(f"BATTERY: {v / 100:.2f} V {a / 100:.2f} A {mah} mAh (KISS sent 24.6 V 3.2 A 120 mAh)")
    if 0x21 in types:
        print("FLIGHT_MODE:", types[0x21][-1].split(b"\x00")[0])
    msp = Msp(pump=f.hold)
    bat = msp.request(123)
    print("MSP_BATTERY_STATE raw:", bat.hex())
    print("arming disabled:", msp_arming_disabled(msp))
except Exception as e:
    print("FAIL:", repr(e), "poll:", proc.poll())
    raise
finally:
    link.close()
    stop_sitl(proc)
