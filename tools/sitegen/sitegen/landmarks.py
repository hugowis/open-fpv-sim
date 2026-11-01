import math

from sitegen.geo import Projection
from sitegen.mesh import MeshBuilder
from sitegen.polygon import EPS_M, MAX_COORD_M, normalize_ring, signed_area, triangulate

# shortcut: no col_ nodes in v1 - the visible geometry is what collides, so a door or breach is
# passable exactly where the mesh is open; add collision proxies if a landmark needs a solid core.
# Every builder validates, then fills a scratch mesh, so a ValueError never leaves half a landmark.


def _err(field, msg):
    return ValueError(f"{field}: {msg}")


def _real(field, v):
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        raise _err(field, "must be a number")
    try:
        x = float(v)
    except OverflowError:
        x = math.inf
    if not abs(x) <= MAX_COORD_M:  # also rejects NaN
        raise _err(field, f"must be finite and within {MAX_COORD_M:g}")
    return x


def _size(field, v, zero_ok=False):
    x = _real(field, v)
    if not (x >= 0 if zero_ok else x > 0):
        raise _err(field, "must be > 0" if not zero_ok else "must be >= 0")
    return x


def _int(field, v, lo, hi):
    if isinstance(v, bool) or not isinstance(v, int) or not lo <= v <= hi:
        raise _err(field, f"must be an integer from {lo} to {hi}")
    return v


def _seq(field, v, n=None):
    if not isinstance(v, (list, tuple)) or (n is not None and len(v) != n):
        raise _err(field, "must be a list" if n is None else f"must be a list of {n} values")
    return list(v)


def _pair(field, v):
    a, b = _seq(field, v, 2)
    return _real(f"{field}[0]", a), _real(f"{field}[1]", b)


def _material(field, v):
    if not isinstance(v, str) or not v:
        raise _err(field, "must be a non-empty string")
    return v


def _tri(mb, a, b, c, material, out):
    """Adds a triangle whose front face looks along `out` (NED); skips slivers."""
    u, v = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
    n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
    if math.sqrt(sum(x * x for x in n)) < 1e-9:
        return
    # A triangle ordered north-then-east faces down, so the cross product is the front normal.
    if sum(n[i] * out[i] for i in range(3)) >= 0:
        mb.add_tri(a, b, c, material)
    else:
        mb.add_tri(a, c, b, material)


def _quad(mb, a, b, c, d, material, out):
    _tri(mb, a, b, c, material, out)
    _tri(mb, a, c, d, material, out)


def _top(prof, v):
    """Height of the polyline vault profile (v descending) at v."""
    for (v0, z0), (v1, z1) in zip(prof, prof[1:]):
        if v1 <= v <= v0:
            return z0 + (z1 - z0) * (v - v0) / (v1 - v0)
    raise _err("end_doors", "door does not fit inside the vault")


def arch(mb, center_ne, heading_deg, span_m, length_m, height_m, material, segments=48, end_doors=()):
    """Half-elliptical vault on the ground over a rectangle; its axis runs along the heading."""
    cn, ce = _pair("center", center_ne)
    heading = math.radians(_real("heading_deg", heading_deg))
    span, length, height = _size("span_m", span_m), _size("length_m", length_m), _size("height_m", height_m)
    material = _material("material", material)
    segments = _int("segments", segments, 4, 4096)
    if segments % 2:
        raise _err("segments", "must be even so the crown is a vertex")
    a = span / 2
    # (v right of the axis, z up) from the right foot over the crown to the left foot, v strictly descending.
    prof = [(a * math.cos(math.pi * i / segments), height * math.sin(math.pi * i / segments) if 0 < i < segments else 0.0)
            for i in range(segments + 1)]
    doors = {}
    for i, d in enumerate(_seq("end_doors", end_doors)):
        f = f"end_doors[{i}]"
        end, w, h = _seq(f, d, 3)
        if end not in ("front", "back"):
            raise _err(f, "end must be 'front' or 'back'")
        if end in doors:
            raise _err(f, f"a second door on the {end} end")
        w, h = _size(f"{f}[1]", w), _size(f"{f}[2]", h)
        if not w < span or h > _top(prof, w / 2):
            raise _err(f, "door does not fit inside the vault")
        doors[end] = (w / 2, h)

    fc, fs = math.cos(heading), math.sin(heading)  # forward and, rotated clockwise, right = (-fs, fc)

    def pt(u, v, z):
        return (cn + u * fc - v * fs, ce + u * fs + v * fc, -z)

    out = MeshBuilder()
    for (v0, z0), (v1, z1) in zip(prof, prof[1:]):
        ov, oz = z1 - z0, v0 - v1  # outward normal of the chord in (v, z)
        _quad(out, pt(-length / 2, v0, z0), pt(length / 2, v0, z0), pt(length / 2, v1, z1), pt(-length / 2, v1, z1),
              material, (-fs * ov, fc * ov, -oz))
    for sign, end in ((1, "front"), (-1, "back")):
        u, normal = sign * length / 2, (sign * fc, sign * fs, 0.0)
        half, door_h = doors.get(end, (0.0, 0.0))
        pts = prof
        if end in doors:  # break the end wall at the door jambs so no column spans the doorway
            pts = sorted(prof + [(v, _top(prof, v)) for v in (half, -half)], key=lambda p: -p[0])
        for (v0, z0), (v1, z1) in zip(pts, pts[1:]):
            floor = door_h if abs(v0 + v1) / 2 < half else 0.0
            _quad(out, pt(u, v0, floor), pt(u, v1, floor), pt(u, v1, z1), pt(u, v0, z0), material, normal)
    mb.extend(out)


def stack(mb, center_ne, base_radius_m, top_radius_m, height_m, bands, segments=24):
    """Tapered tube standing on the ground; bands are (material, height fraction) from the top down."""
    cn, ce = _pair("center", center_ne)
    base, top = _size("base_radius_m", base_radius_m), _size("top_radius_m", top_radius_m, zero_ok=True)
    height = _size("height_m", height_m)
    segments = _int("segments", segments, 3, 4096)
    bs = [_seq(f"bands[{i}]", b, 2) for i, b in enumerate(_seq("bands", bands))]
    if not bs:
        raise _err("bands", "needs at least one band")
    fracs = [_size(f"bands[{i}][1]", f) for i, (_, f) in enumerate(bs)]
    mats = [_material(f"bands[{i}][0]", m) for i, (m, _) in enumerate(bs)]
    if abs(sum(fracs) - 1.0) > 1e-9:
        raise _err("bands", f"fractions must sum to 1, got {sum(fracs)!r}")

    def ring(h, k):  # k-th point of the ring at height h
        r = base + (top - base) * h / height
        a = 2 * math.pi * k / segments
        return (cn + r * math.cos(a), ce + r * math.sin(a), -h)

    # shortcut: open at both ends (no caps), a top-down view sees through it; add caps if a stack is ever viewed from above.
    out = MeshBuilder()
    hi, cum = height, 0.0
    for i, (mat, f) in enumerate(zip(mats, fracs)):
        cum += f
        lo = 0.0 if i == len(bs) - 1 else height * (1 - cum)
        for k in range(segments):
            mid = 2 * math.pi * (k + 0.5) / segments
            _quad(out, ring(lo, k), ring(lo, k + 1), ring(hi, k + 1), ring(hi, k), mat,
                  (math.cos(mid), math.sin(mid), 0.0))
        hi = lo
    mb.extend(out)


def walls(mb, corners_ne, height_m, material, openings=(), roof=False):
    """Vertical walls round a closed polyline (wall i joins corners i and i+1), openings cut as rectangles."""
    corners = [_pair(f"corners[{i}]", c) for i, c in enumerate(_seq("corners", corners_ne))]
    height = _size("height_m", height_m)
    material = _material("material", material)
    if not isinstance(roof, bool):
        raise _err("roof", "must be true or false")
    ring = normalize_ring(corners)
    if ring is None:
        raise _err("corners", "not a usable footprint")
    holes: dict[int, list] = {}
    for i, o in enumerate(_seq("openings", openings)):
        f = f"openings[{i}]"
        wall, off, w, lo, hi = _seq(f, o, 5)
        wall = _int(f"{f}[0]", wall, 0, len(corners) - 1)
        off, w, lo, hi = _real(f"{f}[1]", off), _size(f"{f}[2]", w), _real(f"{f}[3]", lo), _real(f"{f}[4]", hi)
        if not 0 <= lo < hi <= height:
            raise _err(f, f"needs 0 <= bottom < top <= the wall height {height:g}")
        holes.setdefault(wall, []).append((off, w, lo, hi))

    outside = 1.0 if signed_area(corners) > 0 else -1.0  # counter-clockwise: outside is right of each edge
    out = MeshBuilder()
    for i, (p0, p1) in enumerate(zip(corners, corners[1:] + corners[:1])):
        length, s = math.dist(p0, p1), 0.0
        rects = []  # (s0, s1, z0, z1) along the wall and up it
        for off, w, lo, hi in sorted(holes.get(i, [])):
            if off < s or off + w > length:
                raise _err("openings", f"wall {i}: opening at {off:g} (width {w:g}) overlaps another or leaves the wall")
            rects += [(s, off, 0.0, height), (off, off + w, 0.0, lo), (off, off + w, hi, height)]
            s = off + w
        rects.append((s, length, 0.0, height))
        if length <= EPS_M:
            continue
        dn, de = (p1[0] - p0[0]) / length, (p1[1] - p0[1]) / length
        normal = (-outside * de, outside * dn, 0.0)

        def at(s, z):
            return (p0[0] + dn * s, p0[1] + de * s, -z)

        for s0, s1, z0, z1 in rects:
            _quad(out, at(s0, z0), at(s1, z0), at(s1, z1), at(s0, z1), material, normal)
    if roof:  # flat, front face up (NED up is negative down)
        try:
            roof_tris = triangulate(ring)
        except ValueError as e:
            raise _err("roof", str(e)) from None
        for a, b, c in roof_tris:
            _tri(out, (*ring[a], -height), (*ring[b], -height), (*ring[c], -height), material, (0.0, 0.0, -1.0))
    mb.extend(out)


def _need(p, key):
    if key not in p:
        raise _err(key, "missing")
    return p[key]


def _latlon(field, v, projection):
    lat, lon = _pair(field, v)
    if not (-90 <= lat <= 90 and -180 <= lon <= 180):
        raise _err(field, "latitude must be within -90..90 and longitude within -180..180")
    return projection.to_ned(lat, lon)


def _place(p, projection):
    if ("center" in p) == ("center_ned_m" in p):
        raise _err("center", "give exactly one of center, center_ned_m")
    return _pair("center_ned_m", p["center_ned_m"]) if "center_ned_m" in p else _latlon("center", p["center"], projection)


def build_landmark(mb: MeshBuilder, spec, projection: Projection) -> None:
    """Dispatch a recipe landmark; a ValueError reads `<landmark name>.<field>: <message>`."""
    p = spec.params
    try:
        if spec.kind == "arch":
            arch(mb, _place(p, projection), p.get("heading_deg", 0), _need(p, "span_m"),
                 _need(p, "length_m"), _need(p, "height_m"), _need(p, "material"), p.get("segments", 48),
                 p.get("end_doors", ()))
        elif spec.kind == "stack":
            stack(mb, _place(p, projection), _need(p, "base_radius_m"),
                  _need(p, "top_radius_m"), _need(p, "height_m"), _need(p, "bands"), p.get("segments", 24))
        elif spec.kind == "walls":
            if ("corners" in p) == ("corners_ned_m" in p):
                raise _err("corners", "give exactly one of corners, corners_ned_m")
            if "corners_ned_m" in p:
                corners = p["corners_ned_m"]
            else:
                corners = [_latlon(f"corners[{i}]", c, projection) for i, c in enumerate(_seq("corners", p["corners"]))]
            walls(mb, corners, _need(p, "height_m"), _need(p, "material"), p.get("openings", ()), p.get("roof", False))
        else:
            raise _err("kind", f"unknown kind {spec.kind!r} (expected arch, stack or walls)")
    except ValueError as e:
        raise ValueError(f"{spec.name}.{e}") from None
