"""The ELRS link end to end against real Betaflight (M3c): arms over the geometric link at home; 50 km out at
10 mW the receiver is silent, Betaflight fails safe and the OSD's LQ cell reads 0. Needs OFS_SITL_LAUNCH."""
import os
import pathlib
import re
import shutil

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)


def test_at_home_the_quad_arms_over_the_geometric_link(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)  # boot and gyro calibration
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(1.0)
    assert min(s.motor_cmd) > 0.0, f"never armed: {s.motor_cmd}"
    assert s.radio.link_up and s.radio.lq_pct == 100.0, s.radio


def test_fifty_km_out_at_ten_milliwatts_betaflight_fails_safe(sim, tmp_path):
    text = pathlib.Path(QUAD).read_text()
    text = text.replace("tx_power_mw = 250", "tx_power_mw = 10")
    text = text.replace("position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [50000.0, 0.0, -0.03]")
    quad = tmp_path / "far-10mw.toml"
    quad.write_text(text)
    # fc.betaflight_diff resolves next to the quad file, so the copy needs the diff beside it.
    shutil.copy(pathlib.Path(QUAD).with_suffix(".betaflight.diff"), tmp_path / "opendrone-5f-freestyle.betaflight.diff")
    sim.load(str(quad), seed=1)
    s = sim.run(4.0)  # boot and calibration, with the receiver silent the whole time
    assert not s.radio.link_up, s.radio
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(2.0)
    assert max(s.motor_cmd) == 0.0, f"Betaflight armed without a receiver: {s.motor_cmd}"
    # The shipped diff places the LQ element on the OSD (osd_link_quality_pos): it reads 0 with no link. Betaflight
    # renders the CRSF element as icon, RF mode, ":<lq>" (osd_elements.c) — the plain "LQ 0" form is only the
    # craft-name fallback — so with the receiver silent it reads e.g. "{0: 0".
    texts = set()
    for _ in range(5):
        osd = sim.get_osd()
        if osd.present:
            texts.add(osd.text)
        sim.run(0.2)
    assert texts, "the OSD never appeared"
    assert any(re.search(r".\d:\s*0\b", row) for t in texts for row in t.splitlines()), "\n\n".join(sorted(texts))
