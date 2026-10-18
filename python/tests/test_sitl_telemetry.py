"""The FC telemetry chain end to end against real Betaflight (M3d): per-motor eRPM reaches the RPM
filter's source (MSP_MOTOR_TELEMETRY shows four distinct values matching the sim's motors), the ELRS
downlink carries Betaflight's telemetry to the client (battery tracks the sim's, a mode string
arrives), and a radio cut ages the telemetry out while the sim's own battery lives on. Needs
OFS_SITL_LAUNCH."""
import os
import socket
import struct
import time

import pytest

from conftest import QUAD
from ofs.faults import RadioLinkLoss

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM = (1.0, 1.0, -1.0, -1.0)
POLE_PAIRS = 7.0
MSP_MOTOR_TELEMETRY = 139
MSP_RC = 105


def _msp_requester(sim):
    """Minimal MSP v1 client over the Configurator TCP port, pumped by sim time."""
    address = sim.configurator_address  # an attribute set by load(): "tcp://127.0.0.1:5761"
    port = int(address.rsplit(":", 1)[1])
    s = socket.create_connection(("127.0.0.1", port), timeout=2.0)
    s.settimeout(0.05)

    def request(cmd):
        ck = cmd
        s.sendall(b"$M<" + bytes([0, cmd]) + bytes([ck]))
        deadline = time.monotonic() + 2.0
        while True:
            sim.run(0.02)  # MSP is serviced only while simulated time advances
            try:
                b = s.recv(4096)
            except socket.timeout:  # the request may reach Betaflight only after this slice (Windows): pump on
                if time.monotonic() >= deadline:
                    raise
                continue
            if b"$M>" in b:
                i = b.index(b"$M>")
                n, got = b[i + 3], b[i + 4]
                payload = b[i + 5:i + 5 + n]
                if got == cmd:
                    return payload

    return request


def test_per_motor_erpm_reaches_betaflight(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)
    sim.set_sticks(throttle=0.0, aux=ARM)
    sim.run(1.0)  # armed, idle
    sim.set_sticks(throttle=0.5, aux=ARM)
    s = sim.run(1.0)  # ~half throttle: motors spin up
    request = _msp_requester(sim)
    rc = request(MSP_RC)  # MSP order: roll, pitch, yaw, throttle
    assert struct.unpack_from("<4H", rc, 0) == (1500, 1500, 1500, 1500), \
        "telemetry did not disturb RC: the sticks read back as sent"
    # The four motors differ by a few rpm only, and Betaflight's value is quantized in steps of about 14 rpm, so one
    # sample can read all four alike (which one depends on how much simulated time the reply took: a few ms of wall
    # time more on Windows, where a native SITL fails a single-sample version of this about 5 runs in 8). Look again.
    for _ in range(20):
        d = request(MSP_MOTOR_TELEMETRY)
        rpm = [struct.unpack_from("<I", d, 1 + i * 13)[0] for i in range(d[0])]
        if len(set(rpm)) > 1:
            break
        sim.run(0.05)
    assert len(rpm) == 4 and len(set(rpm)) > 1, f"expected distinct per-motor RPM, got {rpm}"
    sim_rpm = s.motor_rpm
    for bf, ours in zip(rpm, sim_rpm):
        # MSP_MOTOR_TELEMETRY's rpm is eRPM/POLE_PAIRS (mechanical), the same unit as the sim's motor_rpm
        assert abs(bf - ours) <= max(500.0, 0.05 * ours), \
            f"Betaflight reports {bf} rpm (eRPM/{POLE_PAIRS}), the sim models {ours} rpm: {rpm} vs {sim_rpm}"


def test_battery_and_mode_arrive_over_the_downlink(sim):
    sim.load(QUAD, seed=1)
    s = sim.run(6.0)
    assert s.fc_telemetry.frame_count > 0, "no telemetry frame flew a downlink slot"
    assert abs(s.fc_telemetry.battery_v - s.battery_voltage_v) <= 0.5, \
        f"Betaflight {s.fc_telemetry.battery_v} V vs the sim {s.battery_voltage_v} V"
    assert s.fc_telemetry.flight_mode != "", s.fc_telemetry
    assert s.fc_telemetry.telemetry_age_s < 1.0, s.fc_telemetry


def test_a_radio_cut_ages_the_telemetry_out(sim):
    sim.load(QUAD, seed=1)
    sim.run(6.0)
    assert sim.run(0.1).fc_telemetry.frame_count > 0
    sim.inject(RadioLinkLoss())
    s = sim.run(2.0)
    assert not s.radio.link_up
    assert s.fc_telemetry.telemetry_age_s > 1.0, "the telemetry went stale with the link"
    assert s.battery_voltage_v > 0.0, "the simulator's own battery model lives on"
    sim.clear_faults()
