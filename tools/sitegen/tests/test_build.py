import json
import os
import shutil
import struct
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from sitegen import __main__ as cli
from sitegen import osm
from sitegen.build import BuildError, build_site
from sitegen.recipe import RecipeError

TESTS = Path(__file__).resolve().parent
DEMO = TESTS / "fixtures" / "demo"
GOLDEN = TESTS / "golden"
OUT_WORLD, OUT_GLB = "out/demo.toml", "out/assets/demo.glb"
DEG = 111194.9  # metres per degree on the sphere the projection uses, to six figures


def demo(tmp_path, edit):
    d = tmp_path / "demo"
    shutil.copytree(DEMO, d)
    r = d / "recipe.toml"
    r.write_text(edit(r.read_text(encoding="utf-8")), encoding="utf-8", newline="\n")
    return r


def mini(tmp_path, tail="", ways=((0, 0, 10),)):
    """A tiny site at origin 0, 0. A way is (north, east, side) of a square in metres, or a ready (lat, lon) ring."""
    d = tmp_path / "mini"
    d.mkdir(parents=True)

    def sq(n, e, s):
        return [(n / DEG, e / DEG), (n / DEG, (e + s) / DEG), ((n + s) / DEG, (e + s) / DEG), ((n + s) / DEG, e / DEG), (n / DEG, e / DEG)]

    ws = [osm.Way(i + 1, {"building": "yes"}, sq(*w) if len(w) == 3 else w) for i, w in enumerate(ways)]
    osm.write_snapshot(d / "s.json", ws, (-1, -1, 1, 1), "test")
    r = d / "recipe.toml"
    r.write_text(
        f"""[site]
name = "mini"
origin = [0.0, 0.0]
half_extent_m = [50.0, 50.0]
snapshot = "s.json"
[output]
world = "w.toml"
glb = "m.glb"
[colors]
building = [0.5, 0.5, 0.5]
{tail}""",
        encoding="utf-8",
        newline="\n",
    )
    return r


STACK = ('[[landmarks]]\nkind = "stack"\nname = "s"\ncenter_ned_m = [0.0, 0.0]\nbase_radius_m = {r}\n'
         'top_radius_m = 1.0\nheight_m = 5.0\nbands = [["{m}", 1.0]]\n')


def test_build_is_deterministic(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    build_site(DEMO / "recipe.toml", a)
    build_site(DEMO / "recipe.toml", b)
    for f in (OUT_WORLD, OUT_GLB):
        assert (a / f).read_bytes() == (b / f).read_bytes()


def test_golden_matches(tmp_path):
    build_site(DEMO / "recipe.toml", tmp_path)
    assert (tmp_path / OUT_GLB).read_bytes() == (GOLDEN / "demo.glb").read_bytes()
    assert (tmp_path / OUT_WORLD).read_bytes() == (GOLDEN / "demo.toml").read_bytes()
    assert b"\r" not in (GOLDEN / "demo.toml").read_bytes()


def test_report_counts(tmp_path):
    r = build_site(DEMO / "recipe.toml", tmp_path)
    assert (r.footprints_used, r.skipped, r.landmarks) == (3, {"recipe_skip": 1}, 3)
    assert r.materials == ["brick", "building", "concrete", "white"]
    assert r.triangles == 528
    assert "3 footprints" in str(r) and "recipe_skip=1" in str(r)


def test_footprint_outside_extent_is_dropped(tmp_path):
    # east spans: 1001 -40..-20, 1003 -30..-10 (reaches in), 1004 20..40; the extent is east +-15
    r = demo(tmp_path, lambda t: t.replace("half_extent_m = [50.0, 50.0]", "half_extent_m = [50.0, 15.0]"))
    rep = build_site(r, tmp_path / "o")
    assert (rep.footprints_used, rep.skipped) == (1, {"recipe_skip": 1, "outside_extent": 2})


def wide(tmp_path, tail, ways):
    """mini() with a 300 m half extent."""
    r = mini(tmp_path, tail, ways)
    r.write_text(r.read_text(encoding="utf-8").replace("[50.0, 50.0]", "[300.0, 300.0]"), encoding="utf-8", newline="\n")
    return r


def glb_max(path):
    blob = path.read_bytes()
    doc = json.loads(blob[20 : 20 + struct.unpack_from("<I", blob, 12)[0]])
    return [max(a["max"][i] for a in doc["accessors"] if a["type"] == "VEC3") for i in range(3)]


def test_a_footprint_crossing_the_extent_edge_is_kept_whole(tmp_path):
    rep = build_site(wide(tmp_path, "", [(0, 0, 10), (0, 100, 400)]), tmp_path / "o")  # 400 m east-west from 100 m to 500 m
    assert (rep.footprints_used, rep.skipped) == (2, {})
    assert glb_max(rep.glb_path)[0] == pytest.approx(500, abs=0.5)  # Godot x is east


def test_a_footprint_wholly_outside_is_dropped(tmp_path):
    rep = build_site(wide(tmp_path, "", [(0, 0, 10), (0, 301, 10), (-400, 0, 50)]), tmp_path / "o")
    assert (rep.footprints_used, rep.skipped) == (1, {"outside_extent": 2})


def test_a_footprint_override_by_negative_id_applies(tmp_path):
    r = mini(tmp_path, "[[footprints]]\nid = -50001\nheight_m = 20.0\n")
    sq = osm.read_snapshot(tmp_path / "mini" / "s.json")[0].ring
    osm.write_snapshot(tmp_path / "mini" / "s.json", [osm.Way(-50001, {"building": "yes"}, sq)], (-1, -1, 1, 1), "test")
    rep = build_site(r, tmp_path / "o")
    assert not any("snapshot" in w for w in rep.warnings) and glb_max(rep.glb_path)[1] == pytest.approx(20.0)  # Godot y is up


def test_unlisted_material_warns_and_an_unused_listed_one_is_dropped(tmp_path):
    r = demo(tmp_path, lambda t: t.replace("[[footprints]]", '[[materials]]\nname = "glass"\nrf_loss_db = 2.0\n\n[[footprints]]', 1))
    rep = build_site(r, tmp_path / "o")
    text = "\n".join(rep.warnings)
    assert "'brick'" in text and "'white'" in text and "'glass'" in text
    world = tomllib.loads((tmp_path / "o" / OUT_WORLD).read_text(encoding="utf-8"))
    assert [m["name"] for m in world["materials"]] == ["building", "concrete"]


def test_world_header_has_attribution_and_cyrillic_name_round_trips(tmp_path):
    build_site(DEMO / "recipe.toml", tmp_path)
    raw = (tmp_path / OUT_WORLD).read_bytes()
    text = raw.decode("utf-8")
    head = text.split("schema_version")[0]
    assert "../recipe.toml" in head and "edit the recipe, not this file" in head
    assert "# © OpenStreetMap contributors (ODbL 1.0, https://www.openstreetmap.org/copyright)" in head
    assert "estimates" in head
    world = tomllib.loads(text)
    assert world["name"] == "Чорнобиль Демо" and "Чорнобиль Демо".encode() in raw
    assert world["schema_version"] == 1
    assert world["meshes"] == [{"name": "demo", "file": "assets/demo.glb"}]
    assert world["receiver"]["antennas"][0]["name"] == "omni"


def test_name_and_schema_version_default_when_the_recipe_has_none(tmp_path):
    build_site(mini(tmp_path, "[world.pilot]\nfacing_deg = 0.0\n"), tmp_path / "o")
    world = tomllib.loads((tmp_path / "o" / "w.toml").read_text(encoding="utf-8"))
    assert (world["schema_version"], world["name"]) == (1, "mini")


def test_an_empty_site_is_a_build_error_and_writes_nothing(tmp_path):
    r = mini(tmp_path, "[[footprints]]\nid = 1\nskip = true\n")
    with pytest.raises(BuildError, match="no geometry"):
        build_site(r, tmp_path / "o")
    assert not (tmp_path / "o").exists()


def test_a_material_without_a_colour_is_a_build_error_naming_the_field(tmp_path):
    r = mini(tmp_path, '[[footprints]]\nid = 1\nmaterial = "ghost"\n')
    with pytest.raises(BuildError, match=r"footprints\[0\]\.material: 'ghost'"):
        build_site(r, tmp_path / "o")
    r = mini(tmp_path / "x", STACK.format(r=1.0, m="ghost"))
    with pytest.raises(BuildError, match=r"landmarks\[s\].*ghost"):
        build_site(r, tmp_path / "o")


def test_bad_footprints_are_skipped_and_counted_not_fatal(tmp_path):
    bowtie = [(0.0, 0.0), (0.0, 0.0002), (0.0002, 0.0), (0.0002, 0.0002), (0.0, 0.0)]
    speck = [(0.0, 0.0), (0.0, 0.000004), (0.000004, 0.000004), (0.000004, 0.0), (0.0, 0.0)]
    r = mini(tmp_path, "[[footprints]]\nid = 4\nheight_m = 1e8\n", ways=[(0, 0, 10), bowtie, speck, (20, 20, 10)])
    rep = build_site(r, tmp_path / "o")
    assert (rep.footprints_used, rep.skipped) == (1, {"bad_ring": 2, "extrude_error": 1})


def test_a_landmark_error_aborts_the_build(tmp_path, capsys):
    r = mini(tmp_path, STACK.format(r=-1.0, m="building"))
    with pytest.raises(BuildError, match=r"^s\.base_radius_m"):
        build_site(r, tmp_path / "o")
    assert cli.main(["build", str(r)]) == 1
    assert "s.base_radius_m" in capsys.readouterr().err


def test_world_may_not_define_meshes(tmp_path):
    with pytest.raises(BuildError, match="world.meshes"):
        build_site(mini(tmp_path, '[[world.meshes]]\nname = "x"\nfile = "y"\n'), tmp_path / "o")


def test_a_recipe_error_exits_1_listing_all_problems(tmp_path):
    r = tmp_path / "bad.toml"
    r.write_text('[site]\nname = "bad name"\norigin = [1.0]\n', encoding="utf-8")
    p = subprocess.run([sys.executable, "-m", "sitegen", "build", str(r)], cwd=TESTS.parent, capture_output=True, text=True, encoding="utf-8")
    assert p.returncode == 1
    for field in ("site.name", "site.origin", "site.snapshot", "output"):
        assert field in p.stderr
    assert len(p.stderr.strip().splitlines()) >= 4


def test_the_cli_builds_and_prints_the_report(tmp_path, capsys):
    assert cli.main(["build", str(mini(tmp_path))]) == 0
    assert "1 footprints" in capsys.readouterr().out
    assert (tmp_path / "mini" / "w.toml").exists()


def test_fetch_dispatches_to_osm(monkeypatch):
    seen = []
    monkeypatch.setattr(osm, "fetch_main", lambda p: seen.append(p) or 7)
    assert cli.main(["fetch", "r.toml"]) == 7 and seen == [Path("r.toml")]


def test_an_unsorted_snapshot_with_a_duplicate_id_builds_the_same_bytes(tmp_path):
    base = tmp_path / "base"
    build_site(DEMO / "recipe.toml", base)
    r = demo(tmp_path, lambda t: t)
    snap = r.parent / "demo.osm.json"
    doc = json.loads(snap.read_text(encoding="utf-8"))
    doc["ways"] = doc["ways"][::-1] + [{**doc["ways"][0], "tags": {"building": "yes", "height": "99"}}]
    snap.write_text(json.dumps(doc, ensure_ascii=False), encoding="utf-8")
    rep = build_site(r, tmp_path / "o")
    assert rep.skipped == {"recipe_skip": 1, "duplicate_id": 1}
    for f in (OUT_WORLD, OUT_GLB):
        assert (tmp_path / "o" / f).read_bytes() == (base / f).read_bytes()


def test_the_cli_prints_cyrillic_to_a_piped_stdout(tmp_path):
    d = tmp_path / "тест"
    shutil.copytree(DEMO, d)
    env = {k: v for k, v in os.environ.items() if k not in ("PYTHONUTF8", "PYTHONIOENCODING")}
    p = subprocess.run([sys.executable, "-m", "sitegen", "build", str(d / "recipe.toml")], cwd=TESTS.parent, env=env,
                       capture_output=True)
    assert p.returncode == 0, p.stderr.decode("utf-8", "replace")
    assert "тест".encode() in p.stdout


def tree(root):
    return {p: p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def test_a_toml_date_in_world_fails_before_anything_is_written(tmp_path):
    r = demo(tmp_path, lambda t: t + "\n[world.when]\nbuilt = 2026-11-01\n")
    out = tmp_path / "o"
    (out / "out").mkdir(parents=True)
    good = out / OUT_WORLD
    good.write_bytes(b"previous good world\n")
    with pytest.raises(BuildError, match="^world:"):
        build_site(r, out)
    assert tree(out) == {good: b"previous good world\n"}


@pytest.mark.parametrize("field, target", [("world", "demo.osm.json"), ("glb", "../recipe.toml"), ("glb", "../demo.osm.json")])
def test_an_output_naming_an_input_is_refused_and_the_input_is_untouched(tmp_path, field, target):
    old = {"world": 'world = "out/demo.toml"', "glb": 'glb = "assets/demo.glb"'}[field]
    r = demo(tmp_path, lambda t: t.replace(old, f'{field} = "{target}"'))
    before = tree(r.parent)
    with pytest.raises(BuildError, match="same file"):
        build_site(r)
    assert tree(r.parent) == before


def test_the_world_and_the_glb_may_not_be_one_file(tmp_path):
    r = demo(tmp_path, lambda t: t.replace('glb = "assets/demo.glb"', 'glb = "demo.toml"'))
    with pytest.raises(BuildError, match="same file"):
        build_site(r, tmp_path / "o")
    assert not (tmp_path / "o").exists()


def test_a_write_failure_is_a_build_error(tmp_path):
    out = tmp_path / "o"
    (out / OUT_WORLD).mkdir(parents=True)  # a directory where the world file goes
    with pytest.raises(BuildError, match="cannot write"):
        build_site(DEMO / "recipe.toml", out)
    assert not list(out.rglob("*.tmp"))


@pytest.mark.parametrize("field, edit", [
    ("site.snapshot", ('snapshot = "demo.osm.json"', 'snapshot = ""')),
    ("output.world", ('world = "out/demo.toml"', 'world = ""')),
    ("output.glb", ('glb = "assets/demo.glb"', 'glb = ""')),
])
def test_an_empty_path_is_a_recipe_error_naming_the_field(tmp_path, field, edit):
    r = demo(tmp_path, lambda t: t.replace(*edit))
    with pytest.raises(RecipeError, match=f"{field}: must not be empty"):
        build_site(r, tmp_path / "o")
