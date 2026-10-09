"""OSD, VTX and ESC telemetry through the API against Betaflight SITL (spec docs/superpowers/specs/2026-10-08-m3a).
Needs OFS_SITL_LAUNCH."""
import os
import pathlib
import re
import shutil
import socket

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

MSP_PORT = 5761
MSP_REBOOT = 68
MSP_SET_VTX_CONFIG = 89


class Msp:
    """A minimal MSP v1 client on Betaflight's UART1 port; it advances the simulation while it waits."""

    def __init__(self, sim):
        self.sim = sim
        self.sock = socket.create_connection(("127.0.0.1", MSP_PORT), timeout=5)
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


def first_number(rows, row, col_from=0):
    m = re.search(r"\d+\.\d", rows[row][col_from:])
    assert m, f"no number in OSD row {row}: {rows[row]!r}\n" + "\n".join(rows)
    return float(m.group(0))


def test_the_osd_shows_the_batterys_real_voltage(sim):
    sim.load(QUAD, seed=1)
    state = sim.run(6.0)
    osd = sim.get_osd()
    assert osd.present and (osd.cols, osd.rows) == (30, 16), (osd.present, osd.cols, osd.rows)
    shown = first_number(osd.rows_text(), 14, 1)  # the battery voltage element sits at row 14, column 1
    assert abs(shown - state.battery_voltage_v) < 0.3, f"OSD {shown} V, battery model {state.battery_voltage_v} V"
    assert "OpenFPV" in osd.text or "OPENFPV" in osd.text.upper(), osd.text  # the craft name element


def test_a_low_battery_raises_betaflights_warning(sim, tmp_path):
    quad = pathlib.Path(QUAD)
    low = tmp_path / quad.name
    low.write_text(quad.read_text().replace("initial_soc = 1.0", "initial_soc = 0.02"))
    shutil.copy(quad.with_suffix(".betaflight.diff"), tmp_path / "opendrone-5f-freestyle.betaflight.diff")
    sim.load(str(low), seed=1)
    sim.run(8.0)
    # The OSD's warnings element shows one warning at a time and cycles through the active ones ("BATT < FULL" is
    # another), so look at a few seconds of frames rather than one.
    seen = []
    for _ in range(24):
        sim.run(0.25)
        seen.append(sim.get_osd().text)
    assert any("LOW BATTERY" in text for text in seen), "\n\n".join(dict.fromkeys(seen))


def test_the_stick_menu_opens_on_the_osd(sim):
    sim.load(QUAD, seed=1)
    sim.run(5.0)
    sim.set_sticks(throttle=0.5, yaw=-1.0, pitch=1.0)  # throttle centre + yaw left + pitch forward, disarmed
    sim.run(2.0)
    text = sim.get_osd().text
    assert "FEATURES" in text or "SAVE" in text, text


def test_two_lockstep_runs_draw_identical_osd_frames(sim):
    frames = []
    for _ in range(2):
        sim.load(QUAD, seed=3)
        sim.run(6.0)
        osd = sim.get_osd()
        frames.append((osd.seq, osd.present, osd.cells))
    assert frames[0] == frames[1], "the OSD differs between identical lockstep runs"


def test_the_vtx_starts_as_configured_and_follows_the_configurator_vtx_tab(sim):
    sim.load(QUAD, seed=1)
    state = sim.run(6.0)
    assert state.vtx == ofs.Vtx(present=True, band=5, channel=1, freq_mhz=5658, power_mw=200, pit_mode=False), state.vtx
    msp = Msp(sim)
    try:
        # band R channel 3 = (5 - 1) * 8 + (3 - 1) = 34, power level 3 (600 mW), pit off
        msp.request(MSP_SET_VTX_CONFIG, bytes([34, 0, 3, 0]))
        state = sim.run(2.0)
    finally:
        msp.close()
    assert (state.vtx.band_letter, state.vtx.channel, state.vtx.freq_mhz, state.vtx.power_mw) == ("R", 3, 5732, 600), state.vtx
    changes = [e for e in sim.events() if e.kind == "vtx_changed"]
    assert changes and "R3" in changes[-1].message and "5732" in changes[-1].message, changes


def test_the_osd_and_vtx_survive_a_betaflight_reboot(sim):
    sim.load(QUAD, seed=1)
    sim.run(6.0)
    msp = Msp(sim)
    try:
        msp.send(MSP_REBOOT)
        state = sim.run(1.0)
        for _ in range(30):  # Betaflight reboots (about 1.6 s of real time), the simulator relaunches it
            state = sim.run(0.5)
            if state.fc_restarts == 1 and sim.get_osd().present:
                break
    finally:
        msp.close()
    assert state.fc_restarts == 1
    assert sim.get_osd().present, "the OSD never came back after the reboot"
    assert state.vtx.present and state.vtx.freq_mhz == 5658, state.vtx
