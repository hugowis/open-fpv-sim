import http.client
import json
import math
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import pytest

from sitegen import osm

FIXTURE = Path(__file__).parent / "fixtures" / "overpass_small.json"
RELATIONS = Path(__file__).parent / "fixtures" / "overpass_relations.json"
UA = "test-agent/1"
BBOX = (51.38, 30.09, 51.40, 30.11)
RECIPE = """
[site]
name = "demo"
origin = [51.39, 30.1]
half_extent_m = [1000.0, 500.0]
snapshot = "demo.osm.json"
[output]
world = "w.toml"
glb = "demo.glb"
[colors]
building = [0.6, 0.6, 0.6]
"""


@pytest.fixture(autouse=True)
def no_network(monkeypatch):
    def boom(*a, **k):
        raise AssertionError("the real network was touched")

    monkeypatch.setattr(urllib.request, "urlopen", boom)


def fixture_data():
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


class Resp:
    def __init__(self, body):
        self.body = body if isinstance(body, bytes) else json.dumps(body).encode("utf-8")

    def read(self):
        return self.body

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


def http_error(code):
    return urllib.error.HTTPError("https://x", code, "err", {}, None)


class FakeOpener:
    """Plays `script` (an exception or a body per call, the last one repeats); 406 without a User-Agent, like the public server."""

    def __init__(self, script):
        self.script, self.calls = list(script), []

    def __call__(self, req, timeout=None):
        self.calls.append(req)
        if not req.get_header("User-agent"):
            raise http_error(406)
        item = self.script.pop(0) if len(self.script) > 1 else self.script[0]
        if isinstance(item, Exception):
            raise item
        return Resp(item)


def no_sleep(s):
    pass


def test_parse_skips_open_ways_and_relations_and_counts_them():
    ways, skipped = osm.parse_overpass(fixture_data())
    assert [w.id for w in ways] == [101, 102, 104, 105]
    assert skipped == {"open_way": 1, "relation": 1, "no_geometry": 0, "malformed": 0, "relation_hole": 0}
    assert ways[0].ring[0] == ways[0].ring[-1] == (51.39, 30.099)
    assert ways[2].tags["name"] == "Саркофаг"
    assert "addr:street" not in ways[0].tags


def test_parse_survives_hostile_input():
    data = {"elements": [
        "junk", 3, None,
        {"type": "way", "id": "x", "geometry": []},
        {"type": "way", "id": True, "tags": {}, "geometry": []},
        {"type": "way", "id": 1, "tags": {"building": "yes"}},
        {"type": "way", "id": 2, "tags": "nope", "geometry": [{"lat": 0, "lon": 0}]},
        {"type": "way", "id": 3, "tags": {"building": "yes"},
         "geometry": [{"lat": 0, "lon": 0}, {"lat": 1}, {"lat": 1, "lon": 1}, {"lat": 0, "lon": 0}]},
        {"type": "way", "id": 4, "tags": {"building": "yes"},
         "geometry": [{"lat": 0, "lon": 0}, {"lat": 0, "lon": 1}, {"lat": 1, "lon": 1}, {"lat": 0, "lon": 0}]},
    ]}
    ways, skipped = osm.parse_overpass(data)
    assert [w.id for w in ways] == [4]
    assert skipped == {"open_way": 0, "relation": 0, "no_geometry": 1, "malformed": 7, "relation_hole": 0}


def test_parse_huge_int_coordinates_are_skipped_not_raised():
    ring = [{"lat": 10**400, "lon": 0}, {"lat": 0, "lon": 0}, {"lat": 1, "lon": 1}, {"lat": 10**400, "lon": 0}]
    ways, skipped = osm.parse_overpass({"elements": [
        {"type": "way", "id": 1, "tags": {}, "geometry": ring},
        {"type": "way", "id": 2, "tags": {}, "geometry": [{"lat": float("nan"), "lon": 0}]}]})
    assert ways == [] and skipped["malformed"] == 2


def relations():
    return json.loads(RELATIONS.read_text(encoding="utf-8"))


def square(s=0.0):
    return [{"lat": s, "lon": 0}, {"lat": s, "lon": 1}, {"lat": s + 0.0005, "lon": 1}, {"lat": s + 0.0005, "lon": 0}, {"lat": s, "lon": 0}]


def rel(rid, members, tags=None):
    return {"type": "relation", "id": rid, "tags": {"building": "yes"} if tags is None else tags, "members": members}


def test_relation_with_one_closed_outer_becomes_a_way():
    ways, skipped = osm.parse_overpass(relations())
    hall = next(w for w in ways if w.id == -(8705336 * 100 + 1))
    assert hall.tags == {"building": "industrial", "name": "Турбінний зал", "name:en": "Turbine hall"}
    assert hall.ring[0] == hall.ring[-1] == (51.389, 30.099) and len(hall.ring) == 5
    assert [w.id for w in ways] == [-870533601, -50001]
    assert skipped["relation"] == 1


def test_outer_ring_is_stitched_from_member_ways_in_either_direction():
    ways, _ = osm.parse_overpass(relations())
    ring = next(w for w in ways if w.id == -50001).ring
    assert ring == [(51.388, 30.097), (51.388, 30.0974), (51.3883, 30.0974), (51.3883, 30.097), (51.388, 30.097)]


def test_relation_ring_ids_count_from_one_and_are_capped_at_99():
    outers = [{"type": "way", "ref": i, "role": "outer", "geometry": square(i / 1000)} for i in range(1, 102)]
    ways, skipped = osm.parse_overpass({"elements": [rel(7, outers[:2]), rel(8, outers)]})
    assert [w.id for w in ways if w.id > -800] == [-702, -701]
    assert sorted(w.id for w in ways if w.id <= -800) == [-899 + i for i in range(99)] and len(ways) == 101
    assert skipped["relation"] == 2


def test_relation_hole_is_counted_not_built():
    ways, skipped = osm.parse_overpass(relations())
    assert skipped["relation_hole"] == 1
    assert len(next(w for w in ways if w.id == -870533601).ring) == 5


def test_unclosable_relation_is_skipped_and_counted_never_raises():
    outer = {"type": "way", "ref": 1, "role": "outer"}
    nan = {"lat": float("nan"), "lon": 0}
    data = {"elements": [
        rel(1, [{**outer, "geometry": square()[:3]}]),  # open chain
        rel(2, []),
        rel(3, [outer, {**outer, "geometry": []}]),  # no geometry
        rel(4, [{**outer, "geometry": [nan, *square()[1:]]}]),
        rel(5, "members"),
        rel(6, ["junk", 3, None]),
        rel(7, [{**outer, "ref": "x", "geometry": square()}]),
        rel(8, [{**outer, "geometry": [{"lat": 10**400, "lon": 0}, *square()[1:]]}]),
        rel(9, [{**outer, "geometry": [{"lat": 91, "lon": 0}, *square()[1:]]}]),
        rel(10, [{**outer, "geometry": [{"lat": 0}, *square()[1:]]}]),
        {"type": "relation", "id": 11, "tags": {"building": "yes"}},
        {"type": "relation", "id": "x", "tags": {"building": "yes"}, "members": []},
        {"type": "relation", "id": 12, "tags": {"type": "route"}, "members": [{**outer, "geometry": square()}]},
    ]}
    ways, skipped = osm.parse_overpass(data)
    assert ways == []
    assert sum(skipped.values()) == 13 and skipped["malformed"] >= 1 and skipped["relation_hole"] == 0


@pytest.mark.parametrize("tags, built", [({"type": "multipolygon"}, True), ({"building": "yes"}, True), ({"type": "route"}, False)])
def test_a_relation_needs_the_multipolygon_type_or_a_building_tag(tags, built):
    ways, _ = osm.parse_overpass({"elements": [rel(1, [{"type": "way", "ref": 1, "role": "outer", "geometry": square()}], tags)]})
    assert bool(ways) is built


def test_snapshot_round_trips_negative_ids_sorted(tmp_path):
    ways, _ = osm.parse_overpass(relations())
    ways += osm.parse_overpass(fixture_data())[0]
    a, b = tmp_path / "a.json", tmp_path / "b.json"
    osm.write_snapshot(a, ways, BBOX, "note")
    osm.write_snapshot(b, list(reversed(ways)), BBOX, "note")
    assert a.read_bytes() == b.read_bytes()
    ids = [w["id"] for w in json.loads(a.read_bytes())["ways"]]
    assert ids == sorted(ids) and ids[:2] == [-870533601, -50001]
    assert osm.read_snapshot(a) == sorted(ways, key=lambda w: w.id)


@pytest.mark.parametrize("tags, expected", [
    ({"height": "70"}, 70.0),
    ({"height": "70 m"}, 70.0),
    ({"height": "70M"}, 70.0),
    ({"height": " 70 "}, 70.0),
    ({"height": "12.5"}, 12.5),
    ({"height": "12,5"}, 12.5),
    ({"height": "70", "building:levels": "3"}, 70.0),
    ({"building:levels": "3"}, 9.0),
    ({"building:levels": "3.5"}, 10.5),
    ({"height": "tall", "building:levels": "2"}, 6.0),
    ({"height": "0", "building:levels": "2"}, 6.0),
    ({"height": "-4", "building:levels": "2"}, 6.0),
    ({"height": "nan", "building:levels": "2"}, 6.0),
    ({"height": "inf", "building:levels": "2"}, 6.0),
    ({"height": "70 ft", "building:levels": "2"}, 6.0),
    ({"height": "5000", "building:levels": "2"}, 6.0),
    ({"building:levels": "0"}, 8.0),
    ({"building:levels": "many"}, 8.0),
    ({}, 8.0),
])
def test_height_rules(tags, expected):
    assert osm.height_of(tags, 8.0, 3.0) == expected


def test_snapshot_round_trip_and_cyrillic_survives(tmp_path):
    ways, _ = osm.parse_overpass(fixture_data())
    p = tmp_path / "s.osm.json"
    osm.write_snapshot(p, ways, BBOX, "note")
    assert "Саркофаг" in p.read_text(encoding="utf-8")
    assert osm.read_snapshot(p) == ways


def test_snapshot_is_sorted_and_stable_bytes(tmp_path):
    ways, _ = osm.parse_overpass(fixture_data())
    a, b = tmp_path / "a.json", tmp_path / "b.json"
    osm.write_snapshot(a, ways, BBOX, "note")
    osm.write_snapshot(b, list(reversed(ways)), BBOX, "note")
    raw = a.read_bytes()
    assert raw == b.read_bytes()
    assert raw.endswith(b"\n") and b"\r" not in raw
    head = json.loads(raw)
    assert [w["id"] for w in head["ways"]] == [101, 102, 104, 105]
    assert head["format"] == 1 and head["bbox"] == list(BBOX)
    assert head["attribution"].startswith("© OpenStreetMap contributors, ODbL 1.0")
    assert "[out:json]" in head["query"] and head["source_note"] == "note"


@pytest.mark.parametrize("text, msg", [
    ("not json", "snapshot"),
    ("[]", "snapshot"),
    ('{"format": 2, "ways": []}', "format"),
    ('{"format": 1, "ways": [{"id": "x"}]}', "snapshot"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {}, "ring": [[1]]}]}', "snapshot"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {}, "ring": [[1, %s]]}]}' % ("1" + "0" * 400), "coordinate"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {}, "ring": [[NaN, 1]]}]}', "coordinate"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {}, "ring": [[1, 1e999]]}]}', "coordinate"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {}, "ring": [[91, 1]]}]}', "coordinate"),
    ('{"format": 1, "ways": [{"id": 1, "tags": {"height": 70}, "ring": []}]}', "strings"),
])
def test_read_snapshot_rejects_malformed_files(tmp_path, text, msg):
    p = tmp_path / "bad.json"
    p.write_text(text, encoding="utf-8")
    with pytest.raises(ValueError, match=msg):
        osm.read_snapshot(p)


def test_fetch_retries_on_502_then_succeeds():
    op, naps = FakeOpener([http_error(502), http_error(502), fixture_data()]), []
    assert osm.fetch(BBOX, UA, opener=op, sleep=naps.append)["elements"]
    assert len(op.calls) == 3 and naps == [25, 25]
    req = op.calls[0]
    assert req.full_url == "https://overpass-api.de/api/interpreter" and req.get_method() == "POST"
    body = urllib.parse.unquote_plus(req.data.decode("ascii"))
    assert ('[out:json][timeout:60]; (way["building"](51.38,30.09,51.4,30.11); '
            'relation["building"](51.38,30.09,51.4,30.11);); out tags geom;') in body


def test_fetch_query_asks_for_relations_too():
    q = osm._query(BBOX)
    assert q == ('[out:json][timeout:60]; (way["building"](51.38,30.09,51.4,30.11); '
                 'relation["building"](51.38,30.09,51.4,30.11);); out tags geom;')


def test_fetch_gives_up_after_tries():
    op, naps = FakeOpener([http_error(429)]), []
    with pytest.raises(osm.FetchError, match="429"):
        osm.fetch(BBOX, UA, opener=op, sleep=naps.append, tries=3)
    assert len(op.calls) == 3 and naps == [25, 25]


@pytest.mark.parametrize("err", [http_error(400), http_error(500), urllib.error.URLError("dns"), TimeoutError(), OSError("x")])
def test_fetch_other_errors_are_immediate(err):
    op = FakeOpener([err])
    with pytest.raises(osm.FetchError):
        osm.fetch(BBOX, UA, opener=op, sleep=lambda s: pytest.fail("must not retry"))
    assert len(op.calls) == 1


def test_fetch_sends_a_user_agent():
    op = FakeOpener([fixture_data()])
    osm.fetch(BBOX, osm.USER_AGENT, opener=op, sleep=no_sleep)
    assert op.calls[0].get_header("User-agent") == osm.USER_AGENT
    with pytest.raises(osm.FetchError, match="406"):
        osm.fetch(BBOX, "", opener=FakeOpener([fixture_data()]), sleep=no_sleep)


@pytest.mark.parametrize("body", [b"<html>", b"[]", b'{"elements": 3}', b"\xff\xfe",
                                  b'{"elements": [], "remark": "runtime error: timed out"}'])
def test_fetch_rejects_bad_bodies(body):
    with pytest.raises(osm.FetchError):
        osm.fetch(BBOX, UA, opener=FakeOpener([body]), sleep=no_sleep)


def recipe_file(tmp_path):
    p = tmp_path / "demo.toml"
    p.write_text(RECIPE, encoding="utf-8")
    return p


def test_fetch_main_writes_the_snapshot_with_the_recipe_bbox(tmp_path, capsys):
    op = FakeOpener([fixture_data()])
    assert osm.fetch_main(recipe_file(tmp_path), opener=op, sleep=no_sleep) == 0
    snap = tmp_path / "demo.osm.json"
    assert [w.id for w in osm.read_snapshot(snap)] == [101, 102, 104, 105]
    s, w, n, e = json.loads(snap.read_text(encoding="utf-8"))["bbox"]
    assert (n - s) / 2 == pytest.approx(1000 / 6_371_000 * 57.29578, abs=1e-5)
    assert (e - w) / 2 == pytest.approx(500 / (6_371_000 * math.cos(math.radians(51.39))) * 57.29578, rel=1e-3)
    assert "4 buildings" in capsys.readouterr().out
    assert not list(tmp_path.glob("*.tmp"))


def test_empty_response_does_not_overwrite_a_good_snapshot(tmp_path, capsys):
    rp = recipe_file(tmp_path)
    snap = tmp_path / "demo.osm.json"
    assert osm.fetch_main(rp, opener=FakeOpener([fixture_data()]), sleep=no_sleep) == 0
    good = snap.read_bytes()
    capsys.readouterr()
    for script in ([{"elements": []}], [http_error(500)], [{"elements": [{"type": "relation", "id": 1}]}]):
        assert osm.fetch_main(rp, opener=FakeOpener(script), sleep=no_sleep) == 1
        assert snap.read_bytes() == good
    assert capsys.readouterr().err.count("\n") == 3
    assert not list(tmp_path.glob("*.tmp"))


def test_fetch_main_reports_a_bad_recipe(tmp_path, capsys):
    assert osm.fetch_main(tmp_path / "missing.toml", opener=FakeOpener([{}]), sleep=no_sleep) == 1
    assert "missing.toml" in capsys.readouterr().err


def test_truncated_body_is_a_fetch_error_and_keeps_the_snapshot(tmp_path, capsys):
    class Truncated(Resp):
        def read(self):
            raise http.client.IncompleteRead(b"{", 100)

    rp = recipe_file(tmp_path)
    snap = tmp_path / "demo.osm.json"
    assert osm.fetch_main(rp, opener=FakeOpener([fixture_data()]), sleep=no_sleep) == 0
    good = snap.read_bytes()
    capsys.readouterr()
    assert osm.fetch_main(rp, opener=lambda req, timeout=None: Truncated(b""), sleep=no_sleep) == 1
    assert "IncompleteRead" in capsys.readouterr().err
    assert snap.read_bytes() == good
