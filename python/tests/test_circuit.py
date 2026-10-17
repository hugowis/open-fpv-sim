"""The scripted pilot flies the closed circuit through its gates (M8b), open loop on the built-in stabilizer: no
Betaflight needed. The same pilot flies real Betaflight in test_sitl_circuit.py."""
import importlib.util

import pytest

from conftest import QUAD, REPO

CIRCUIT = str(REPO / "worlds" / "circuit.toml")


def _circuit_module():
    spec = importlib.util.spec_from_file_location("circuit", REPO / "python" / "examples" / "circuit.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load(sim):
    sim.load(QUAD, seed=1, open_loop_fc=True, stabilize=True, world=CIRCUIT)


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
    with pytest.raises(RuntimeError, match=r"waypoint 1 .*10000"):
        _circuit_module().fly_circuit(sim, waypoints=[(20.0, 0.0), (10000.0, 0.0)], per_waypoint_s=5.0)


def test_the_circuit_is_deterministic(sim):
    circuit = _circuit_module()
    runs = []
    for _ in range(2):
        _load(sim)
        result = circuit.fly_circuit(sim)
        runs.append((result["events"], result["final"]))
    assert runs[0] == runs[1]
