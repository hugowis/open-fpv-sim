import math
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path

# Landmark keys per kind; the values are validated by the builders (Task 5).
_PLACE = {"center", "center_ned_m"}
_LANDMARK_KEYS = {
    "arch": _PLACE | {"heading_deg", "span_m", "length_m", "height_m", "material", "segments", "end_doors"},
    "stack": _PLACE | {"base_radius_m", "top_radius_m", "height_m", "bands", "segments"},
    "walls": {"corners", "corners_ned_m", "height_m", "material", "openings", "roof"},
}
_PLACE_KEYS = {"arch": _PLACE, "stack": _PLACE, "walls": {"corners", "corners_ned_m"}}


class RecipeError(Exception):
    """args[0] lists every problem, one `field.path: message` per line."""


@dataclass(frozen=True)
class Site:
    name: str
    origin: tuple[float, float]
    half_extent_m: tuple[float, float]
    snapshot: str
    snapshot_path: Path


@dataclass(frozen=True)
class Defaults:
    building_height_m: float = 8.0
    level_height_m: float = 3.0


@dataclass(frozen=True)
class Material:
    name: str
    rf_loss_db: float


@dataclass(frozen=True)
class Footprint:
    id: int
    skip: bool
    height_m: float | None
    material: str | None
    openings: tuple = ()  # (wall, offset_m, width_m, bottom_m, top_m); the wall indexes the normalised ring


@dataclass(frozen=True)
class Landmark:
    kind: str
    name: str
    params: dict  # raw builder values; shared with the TOML parse, do not mutate


@dataclass(frozen=True)
class Recipe:
    site: Site
    world_path: Path  # resolved against the recipe's directory
    glb_rel: str  # as written: the world file's `file`, relative to the world file
    glb_path: Path  # resolved against the world file's directory
    defaults: Defaults
    colors: dict[str, tuple[float, float, float]]
    materials: tuple[Material, ...]
    footprints: tuple[Footprint, ...]
    landmarks: tuple[Landmark, ...]
    world: dict  # copied verbatim into the world file


def _num(v):
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return False
    try:  # TOML ints are unbounded: float() overflows past ~1e308, which is not finite either
        return math.isfinite(float(v))
    except OverflowError:
        return False


class _Check:
    def __init__(self):
        self.errs: list[str] = []

    def err(self, path, msg):
        self.errs.append(f"{path}: {msg}")

    def table(self, parent, key, path, allowed, required=True):
        t = parent.get(key)
        if t is None:
            if required:
                self.err(path, "missing table")
            return {}
        if not isinstance(t, dict):
            self.err(path, "must be a table")
            return {}
        if allowed is not None:
            self.keys(t, path, allowed)
        return t

    def items(self, d, key):
        """(index, path, table) for each entry of the array-of-tables d[key]; bad shapes are recorded."""
        a = d.get(key, [])
        if not isinstance(a, list):
            self.err(key, "must be an array of tables")
            return
        for i, t in enumerate(a):
            if isinstance(t, dict):
                yield i, f"{key}[{i}]", t
            else:
                self.err(f"{key}[{i}]", "must be a table")

    def keys(self, t, path, allowed):
        for k in sorted(set(t) - set(allowed)):
            self.err(f"{path}.{k}", "unknown key")

    def get(self, t, path, key, kind, required=True):
        """Value of t[key] if it is a `kind`, else records the problem and returns None."""
        if key not in t:
            if required:
                self.err(f"{path}.{key}", "missing")
            return None
        v = t[key]
        if kind is float and _num(v) or kind is not float and isinstance(v, kind) and not (kind is int and isinstance(v, bool)):
            return float(v) if kind is float else v
        self.err(f"{path}.{key}", f"must be a {kind.__name__}")
        return None

    def pair(self, t, path, key, lo, hi, positive=False):
        v = t.get(key)
        if not (isinstance(v, list) and len(v) == 2 and all(_num(x) for x in v)):
            self.err(f"{path}.{key}", "missing" if key not in t else "must be [a, b] numbers")
            return None
        a, b = float(v[0]), float(v[1])
        if positive and not (a > 0 and b > 0):
            self.err(f"{path}.{key}", "both values must be > 0")
        elif not positive and not (lo[0] <= a <= hi[0] and lo[1] <= b <= hi[1]):
            self.err(f"{path}.{key}", f"must lie within {lo[0]}..{hi[0]}, {lo[1]}..{hi[1]}")
        return a, b

    def positive(self, t, path, key, required=False):
        v = self.get(t, path, key, float, required)
        if v is not None and not v > 0:
            self.err(f"{path}.{key}", "must be > 0")
        return v


def _site(c, d):
    s = c.table(d, "site", "site", {"name", "origin", "half_extent_m", "snapshot"})
    name = c.get(s, "site", "name", str)
    if name is not None and not re.fullmatch(r"[A-Za-z0-9_-]+", name):
        c.err("site.name", "only letters, digits, _ and -")
    origin = c.pair(s, "site", "origin", (-90, -180), (90, 180))
    extent = c.pair(s, "site", "half_extent_m", None, None, positive=True)
    snap = c.get(s, "site", "snapshot", str)
    return name, origin, extent, snap


def _openings(c, f, p):
    v = f.get("openings", [])
    if not isinstance(v, list):
        c.err(f"{p}.openings", "must be a list of [wall, offset_m, width_m, bottom_m, top_m]")
        return ()
    out = []
    for k, o in enumerate(v):
        q, before = f"{p}.openings[{k}]", len(c.errs)
        if not (isinstance(o, list) and len(o) == 5):
            c.err(q, "must be [wall, offset_m, width_m, bottom_m, top_m]")
            continue
        if isinstance(o[0], bool) or not isinstance(o[0], int) or o[0] < 0:
            c.err(f"{q}[0]", "must be an integer >= 0")
        for j in range(1, 5):
            if not _num(o[j]):
                c.err(f"{q}[{j}]", "must be a finite number")
        if _num(o[2]) and not o[2] > 0:
            c.err(f"{q}[2]", "must be > 0")
        if len(c.errs) == before:
            out.append((o[0], *map(float, o[1:])))
    return tuple(out)


def _footprints(c, d):
    out, seen = [], set()
    for _, p, f in c.items(d, "footprints"):
        c.keys(f, p, {"id", "skip", "height_m", "material", "openings"})
        fid = c.get(f, p, "id", int)
        if fid is not None and fid in seen:
            c.err(f"{p}.id", f"duplicate id {fid}")
        seen.add(fid)
        out.append(Footprint(fid, bool(c.get(f, p, "skip", bool, False)), c.positive(f, p, "height_m"),
                             c.get(f, p, "material", str, False), _openings(c, f, p)))
    return tuple(out)


def _landmarks(c, d):
    out, seen = [], set()
    for _, p, m in c.items(d, "landmarks"):
        kind, name = c.get(m, p, "kind", str), c.get(m, p, "name", str)
        if name is not None:
            if name in seen:
                c.err(f"{p}.name", f"duplicate name {name!r}")
            seen.add(name)
        if kind is None:
            continue
        if kind not in _LANDMARK_KEYS:
            c.err(f"{p}.kind", f"unknown kind {kind!r} (expected {', '.join(_LANDMARK_KEYS)})")
            continue
        c.keys(m, p, _LANDMARK_KEYS[kind] | {"kind", "name"})
        given = sorted(_PLACE_KEYS[kind] & set(m))
        if len(given) != 1:
            c.err(p, f"give exactly one of {', '.join(sorted(_PLACE_KEYS[kind]))}")
        out.append(Landmark(kind, name, {k: v for k, v in m.items() if k not in ("kind", "name")}))
    return tuple(out)


def load_recipe(path: Path) -> Recipe:
    path = Path(path)
    try:
        with open(path, "rb") as f:
            d = tomllib.load(f)
    except OSError as e:
        raise RecipeError(f"file: {e}") from None
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as e:
        raise RecipeError(f"toml: {e}") from None
    c = _Check()
    for k in sorted(set(d) - {"site", "output", "defaults", "colors", "materials", "footprints", "landmarks", "world"}):
        c.err(k, "unknown key")

    name, origin, extent, snap = _site(c, d)
    o = c.table(d, "output", "output", {"world", "glb"})
    world, glb = c.get(o, "output", "world", str), c.get(o, "output", "glb", str)
    for field, v in (("site.snapshot", snap), ("output.world", world), ("output.glb", glb)):
        if v == "":
            c.err(field, "must not be empty")
    df = c.table(d, "defaults", "defaults", {"building_height_m", "level_height_m"}, required=False)
    defaults = Defaults(c.positive(df, "defaults", "building_height_m") or 8.0,
                        c.positive(df, "defaults", "level_height_m") or 3.0)

    colors = {}
    ct = c.table(d, "colors", "colors", None)
    for k, v in ct.items():
        if isinstance(v, list) and len(v) == 3 and all(_num(x) and 0 <= x <= 1 for x in v):
            colors[k] = tuple(float(x) for x in v)
        else:
            c.err(f"colors.{k}", "must be [r, g, b] with each in 0..1")
    if "colors" in d and "building" not in ct:
        c.err("colors.building", "missing")

    mats, seen = [], set()
    for _, p, m in c.items(d, "materials"):
        c.keys(m, p, {"name", "rf_loss_db"})
        n, loss = c.get(m, p, "name", str), c.get(m, p, "rf_loss_db", float)
        if n is not None and n in seen:
            c.err(f"{p}.name", f"duplicate name {n!r}")
        seen.add(n)
        if loss is not None and loss < 0:
            c.err(f"{p}.rf_loss_db", "must be >= 0")
        mats.append(Material(n, loss))

    footprints, landmarks = _footprints(c, d), _landmarks(c, d)
    wt = d.get("world", {})
    if not isinstance(wt, dict):
        c.err("world", "must be a table")
        wt = {}
    if c.errs:
        raise RecipeError("\n".join(c.errs))

    world_path = path.parent / world
    return Recipe(Site(name, origin, extent, snap, path.parent / snap), world_path, glb,
                  world_path.parent / glb, defaults, colors, tuple(mats), footprints, landmarks, wt)
