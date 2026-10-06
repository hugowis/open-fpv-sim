"""Protocol 2 features without firmware: real-time sessions, streams, faults, events and session lifetime."""
import time

import pytest

import ofs
from conftest import QUAD


def wait_for(predicate, timeout_s=3.0):
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return False


def test_realtime_session_follows_the_wall_clock(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    assert sim.state().time_s == 0.0 and not sim.state().running
    sim.start()
    time.sleep(1.0)
    s = sim.state()
    assert s.running
    assert 0.6 <= s.time_s <= 1.4, s.time_s
    sim.pause()
    paused_at = sim.state().time_s
    time.sleep(0.3)
    assert sim.state().time_s == paused_at
    with pytest.raises(ofs.InvalidState):
        sim.load(QUAD, open_loop_fc=True)  # lockstep
        sim.start()


def test_stream_states_yields_increasing_times(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.start()
    times = []
    for s in sim.stream_states(rate_hz=50):
        times.append(s.time_s)
        if len(times) == 5:
            break
    assert times == sorted(times)
    with pytest.raises(ofs.InvalidArgument):
        next(sim.stream_states(rate_hz=1000))


def test_radio_link_loss_fault_and_events(sim):
    sim.load(QUAD, open_loop_fc=True)
    assert sim.run(0.5).radio.link_up
    sim.inject(ofs.faults.RadioLinkLoss())
    assert not sim.run(0.5).radio.link_up
    sim.clear_faults()
    s = sim.run(0.5)
    assert s.radio.link_up and s.radio.lq_pct > 0
    kinds = []
    assert wait_for(lambda: kinds.extend(e.kind for e in sim.events()) or kinds.count("link_up") == 2), kinds
    assert kinds == ["link_up", "link_down", "link_up"]


def test_closing_a_client_ends_its_session(sim):
    other = ofs.connect(sim.address)
    other.load(QUAD, open_loop_fc=True)
    sim.state()  # one session per server: visible to every client
    other.close()

    def unloaded():
        try:
            sim.state()
            return False
        except ofs.NotLoaded:
            return True

    assert wait_for(unloaded), "the session outlived its client"


def test_keep_alive_sessions_outlive_their_client(sim):
    other = ofs.connect(sim.address)
    other.load(QUAD, open_loop_fc=True, keep_alive=True)
    other.close()
    time.sleep(0.5)
    sim.state()
