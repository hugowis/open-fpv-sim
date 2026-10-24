"""The scripted pilot flies the closed circuit through its gates (M8b), open loop on the built-in stabilizer: no
Betaflight needed. The same pilot flies real Betaflight in test_sitl_circuit.py."""
import importlib.util
import time

import pytest

from conftest import QUAD, REPO

CIRCUIT = str(REPO / "worlds" / "circuit.toml")


def _circuit_module():
    spec = importlib.util.spec_from_file_location("circuit", REPO / "python" / "examples" / "circuit.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load(sim):
    # record=False: a recording's `recording_started` event reaches the client on a background thread and can land
    # in a flight's events, with a different file name on every load.
    sim.load(QUAD, seed=1, open_loop_fc=True, stabilize=True, world=CIRCUIT, record=False)


def _race_events(result):
    return [(k, m) for k, m in result["events"] if k in ("gate_passed", "lap_completed")]


def test_the_stabilized_quad_flies_the_circuit(sim):
    _load(sim)
    result = _circuit_module().fly_circuit(sim)
    race = _race_events(result)
    assert [k for k, _ in race] == ["gate_passed"] * 4 + ["lap_completed"], race
    assert race[0][1].startswith("Gate0 (gate 0) start"), race
    for i in (1, 2, 3):
        assert race[i][1].startswith(f"Gate{i} (gate {i}) split "), race
    assert race[4][1].startswith("lap 1: ") and race[4][1].endswith("(best)"), race
    assert result["laps"] == 1
    assert result["collisions"] == 0
    assert result["seconds"] < 120.0, result["seconds"]


def test_a_missed_gate_means_no_lap(sim):
    circuit = _circuit_module()
    _load(sim)
    # The same loop flown 30 m too far north: Gate2 is never passed, so the lap cannot complete.
    wide = [(20.0, -20.0), (20.0, 20.0), (90.0, 20.0), (90.0, -20.0), (20.0, -20.0), (20.0, 20.0)]
    result = circuit.fly_circuit(sim, waypoints=wide)
    assert result["laps"] == 0, _race_events(result)
    assert "lap_completed" not in [k for k, _ in result["events"]]
    assert not any(m.startswith("Gate2") for _, m in _race_events(result)), "Gate2 was missed"


def test_an_unreachable_waypoint_raises_naming_it(sim):
    _load(sim)
    # 8 s per waypoint: the deterministic take-off plus the 20 m first leg takes about 4 s of the old 5 s budget,
    # so the raise is the far waypoint's.
    with pytest.raises(RuntimeError, match=r"waypoint 1 .*10000"):
        _circuit_module().fly_circuit(sim, waypoints=[(20.0, 0.0), (10000.0, 0.0)], per_waypoint_s=8.0)


def test_a_flight_that_outlasts_max_seconds_raises_naming_the_budget(sim):
    _load(sim)
    with pytest.raises(RuntimeError, match=r"more than 0 s"):
        _circuit_module().fly_circuit(sim, max_seconds=0.1)


def test_the_circuit_is_deterministic(sim):
    circuit = _circuit_module()
    runs = []
    for _ in range(2):
        _load(sim)
        result = circuit.fly_circuit(sim)
        # Only what the flight itself raised: session events (a recording, a link) arrive asynchronously.
        runs.append(([e for e in result["events"] if e[0] in ("gate_passed", "lap_completed", "collision")], result["final"]))
    assert runs[0] == runs[1]


def test_the_lap_limit_finishes_the_circuit_and_a_reset_races_it_again(sim, tmp_path):
    # The circuit with a one-lap race; flown to the line, the race is over: `finished`, no clock.
    limited = tmp_path / "circuit-one-lap.toml"
    limited.write_text(open(CIRCUIT, encoding="utf-8").read() + "\n[race]\nlaps = 1\n", encoding="utf-8")
    sim.load(QUAD, seed=1, open_loop_fc=True, stabilize=True, world=str(limited), record=False)
    result = _circuit_module().fly_circuit(sim)
    race = sim.state().race
    assert (result["laps"], race.finished, race.running, race.lap_limit) == (1, True, False, 1), race
    # The two events are published together but the client reads them on a background thread: wait for the second.
    events = list(result["events"])
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline and not any(k == "race_finished" for k, _ in events):
        events += [(e.kind, e.message) for e in sim.events()]
        time.sleep(0.01)
    kinds = [k for k, _ in events if k in ("lap_completed", "race_finished")]
    assert kinds == ["lap_completed", "race_finished"], events
    sim.reset_race()
    race = sim.run(0.05).race
    assert (race.laps, race.finished, race.running, race.next_gate, race.best_lap_s) == (0, False, False, 0, 0.0), race
