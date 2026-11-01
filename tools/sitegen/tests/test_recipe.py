from pathlib import Path

import pytest

from sitegen.recipe import RecipeError, load_recipe

MINIMAL = """
[site]
name = "demo"
origin = [51.3895, 30.0990]
half_extent_m = [300.0, 250.0]
snapshot = "demo.osm.json"
[output]
world = "../../worlds/demo.toml"
glb = "assets/demo.glb"
[colors]
building = [0.62, 0.60, 0.56]
"""


def write(tmp_path, text, name="recipes/demo.toml"):
    p = tmp_path / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")
    return p


def problems(tmp_path, text):
    with pytest.raises(RecipeError) as e:
        load_recipe(write(tmp_path, text))
    return str(e.value).splitlines()


def test_minimal_recipe_loads(tmp_path):
    r = load_recipe(write(tmp_path, MINIMAL))
    assert r.site.name == "demo"
    assert r.site.origin == (51.3895, 30.0990)
    assert r.site.half_extent_m == (300.0, 250.0)
    assert (r.defaults.building_height_m, r.defaults.level_height_m) == (8.0, 3.0)
    assert r.colors == {"building": (0.62, 0.60, 0.56)}
    assert r.materials == () and r.footprints == () and r.landmarks == () and r.world == {}


def test_full_recipe_loads(tmp_path):
    r = load_recipe(write(tmp_path, MINIMAL + """
[defaults]
building_height_m = 9.5
[[materials]]
name = "building"
rf_loss_db = 18.0
[[footprints]]
id = 123456
skip = true
[[footprints]]
id = 7
height_m = 70.0
material = "concrete"
[[landmarks]]
kind = "stack"
name = "VP2"
center = [51.39, 30.1]
height_m = 150.0
[[landmarks]]
kind = "walls"
name = "hall"
corners_ned_m = [[0, 0], [0, 10], [10, 10]]
[world]
pilot = { x = 1 }
"""))
    assert r.defaults.building_height_m == 9.5 and r.defaults.level_height_m == 3.0
    assert (r.materials[0].name, r.materials[0].rf_loss_db) == ("building", 18.0)
    assert [(f.id, f.skip, f.height_m, f.material) for f in r.footprints] == [
        (123456, True, None, None),
        (7, False, 70.0, "concrete"),
    ]
    assert [(m.kind, m.name) for m in r.landmarks] == [("stack", "VP2"), ("walls", "hall")]
    assert r.landmarks[0].params == {"center": [51.39, 30.1], "height_m": 150.0}
    assert r.world == {"pilot": {"x": 1}}


def test_every_problem_is_reported_with_its_field_path(tmp_path):
    lines = problems(tmp_path, """
[site]
name = "bad name"
origin = [91.0, 30.0]
half_extent_m = [0.0, 300.0]
snapshot = "a.json"
typo = 1
[output]
world = "w.toml"
glb = "a.glb"
[colors]
roof = [0.1, 1.5, 0.2]
[[landmarks]]
kind = "stack"
name = "VP2"
center = [51.0, 30.0]
[[landmarks]]
kind = "stack"
name = "VP2"
center = [51.0, 30.0]
[[landmarks]]
kind = "pyramid"
name = "p"
[[landmarks]]
kind = "arch"
name = "a"
center = [51.0, 30.0]
center_ned_m = [0.0, 0.0]
oops = 1
[[footprints]]
id = 5
[[footprints]]
id = 5
height_m = -1.0
""")
    prefixes = [ln.split(":")[0] for ln in lines]
    for want in [
        "site.name", "site.origin", "site.half_extent_m", "site.typo",
        "colors.roof", "colors.building",
        "landmarks[1].name", "landmarks[2].kind", "landmarks[3]", "landmarks[3].oops",
        "footprints[1].id", "footprints[1].height_m",
    ]:
        assert want in prefixes, (want, lines)
    assert all(": " in ln for ln in lines)


def test_unknown_key_and_missing_table_are_errors(tmp_path):
    lines = problems(tmp_path, "[site]\nname = 'x'\n[surprise]\na = 1\n")
    prefixes = [ln.split(":")[0] for ln in lines]
    assert "surprise" in prefixes
    assert {"output", "colors"} <= set(prefixes)
    assert "site.origin" in prefixes


def test_syntax_error_is_a_recipe_error(tmp_path):
    assert problems(tmp_path, "[site\n")[0].startswith("toml: ")


def test_relative_paths_resolve_against_the_recipe(tmp_path):
    p = write(tmp_path, MINIMAL)
    r = load_recipe(p)
    assert r.site.snapshot_path == p.parent / "demo.osm.json"
    assert r.world_path == p.parent / "../../worlds/demo.toml"
    assert r.glb_rel == "assets/demo.glb"
    assert r.glb_path == r.world_path.parent / "assets/demo.glb"
    assert isinstance(r.world_path, Path)
