import http.client
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
FETCH_NOTE = f"Overpass API {OVERPASS_URL}"  # the source_note of a fetched snapshot; the licence-gate test keys off it
USER_AGENT = "open-fpv-sim-sitegen/0.1 (+https://github.com/hugowis/open-fpv-sim)"
ATTRIBUTION = "© OpenStreetMap contributors, ODbL 1.0 https://www.openstreetmap.org/copyright"
FORMAT = 1
KEEP_TAGS = ("building", "height", "building:levels", "name", "name:en", "man_made", "roof:shape")
SKIP_KEYS = ("open_way", "relation", "no_geometry", "malformed", "relation_hole")
MAX_RINGS = 99  # per relation: way ids are -(relation * 100 + k)
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


def _coord(v, lim):
    # The range test comes first: it also rejects NaN/inf and never overflows on a hostile huge JSON int, unlike isfinite/float.
    ok = isinstance(v, (int, float)) and not isinstance(v, bool) and abs(v) <= lim
    return float(v) if ok else None


def _ring(points):
    """[(lat, lon)] from (lat, lon) pairs, or None if any point is not a finite in-range number pair."""
    ring = []
    for lat, lon in points:
        lat, lon = _coord(lat, 90), _coord(lon, 180)
        if lat is None or lon is None:
            return None
        ring.append((lat, lon))
    return ring


def _stitch(pieces):
    """Closed rings (first point repeated last) from open polylines joined end to end, greedy and in any order; a piece may be reversed."""
    pieces, rings = [list(p) for p in pieces if len(p) > 1], []
    while pieces:
        cur = pieces.pop(0)
        while cur[0] != cur[-1]:
            for i, p in enumerate(pieces):
                if p[0] == cur[-1]:
                    cur += p[1:]
                elif p[-1] == cur[-1]:
                    cur += p[-2::-1]
                else:
                    continue
                del pieces[i]
                break
            else:
                break  # shortcut: only endpoint matches join, so a ring that needs a split or a snapped gap is skipped
        if cur[0] == cur[-1] and len(cur) >= 4:
            rings.append(cur)
    return rings


def _relation(e, skipped):
    """One Way per closed outer ring of a multipolygon (building) relation; holes are only counted."""
    rid, tags, members = e.get("id"), e.get("tags", {}), e.get("members")
    if not isinstance(rid, int) or isinstance(rid, bool) or rid <= 0 or not isinstance(tags, dict):
        skipped["malformed"] += 1
        return []
    if not (tags.get("type") == "multipolygon" or "building" in tags) or not isinstance(members, list):
        skipped["relation"] += 1
        return []
    outers = []
    for m in members:
        geom = m.get("geometry") if isinstance(m, dict) else None
        ref = m.get("ref") if isinstance(m, dict) else None
        if not isinstance(ref, int) or isinstance(ref, bool) or (geom is not None and not isinstance(geom, list)):
            skipped["malformed"] += 1
            return []
        if m.get("role") == "inner":
            skipped["relation_hole"] += 1
        elif m.get("role") == "outer" and geom:
            if (pts := _ring((n.get("lat"), n.get("lon")) if isinstance(n, dict) else (None, None) for n in geom)) is None:
                skipped["malformed"] += 1
                return []
            outers.append(pts)
    rings = _stitch(outers)
    kept = {k: v for k, v in tags.items() if k in KEEP_TAGS and isinstance(v, str)}
    skipped["relation"] += max(len(rings) - MAX_RINGS, 0 if rings else 1)
    return [Way(-(rid * 100 + k), kept, r) for k, r in enumerate(rings[:MAX_RINGS], 1)]


def parse_overpass(data: dict) -> tuple[list[Way], dict[str, int]]:
    """Closed ways and multipolygon relations' outer rings, sorted by id, and a count per skip reason (every key always present)."""
    skipped = dict.fromkeys(SKIP_KEYS, 0)
    ways = []
    for e in data.get("elements", []) if isinstance(data, dict) else []:
        kind = e.get("type") if isinstance(e, dict) else None
        if kind == "relation":
            ways += _relation(e, skipped)
            continue
        wid, tags, geom = (e.get("id"), e.get("tags", {}), e.get("geometry")) if kind == "way" else (None, None, None)
        if not isinstance(wid, int) or isinstance(wid, bool) or not isinstance(tags, dict):
            skipped["malformed"] += 1
        elif not isinstance(geom, list) or not geom:
            skipped["no_geometry"] += 1
        elif (ring := _ring((n.get("lat"), n.get("lon")) if isinstance(n, dict) else (None, None) for n in geom)) is None:
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
    b = ",".join(str(v) for v in bbox)
    return '[out:json][timeout:60]; (way["building"](%s); relation["building"](%s);); out tags geom;' % (b, b)


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
            if not all(isinstance(k, str) and isinstance(v, str) for k, v in tags.items()):
                raise TypeError("tags must map strings to strings")
            if (ring := _ring(w["ring"])) is None:
                raise ValueError("ring has a non-finite or out-of-range coordinate")
            ways.append(Way(wid, tags, ring))
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
                e.close()
                sleep(BACKOFF_S)
                continue
            e.close()
            raise FetchError(f"Overpass answered HTTP {e.code}") from None
        # URLError and TimeoutError are OSErrors, a truncated body is an HTTPException, bad JSON/UTF-8 are ValueErrors
        except (OSError, http.client.HTTPException, ValueError) as e:
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
        write_snapshot(site.snapshot_path, ways, bbox, FETCH_NOTE)
    except OSError as e:
        return fail(f"cannot write {site.snapshot_path}: {e}")
    print(f"fetch: {len(ways)} buildings -> {site.snapshot_path} (skipped {skipped})")
    return 0
