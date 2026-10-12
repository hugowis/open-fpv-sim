"""The fault catalog through real Betaflight SITL (spec §8). Needs OFS_SITL_LAUNCH."""
import os

import pytest

import ofs
from ofs.faults import BurstLoss, MotorFailure, VtxInterference, WeakCell

from conftest import QUAD
from msp_status import ARMING_DISABLED_FAILSAFE, ARMING_DISABLED_RX_FAILSAFE, StatusClient

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM = (1.0, 1.0, -1.0, -1.0)


def _armed_and_hovering(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)
    sim.set_sticks(throttle=0.0, aux=ARM)
    sim.run(1.0)
    sim.set_sticks(throttle=0.55, aux=ARM)
    sim.run(2.0)


def test_motor_failure_takes_the_quad_down(sim):
    _armed_and_hovering(sim)
    healthy = sim.state()
    sim.inject(MotorFailure(motor=0, scale=0.0))
    sim.run(0.2)  # the tilt is a transient: Betaflight re-levels in ~0.5 s once its mixer compensates
    tilting = sim.state()
    sim.run(0.8)  # the dead motor coasts down through half the healthy rpm about a second in
    dead = sim.state()
    assert dead.motor_rpm[0] < healthy.motor_rpm[0] * 0.5
    assert dead.motor_cmd[0] > healthy.motor_cmd[0], "Betaflight compensates"
    assert abs(tilting.euler_deg()[0]) + abs(tilting.euler_deg()[1]) > 5.0, "the quad tilts off the lost corner"


def test_burst_loss_failsafes_and_betaflight_flags_it(sim):
    _armed_and_hovering(sim)
    status = StatusClient(sim)
    sim.inject(BurstLoss(on_ms=2500, off_ms=1000))
    sim.run(2.0)  # stage 1 flags at once, stage 2 completes its landing ~1.6 s in; the burst's
    # off-window restores the link at 2.5 s and FAILSAFE clears ~1 s later, so check inside the window
    flags = status.arming_disable_flags()
    assert flags & ARMING_DISABLED_RX_FAILSAFE, hex(flags)
    assert flags & ARMING_DISABLED_FAILSAFE, hex(flags)
    sim.clear_faults()
    sim.run(2.0)
    flags = status.arming_disable_flags()
    assert not flags & ARMING_DISABLED_FAILSAFE, f"recovered: {flags:#x}"
    status.close()


def test_weak_cell_reads_through_betaflight_telemetry(sim):
    _armed_and_hovering(sim)
    healthy = sim.state()
    sim.inject(WeakCell(cell=0, r_offset_ohm=0.02, v_offset_v=0.1))
    sim.run(0.5)
    weak = sim.state()
    cells = weak.battery_cell_voltages
    assert len(cells) == 6
    assert cells[0] < min(cells[1:]), "cell 0 carries the delta"
    assert weak.fc_telemetry.battery_v < healthy.fc_telemetry.battery_v


def test_vtx_interference_degrades_the_picture(sim):
    _armed_and_hovering(sim)
    clean = sim.state().video
    sim.inject(VtxInterference(freq_mhz=5700.0, power_mw=500.0, position_ned_m=(10.0, 0.0, -1.5)))
    sim.run(0.5)
    noisy = sim.state().video
    assert noisy.interference_dbm > clean.interference_dbm
    assert noisy.snr_db < clean.snr_db
