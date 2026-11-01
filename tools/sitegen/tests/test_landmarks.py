import math

import pytest
from helpers import bounds, normal_ned, segment_hits
from sitegen.geo import Projection
from sitegen.landmarks import arch, build_landmark, stack, walls
from sitegen.mesh import MeshBuilder
from sitegen.recipe import Landmark

SQUARE = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]  # wall 0 runs east along north = 0


def test_arch_dimensions():
    for heading, (dn, de) in ((0, (20, 10)), (90, (10, 20))):
        mb = MeshBuilder()
        arch(mb, (100.0, 50.0), heading, 10.0, 20.0, 6.0, "m")
        (n0, n1), (e0, e1), (d0, d1) = bounds(mb)
        assert n1 - n0 == pytest.approx(dn) and e1 - e0 == pytest.approx(de)
        assert (n0 + n1) / 2 == pytest.approx(100) and (e0 + e1) / 2 == pytest.approx(50)
        assert d0 == pytest.approx(-6.0) and d1 == pytest.approx(0.0)


@pytest.mark.parametrize("heading", [0, 90, 30])
def test_arch_end_door_is_open(heading):
    mb = MeshBuilder()
    arch(mb, (0.0, 0.0), heading, 10.0, 20.0, 6.0, "m", end_doors=[("front", 3.0, 3.0)])
    f = (math.cos(math.radians(heading)), math.sin(math.radians(heading)))
    r = (-f[1], f[0])

    def ray(end, side, z):  # from the vault centre out through the end wall, `side` metres right
        return (side * r[0], side * r[1], -z), (end * 20 * f[0] + side * r[0], end * 20 * f[1] + side * r[1], -z)

    assert segment_hits(mb, *ray(1, 0.0, 1.5)) == 0
    assert segment_hits(mb, *ray(1, 1.0, 1.5)) == 0
    assert segment_hits(mb, *ray(1, 2.5, 1.5)) > 0  # 1 m beside the door's edge
    assert segment_hits(mb, *ray(1, 0.0, 3.5)) > 0  # above the door
    assert segment_hits(mb, *ray(-1, 0.0, 1.5)) > 0  # the back wall has no door


def test_arch_faces_point_out():
    mb = MeshBuilder()
    arch(mb, (0.0, 0.0), 0.0, 10.0, 20.0, 6.0, "m")
    for t in mb.triangles:
        n = normal_ned(t)
        c = [sum(p[i] for p in t[:3]) / 3 for i in range(3)]
        if len({p[0] for p in t[:3]}) == 1:  # an end wall: out along +-north
            assert n[0] * c[0] > 0
        else:  # the vault: away from the axis in the (east, down) cross-section
            assert n[1] * c[1] + n[2] * c[2] > 0


def test_arch_invalid():
    for kw, field in (
        ({"span_m": -1.0}, "span_m"),
        ({"segments": 7}, "segments"),
        ({"end_doors": [("front", 11.0, 1.0)]}, "end_doors"),  # wider than the vault
        ({"end_doors": [("front", 9.0, 5.0)]}, "end_doors"),  # corners poke out of the ellipse
        ({"end_doors": [("side", 1.0, 1.0)]}, "end_doors"),
        ({"end_doors": [("front", 1.0, 1.0), ("front", 1.0, 1.0)]}, "end_doors"),
        ({"height_m": float("nan")}, "height_m"),
        ({"material": ""}, "material"),
    ):
        args = {"span_m": 10.0, "length_m": 20.0, "height_m": 6.0, "material": "m"} | kw
        mb = MeshBuilder()
        with pytest.raises(ValueError, match=field):
            arch(mb, (0.0, 0.0), 0.0, **args)
        assert mb.triangles == []


def test_stack_radii_and_bands():
    mb = MeshBuilder()
    bands = [("red", 0.5), ("white", 0.25), ("steel", 0.25)]
    stack(mb, (10.0, -5.0), 5.0, 3.0, 40.0, bands, segments=12)
    for a, b, c, _ in mb.triangles:
        for p in (a, b, c):
            assert math.hypot(p[0] - 10, p[1] + 5) == pytest.approx(5.0 + (3.0 - 5.0) * -p[2] / 40.0)
    order = []
    for t in sorted(mb.triangles, key=lambda t: sum(p[2] for p in t[:3])):  # most negative down = highest first
        if t[3] not in order:
            order.append(t[3])
    assert order == ["red", "white", "steel"]
    lo = {m: min(p[2] for t in mb.triangles if t[3] == m for p in t[:3]) for m in order}
    hi = {m: max(p[2] for t in mb.triangles if t[3] == m for p in t[:3]) for m in order}
    assert (lo["red"], hi["red"]) == pytest.approx((-40.0, -20.0))
    assert (lo["white"], hi["white"]) == pytest.approx((-20.0, -10.0))
    assert (lo["steel"], hi["steel"]) == pytest.approx((-10.0, 0.0))
    for t in mb.triangles:
        n = normal_ned(t)
        assert n[0] * (sum(p[0] for p in t[:3]) / 3 - 10) + n[1] * (sum(p[1] for p in t[:3]) / 3 + 5) > 0


def test_stack_invalid():
    for kw in ({"bands": [("a", 0.5), ("b", 0.4)]}, {"bands": [("a", 1.5), ("b", -0.5)]}, {"bands": []},
               {"segments": 2}, {"base_radius_m": 0.0}, {"top_radius_m": -1.0}, {"bands": [("a", "x")]}):
        args = {"base_radius_m": 5.0, "top_radius_m": 3.0, "height_m": 40.0, "bands": [("a", 1.0)]} | kw
        mb = MeshBuilder()
        with pytest.raises(ValueError, match=next(iter(kw))):
            stack(mb, (0.0, 0.0), **args)
        assert mb.triangles == []


def test_walls_breach_is_open_and_wall_is_not():
    mb = MeshBuilder()
    walls(mb, SQUARE, 3.0, "w", openings=[(0, 4.0, 2.0, 0.0, 2.0)])

    def through(e, z):  # across wall 0 (north = 0)
        return segment_hits(mb, (-2.0, e, -z), (2.0, e, -z))

    assert through(5.0, 1.0) == 0
    assert through(3.5, 1.0) > 0 and through(6.5, 1.0) > 0
    assert through(5.0, 2.5) > 0  # above the breach
    assert through(1.0, 2.9) > 0
    assert segment_hits(mb, (5.0, 5.0, -10.0), (5.0, 5.0, 10.0)) == 0  # no roof by default


def test_walls_window_in_the_middle_and_outward_faces():
    mb = MeshBuilder()
    walls(mb, SQUARE, 3.0, "w", openings=[(0, 4.0, 2.0, 1.0, 2.0), (0, 1.0, 1.0, 0.0, 3.0)])

    def through(e, z):
        return segment_hits(mb, (-2.0, e, -z), (2.0, e, -z))

    assert through(5.0, 1.5) == 0 and through(5.0, 0.5) > 0 and through(5.0, 2.5) > 0
    assert through(1.5, 2.9) == 0  # a full-height slot touching the top
    for t in mb.triangles:  # the footprint is clockwise here; faces still point out
        n = normal_ned(t)
        c = [sum(p[i] for p in t[:3]) / 3 for i in range(2)]
        assert n[0] * (c[0] - 5) + n[1] * (c[1] - 5) > 0


def test_walls_roof_faces_up():
    mb = MeshBuilder()
    walls(mb, SQUARE[::-1], 3.0, "w", roof=True)
    assert segment_hits(mb, (3.0, 6.0, -10.0), (3.0, 6.0, 10.0)) == 1  # off both diagonals
    roofs = [t for t in mb.triangles if all(p[2] == -3.0 for p in t[:3])]
    assert len(roofs) == 2 and all(normal_ned(t)[2] < 0 for t in roofs)  # NED down < 0 is up


def test_walls_invalid():
    bad = [
        ([(0, 0), (0, 10)], [], "corners"),
        ([(0, 0), (0, 10), (10, 0), (10, 10)], [], "corners"),  # bow tie
        (SQUARE, [(4, 0.0, 1.0, 0.0, 1.0)], "openings"),  # no wall 4
        (SQUARE, [(0, -1.0, 2.0, 0.0, 1.0)], "openings"),
        (SQUARE, [(0, 9.0, 2.0, 0.0, 1.0)], "openings"),  # runs off the wall
        (SQUARE, [(0, 1.0, 2.0, -1.0, 1.0)], "openings"),
        (SQUARE, [(0, 1.0, 2.0, 0.0, 4.0)], "openings"),  # above the wall
        (SQUARE, [(0, 1.0, 2.0, 1.0, 1.0)], "openings"),
        (SQUARE, [(0, 1.0, 3.0, 0.0, 1.0), (0, 3.0, 2.0, 0.0, 1.0)], "openings"),  # overlap
        (SQUARE, [(0, 1.0, "x", 0.0, 1.0)], "openings"),
        (SQUARE, [(0, 1.0, 1.0)], "openings"),
    ]
    for corners, openings, field in bad:
        mb = MeshBuilder()
        with pytest.raises(ValueError, match=field):
            walls(mb, corners, 3.0, "w", openings=openings)
        assert mb.triangles == []
    with pytest.raises(ValueError, match="roof"):
        walls(MeshBuilder(), SQUARE, 3.0, "w", roof=1)
    # Touching openings on one wall are fine.
    walls(MeshBuilder(), SQUARE, 3.0, "w", openings=[(0, 1.0, 2.0, 0.0, 1.0), (0, 3.0, 2.0, 0.0, 1.0)])


def test_landmark_dispatch_and_unknown_kind():
    proj = Projection(48.0, 2.0)
    n, e = proj.to_ned(48.001, 2.002)
    a, b = MeshBuilder(), MeshBuilder()
    build_landmark(a, Landmark("arch", "hangar", {"center": [48.001, 2.002], "span_m": 10, "length_m": 20, "height_m": 6,
                                                  "material": "m", "heading_deg": 15, "segments": 8,
                                                  "end_doors": [["front", 3, 3]]}), proj)
    arch(b, (n, e), 15, 10, 20, 6, "m", segments=8, end_doors=[("front", 3, 3)])
    assert a.triangles == b.triangles and a.triangles

    c, d = MeshBuilder(), MeshBuilder()
    build_landmark(c, Landmark("stack", "chimney", {"center_ned_m": [1, 2], "base_radius_m": 5, "top_radius_m": 3,
                                                    "height_m": 40, "bands": [["a", 0.5], ["b", 0.5]]}), proj)
    stack(d, (1, 2), 5, 3, 40, [("a", 0.5), ("b", 0.5)])
    assert c.triangles == d.triangles and c.triangles

    w = MeshBuilder()
    build_landmark(w, Landmark("walls", "shed", {"corners": [[48.0, 2.0], [48.0, 2.001], [48.001, 2.001], [48.001, 2.0]],
                                                 "height_m": 3, "material": "w", "roof": True}), proj)
    assert w.triangles

    with pytest.raises(ValueError, match=r"^tower\.kind: unknown kind 'tower'"):
        build_landmark(MeshBuilder(), Landmark("tower", "tower", {}), proj)


def test_landmark_errors_name_the_landmark_and_field():
    proj = Projection(48.0, 2.0)
    base = {"center": [48.0, 2.0], "span_m": 10, "length_m": 20, "height_m": 6, "material": "m"}
    for params, msg in (
        ({k: v for k, v in base.items() if k != "span_m"}, r"^hangar\.span_m: missing"),
        ({k: v for k, v in base.items() if k != "center"}, r"^hangar\.center"),
        (base | {"center_ned_m": [0, 0]}, r"^hangar\.center"),
        (base | {"center": [95.0, 2.0]}, r"^hangar\.center"),
        (base | {"center": "here"}, r"^hangar\.center"),
        (base | {"span_m": True}, r"^hangar\.span_m"),
        (base | {"end_doors": [5]}, r"^hangar\.end_doors\[0\]"),
        (base | {"end_doors": "front"}, r"^hangar\.end_doors"),
        (base | {"segments": 8.5}, r"^hangar\.segments"),
        (base | {"heading_deg": None}, r"^hangar\.heading_deg"),
    ):
        with pytest.raises(ValueError, match=msg):
            build_landmark(MeshBuilder(), Landmark("arch", "hangar", params), proj)
    shed = Landmark("walls", "shed", {"corners": [[48.0, 2.0]], "height_m": 3, "material": "w"})
    with pytest.raises(ValueError, match=r"^shed\.corners"):
        build_landmark(MeshBuilder(), shed, proj)
