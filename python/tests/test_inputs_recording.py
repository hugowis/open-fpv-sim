"""Recording a session's sticks and replaying them (spec §6), open loop: no Betaflight needed."""
import time

import pytest

import ofs
from conftest import QUAD

HEADER = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4"


def test_a_recorded_flight_replays_to_the_same_motor_commands(sim):
    sim.load(QUAD, open_loop_fc=True)
    path = sim.record_inputs()
    sim.set_sticks(throttle=0.3)
    first = sim.run(0.5).motor_cmd
    sim.set_sticks(throttle=0.9, roll=0.2)
    second = sim.run(0.5).motor_cmd
    assert sim.stop_inputs_recording() == path
    with open(path, encoding="utf-8") as f:
        assert f.read().splitlines() == [HEADER, "0,0,0,0,0.3,-1,-1,-1,-1", "0.5,0.2,0,0,0.9,-1,-1,-1,-1"]
    sim.load(QUAD, open_loop_fc=True)
    sim.play_inputs(path)
    assert sim.run(0.5).motor_cmd == first
    assert sim.run(0.5).motor_cmd == second


def test_the_recording_reports_its_events_and_refuses_misuse(sim):
    sim.load(QUAD, open_loop_fc=True)
    with pytest.raises(ofs.InvalidState):
        sim.stop_inputs_recording()
    path = sim.record_inputs()
    with pytest.raises(ofs.InvalidState):
        sim.record_inputs()
    sim.run(0.1)
    sim.stop_inputs_recording()
    # Events arrive from a background stream thread: wait for both instead of reading once (a slower machine, such as a
    # macOS runner, reads before the second has landed).
    kinds = []
    deadline = time.monotonic() + 5.0
    while len(kinds) < 2 and time.monotonic() < deadline:
        kinds += [(e.kind, e.message) for e in sim.events() if e.kind.startswith("inputs_recording")]
        if len(kinds) < 2:
            time.sleep(0.02)
    assert kinds[0] == ("inputs_recording_started", path)
    assert kinds[1][0] == "inputs_recording_stopped" and kinds[1][1].startswith(path)
