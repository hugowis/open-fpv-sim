def _sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def segment_hits(mb, a, b) -> int:
    """Triangles of the builder the segment a-b crosses (Moller-Trumbore, two-sided, NED points)."""
    d = _sub(b, a)
    hits = 0
    for p0, p1, p2, _ in mb.triangles:
        e1, e2 = _sub(p1, p0), _sub(p2, p0)
        h = _cross(d, e2)
        det = _dot(e1, h)
        if abs(det) < 1e-12:
            continue  # parallel to the triangle
        s = _sub(a, p0)
        u = _dot(s, h) / det
        q = _cross(s, e1)
        v = _dot(d, q) / det
        t = _dot(e2, q) / det
        if u >= 0 and v >= 0 and u + v <= 1 and 0 <= t <= 1:
            hits += 1
    return hits


def bounds(mb):
    pts = [p for t in mb.triangles for p in t[:3]]
    return [(min(p[i] for p in pts), max(p[i] for p in pts)) for i in range(3)]


def normal_ned(tri):
    """Front-face normal of an NED triangle: its cross product (NED and ENU are both right-handed)."""
    return _cross(_sub(tri[1], tri[0]), _sub(tri[2], tri[0]))
