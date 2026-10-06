"""Radio failsafe through the API against Betaflight SITL (spec §8.3). Needs OFS_SITL_LAUNCH."""
import os

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)


def test_radio_cut_disarms_on_betaflight_failsafe_timing(sim):
    sim.load(QUAD, seed=1)
    assert sim.configurator_address == "tcp://127.0.0.1:5761"
    sim.run(4.0)  # boot and gyro calibration
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(1.0)
    assert min(s.motor_cmd) > 0.0, f"never armed: {s.motor_cmd}"
    sim.inject(ofs.faults.RadioLinkLoss())
    cut = s.time_s
    while s.time_s < cut + 4.0 and max(s.motor_cmd) > 0.0:
        s = sim.run(0.01)
    assert max(s.motor_cmd) == 0.0, "Betaflight never disarmed"
    assert 1.4 <= s.time_s - cut <= 2.2, s.time_s - cut
    assert not s.radio.link_up
