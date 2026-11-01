import json
import math
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path

from .geo import EARTH_RADIUS_M
from .recipe import RecipeError, load_recipe

OVERPASS_URL = "https://overpass-api.de/api/interpreter"
USER_AGENT = "open-fpv-sim-sitegen/0.1 (+https://github.com/hugowis/open-fpv-sim)"
ATTRIBUTION = "© OpenStreetMap contributors, ODbL 1.0 https://www.openstreetmap.org/copyright"
FORMAT = 1
KEEP_TAGS = ("building", "height", "building:levels", "name", "name:en", "man_made", "roof:shape")
SKIP_KEYS = ("open_way", "relation", "no_geometry", "malformed")
RETRY_CODES = (429, 502, 504)
BACKOFF_S = 25
MAX_HEIGHT_M = 1000.0  # a taller "height" tag is a typo; fall through to the next rule


class FetchError(Exception):
    pass


@dataclass(frozen=True)
class Way:
    id: int
    tags: dict
    ring: list  # (lat, lon) as delivered: a closed way repeats its first node last


def _coord(n, key, lim):
    v = n.get(key) if isinstance(n, dict) else None
    ok = isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) and abs(v) <= lim
    return float(v) if ok else None


def _ring(geometry):
    ring = []
    for n in geometry:
        lat, lon = _coord(n, "lat", 90), _coord(n, "lon", 180)
        if lat is None or lon is None:
            return None
        ring.append((lat, lon))
    return ring


def parse_overpass(data: dict) -> tuple[list[Way], dict[str, int]]:
    """Closed ways with geometry, sorted by id, and a count per skip reason (every key always present)."""
    skipped = dict.fromkeys(SKIP_KEYS, 0)
    ways = []
    for e in data.get("elements", []) if isinstance(data, dict) else []:
        kind = e.get("type") if isinstance(e, dict) else None
        if kind == "relation":
            skipped["relation"] += 1
            continue
        wid, tags, geom = (e.get("id"), e.get("tags", {}), e.get("geometry")) if kind == "way" else (None, None, None)
        if not isinstance(wid, int) or isinstance(wid, bool) or not isinstance(tags, dict):
            skipped["malformed"] += 1
        elif not isinstance(geom, list) or not geom:
            skipped["no_geometry"] += 1
        elif (ring := _ring(geom)) is None:
            skipped["malformed"] += 1
        elif len(ring) < 4 or ring[0] != ring[-1]:
            skipped["open_way"] += 1
        else:
            kept = {k: v for k, v in tags.items() if k in KEEP_TAGS and isinstance(v, str)}
            ways.append(Way(wid, kept, ring))
    return sorted(ways, key=lambda w: w.id), skipped


def _number(s, metres):
    s = s.strip()
    if metres and s and s[-1] in "mM":  # shortcut: only metres; "70 ft" and friends fall through, add units if a site needs them
        s = s[:-1]
    try:
        v = float(s.replace(",", "."))
    except ValueError:
        return None
    return v if math.isfinite(v) and 0 < v <= MAX_HEIGHT_M else None


def height_of(tags, building_height_m, level_height_m) -> float:
    h = _number(tags.get("height", ""), True)
    if h is not None:
        return h
    n = _number(tags.get("building:levels", ""), False)
    if n is not None and (h := n * level_height_m) <= MAX_HEIGHT_M:
        return h
    return building_height_m


def _query(bbox):
    return '[out:json][timeout:60];way["building"](%s);out tags geom;' % ",".join(str(v) for v in bbox)


def write_snapshot(path, ways, bbox, source_note):
    doc = {
        "attribution": ATTRIBUTION,
        "bbox": list(bbox),
        "query": _query(bbox),
        "source_note": source_note,
        "format": FORMAT,
        "ways": [{"id": w.id, "tags": w.tags, "ring": [list(p) for p in w.ring]} for w in sorted(ways, key=lambda w: w.id)],
    }
    path = Path(path)
    tmp = path.with_name(path.name + ".tmp")  # replace is atomic: a failed write never truncates a good snapshot
    try:
        with open(tmp, "w", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(doc, ensure_ascii=False, sort_keys=True, indent=1) + "\n")
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


def read_snapshot(path) -> list[Way]:
    try:
        with open(path, encoding="utf-8") as f:
            doc = json.load(f)
    except (OSError, ValueError) as e:
        raise ValueError(f"snapshot {path}: {e}") from None
    if not isinstance(doc, dict):
        raise ValueError(f"snapshot {path}: not a JSON object")
    if doc.get("format") != FORMAT:
        raise ValueError(f"snapshot {path}: unsupported format {doc.get('format')!r} (expected {FORMAT})")
    ways = []
    try:
        for w in doc["ways"]:
            wid, tags = w["id"], w["tags"]
            if not isinstance(wid, int) or isinstance(wid, bool) or not isinstance(tags, dict):
                raise TypeError("bad id or tags")
            ways.append(Way(wid, tags, [(float(lat), float(lon)) for lat, lon in w["ring"]]))
    except (KeyError, TypeError, ValueError) as e:
        raise ValueError(f"snapshot {path}: malformed way ({type(e).__name__}: {e})") from None
    return ways


def fetch(bbox, user_agent, *, opener=urllib.request.urlopen, sleep=time.sleep, tries=3) -> dict:
    req = urllib.request.Request(
        OVERPASS_URL, data=urllib.parse.urlencode({"data": _query(bbox)}).encode("ascii"),
        headers={"User-Agent": user_agent}, method="POST")
    for attempt in range(tries):
        try:
            with opener(req, timeout=90) as r:
                data = json.loads(r.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code in RETRY_CODES and attempt < tries - 1:
                sleep(BACKOFF_S)
                continue
            raise FetchError(f"Overpass answered HTTP {e.code}") from None
        except (OSError, ValueError) as e:  # URLError and TimeoutError are OSErrors; bad JSON/UTF-8 are ValueErrors
            raise FetchError(f"{type(e).__name__}: {e}") from None
        if not isinstance(data, dict) or not isinstance(data.get("elements"), list):
            raise FetchError("response is not an Overpass JSON object with an `elements` list")
        if "remark" in data:  # Overpass reports a timed-out or out-of-memory query as a 200 with a partial result
            raise FetchError(f"Overpass remark: {data['remark']}")
        return data
    raise FetchError("no attempts made")


def _bbox(origin, half_extent_m):
    lat0, lon0 = origin
    dlat = math.degrees(half_extent_m[0] / EARTH_RADIUS_M)
    dlon = math.degrees(half_extent_m[1] / (EARTH_RADIUS_M * math.cos(math.radians(lat0))))
    return tuple(round(v, 6) for v in (lat0 - dlat, lon0 - dlon, lat0 + dlat, lon0 + dlon))


def fetch_main(recipe_path: Path, *, opener=urllib.request.urlopen, sleep=time.sleep) -> int:
    def fail(msg):
        print(f"fetch: {msg}", file=sys.stderr)
        return 1

    try:
        site = load_recipe(recipe_path).site
    except RecipeError as e:
        return fail(f"recipe: {str(e).replace(chr(10), '; ')}")
    bbox = _bbox(site.origin, site.half_extent_m)
    try:
        ways, skipped = parse_overpass(fetch(bbox, USER_AGENT, opener=opener, sleep=sleep))
    except FetchError as e:
        return fail(f"{e}; {site.snapshot_path} left untouched")
    if not ways:
        return fail(f"no buildings in the response; {site.snapshot_path} left untouched")
    try:
        write_snapshot(site.snapshot_path, ways, bbox, f"Overpass API {OVERPASS_URL}")
    except OSError as e:
        return fail(f"cannot write {site.snapshot_path}: {e}")
    print(f"fetch: {len(ways)} buildings -> {site.snapshot_path} (skipped {skipped})")
    return 0
