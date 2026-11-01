# Site generator (piece 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A dev tool that turns a site recipe plus an OpenStreetMap snapshot into a `.glb` and a world file the engine already loads, so a real place becomes a map by changing data, not code.

**Architecture:** A stdlib-only Python package `tools/sitegen/sitegen/`. `fetch` saves a normalised OSM snapshot; `build` reads recipe + snapshot, extrudes building footprints, adds parametric landmarks (arch, stack, walls with openings), and writes a deterministic GLB (`doubleSided`, named materials) and `worlds/<site>.toml`. A Rust test loads the generator's golden GLB with the engine's own loader, so the two sides cannot drift.

**Tech Stack:** Python >= 3.11 (`tomllib`, `json`, `struct`, `urllib`), pytest; one Rust test in `ofs-config`. No new dependency anywhere.

**Spec:** `docs/superpowers/specs/2026-11-01-mesh-worlds-and-chernobyl-design.md` ("Piece 2"). Piece 1 (engine) is merged (#176). Piece 3 (the Chernobyl map) is a separate plan and is where the real OSM snapshot is committed.

## Global Constraints

- Output is **deterministic**: the same recipe and snapshot give byte-identical files. Coordinates are rounded to 1 mm before the float32 pack, dict/set order never leaks (sort by id/name), no timestamps. The golden is compared on Linux and Windows CI.
- Frames: the generator works in NED metres from the site origin (north, east, down; up is negative). The GLB is in Godot's frame: x east, y up, -z north, i.e. (x, y, z) = (east, -down, -north). `heading_deg` is clockwise from north.
- Every material is `doubleSided` (collision is two-sided and the engine draws no back faces otherwise; walls would vanish from one side).
- No `col_` nodes in v1 (the visible geometry collides). Add a `shortcut:` comment where this is assumed.
- Roofs of OSM footprints are flat; `roof:shape` is reported, not modelled.
- OSM data is ODbL: attribution "© OpenStreetMap contributors" in every generated world header and in the docs. **No snapshot is committed until the licence note (Task 8) is confirmed by the repo owner.** Tests use hand-written fixtures only; CI never touches the network.
- Always open text files with `encoding="utf-8"` (Python on Windows defaults to cp1252; OSM names are Cyrillic). JSON written with `ensure_ascii=False`, `\n` line endings, sorted keys.
- Overpass etiquette: a descriptive `User-Agent` (the public server answers 406 without one), retry on 429/502/504 with backoff, never overwrite a good snapshot with an empty or error response.
- Files LF. Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.

## Review Focus

- Odd OSM input: open ways, fewer than 3 nodes, duplicate or collinear nodes, self-intersecting rings, clockwise vs counter-clockwise, zero or absurd heights, `height="70 m"`: skipped or normalised and counted in the build report, never an exception or NaN (Tasks 4, 6).
- Faces outward and complete: every extruded building is closed (walls + flat roof; no floor needed) and a triangle never spans an opening (Tasks 4, 5).
- Frame check: a point 100 m north of the origin lands at z = -100 in the GLB and loads back as NED north +100 through the engine loader (Tasks 2, 7).
- Non-ASCII tags (Cyrillic names) survive snapshot -> report -> world header unchanged (Tasks 6, 7).
- A failed or partial Overpass response leaves the existing snapshot untouched and exits non-zero (Task 6).

---

### Task 1: Harden the `.glb` loader against stride and overflow panics (prerequisite)

Parked by the piece-1 final review (ledger Ruling 6): in a debug build a hand-crafted `.glb` can still panic the gltf crate. The generator's output goes through this loader, so close it first.

**Files:**
- Modify: `crates/ofs-config/src/mesh.rs`
- Test: `crates/ofs-config/tests/mesh.rs` (hand-built JSON as the existing hostile-file tests do)

**Interfaces:**
- Produces: no API change. `load_glb_with` returns `Err` naming the file for: a POSITION bufferView `byteStride` smaller than the element size (12); a bufferView or accessor whose `byteOffset + byteLength` overflows `u64`/exceeds the buffer; the same for the indices accessor.

- [ ] **Step 1: Write the failing tests** (`#[test]`, debug build): `a_short_byte_stride_is_an_error_not_a_panic` (POSITION F32 VEC3 with `byteStride: 4`), `an_overflowing_buffer_view_offset_is_an_error` (`byteOffset: 18446744073709551600`), `an_overflowing_accessor_offset_is_an_error`, `a_buffer_view_past_the_buffer_is_an_error`. Each asserts `Err` containing the path.
- [ ] **Step 2: Run** `cargo test -p ofs-config --test mesh` — the new tests panic in gltf (`util.rs:376`, `util.rs:12/384`).
- [ ] **Step 3: Implement** the pre-checks next to the existing accessor-type checks (checked arithmetic; stride >= element size; ranges inside the buffer) before any `reader.read_*`. If a case still escapes, fall back to `std::panic::catch_unwind` around the read and say so in the commit.
- [ ] **Step 4: Run** `cargo test -p ofs-config` and `cargo clippy -p ofs-config --all-targets --locked -- -D warnings` — pass.
- [ ] **Step 5: Commit** — `fix(config): a short byteStride or overflowing offset in a .glb is an error, not a panic`

---

### Task 2: Package skeleton, projection and the deterministic GLB writer

**Files:**
- Create: `tools/sitegen/sitegen/{__init__,geo,mesh,glb}.py`, `tools/sitegen/tests/{conftest,test_geo,test_glb}.py`
- Create: `crates/ofs-config/tests/sitegen_golden.rs`, `tools/sitegen/tests/golden/fixture.glb`

**Interfaces:**
- Produces:
  - `geo.Projection(lat0: float, lon0: float)`; `.to_ned(lat, lon) -> tuple[float, float]` (north, east; local tangent plane, R = 6_371_000 m, east scaled by cos(lat0)); `geo.quantize(x: float) -> float` (round to 1 mm).
  - `mesh.MeshBuilder` holding `triangles: list[tuple[Point, Point, Point, str]]` with `Point = tuple[float, float, float]` in NED, methods `add_tri(a, b, c, material)`, `add_quad(a, b, c, d, material)` (two triangles, corners in order), `extend(other)`.
  - `glb.write_glb(nodes: dict[str, MeshBuilder], materials: dict[str, tuple[float, float, float]]) -> bytes`: nodes and materials written in sorted-name order, NED -> Godot frame, `doubleSided: true`, one primitive per (node, material), float32 positions, uint32 indices, 4-byte aligned chunks.
- Consumes: nothing.

- [ ] **Step 1: Write the failing tests:** `test_to_ned_axes` (a point 0.001 deg north of the origin has north ~ 111.19 m, east 0; east scales with cos(lat)), `test_quantize_is_millimetre`, `test_glb_header_and_chunks` (magic `glTF`, version 2, total length, JSON chunk then BIN chunk), `test_frame_maps_north_to_minus_z` (a vertex at NED (100, 0, 0) is (0, 0, -100) in the BIN data), `test_materials_are_double_sided`, `test_same_input_same_bytes`, `test_node_order_does_not_depend_on_insertion_order`. Rust `sitegen_golden.rs`: `the_generators_golden_glb_loads` loads `tools/sitegen/tests/golden/fixture.glb` with `ofs_config::mesh::load_glb` and asserts the triangle count and that the point written at NED (100, 0, -5) comes back as north +100 (use `TriMesh::bounds`).
- [ ] **Step 2: Run** `python -m pytest tools/sitegen/tests -q` and `cargo test -p ofs-config --test sitegen_golden` — fail (no package / no file).
- [ ] **Step 3: Implement** the three modules (stdlib `struct`, `json`, `math`). `conftest.py` puts `tools/sitegen` on `sys.path`. Generate `fixture.glb` from a tiny fixed `MeshBuilder` via a test helper `tests/make_golden.py` and commit the bytes; `test_golden_is_current` regenerates and compares.
- [ ] **Step 4: Run** both suites — pass.
- [ ] **Step 5: Commit** — `feat(sitegen): projection, mesh builder and a deterministic GLB writer (+ engine loads the golden)`

---

### Task 3: The recipe

**Files:**
- Create: `tools/sitegen/sitegen/recipe.py`, `tools/sitegen/tests/test_recipe.py`

**Interfaces:**
- Consumes: `geo.Projection`.
- Produces: `recipe.load_recipe(path: Path) -> Recipe` (frozen dataclasses) and `recipe.RecipeError(Exception)` whose message lists every problem as `field.path: message`. Recipe TOML:

```toml
[site]
name = "demo"                      # letters, digits, _ and -
origin = [51.3895, 30.0990]        # lat, lon of NED (0, 0)
half_extent_m = [300.0, 300.0]     # north, east: footprints outside are dropped
snapshot = "demo.osm.json"         # relative to the recipe
[output]
world = "../../worlds/demo.toml"   # relative to the recipe file
glb = "assets/demo.glb"            # relative to the world file, as the engine resolves a mesh `file`
[defaults]
building_height_m = 8.0
level_height_m = 3.0
[colors]                           # material name -> [r, g, b], 0..1; "building" is required
building = [0.62, 0.60, 0.56]
[[materials]]                      # becomes [[materials]] in the world (rf_loss_db)
name = "building"
rf_loss_db = 18.0
[[footprints]]                     # per-OSM-way override
id = 123456
skip = true                        # or height_m = 70.0 / material = "concrete"
[[landmarks]]                      # Task 5 kinds: arch, stack, walls
kind = "stack"
name = "VP2"
# kind-specific numbers: see Task 5
[world]                            # copied verbatim into the world file (pilot, receiver, ...)
```

- [ ] **Step 1: Write the failing tests:** `test_minimal_recipe_loads`, `test_every_problem_is_reported_with_its_field_path` (bad name, origin out of range, non-positive extent, missing `colors.building`, colour outside 0..1, duplicate landmark name, unknown landmark `kind`, duplicate footprint id, unknown key anywhere -> error: no silent typos), `test_relative_paths_resolve_against_the_recipe`.
- [ ] **Step 2: Run** `python -m pytest tools/sitegen/tests/test_recipe.py` — fail.
- [ ] **Step 3: Implement** `load_recipe` with strict key checking (reject unknown keys, as the engine's world loader does).
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** — `feat(sitegen): the site recipe and its validation`

---

### Task 4: Footprint extrusion

**Files:**
- Create: `tools/sitegen/sitegen/polygon.py`, `tools/sitegen/tests/test_polygon.py`

**Interfaces:**
- Produces:
  - `polygon.signed_area(ring: list[tuple[float, float]]) -> float` (positive = counter-clockwise seen with north up, east right).
  - `polygon.normalize_ring(ring) -> list[tuple[float, float]] | None`: drops a repeated closing node and consecutive duplicates, returns counter-clockwise, `None` if fewer than 3 distinct nodes, near-zero area (< 0.5 m^2), or self-intersecting.
  - `polygon.triangulate(ring) -> list[tuple[int, int, int]]` (ear clipping on a normalised simple ring, counter-clockwise triangles, `len(ring) - 2` of them).
  - `polygon.extrude(mb: MeshBuilder, ring, height_m: float, material: str) -> None`: wall quads (outward facing), a flat roof at `-height_m` (down negative), no floor.

- [ ] **Step 1: Write the failing tests:** `test_square_and_l_shape_triangulate_to_n_minus_2`, `test_clockwise_ring_is_flipped`, `test_concave_u_shape_covers_its_area` (sum of triangle areas == ring area), `test_bad_rings_return_none` (2 nodes, repeated node only, collinear, bow-tie self-intersection, 0.1 m sliver), `test_extrusion_is_closed_and_outward` (every wall normal points away from the centroid, roof normal points up; edge-use count: each wall-roof/wall-wall edge shared by exactly two triangles), `test_extrusion_height_and_roof_level`.
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement** with plain O(n^2) ear clipping and an O(n^2) segment-intersection simplicity check (footprints have tens of nodes).
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** — `feat(sitegen): footprint rings - clean up, triangulate and extrude`

---

### Task 5: Landmark builders

**Files:**
- Create: `tools/sitegen/sitegen/landmarks.py`, `tools/sitegen/tests/test_landmarks.py`

**Interfaces:**
- Consumes: `MeshBuilder`.
- Produces (each appends to a `MeshBuilder`, all positions NED metres, `heading_deg` clockwise from north, the shape's axis runs along the heading):
  - `arch(mb, center_ne, heading_deg, span_m, length_m, height_m, material, segments=48, end_doors=())`: a half-elliptical vault (semi-axes span/2 and height) over a rectangle `length_m` long, plus two end walls; `end_doors` is a list of `(end: "front"|"back", width_m, height_m)` openings centred in the end wall (the wall is built from rectangles around the hole, never a triangle across it).
  - `stack(mb, center_ne, base_radius_m, top_radius_m, height_m, bands: list[tuple[str, float]], segments=24)`: a tapered tube; `bands` is `[(material, height_fraction), ...]` from the top down, fractions summing to 1.
  - `walls(mb, corners_ne: list[tuple[float, float]], height_m, material, openings=())`: vertical walls along a closed polyline with a flat roof via `extrude`'s roof only when `roof=True`; `openings` are `(wall_index, offset_m, width_m, bottom_m, top_m)` rectangular breaches.
  - `build_landmark(mb, spec, projection)` dispatching on a recipe `[[landmarks]]` entry (`kind = "arch" | "stack" | "walls"`, its numbers, and a `center = [lat, lon]` or `center_ned_m = [n, e]`).

- [ ] **Step 1: Write the failing tests:** `test_arch_dimensions` (bounds: width == span, height == height, length == length for heading 0 and 90), `test_arch_end_door_is_open` (a segment through the doorway crosses no triangle of the builder; a segment 1 m beside it crosses the wall; use `segment_hits(mb, a, b)` from a test helper implementing Moller-Trumbore), `test_stack_radii_and_bands` (radius at height h is the linear taper; the band materials appear in order), `test_walls_breach_is_open_and_wall_is_not`, `test_landmark_dispatch_and_unknown_kind`.
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement.** Triangulate wall-with-hole panels as the rectangles above/below/left/right of each hole (no ear clipping across holes).
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** — `feat(sitegen): arch, stack and walls-with-openings landmarks`

---

### Task 6: OSM snapshot - parse, normalise, fetch

**Files:**
- Create: `tools/sitegen/sitegen/osm.py`, `tools/sitegen/tests/{test_osm.py,fixtures/overpass_small.json}`

**Interfaces:**
- Produces:
  - `osm.Way` dataclass: `id: int`, `tags: dict[str, str]`, `ring: list[tuple[float, float]]` (lat, lon).
  - `osm.parse_overpass(data: dict) -> list[Way]`: closed `way` elements with geometry, sorted by id; open ways and relations are skipped and counted (returned as a second value `skipped: dict[str, int]`).
  - `osm.height_of(tags, building_height_m, level_height_m) -> float`: `height` tag ("70", "70 m", "12.5") > `building:levels` x level height > default; non-numeric or <= 0 falls through to the next rule.
  - `osm.write_snapshot(path, ways, bbox, source_note)` and `osm.read_snapshot(path) -> list[Way]`: the committed format, sorted by id, only the tags that matter (`building`, `height`, `building:levels`, `name`, `name:en`, `man_made`, `roof:shape`), UTF-8, `ensure_ascii=False`, trailing newline, a header with the bbox, the query and the attribution line.
  - `osm.fetch(bbox, user_agent, *, opener=urllib.request.urlopen, sleep=time.sleep, tries=3) -> dict`: POSTs `[out:json][timeout:60]; way["building"](s,w,n,e); out tags geom;` to `https://overpass-api.de/api/interpreter`, retries 429/502/504 with a 25 s backoff, raises `FetchError` otherwise; the caller injects `opener`/`sleep` in tests.
  - CLI `python -m sitegen fetch <recipe>`: bbox from the recipe's origin and half extents, writes the snapshot only after a successful non-empty parse (otherwise exits 1 and leaves the old file).

- [ ] **Step 1: Write the failing tests** (fixture = 6 hand-written ways: a square, an L, an open way, a Cyrillic-named building with `height = "70"`, a `building:levels` one, a relation): `test_parse_skips_open_ways_and_relations_and_counts_them`, `test_height_rules` (table of tag dicts), `test_snapshot_round_trip_and_cyrillic_survives`, `test_snapshot_is_sorted_and_stable_bytes`, `test_fetch_retries_on_502_then_succeeds` (fake opener raising `HTTPError(502)` twice), `test_fetch_gives_up_after_tries`, `test_fetch_sends_a_user_agent` (406 without one in the fake), `test_empty_response_does_not_overwrite_a_good_snapshot`.
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement** with `urllib.request` only.
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** — `feat(sitegen): OSM snapshot format, parser and a polite Overpass fetch`

---

### Task 7: `build` - recipe + snapshot -> `.glb` + world file

**Files:**
- Create: `tools/sitegen/sitegen/{build,__main__}.py`, `tools/sitegen/tests/{test_build.py,fixtures/demo/recipe.toml,fixtures/demo/demo.osm.json}`, `tools/sitegen/tests/golden/demo.glb`, `tools/sitegen/tests/golden/demo.toml`
- Modify: `crates/ofs-config/tests/sitegen_golden.rs`

**Interfaces:**
- Consumes: Tasks 2-6.
- Produces: `build.build_site(recipe_path: Path, out_dir: Path | None = None) -> BuildReport` writing the `.glb` and the world file (paths from the recipe, or under `out_dir` for tests); `BuildReport` (dataclass: `footprints_used`, `skipped: dict[str, int]` by reason, `landmarks`, `triangles`, `materials`); CLI `python -m sitegen build <recipe>` prints the report and exits 1 on any `RecipeError`. The world file: a generated-file header (recipe path, "generated - edit the recipe", the `© OpenStreetMap contributors` attribution, "all values are estimates"), the recipe's `[world]` table, one `[[meshes]]` (`name` = site name, `file` = the recipe's `output.glb`), and the recipe's `[[materials]]` (a material the mesh uses but the recipe does not list is simply left out, with a build-report warning, because an unlisted material loses 0 dB; a material the recipe lists but no triangle uses is dropped from the world file with a warning, because the engine rejects a `[[materials]]` entry no mesh uses).

- [ ] **Step 1: Write the failing tests** (fixture site: 4 ways in a 100 x 100 m box, one `skip`ped, one with a `height_m` override, plus one landmark of each kind): `test_build_is_deterministic` (twice -> identical bytes), `test_golden_matches` (`demo.glb` and `demo.toml` byte-equal), `test_report_counts` (used, skipped by reason, triangles), `test_footprint_outside_extent_is_dropped`, `test_unlisted_material_warns_and_an_unused_listed_one_is_dropped`, `test_world_header_has_attribution_and_cyrillic_name_round_trips`, `test_a_recipe_error_exits_1_listing_all_problems` (CLI via `subprocess`). Rust: extend `sitegen_golden.rs` with `the_demo_world_loads` loading `tools/sitegen/tests/golden/demo.toml` through `ofs_config::world::load` (copy both files into a tempdir with the recipe's relative layout so `file` resolves) and asserting `load_meshes()` works and a footprint's height matches the recipe.
- [ ] **Step 2: Run** `python -m pytest tools/sitegen/tests -q` and `cargo test -p ofs-config --test sitegen_golden` — fail.
- [ ] **Step 3: Implement** `build_site`: project each way ring to NED, `normalize_ring`, `extrude` with the `height_of` rule and the material, then each landmark; write files with LF.
- [ ] **Step 4: Run** both suites; regenerate the goldens with `python tools/sitegen/tests/make_golden.py` and re-run to prove they are stable.
- [ ] **Step 5: Commit** — `feat(sitegen): build a site from a recipe and an OSM snapshot`

---

### Task 8: Docs, CI, and the licence note (a gate)

**Files:**
- Create: `docs/research/sitegen-licensing.md`, `tools/sitegen/README.md`
- Modify: `.github/workflows/ci.yml` (a step in `core`: `python -m pytest tools/sitegen/tests -q`), `docs/dev-setup.md` (a "Generating a world from real data" section), `docs/roadmap.md`, `CHANGELOG.md`

- [ ] **Step 1: Write `sitegen-licensing.md`:** what ODbL asks of an extract (attribution; share-alike applies to the *Derivative Database*, i.e. the committed snapshot, which stays ODbL, while the generated meshes and worlds are a Produced Work that may carry the repo's GPLv3 with the attribution), how the repo satisfies it (snapshot headers, world headers, docs line), what to do if a recipe's footprints are hand-listed instead, and the fallback. Cite the ODbL text and the OSM attribution guidelines by URL. State clearly: this is the implementer's reading, **not legal advice, and the repo owner confirms it before any snapshot is committed in piece 3**.
- [ ] **Step 2: Write the README and the dev-setup section** (commands: `fetch`, `build`, the recipe schema table, the determinism and `doubleSided` rules, the Overpass etiquette findings, "always utf-8").
- [ ] **Step 3: CI:** the pytest step runs on the existing Linux `core` job and the Windows job (Python 3.12 is already set up there); no new dependency to install.
- [ ] **Step 4: Run** `python -m pytest tools/sitegen/tests -q`, `cargo test --workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- [ ] **Step 5: Commit** — `docs(sitegen): the generator, its licence note and CI`

**Human review point (not an implementer task):** open the demo world in Godot (`worlds/` copy of `tools/sitegen/tests/golden/demo.*` under a temp name) and look at it before piece 3 starts; recognisability is the owner's call.

---

## Self-review

- **Spec coverage:** recipe + snapshot + landmark builders + ground (flat only; DEM deferred, noted in Task 8 docs) + deterministic output + generic by construction (Tasks 3, 7); licence gate (Task 8); fetch-once-and-commit (Task 6); tested on a synthetic site not Chernobyl (Task 7); the engine loads the output (Tasks 2, 7).
- **Left to piece 3:** the real Chernobyl recipe and snapshot, true dimensions with sources, opening sizes versus the shipped quads, and the `WorldConfig` mesh cache.
- **Deliberately not built:** a cooling-tower builder, roof shapes, textures, relations/multipolygons, `col_` proxies, DEM terrain.
