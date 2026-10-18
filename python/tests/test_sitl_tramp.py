"""The Tramp VTX through the API against Betaflight SITL (spec 2026-10-10-m3e §6). Needs OFS_SITL_LAUNCH."""
import os
import pathlib
import socket
import struct

import pytest

import ofs
from conftest import WORLD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

QUAD = str(pathlib.Path(__file__).resolve().parents[2] / "quads" / "opendrone-5f-tramp.toml")
MSP_SET_VTX_CONFIG = 89


class Msp:
    """A minimal MSP v1 client on Betaflight's UART1 port; it advances the simulation while it waits."""

    def __init__(self, sim):
        self.sim = sim
        # The session's slot decides the port (5761 + 10 per slot): follow the address the server reported.
        msp_port = int(sim.configurator_address.rsplit(":", 1)[1])
        self.sock = socket.create_connection(("127.0.0.1", msp_port), timeout=5)
        self.sock.setblocking(False)
        self.buf = b""

    def close(self):
        self.sock.close()

    def send(self, cmd, payload=b""):
        body = bytes([len(payload), cmd]) + payload
        checksum = 0
        for b in body:
            checksum ^= b
        self.sock.sendall(b"$M<" + body + bytes([checksum]))

    def _take(self, cmd):
        while True:
            start = self.buf.find(b"$M")
            if start < 0:
                self.buf = b""
                return None
            self.buf = self.buf[start:]
            if len(self.buf) < 5:
                return None
            size = self.buf[3]
            if len(self.buf) < 6 + size:
                return None
            got_cmd, payload = self.buf[4], self.buf[5:5 + size]
            self.buf = self.buf[6 + size:]
            if got_cmd == cmd:
                return payload

    def request(self, cmd, payload=b"", pumps=500):
        self.send(cmd, payload)
        for _ in range(pumps):
            self.sim.run(0.01)
            try:
                self.buf += self.sock.recv(4096)
            except BlockingIOError:
                pass
            reply = self._take(cmd)
            if reply is not None:
                return reply
        raise TimeoutError(f"no MSP reply to command {cmd}")


def test_the_tramp_vtx_follows_betaflight(sim):
    name = sim.load(QUAD, world=WORLD)
    assert "Tramp" in name
    sim.run(0.05)  # a couple of fields: the models publish their power-up state while Betaflight still boots
    state = sim.state()
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False), \
        "the VTX's power-up state R1 at default_power_index 2 of the tramp table"
    assert state.video.present, "the video link runs"
    assert state.video.latency_ms == 40, "the flat world has no latency_ms: the default two fields, quantized"
    assert state.video.field_valid, "a clean link at the pad completes every field"

    # Betaflight's init: it queries 'r' then 'v' and adopts the VTX's frequency.
    sim.run(3.0)
    state = sim.state()
    assert state.vtx.freq_mhz == 5658, "the FC adopted the VTX's frequency (no band/channel on tramp)"

    # MSP_SET_VTX_CONFIG with a frequency and a power level: Betaflight sends 'F' then 'P' (mW from its built-in
    # 25/100/200/400/600 table, level 5 = 600 mW); the simulated VTX follows.
    msp = Msp(sim)
    try:
        msp.request(MSP_SET_VTX_CONFIG, struct.pack("<HBB", 5800, 5, 0))
        state = sim.run(2.0)
    finally:
        msp.close()
    assert state.vtx.freq_mhz == 5800, "the 'F' command reached the model"
    assert state.vtx.power_mw == 600, "the 'P' command reached the model (mW from the FC's built-in table)"
    assert state.video.present, "the link retuned, not died"
