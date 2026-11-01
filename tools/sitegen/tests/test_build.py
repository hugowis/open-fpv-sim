import json
import os
import shutil
import struct
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

from helpers import segment_hits
from sitegen import __main__ as cli
from sitegen import osm
from sitegen.build import BuildError, build_site, describe_walls
from sitegen.mesh import MeshBuilder
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


def glb_triangles(path):
    """The triangles of a written .glb as a MeshBuilder in NED (Godot (x, y, z) is NED (-z, x, -y))."""
    blob = path.read_bytes()
    jl = struct.unpack_from("<I", blob, 12)[0]
    doc, bin0 = json.loads(blob[20 : 20 + jl]), 20 + jl + 8
    mb = MeshBuilder()
    for mesh in doc["meshes"]:
        for prim in mesh["primitives"]:
            def chunk(acc, fmt):
                a = doc["accessors"][acc]
                off = bin0 + doc["bufferViews"][a["bufferView"]]["byteOffset"]
                return struct.unpack_from(f"<{a['count'] * len(fmt)}{fmt[0]}", blob, off)

            pos, idx = chunk(prim["attributes"]["POSITION"], "fff"), chunk(prim["indices"], "I")
            pts = [(-pos[i + 2], pos[i], -pos[i + 1]) for i in range(0, len(pos), 3)]
            for i in range(0, len(idx), 3):
                mb.add_tri(pts[idx[i]], pts[idx[i + 1]], pts[idx[i + 2]], "m")
    return mb


FP = "[[footprints]]\nid = 1\nheight_m = 10.0\n"
SQUARE = [(0.0, 0.0), (0.0, 20.0), (20.0, 20.0), (20.0, 0.0), (0.0, 0.0)]  # counter-clockwise, 20 m; wall 0 is the south one


def ring_ll(pts):
    return [(n / DEG, e / DEG) for n, e in pts]


def test_a_footprint_breach_is_open_and_the_wall_beside_it_is_not(tmp_path):
    rep = build_site(mini(tmp_path, FP + "openings = [[0, 5.0, 4.0, 0.0, 3.0]]\n"), tmp_path / "o")
    mb = glb_triangles(rep.glb_path)
    assert segment_hits(mb, (-1, 7, -1.5), (1, 7, -1.5)) == 0  # through the hole (east 5..9, up to 3 m)
    assert segment_hits(mb, (-1, 4, -1.5), (1, 4, -1.5)) == 1  # 1 m beside it: the wall
    assert segment_hits(mb, (-1, 7, -4), (1, 7, -4)) == 1  # above the hole: the lintel
    assert segment_hits(mb, (10, 10, -20), (10, 10, 0)) == 1  # the roof is there


@pytest.mark.parametrize("opening, text", [("4, 5.0, 4.0, 0.0, 3.0", "openings[0]"), ("0, 18.0, 4.0, 0.0, 3.0", "openings"),
                                           ("0, 5.0, 4.0, 0.0, 11.0", "openings[0]")])
def test_a_breach_that_does_not_fit_is_a_build_error_naming_the_footprint(tmp_path, capsys, opening, text):
    r = mini(tmp_path, FP + f"openings = [[{opening}]]\n")
    with pytest.raises(BuildError) as e:
        build_site(r, tmp_path / "o")
    assert e.value.args[0].startswith("footprint 1.") and text in e.value.args[0]
    assert cli.main(["build", str(r)]) == 1 and "footprint 1." in capsys.readouterr().err
    assert not (tmp_path / "mini" / "w.toml").exists()


def test_breach_index_follows_the_normalised_counter_clockwise_ring(tmp_path):
    south = {}
    for name, pts in (("ccw", SQUARE), ("cw", SQUARE[::-1])):  # normalising the clockwise ring rotates it
        r = mini(tmp_path / name, FP, ways=[ring_ll(pts)])
        south[name] = next(int(ln.split()[0]) for ln in describe_walls(r, 1) if ln.split()[-1] == "180")
        r.write_text(r.read_text(encoding="utf-8") + f"openings = [[{south[name]}, 5.0, 4.0, 0.0, 3.0]]\n", encoding="utf-8", newline="\n")
        mb = glb_triangles(build_site(r, tmp_path / name / "o").glb_path)
        assert segment_hits(mb, (-1, 7, -1.5), (1, 7, -1.5)) == 0, name
        assert segment_hits(mb, (-1, 4, -1.5), (1, 4, -1.5)) == 1, name
        assert segment_hits(mb, (10, 3, -1.5), (10, -3, -1.5)) == 1, name  # the west wall is whole
    assert south["ccw"] != south["cw"]


def test_walls_description_lists_every_wall(tmp_path):
    ring = [(0.0, 0.0), (0.0, 30.0), (10.0, 30.0), (10.0, 15.0), (20.0, 15.0), (20.0, 0.0), (0.0, 0.0)]  # an L, counter-clockwise
    sliver = [(0.0, 0.0), (0.0, 0.5), (0.5, 0.5), (0.0, 0.0)]
    r = mini(tmp_path, ways=[ring_ll(ring), ring_ll(sliver)])
    rows = [ln.split() for ln in describe_walls(r, 1)]
    assert [int(x[0]) for x in rows] == list(range(6))
    assert sum(float(x[3]) for x in rows) == pytest.approx(30 + 10 + 15 + 10 + 15 + 20, abs=0.5)
    assert [(float(x[1]), float(x[2]), float(x[3]), int(x[4])) for x in rows[:3]] == [
        (0.0, 0.0, 30.0, 180), (0.0, 30.0, 10.0, 90), (10.0, 30.0, 15.0, 0)]
    with pytest.raises(BuildError, match="999"):
        describe_walls(r, 999)
    with pytest.raises(BuildError, match="not a usable footprint"):
        describe_walls(r, 2)


def test_walls_cli_accepts_a_negative_id(tmp_path):
    r = mini(tmp_path)
    osm.write_snapshot(r.parent / "s.json", [osm.Way(-70500001, {"building": "yes"}, ring_ll(SQUARE))], (-1, -1, 1, 1), "test")
    cmd = [sys.executable, "-m", "sitegen", "walls", str(r)]
    p = subprocess.run([*cmd, "-70500001"], cwd=TESTS.parent, capture_output=True, text=True, encoding="utf-8")
    assert p.returncode == 0 and len(p.stdout.splitlines()) == 4, p.stderr
    assert p.stdout.splitlines()[0].split() == ["0", "0.0", "0.0", "20.0", "180"]
    p = subprocess.run([*cmd, "-5"], cwd=TESTS.parent, capture_output=True, text=True, encoding="utf-8")
    assert p.returncode == 1 and "-5" in p.stderr and p.stdout == ""


def test_a_footprint_whose_box_exactly_touches_the_extent_edge_is_kept(tmp_path):
    from sitegen.geo import Projection

    ring = [(0.0, 60 / DEG), (0.0, 70 / DEG), (10 / DEG, 70 / DEG), (10 / DEG, 60 / DEG), (0.0, 60 / DEG)]
    edge = Projection(0.0, 0.0).to_ned(*ring[0])[1]  # the box's west side, to the last bit
    results = {}
    for name, east in (("on", edge), ("inside", edge - 1e-6)):
        r = mini(tmp_path / name, ways=[(0, 0, 10), ring])
        r.write_text(r.read_text(encoding="utf-8").replace("[50.0, 50.0]", f"[50.0, {east!r}]"), encoding="utf-8", newline="\n")
        results[name] = build_site(r, tmp_path / name / "o").skipped
    assert results == {"on": {}, "inside": {"outside_extent": 1}}  # edges included; one hair short and it is out


BOWTIE = [(0.0, 0.0), (0.0, 0.0002), (0.0002, 0.0), (0.0002, 0.0002), (0.0, 0.0)]


@pytest.mark.parametrize("reason, head, ways", [
    ("recipe_skip", "skip = true\n", [(0, 0, 10), (0, 0, 10)]),
    ("outside_extent", "", [(0, 0, 10), (0, 100, 10)]),
    ("bad_ring", "", [(0, 0, 10), BOWTIE]),
])
def test_openings_on_a_skipped_footprint_are_reported_not_silent(tmp_path, reason, head, ways):
    r = mini(tmp_path, f"[[footprints]]\nid = 2\n{head}openings = [[0, 1.0, 2.0, 0.0, 2.0]]\n", ways=ways)
    rep = build_site(r, tmp_path / "o")
    assert rep.skipped == {reason: 1} and rep.footprints_used == 1
    assert [w for w in rep.warnings if "openings" in w] == [f"footprint 2: openings ignored, skipped as {reason}"]


def test_a_roof_that_cannot_be_triangulated_names_the_footprint_and_the_roof(tmp_path, monkeypatch):
    def fail(ring):
        raise ValueError("no ear found: ring is not a simple counter-clockwise polygon")

    monkeypatch.setattr("sitegen.landmarks.triangulate", fail)
    with pytest.raises(BuildError, match=r"^footprint 1\.roof: no ear found"):
        build_site(mini(tmp_path, FP + "openings = [[0, 5.0, 4.0, 0.0, 3.0]]\n"), tmp_path / "o")

