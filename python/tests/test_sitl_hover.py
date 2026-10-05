"""M1 exit criteria. Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults automatically on Linux and under WSL)."""
import importlib.util
import os

import pytest

from conftest import QUAD, REPO

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")


def _hover_module():
    spec = importlib.util.spec_from_file_location("hover", REPO / "python" / "examples" / "hover.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_betaflight_sitl_hover(sim, tmp_path):
    sim.load(QUAD, seed=1)
    result = _hover_module().fly_hover(sim)
    assert result["armed"], f"motors never spun: read {tmp_path}/opendrone-5f-freestyle/sitl.log for 'Arming disabled'"
    assert result["max_alt_err_m"] < 0.3, result
    assert result["max_tilt_deg"] < 5.0, result


def test_betaflight_sitl_hover_is_deterministic(sim):
    """Spec §5.5/§9: same quad + seed + inputs in lockstep give identical runs, Betaflight SITL included.

    Every load() relaunches SITL from <data_dir>/<quad stem>/eeprom.bin. The first-ever load also applies
    betaflight.diff, so a warm-up flight runs first and both compared flights boot from the same EEPROM.
    (If ofs-sim logs a first-datagram resend warning, SITL may have run an extra t=0 tick: see the log.)
    """
    hover = _hover_module()
    flights = []
    for _ in range(3):  # warm-up, then the two compared flights
        sim.load(QUAD, seed=1)
        flights.append(hover.fly_hover(sim))
        flights[-1]["final"] = sim.state()
    a, b = flights[1], flights[2]
    assert a["armed"], "motors never spun"
    assert len(a["states"]) == len(b["states"])
    diverged = next((i for i, (x, y) in enumerate(zip(a["states"], b["states"])) if x != y), None)
    assert diverged is None, f"runs diverge at state {diverged}:\n{a['states'][diverged]}\n{b['states'][diverged]}"
    assert a["final"] == b["final"]
