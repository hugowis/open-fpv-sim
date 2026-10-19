"""Two simulators on one machine, each flying real Betaflight (#15). Needs Betaflight SITL: set OFS_SITL_LAUNCH.

Each `ofs.launch()` is a separate `ofs-sim` process. The first takes the stock ports (Configurator on tcp:5761), the
next takes the following block of ten ports (tcp:5771), and neither one's startup or shutdown touches the other's SITL.
"""
import os
import signal
import subprocess
import sys

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)  # AUX1 high = ARM, AUX2 high = ANGLE (see the quad's betaflight.diff)
DISARMED = (-1.0, -1.0, -1.0, -1.0)


@pytest.fixture
def launcher(sim_bin, tmp_path):
    """`launch(name)` starts a server with its own data directory; every one is closed at the end."""
    started = []

    def launch(name):
        sim = ofs.launch(binary=sim_bin, data_dir=str(tmp_path / name))
        started.append(sim)
        return sim

    yield launch
    for sim in reversed(started):
        sim.close()


def _boot_and_arm(sim):
    sim.set_sticks(throttle=0.0, aux=DISARMED)
    sim.run(4.0)  # boot and gyro calibration, perfectly still
    sim.set_sticks(throttle=0.0, aux=ARM_AND_ANGLE)
    state = sim.run(1.0)
    assert max(state.motor_cmd) > 0.0, f"never armed: {state.motor_cmd}"


def test_two_sims_fly_betaflight_side_by_side(launcher):
    a, b = launcher("a"), launcher("b")
    a.load(QUAD, seed=1)
    b.load(QUAD, seed=1)
    assert a.configurator_address == "tcp://127.0.0.1:5761"
    assert b.configurator_address == "tcp://127.0.0.1:5771"
    _boot_and_arm(a)
    _boot_and_arm(b)
    a.set_sticks(throttle=0.6, aux=ARM_AND_ANGLE)
    b.set_sticks(throttle=0.4, aux=ARM_AND_ANGLE)
    state_a, state_b = a.run(1.0), b.run(1.0)
    assert max(state_a.motor_cmd) > max(state_b.motor_cmd) + 0.05, "each session follows its own sticks"
    assert (state_a.fc_restarts, state_b.fc_restarts) == (0, 0)
    assert state_a.altitude_m > state_b.altitude_m > -0.5


def test_closing_one_sim_does_not_disturb_the_other(launcher):
    a, b = launcher("a"), launcher("b")
    a.load(QUAD, seed=1)
    b.load(QUAD, seed=1)
    _boot_and_arm(a)
    _boot_and_arm(b)
    b.set_sticks(throttle=0.4, aux=ARM_AND_ANGLE)
    assert max(b.run(0.5).motor_cmd) > 0.0
    a.close()  # unloads A: its SITL is stopped and its slot's cleanup runs
    after = b.run(1.0)
    assert max(after.motor_cmd) > 0.0 and after.fc_restarts == 0, f"B's Betaflight was disturbed: {after}"
    # A's slot is free again, and it is the lowest free one: the next simulator gets the stock ports back.
    c = launcher("c")
    c.load(QUAD, seed=1)
    assert c.configurator_address == "tcp://127.0.0.1:5761"
    assert b.run(0.2).fc_restarts == 0


def _children(pid):
    """The pids of a process's children (the SITL a simulator started is one), by `pgrep -P`: macOS and Linux."""
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout
    return [int(p) for p in out.split()]


@pytest.mark.skipif(sys.platform == "win32", reason="finds the orphan SITL with pgrep -P")
def test_a_killed_simulators_orphan_sitl_does_not_block_the_next_session(launcher):
    a, b = launcher("a"), launcher("b")
    a.load(QUAD, seed=1)
    b.load(QUAD, seed=1)
    _boot_and_arm(b)
    orphans = _children(a._process.pid)
    assert orphans, "A's simulator started a SITL"
    # A hard kill gives the simulator no chance to stop its SITL: slot 0's Betaflight is left running, holding that
    # slot's ports. The next simulator must still load and fly (it skips the busy slot), without disturbing B.
    a._process.kill()
    a._process.wait()
    try:
        c = launcher("c")
        c.load(QUAD, seed=1)
        assert c.configurator_address not in (b.configurator_address, a.configurator_address), c.configurator_address
        _boot_and_arm(c)
        assert b.run(0.5).fc_restarts == 0, "B's Betaflight was disturbed"
    finally:
        for pid in orphans:  # leave no SITL behind: it would hold a slot for every later run
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
