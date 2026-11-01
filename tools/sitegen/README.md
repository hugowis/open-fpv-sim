# sitegen: a world from real data

Turns a **recipe** (a TOML file of your decisions) and an **OpenStreetMap snapshot** (a JSON file of building outlines) into
a triangle mesh (`.glb`) and a world file the simulator loads (`[[meshes]]`, see "The world file" in
`docs/dev-setup.md`). Pure standard library, Python 3.11 or newer (it reads TOML with `tomllib`); nothing to install.

Building outlines come from OpenStreetMap contributors, ODbL 1.0 (<https://www.openstreetmap.org/copyright>). The licence
note, with the open question of whether it is fine under this repository's GPLv3, is `docs/research/sitegen-licensing.md`
(status: unconfirmed). **No real snapshot is committed until the owner has confirmed that note.**

## Commands

Run from `tools/sitegen`, or from the repository root with `PYTHONPATH=tools/sitegen`:

    cd tools/sitegen
    python -m sitegen fetch path/to/recipe.toml    # download the snapshot the recipe names (network)
    python -m sitegen build path/to/recipe.toml    # recipe + snapshot -> .glb + world file

    PYTHONPATH=tools/sitegen python -m sitegen build path/to/recipe.toml    # from the repository root

The demo, which needs no network: `python -m sitegen build tests/fixtures/demo/recipe.toml`. **Never `fetch` the demo.** Its
snapshot is hand-made (its `source_note` says so) and the goldens are built from it; the demo's bounding box sits on real ground, so a
`fetch` would return real OpenStreetMap data that replaces it, break the goldens and put ODbL data into the repository ahead of the
licence decision. A build of the demo writes under `tests/fixtures/demo/out/`, which `.gitignore` excludes. The command line has
no output-directory option (the tests pass `out_dir` to `build_site`, in Python); a recipe's `output.world` decides where files go.

| Command | What it does | Exit |
|---|---|---|
| `fetch <recipe>` | Asks Overpass for every `way["building"]` and `relation["building"]` in the box `site.origin` plus or minus `site.half_extent_m`, keeps the closed ways and the closed outer rings of multipolygon relations, and writes the snapshot to `site.snapshot`. Prints the count and what it skipped. | 1 on a bad recipe, a failed request, an empty answer or a write error; the existing snapshot is untouched |
| `build <recipe>` | Reads the recipe and the snapshot, builds the footprints and landmarks, writes the `.glb` and the world file, prints the report. | 1 on a bad recipe or a site that cannot be built; nothing is written |

Paths: the recipe argument is relative to where you run it. Inside the recipe, `site.snapshot` and `output.world` are
relative to the recipe's directory, and `output.glb` is relative to the world file's directory (it becomes the world file's
`[[meshes]] file`, which the engine resolves the same way). The demo is `tests/fixtures/demo/recipe.toml`: it writes
`out/demo.toml` and `out/assets/demo.glb` beside the recipe. To use a generated world, put it under `worlds/` and point the
game at it (`--world=`, or `load(world=...)`).

`fetch` is meant to run once per real site: commit the snapshot (once the licence note is confirmed), then every `build` is offline and repeatable. CI never touches
the network.

## The recipe

All lengths are metres, angles degrees. Coordinates are **NED from the site origin** (`site.origin`, a latitude and
longitude, is north 0, east 0): `[north, east]` pairs, down is negative up. The flat ground is d = 0 and the pilot's
`position_ned_m` in `[world]` uses the same frame. A projection on a sphere of radius 6 371 000 m turns latitude and
longitude into north and east (a local tangent plane, fine for a few kilometres).

An unknown key anywhere is an error, and a bad recipe reports every problem at once, one `table.key: message` per line.

| Table | Key | Required | Meaning |
|---|---|---|---|
| `[site]` | `name` | yes | Letters, digits, `_` and `-`. Names the mesh node and is the default world name. |
| | `origin` | yes | `[lat, lon]`, within -90..90 and -180..180. |
| | `half_extent_m` | yes | `[north, east]`, both > 0. The box the snapshot covers and the build keeps. |
| | `snapshot` | yes | Path of the snapshot file. |
| `[output]` | `world` | yes | Path of the world file to write. |
| | `glb` | yes | Path of the `.glb`, relative to the world file. |
| `[defaults]` | `building_height_m` | no | Height of a footprint with no usable height tag. Default 8. |
| | `level_height_m` | no | Metres per `building:levels`. Default 3. |
| `[colors]` | `<material> = [r, g, b]` | yes | Each in 0..1. A `building` entry is required; every material a triangle uses needs one. |
| `[[materials]]` | `name`, `rf_loss_db` | no (the table) | Copied into the world file's `[[materials]]`. Within an entry both keys are required; `rf_loss_db` >= 0; names are unique. |
| `[[footprints]]` | `id` | yes | An OSM way id, unique in the recipe. |
| | `skip` | no | `true` leaves the building out. |
| | `height_m` | no | > 0. Overrides the height rules. |
| | `material` | no | Default `building`. |
| `[[landmarks]]` | `kind`, `name` | yes | `arch`, `stack` or `walls`; names are unique. The other keys are in the next table. |
| `[world]` | anything | no | Copied into the world file as is (`[pilot]`, `[receiver]`, `[[objects]]`, `name`...). `meshes` and `materials` are refused: sitegen writes them. `schema_version` is 1 and `name` is the site name unless `[world]` sets them. |

Heights of a footprint, in order: the recipe's `height_m`; the OSM `height` tag (a number, optionally ending in `m`, comma or
dot decimal); `building:levels` times `level_height_m`; `building_height_m`. A tag that is not a number in (0, 1000] is
ignored and the next rule applies (feet and other units fall through).

### Landmarks

Each landmark takes exactly one of the two placing keys (`center` or `center_ned_m` for an arch and a stack; `corners` or
`corners_ned_m` for walls): a `[lat, lon]` form or a `[north, east]` form. Every number is finite and at most 1e7 in
magnitude. Errors read `<landmark name>.<field>: <message>`.

| Kind | Key | Required | Meaning and limits |
|---|---|---|---|
| `arch` | `center` / `center_ned_m` | one | Centre of the footprint. |
| | `heading_deg` | no | Direction of the axis, clockwise from north. Default 0. The front end is the end the heading points to. |
| | `span_m`, `length_m`, `height_m` | yes | > 0. A half-ellipse vault on the ground: width `span_m`, height `height_m`, along `length_m`. |
| | `material` | yes | |
| | `segments` | no | Even integer, 4 to 4096. Default 48. |
| | `end_doors` | no | List of `["front" or "back", width_m, height_m]`, one per end. The width is less than `span_m`; the top corners of the door must lie under the vault. An end with no door is closed. |
| `stack` | `center` / `center_ned_m` | one | |
| | `base_radius_m` | yes | > 0. |
| | `top_radius_m` | yes | >= 0. The tube tapers linearly. |
| | `height_m` | yes | > 0. |
| | `bands` | yes | List of `[material, fraction]` from the top down. Fractions are > 0 and sum to 1. |
| | `segments` | no | Integer, 3 to 4096. Default 24. |
| `walls` | `corners` / `corners_ned_m` | one | A closed polyline, 3 or more corners, simple (no crossings or touching edges), area at least 0.5 m2. Either winding. Wall `i` joins corner `i` and corner `i + 1` (the last joins the first). |
| | `height_m`, `material` | yes | `height_m` > 0. |
| | `openings` | no | List of `[wall, offset_m, width_m, bottom_m, top_m]`: a rectangular hole in wall `wall`, starting `offset_m` along it from corner `wall`. `width_m` > 0, `0 <= bottom_m < top_m <= height_m`; openings on one wall may not overlap or run past its end. |
| | `roof` | no | `true` adds a flat roof. Default `false`. |

## What a build does

- Footprints come from the snapshot in id order. Each is cleaned (repeated and mid-edge nodes removed, made counter-clockwise),
  checked for simplicity, extruded to its height with a flat roof and no floor, and kept when the **bounding box of its outline
  touches** the extent rectangle (`|north| <= half_north`, `|east| <= half_east`, edges included). A building straddling the
  edge stays whole, even a 600 m hall with most of its length outside; one wholly outside is dropped.
- Landmarks are added after the footprints, in the recipe's order.
- One mesh node named after the site holds everything, one primitive per material. The world file gets `[[meshes]]` with
  that node's name and the `.glb` path, and a `[[materials]]` table (see below).
- The mesh sits at the world's origin with heading 0 (`[[meshes]]` has no `position_ned_m` or `heading_deg`), so the site's
  `origin` is NED (0, 0). Heading 0 is also the case the engine claims bit-identical across operating systems
  (`docs/dev-setup.md`).

### The report

`build` prints it (to stdout; a failure goes to stderr):

    <n> footprints, <n> landmarks, <n> triangles
    skipped: <reason>=<n>, ...        (or "none")
    materials: <names>
    warning: ...
    wrote <world file>
    wrote <glb>

Footprints left out, by reason:

| Reason | Cause |
|---|---|
| `recipe_skip` | The recipe's `skip = true`. |
| `duplicate_id` | The snapshot lists the id twice; the first wins. |
| `bad_ring` | The outline is not usable (fewer than 3 nodes, under 0.5 m2, self-touching, a spike). |
| `outside_extent` | The bounding box of its outline does not touch the extent rectangle. |
| `extrude_error` | The outline passed the checks but could not be triangulated. |

Warnings (the build still succeeds): a recipe footprint id that is not in the snapshot; a material the mesh uses but
`[[materials]]` does not list (it loses 0 dB); a listed material no triangle uses (left out of the world file, since the
engine refuses an unused one).

Build errors (exit 1, nothing written): a bad recipe; an unreadable or wrongly formatted snapshot; a material with no entry
in `[colors]`; a landmark error; `[world]` defining `meshes` or `materials`; a site with no geometry at all (a hand-made site
needs at least one landmark or footprint).

## Determinism

The same recipe and snapshot give **byte-identical** files: ways and materials are sorted, coordinates are rounded to 1 mm
and then to float32, the GLB's JSON has sorted keys, and the world file is written with LF and no timestamp. The golden files
`tests/golden/demo.glb` and `demo.toml` pin this, checked on Linux and Windows (they are built with sine and cosine, so a
platform whose maths library differs in the last bit would show here). `tests/golden/fixture.glb` uses basic float
operations only and is loaded by the engine's own `.glb` loader in a Rust test, so the two sides cannot drift. After an
intended change, `python tests/make_golden.py` rewrites the goldens; review the diff.

## Rules worth knowing

- **Every material is `doubleSided`.** The engine's collision is two-sided, and Godot draws no back faces unless the
  material asks for them, so a wall seen from its inside, or a building whose winding turned out wrong, would vanish.
  Triangle winding is still correct (outward, roofs up), the flag is a safety net. Colours are flat; there are no textures.
- **Frames.** The generator works in NED (x north, y east, z down). The GLB is written in Godot's frame (x east, y up,
  -z north): a point at NED (n, e, d) is stored at (e, -d, -n), a proper rotation, so the winding is kept. The engine's
  loader maps it back with NED = (-z, x, -y). Heading is clockwise from north.
- **Always UTF-8.** Snapshots are written and read as UTF-8 with `ensure_ascii=False` (OSM names are Cyrillic here), and the
  world file is UTF-8 with LF; the command-line tool also forces UTF-8 on its output because a piped Windows stream is
  cp1252. Do not let an editor change the encoding or the line endings.
- **A footprint is a closed OSM way or a closed outer ring of a multipolygon relation.** Open ways are not buildings. A
  relation (`type=multipolygon` or a `building` tag) becomes one footprint per closed outer ring, with the relation's tags;
  the ring is the `outer` members joined end to end (in member order, a member may be reversed, endpoints must match
  exactly). Its id is `-(relation_id * 100 + k)`, `k` = 1, 2, ... in the order the rings close, at most 99 per relation.
  Negative so it never collides with a way id; a `[[footprints]]` override takes it like any other id.
  **Holes are filled**: `inner` members are ignored and counted as `relation_hole`. A relation with no closable outer
  ring (a gap, a member the endpoint match cannot join, no geometry) is skipped and counted as `relation`.

## The snapshot

`<site>.osm.json`, sorted by way id, stable bytes, with a header: `attribution`, `bbox`, the exact `query`, `source_note`,
`format` (1), and `ways` (each `id`, `tags`, `ring` of `[lat, lon]`; a closed way repeats its first node). Only these tags
are kept: `building`, `height`, `building:levels`, `name`, `name:en`, `man_made`, `roof:shape` (only string values). `fetch`
counts the elements it drops: `open_way`, `relation`, `no_geometry`, `malformed`, and `relation_hole` (inner members
seen); every key is always present. Relation ids are negative and the file is sorted by id, so they come first.

A hand-written snapshot works too (the demo's is one); give it `"format": 1` and a `"ways"` list. `build` always writes the OpenStreetMap credit into the world header, even for hand-made outlines (over-crediting is harmless).

## Overpass etiquette (what `fetch` does)

The public server (`https://overpass-api.de/api/interpreter`) is shared. `fetch`:

- sends a descriptive `User-Agent` naming the tool and this repository. The public server answers HTTP 406 to a request
  without a usable one (the tests' fake server models this, it was not re-checked live in this task);
- asks once, and retries only on HTTP 429, 502 and 504, at most 3 tries in all, waiting 25 s between them; any other error,
  a timeout (90 s per request) or a network failure ends the run;
- treats an answer with a `remark` (Overpass's way of saying a query timed out or ran out of memory, with a 200 and a
  partial result), a body that is not an Overpass JSON object, or a truncated body as an error;
- **never overwrites a good snapshot**: an error or an answer with no buildings leaves the existing file as it was, and the
  new file is written to a temporary name and then renamed, so a failed write cannot leave half a file.

Fetch once, commit the result, and do not put `fetch` in a loop or in CI.

## Limits and non-goals

Deliberately not built (all of them are additions, not bugs):

- Roofs are flat: no pitched roofs, no use of `roof:shape` (it is kept in the snapshot). Footprints have no floor.
- No cooling-tower or other curved-surface builder. A `stack` is a tapered tube, **open at both ends** (a view from above
  sees through it); an `arch` is a half-ellipse vault.
- Flat ground only (d = 0): no elevation data. The engine supports `[terrain]`, but sitegen writes none and does not
  place the mesh on it.
- No textures, no vertex colours, one flat colour per material.
- Multipolygon holes (courtyards) are filled, and a ring that needs more than exact endpoint matching to close is skipped.
- No `col_` collision proxies: every visible triangle collides, so a door or a breach is passable exactly where the mesh is
  open.
- Units other than metres in a `height` tag are ignored.
- A footprint is placed by its nodes alone; the projection is a flat tangent plane, not a map projection.
- The engine's caps apply to the output (8 meshes, 400 000 triangles a world, 64 MiB a file; estimates, see
  `docs/dev-setup.md`).

## Tests

    python -m pytest tools/sitegen/tests -q     # from the repository root; CI runs this in the core and windows jobs

No network. The Rust test `crates/ofs-config/tests/sitegen_golden.rs` loads the goldens with the engine. All values the
generator produces are estimates.
