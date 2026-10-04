"""M1 exit criterion. Needs Betaflight SITL: set OFS_SITL_LAUNCH (cleanup defaults automatically under WSL)."""
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
