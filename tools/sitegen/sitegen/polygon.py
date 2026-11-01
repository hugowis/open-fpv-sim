import math

from sitegen.mesh import MeshBuilder

Ring = list[tuple[float, float]]  # (north, east) metres

EPS_M = 1e-6  # points closer than this, or lines closer than this to a point, are the same
MIN_AREA_M2 = 0.5  # smaller footprints are mapping noise
MAX_COORD_M = 1e7  # keeps every product below finite-overflow range


def signed_area(ring: Ring) -> float:
    """Shoelace in (east, north) order: positive when counter-clockwise, north up and east right."""
    s = 0.0
    for (n0, e0), (n1, e1) in zip(ring, ring[1:] + ring[:1]):
        s += e0 * n1 - e1 * n0
    return s / 2


def _cross(o, a, b) -> float:
    """z of (a - o) x (b - o) with east as x, north as y; positive = counter-clockwise."""
    return (a[1] - o[1]) * (b[0] - o[0]) - (a[0] - o[0]) * (b[1] - o[1])


def _side(a, b, p) -> int:
    """Which side of a->b the point p is on (1 left, -1 right), 0 within EPS_M of the line."""
    c = _cross(a, b, p)
    return 0 if abs(c) <= EPS_M * math.dist(a, b) else (1 if c > 0 else -1)


def _segments_touch(a, b, c, d) -> bool:
    o1, o2, o3, o4 = _side(a, b, c), _side(a, b, d), _side(c, d, a), _side(c, d, b)
    if o1 == o2 == 0:  # collinear: compare the spans along the longer axis
        k = 0 if abs(b[0] - a[0]) >= abs(b[1] - a[1]) else 1
        return max(min(a[k], b[k]), min(c[k], d[k])) <= min(max(a[k], b[k]), max(c[k], d[k])) + EPS_M
    return o1 != o2 and o3 != o4


def normalize_ring(ring) -> Ring | None:
    """Counter-clockwise simple ring without repeated or mid-edge nodes, or None if unusable."""
    try:
        pts = [(float(n), float(e)) for n, e in ring]
    except (TypeError, ValueError, OverflowError):
        return None
    if not all(abs(v) <= MAX_COORD_M for p in pts for v in p):  # also rejects NaN
        return None
    out: Ring = []
    for p in pts:
        if not out or math.dist(out[-1], p) > EPS_M:
            out.append(p)
    while len(out) > 1 and math.dist(out[0], out[-1]) <= EPS_M:
        out.pop()
    # Nodes strictly inside a straight edge add nothing; folds back on themselves stay and fail below.
    again = True
    while again and len(out) >= 3:
        again = False
        for i in range(len(out)):
            a, b, c = out[i - 1], out[i], out[(i + 1) % len(out)]
            if _side(a, c, b) == 0 and (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]) > 0:
                del out[i]
                again = True
                break
    n = len(out)
    if n < 3 or abs(signed_area(out)) < MIN_AREA_M2:
        return None
    if any(_side(out[i - 1], out[(i + 1) % n], out[i]) == 0 for i in range(n)):
        return None  # a remaining collinear triple is a spike
    if signed_area(out) < 0:
        out.reverse()
    for i in range(n):
        for j in range(i + 2, n):
            if i == 0 and j == n - 1:
                continue  # neighbours across the closing edge
            if _segments_touch(out[i], out[i + 1], out[j], out[(j + 1) % n]):
                return None
    return out


def triangulate(ring: Ring) -> list[tuple[int, int, int]]:
    """Ear clipping of a normalised ring into counter-clockwise triangles of ring indices.

    A normalised ring always yields len(ring) - 2 triangles. Anything it cannot clip (not simple,
    not counter-clockwise, collinear) raises ValueError rather than returning a partial mesh.
    """
    if len(ring) < 3:
        raise ValueError("a ring needs at least 3 nodes")
    idx = list(range(len(ring)))
    tris = []
    while len(idx) >= 3:
        m = len(idx)
        for k in range(m):
            i, j, l = idx[k - 1], idx[k], idx[(k + 1) % m]
            a, b, c = ring[i], ring[j], ring[l]
            if _cross(a, b, c) <= EPS_M * math.dist(a, c):
                continue  # reflex or flat corner
            if any(
                _side(a, b, ring[p]) >= 0 and _side(b, c, ring[p]) >= 0 and _side(c, a, ring[p]) >= 0
                for p in idx
                if p not in (i, j, l)
            ):
                continue
            tris.append((i, j, l))
            del idx[k]
            break
        else:
            raise ValueError("no ear found: ring is not a simple counter-clockwise polygon")
    return tris


def extrude(mb: MeshBuilder, ring, height_m: float, material: str) -> None:
    """Walls up to a flat roof at down = -height_m; no floor. Faces point out and up in the world."""
    if not 0 < height_m < MAX_COORD_M:
        raise ValueError(f"height_m must be positive and finite, got {height_m}")
    pts = normalize_ring(ring)
    if pts is None:
        raise ValueError("ring is not a usable footprint")
    top = -height_m
    for (n0, e0), (n1, e1) in zip(pts, pts[1:] + pts[:1]):
        # Counter-clockwise ring: the right-hand side of each edge is outside.
        mb.add_quad((n0, e0, 0.0), (n1, e1, 0.0), (n1, e1, top), (n0, e0, top), material)
    for i, j, k in triangulate(pts):
        mb.add_tri((*pts[i], top), (*pts[j], top), (*pts[k], top), material)
