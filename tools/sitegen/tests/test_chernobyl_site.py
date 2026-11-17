import json
import re
import tomllib
from pathlib import Path

import pytest

from helpers import bounds, segment_hits
from sitegen import osm
from sitegen.build import build_site
from test_build import glb_triangles

ROOT = Path(__file__).resolve().parents[3]
RECIPE = ROOT / "sites" / "chernobyl" / "recipe.toml"
SNAPSHOT = ROOT / "sites" / "chernobyl" / "chernobyl.osm.json"
WORLD = ROOT / "worlds" / "chernobyl.toml"
GLB = ROOT / "worlds" / "assets" / "chernobyl.glb"

PILOT_EYE = (-53.946, -258.065, -1.7)


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    # The recipe's world path climbs two levels out of its directory, and out_dir keeps that layout: two levels of room.
    out = tmp_path_factory.mktemp("chernobyl") / "a" / "b"
    rep = build_site(RECIPE, out_dir=out)
    return rep, glb_triangles(rep.glb_path)


def recipe():
    return tomllib.loads(RECIPE.read_text(encoding="utf-8"))


def test_committed_outputs_are_current(built):
    rep, _ = built
    assert rep.world_path.read_bytes() == WORLD.read_bytes()
    assert rep.glb_path.read_bytes() == GLB.read_bytes()


def test_every_landmark_has_a_source_comment():
    blocks = re.split(r"(?m)^(?=\[\[landmarks\]\])", RECIPE.read_text(encoding="utf-8"))[1:]
    assert blocks
    for b in blocks:
        b = re.split(r"(?m)^\[(?!\[landmarks\]\])", b)[0]  # up to the next table
        assert "# source:" in b or "estimate" in b or "artistic" in b, b


def test_openings_fit_the_biggest_quad():
    frame = tomllib.loads((ROOT / "library" / "frames" / "opendrone-7lr.toml").read_text(encoding="utf-8"))
    prop = tomllib.loads((ROOT / "library" / "props" / "opendrone-7in-prop.toml").read_text(encoding="utf-8"))
    motor = max(abs(c) for m in frame["frame"]["motor_positions_frd_m"] for c in m)
    span = 2 * motor + prop["prop"]["diameter_m"]  # a motor each side of the centre, plus a prop radius beyond each
    assert span == pytest.approx(0.40, abs=0.01)
    doc = recipe()
    lm = doc["landmarks"]
    sides = [d[i] for l in lm for d in l.get("end_doors", []) for i in (1, 2)]
    sides += [o[i] for l in lm for o in l.get("openings", []) for i in (2,)] + [o[4] - o[3] for l in lm for o in l.get("openings", [])]
    fp = doc.get("footprints", [])
    sides += [o[2] for f in fp for o in f.get("openings", [])] + [o[4] - o[3] for f in fp for o in f.get("openings", [])]
    assert sides and all(s >= 2.0 for s in sides)


def test_the_glb_is_small_and_under_the_caps(built):
    rep, _ = built
    assert GLB.stat().st_size < 5 * 1024 * 1024
    assert rep.triangles < 400_000


def test_the_pilot_sees_the_door(built):
    _, mb = built
    assert segment_hits(mb, PILOT_EYE, (-53.946, -108.065, -20.0)) == 0  # the door centre, 20 m up
    # 25 m north of the door's centre line (the door is +-20 m), 5 m inside the end-wall plane: the line crosses the closed wall
    assert segment_hits(mb, PILOT_EYE, (-28.946, -103.065, -10.0)) >= 1
    # the same line to a point 10 m inside the door's edge goes through
    assert segment_hits(mb, PILOT_EYE, (-43.946, -103.065, -10.0)) == 0


def test_the_header_has_the_attribution_and_the_estimates_line():
    head = WORLD.read_text(encoding="utf-8").split("schema_version")[0]
    assert "© OpenStreetMap contributors" in head and "all values are estimates" in head
    assert "name = \"Chernobyl\"" in WORLD.read_text(encoding="utf-8")


def test_no_chimney_is_built(built):
    _, mb = built
    assert -bounds(mb)[2][0] <= 108.5  # NED down is negative up; the vault is the highest thing


def test_the_snapshot_is_fetched_and_credits_openstreetmap():
    doc = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    assert doc["format"] == 1 and doc["ways"]
    assert doc["source_note"].startswith(osm.FETCH_NOTE)
    assert "OpenStreetMap" in doc["attribution"] and "ODbL" in doc["attribution"]
    assert "out body geom" in doc["query"]
    assert any(w["id"] == -870533601 for w in doc["ways"]), "the turbine hall relation ring"


def test_the_nsc_outline_is_replaced_by_the_arch(built):
    rep, _ = built
    assert rep.skipped.get("recipe_skip") == 2
    skipped = {f["id"] for f in recipe().get("footprints", []) if f.get("skip")}
    assert 456732992 in skipped and 500551950 in skipped


def test_the_stale_chimney_is_not_built(built):
    _, mb = built
    assert not any(w["id"] == 301362167 for w in json.loads(SNAPSHOT.read_text(encoding="utf-8"))["ways"])
    for a, b, c, _ in mb.triangles:
        for n, e, d in (a, b, c):
            if (n + 31.0) ** 2 + (e - 85.0) ** 2 <= 8.0 ** 2 and d < -70.0:
                assert False, f"a triangle at {(n, e, d)} stands where the stack was removed"


def test_the_turbine_hall_breach_is_where_the_recipe_says(built):
    _, mb = built
    # West end wall of the main ring: start (-75.2, 58.4), 66.2 m south; breach offset 18 m, 30 m wide, 5..25 m up.
    # Start east of the arch's east end (east 53.94) so the segment tests the turbine hall, not the arch.
    assert segment_hits(mb, (-108.2, 55.0, -15.0), (-108.2, 80.0, -15.0)) == 0
    assert segment_hits(mb, (-92.2, 55.0, -15.0), (-92.2, 80.0, -15.0)) >= 1


def test_report_counts_are_pinned(built):
    rep, _ = built
    assert rep.footprints_used == 38
    assert rep.skipped == {"recipe_skip": 2}
    assert rep.landmarks == 1
    assert rep.triangles == 1259
    assert rep.materials == ["building", "nsc_cladding"]
