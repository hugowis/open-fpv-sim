"""The scripted pilot flies the circuit through its gates on real Betaflight SITL (#53): arm, angle mode, four gates, a lap.
Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults automatically on Linux and under WSL)."""
import importlib.util
import os

import pytest

from conftest import QUAD, REPO

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

CIRCUIT = str(REPO / "worlds" / "circuit.toml")


def _circuit_module():
    spec = importlib.util.spec_from_file_location("circuit", REPO / "python" / "examples" / "circuit.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_betaflight_flies_the_circuit(sim):
    circuit = _circuit_module()
    # Every load() relaunches SITL from <data_dir>/<quad stem>-<hash>/eeprom.bin, and the first-ever load also applies
    # betaflight.diff: a warm-up load first, as test_sitl_hover does, so the flight boots from the saved EEPROM.
    sim.load(QUAD, seed=1, world=CIRCUIT)
    sim.run(1.0)
    sim.load(QUAD, seed=1, world=CIRCUIT)
    result = circuit.fly_circuit(sim, betaflight=True)
    race = [(k, m) for k, m in result["events"] if k in ("gate_passed", "lap_completed")]
    assert [k for k, _ in race] == ["gate_passed"] * 4 + ["lap_completed"], race
    assert race[0][1].startswith("Gate0 (gate 0) start"), race
    for i in (1, 2, 3):
        assert race[i][1].startswith(f"Gate{i} (gate {i}) split "), race
    assert race[4][1].startswith("lap 1: ") and race[4][1].endswith("(best)"), race
    assert result["laps"] == 1
    assert result["collisions"] == 0, result["events"]
    assert result["seconds"] < 120.0, result["seconds"]
