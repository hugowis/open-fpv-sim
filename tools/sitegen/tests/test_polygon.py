import math

import pytest

from sitegen.mesh import MeshBuilder
from sitegen.polygon import extrude, normalize_ring, signed_area, triangulate

# (north, east) rings; the helpers below use east as x and north as y.
SQUARE_CCW = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]
SQUARE = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]  # clockwise: north first, then east
L_SHAPE = [(0.0, 0.0), (0.0, 10.0), (4.0, 10.0), (4.0, 4.0), (10.0, 4.0), (10.0, 0.0)]
# U opens towards north: a 3 m wide notch cut into a 10 x 10 block.
U_SHAPE = [(0, 0), (0, 10), (10, 10), (10, 7), (4, 7), (4, 3), (10, 3), (10, 0)]


def tri_area(ring, t):
    (an, ae), (bn, be), (cn, ce) = (ring[i] for i in t)
    return 0.5 * ((be - ae) * (cn - an) - (bn - an) * (ce - ae))


def inside(ring, n, e):
    hit = False
    for (n0, e0), (n1, e1) in zip(ring, ring[1:] + ring[:1]):
        if (e0 > e) != (e1 > e) and n < n0 + (e - e0) / (e1 - e0) * (n1 - n0):
            hit = not hit
    return hit


def normal_up(tri):
    """Normal of a (north, east, down) triangle in real-world east/north/up axes."""
    p = [(e, n, -d) for n, e, d in tri]
    u = [p[1][i] - p[0][i] for i in range(3)]
    v = [p[2][i] - p[0][i] for i in range(3)]
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def test_square_and_l_shape_triangulate_to_n_minus_2():
    for raw in (SQUARE, L_SHAPE):
        ring = normalize_ring(raw)
        tris = triangulate(ring)
        assert len(tris) == len(ring) - 2
        assert all(tri_area(ring, t) > 0 for t in tris)


def test_clockwise_ring_is_flipped():
    assert signed_area(SQUARE_CCW) == 100.0
    assert signed_area(SQUARE) == -100.0
    original = list(SQUARE)
    ring = normalize_ring(SQUARE)
    assert signed_area(ring) == 100.0
    assert sorted(ring) == sorted(SQUARE)
    assert SQUARE == original


def test_closing_node_duplicates_and_mid_edge_nodes_are_dropped():
    raw = [(0, 0), (0, 0), (5, 0), (10, 0), (10, 10), (0, 10), (0, 0)]
    ring = normalize_ring(raw)
    assert len(ring) == 4 and signed_area(ring) == 100.0
    assert len(triangulate(ring)) == 2


def test_concave_u_shape_covers_its_area():
    ring = normalize_ring(U_SHAPE)
    tris = triangulate(ring)
    assert len(tris) == len(ring) - 2
    assert all(tri_area(ring, t) > 0 for t in tris)
    assert sum(tri_area(ring, t) for t in tris) == pytest.approx(signed_area(ring))
    assert signed_area(ring) == 100 - 24


def test_bad_rings_return_none():
    bad = {
        "two nodes": [(0, 0), (5, 5)],
        "one node repeated": [(3, 3), (3, 3), (3, 3), (3, 3)],
        "collinear": [(0, 0), (5, 0), (10, 0), (20, 0)],
        "bow-tie": [(0, 0), (10, 10), (0, 10), (10, 0)],
        "0.1 m sliver": [(0, 0), (0, 1), (0.1, 1), (0.1, 0)],
        "spike": [(0, 0), (0, 10), (10, 10), (10, 5), (20, 5), (10, 5), (10, 0)],
        "vertex on another edge": [(0, 0), (0, 10), (10, 10), (10, 5), (0, 5), (5, 2)],
        "nan": [(0, 0), (0, 10), (math.nan, 10), (10, 0)],
        "inf": [(0, 0), (0, 10), (math.inf, 10), (10, 0)],
        "too big for a float": [(0, 0), (0, 10), (10**400, 10), (10, 0)],
        "not numbers": [(0, 0), (0, "a"), (10, 10)],
        "empty": [],
    }
    for name, ring in bad.items():
        assert normalize_ring(ring) is None, name


def test_minimum_area_is_half_a_square_metre():
    def rect(n, e):
        return [(0.0, 0.0), (0.0, e), (n, e), (n, 0.0)]

    for n, e in ((0.7, 0.8), (0.9, 0.9), (1.1, 0.9), (0.6, 1.0)):  # 0.56, 0.81, 0.99, 0.6 m2
        assert normalize_ring(rect(n, e)) is not None, (n, e)
    for n, e in ((0.4, 1.0), (0.5, 0.9), (0.1, 1.0)):  # 0.4, 0.45, 0.1 m2
        assert normalize_ring(rect(n, e)) is None, (n, e)


def test_normalize_is_idempotent():
    for raw in (SQUARE, L_SHAPE, U_SHAPE):
        once = normalize_ring(raw)
        assert normalize_ring(once) == once


def test_triangulate_rejects_a_ring_it_cannot_clip():
    with pytest.raises(ValueError):
        triangulate([(0, 0), (0, 1), (0, 2), (0, 3)])
    with pytest.raises(ValueError):
        triangulate([(0, 0), (0, 1)])


def test_extrusion_is_closed_and_outward():
    # Hexagon (convex): every wall normal points away from the centroid.
    hexagon = [(10 * math.cos(k * math.pi / 3), 10 * math.sin(k * math.pi / 3)) for k in range(6)]
    for ring in (hexagon, SQUARE, L_SHAPE, U_SHAPE):
        mb = MeshBuilder()
        extrude(mb, ring, 6.0, "wall")
        norm = normalize_ring(ring)
        edges = {}
        for tri in (t[:3] for t in mb.triangles):
            nx, ny, nz = normal_up(tri)
            assert math.hypot(nx, ny, nz) > 1e-9  # no degenerate triangle
            if all(p[2] == -6.0 for p in tri):
                assert nz > 0  # roof faces up (down is negative)
            else:
                assert nz == 0
                # Probe a metre either side of the wall: out is outside, in is inside.
                cn = sum(p[0] for p in tri) / 3
                ce = sum(p[1] for p in tri) / 3
                k = 1 / math.hypot(nx, ny)
                assert not inside(norm, cn + ny * k, ce + nx * k)
                assert inside(norm, cn - ny * k, ce - nx * k)
            for i in range(3):
                e = tuple(sorted((tri[i], tri[(i + 1) % 3])))
                edges[e] = edges.get(e, 0) + 1
        for (a, b), uses in edges.items():
            on_ground = a[2] == 0 and b[2] == 0
            assert uses == (1 if on_ground else 2), (a, b)  # no floor: ground edge is open


def test_extrusion_height_and_roof_level():
    mb = MeshBuilder()
    ring = list(SQUARE)
    extrude(mb, ring, 7.5, "wall")
    downs = {p[2] for t in mb.triangles for p in t[:3]}
    assert downs == {0.0, -7.5}
    assert {t[3] for t in mb.triangles} == {"wall"}
    assert len(mb.triangles) == 4 * 2 + 2  # 4 wall quads + 2 roof triangles
    assert ring == SQUARE


def test_extrude_rejects_bad_input():
    mb = MeshBuilder()
    with pytest.raises(ValueError):
        extrude(mb, [(0, 0), (5, 5)], 5.0, "wall")
    for h in (0.0, -3.0, math.nan, math.inf):
        with pytest.raises(ValueError):
            extrude(mb, SQUARE, h, "wall")
    assert mb.triangles == []
