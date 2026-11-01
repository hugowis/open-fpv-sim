# Chernobyl reactor-area map (piece 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `worlds/chernobyl.toml` + `worlds/assets/chernobyl.glb`: the present-day Chernobyl reactor area at true scale (the New Safe Confinement over reactor 4, units 3 and 4, the turbine hall), generated from a recipe, flyable around and through its openings.

**Architecture:** Two stages split by the licence gate. **Stage A** (Tasks 1-6, ungated, one PR) makes the engine and the generator ready (mesh cache, OSM relations, footprint breaches) and ships a complete map whose only geometry is the hand-parametrised NSC arch, built from published dimensions, with a hand-made empty snapshot. **Stage B** (Tasks 7-8, one PR) starts only after the repo owner confirms `docs/research/sitegen-licensing.md`: it fetches and commits the real OpenStreetMap snapshot and adds the surrounding buildings and the turbine-hall breach. The map is a recipe in `sites/chernobyl/`, not code.

**Tech Stack:** Rust (`ofs-config`, `ofs-sim` tests), Python stdlib (`tools/sitegen`), pytest, the committed generator output.

**Spec:** `docs/superpowers/specs/2026-11-01-mesh-worlds-and-chernobyl-design.md` ("Piece 3"). Pieces 1 (#176) and 2 (#179) are merged.

## What planning found (changes the spec's content list)

- **The 150 m stack VP-2 no longer exists.** It sat on the roof of unit 4 (75.5 m tall, 9 m diameter, top 150 m above ground) and was dismantled from 31 Oct to 25 Nov 2013 so the NSC could be slid over reactor 4 (World Nuclear News, https://world-nuclear-news.org/Articles/Chernobyl-ventilation-stack-removed). The NSC and VP-2 never stood together. **Ruling: the map is the present-day site; VP-2 is dropped.** A 50 m x 6 m replacement stack stands "on the eastern wall of the plant building" (same article), but its position is not in the sources found; it is not modelled. OpenStreetMap still maps a 125 m chimney (`building:part`, way 301362167) where VP-2 stood: it is stale and must never be built (the fetch query takes `building` ways and relations only, so it does not arrive; Task 7 asserts it).
- **NSC dimensions** (arch span 257 m, length 162 m, height 108 m; Chornobyl NPP project page https://chnpp.gov.ua/en/infocenter/news/204-projects/nsc-construction/578-q-q578; Structurae https://structurae.net/en/structures/chernobyl-new-safe-confinement lists internal height 92.5 m). **Orientation:** built west of reactor 4 and slid east, so its axis runs **east-west**: `heading_deg = 90` ("back" end = west). Its east end almost touches unit 3, so only the **west end** gets a door. Both facts are confirmed against the OSM outline in Task 7 (outline about 275 m north-south by 173 m east-west).
- **The turbine hall is an OSM multipolygon relation** (8705336, "Turbine Halls 4, 3, 2, 1", height 50, about 600 m long, running east along the south side of units 4 and 3). The generator skips relations today, and keeps a footprint only if its centroid is inside the extent, which would drop a 600 m hall. Tasks 2-3 fix both.
- **Real events usable as gameplay**: 600 m2 of the turbine-hall roof and wall collapsed in 2013 (https://world-nuclear-news.org/articles/safety-work-planned-for-chernobyl-turbine-hall), which is the sourced reason for the breach in Task 7.
- The largest shipped quad (opendrone-7lr, `library/frames/opendrone-7lr.toml`: motors at +-0.113 m, 7 in props) spans about 0.40 m tip to tip, so every opening below is at least 5 m on its smallest side.

## Global Constraints

- Every number in the recipe carries a `# source:` comment (URL and the quoted figure) or the word `estimate`; "All values are estimates" in the world header. A part is either measured (a published number) or artistic; `docs/chernobyl-map.md` lists which.
- `schema_version = 1`; the world loads through `ofs_config::world::load` with no change to any other world (the flat-world digest pin and the existing goldens must not change).
- Caps from piece 1: at most 400 000 triangles in all, 64 MiB per file. The committed `chernobyl.glb` stays under 5 MiB.
- Deterministic generation: the same recipe and snapshot give byte-identical `chernobyl.toml` and `chernobyl.glb` on Windows and Linux. A pytest rebuilds both and compares to the committed files.
- OSM data is ODbL: `© OpenStreetMap contributors` in the world header and docs. **No real OSM snapshot is committed before the owner confirms the licence note** (`test_licence_gate.py` enforces it; the owner deletes that test at sign-off, and Task 7 starts by checking it was).
- Always `encoding="utf-8"`; files LF; commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Python >= 3.11, stdlib only.

## Review Focus

- OSM relations that are not simple (an outer ring split over several member ways, an inner ring, an unclosable ring, a relation with no members): built when closable, counted when not, never an exception (Task 2).
- A footprint straddling the extent edge is kept whole; one wholly outside is dropped; the choice is deterministic (Task 2).
- A breach index that points past the ring, or an opening that does not fit its wall, is a build error naming the footprint, never a silently moved hole; and a re-fetched snapshot that changes the ring's edges cannot go unnoticed (Task 3 pins the index meaning; Task 7's test pins the breach's position).
- The pilot's start: not inside geometry, a clear line of sight to the NSC door, and a live video link there (Tasks 4, 5).
- The world is bit-reproducible and its flight timing is recorded (Task 5); a stale 125 m chimney or a mapping of the old sarcophagus never leaks in (Tasks 4, 7).

---

### Task 1: Cache the loaded meshes on `WorldConfig`

A session `Load` reads every `.glb` twice (`world::load` validates by loading, then `build` calls `load_meshes()` again). Piece 3's mesh is the biggest so far.

**Files:**
- Modify: `crates/ofs-config/src/world.rs` (struct `WorldConfig` ~:43, `load_meshes_checked` ~:718, `load` ~:826)
- Test: `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Produces: no signature change. `WorldConfig::load_meshes()` returns the meshes read by `world::load` (the same `Arc<TriMesh>`s) while `meshes`, `materials` and `source_path` are unchanged since; `load_meshes_with(cap)` always re-reads. Add a private `#[serde(skip)] mesh_cache: MeshCache` field: `struct MeshCache(Option<(Vec<MeshSection>, Vec<MaterialSection>, PathBuf, Arc<Vec<WorldMeshData>>)>)` with `Default`, `Clone`, a `Debug` printing only "loaded"/"empty", and a `PartialEq` that is always true (the cache is not part of a world's identity).

- [ ] **Step 1: Write the failing tests** (`tests/world.rs`): `load_meshes_after_load_reads_the_file_once` (copy `worlds/hall.toml` and `assets/hall.glb` to a temp dir, `world::load` it, delete the `.glb`, assert `load_meshes()` is `Ok` and its mesh `Arc::ptr_eq`s a second call's), `a_changed_mesh_section_is_reloaded_not_stale` (after `load`, set `meshes[0].position_ned_m[0] += 10.0` on the `pub` field, restore the `.glb`, assert the new data's `mesh.bounds()` moved by 10 m), `load_meshes_with_a_small_cap_still_refuses` (the existing cap test must still see the error), `a_cloned_world_shares_the_cache`.
- [ ] **Step 2: Run** `cargo test -p ofs-config --test world` — the first two fail.
- [ ] **Step 3: Implement** `MeshCache`: `load` fills it from the `load_meshes_checked` result it already computes (:841); `load_meshes` returns the cached data when the key (`meshes`, `materials`, `source_path`) equals the current fields, otherwise reads as before.
- [ ] **Step 4: Run** `cargo test -p ofs-config -p ofs-sim --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- [ ] **Step 5: Commit** — `perf(config): a world's meshes are read once per load`

---

### Task 2: OSM multipolygon relations and the extent rule

**Files:**
- Modify: `tools/sitegen/sitegen/osm.py`, `tools/sitegen/sitegen/build.py`, `tools/sitegen/README.md`
- Test: `tools/sitegen/tests/test_osm.py`, `tools/sitegen/tests/test_build.py`, `tools/sitegen/tests/fixtures/overpass_relations.json`

**Interfaces:**
- Produces: `osm.parse_overpass` also turns a `relation` element (tags `type=multipolygon` or a `building` tag, with `members` that carry `role` and `geometry`) into one `Way` per closed outer ring, sharing the relation's tags. A ring is the outer members joined end to end (greedy, in member order, a member may be reversed). **Ids: `-(relation_id * 100 + k)`, k = 1.. in ring order, at most 99 rings per relation** (negative so they never collide with way ids; `[[footprints]] id` takes them). Inner members are ignored and counted under a new skipped key `relation_hole`; a relation with no closable outer ring is counted under the existing `relation`; the skipped dict keeps all keys always present. `osm._query` becomes `[out:json][timeout:60]; (way["building"](s,w,n,e); relation["building"](s,w,n,e);); out tags geom;`. `read_snapshot` accepts negative ids and sorts by id.
- Extent rule in `build.py`: a footprint is kept when the **bounding box of its normalised ring intersects** the extent rectangle (`|north| <= half_north`, `|east| <= half_east`), replacing the centroid test; the skip reason stays `outside_extent`.

- [ ] **Step 1: Write the failing tests:** `test_relation_with_one_closed_outer_becomes_a_way` (id `-(rel*100+1)`, tags copied), `test_outer_ring_is_stitched_from_member_ways_in_either_direction` (3 members, one reversed -> one 4-corner square), `test_relation_hole_is_counted_not_built`, `test_unclosable_relation_is_skipped_and_counted_never_raises` (open chain; empty `members`; members without geometry; a member with NaN coordinates), `test_snapshot_round_trips_negative_ids_sorted`, `test_fetch_query_asks_for_relations_too`; in `test_build.py`: `test_a_footprint_crossing_the_extent_edge_is_kept_whole` (a 400 m long rectangle starting inside a 300 m extent: used, and its far end is in the mesh), `test_a_footprint_wholly_outside_is_dropped`. The existing demo goldens must be byte-identical (run `test_golden_matches`).
- [ ] **Step 2: Run** `python -m pytest tools/sitegen/tests -q` — new tests fail.
- [ ] **Step 3: Implement** the relation parser (a small `_stitch(members)` helper), the query, and the bbox-intersect rule. Document both in the README (limits: holes are filled, rings that need more than endpoint matching are skipped).
- [ ] **Step 4: Run** the whole sitegen suite — pass, goldens unchanged.
- [ ] **Step 5: Commit** — `feat(sitegen): multipolygon relations, and a footprint is kept if it touches the extent`

---

### Task 3: Breaches in footprints, and a `walls` inspector

**Files:**
- Modify: `tools/sitegen/sitegen/recipe.py` (`[[footprints]]` keys), `tools/sitegen/sitegen/build.py`, `tools/sitegen/sitegen/__main__.py`, `tools/sitegen/README.md`
- Test: `tools/sitegen/tests/test_recipe.py`, `tools/sitegen/tests/test_build.py`

**Interfaces:**
- Consumes: `landmarks.walls(mb, corners_ne, height_m, material, openings=(), roof=False)`; `polygon.normalize_ring`.
- Produces: `[[footprints]]` accepts `openings = [[wall_index, offset_m, width_m, bottom_m, top_m], ...]` (same meaning as the walls landmark). A footprint with `openings` is built with `walls(..., roof=True)` over its **normalised, counter-clockwise ring**: wall `i` runs from ring node `i` to node `i+1`, the offset measured from node `i`. An opening that does not fit raises `BuildError("footprint <id>.openings[k]: ...")` (not a skip). CLI `python -m sitegen walls <recipe> <way id>` prints one line per wall of that footprint's normalised ring: `index  start_north  start_east  length_m  outward_bearing_deg`, sorted by index; unknown id exits 1.

- [ ] **Step 1: Write the failing tests:** recipe: `test_footprint_openings_are_validated_with_field_paths`; build: `test_a_footprint_breach_is_open_and_the_wall_beside_it_is_not` (segment through the hole crosses no triangle, 1 m beside it crosses one; use `segment_hits` from `tests/helpers.py`), `test_a_breach_that_does_not_fit_is_a_build_error_naming_the_footprint`, `test_breach_index_follows_the_normalised_counter_clockwise_ring` (the same square given clockwise and counter-clockwise puts the breach on the same physical wall), `test_walls_cli_lists_every_wall` (lengths sum to the ring's perimeter; unknown id exits 1).
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement** in `build.py` (a footprint with `openings` takes the walls path; others unchanged) and the CLI subcommand.
- [ ] **Step 4: Run** the sitegen suite — pass, goldens unchanged.
- [ ] **Step 5: Commit** — `feat(sitegen): breaches in footprints and a walls inspector`

---

### Task 4: The NSC, the pilot and the world (Stage A content)

**Files:**
- Create: `sites/chernobyl/recipe.toml`, `sites/chernobyl/chernobyl.osm.json`, `sites/chernobyl/SOURCES.md`, `worlds/chernobyl.toml`, `worlds/assets/chernobyl.glb` (generated: `cd tools/sitegen && python -m sitegen build ../../sites/chernobyl/recipe.toml`)
- Test: `tools/sitegen/tests/test_chernobyl_site.py`

**Interfaces:**
- Consumes: Tasks 1-3; the demo recipe `tools/sitegen/tests/fixtures/demo/recipe.toml` (shape of `[world]`, receiver, materials) and `worlds/hall.toml`.
- Produces: the recipe (`site.name = "chernobyl"`, `half_extent_m = [300.0, 300.0]`, `output.world = "../../worlds/chernobyl.toml"`, `output.glb = "assets/chernobyl.glb"`, `snapshot = "chernobyl.osm.json"`). The snapshot is hand-made: `"ways": []`, `source_note` "empty: no OpenStreetMap data yet", so the licence gate test passes. One landmark `kind = "arch"`, name `NSC`: `span_m = 257.0`, `length_m = 162.0`, `height_m = 108.0`, `heading_deg = 90.0`, `center_ned_m = [0.0, 0.0]` (refined in Task 7), `segments = 48`, `end_doors = [["back", 40.0, 40.0]]` (the west end; a gameplay liberty, labelled artistic), material `nsc_cladding` with `rf_loss_db = 40.0` (estimate). `origin` = the published coordinates of reactor 4 (cite the source in `SOURCES.md`; origin of NED). `[world]`: pilot 150 m west of the door at eye height (NED `[0.0, -231.0, -1.7]`, facing 90 deg), the receiver of `hall.toml`, a name `Chernobyl`. `SOURCES.md`: a table of every number (value, source URL, measured/estimate/artistic).

- [ ] **Step 1: Write the failing tests** (`test_chernobyl_site.py`, all reading the real recipe): `test_committed_outputs_are_current` (rebuild into a temp `out_dir`, bytes equal `worlds/chernobyl.toml` and `worlds/assets/chernobyl.glb`), `test_every_landmark_has_a_source_comment` (each `[[landmarks]]` block in the recipe text contains `# source:` or `estimate`), `test_openings_fit_the_biggest_quad` (every door/breach side >= 2.0 m, with the quad span 2 x 0.113 + 0.178 = 0.40 m derived in the test from the two library numbers it cites), `test_the_glb_is_small_and_under_the_caps` (file size < 5 MiB, triangle count from the build report < 400 000), `test_the_pilot_sees_the_door` (the segment from the pilot's eye to the door centre crosses no triangle; the segment to a point 10 m beside the door's jamb at head height crosses the wall), `test_the_header_has_the_attribution_and_the_estimates_line`, `test_no_chimney_is_built` (no mesh triangle taller than the arch: max height <= 108.5 m).
- [ ] **Step 2: Run** `python -m pytest tools/sitegen/tests/test_chernobyl_site.py -q` — fail (no files).
- [ ] **Step 3: Write the recipe, snapshot and `SOURCES.md`** (look up the cited figures; reactor 4's coordinates from a published source; flag unconfirmed ones `estimate`), then generate the two outputs.
- [ ] **Step 4: Run** the new tests and `python -m pytest tools/sitegen/tests -q` — pass; `cargo test -p ofs-config --locked` (the world must load: add `chernobyl.toml` to the list `tests/world.rs` loads for every shipped world, if such a list exists).
- [ ] **Step 5: Commit** — `feat(worlds): chernobyl - the New Safe Confinement from published dimensions`

---

### Task 5: Fly it - engine tests and the timing record

**Files:**
- Create: `crates/ofs-sim/tests/chernobyl_world.rs`
- Modify: `docs/research/collision.md` (a timing line)

**Interfaces:**
- Consumes: `worlds/chernobyl.toml`; the pattern of `crates/ofs-sim/tests/mesh_world.rs` (`build`, `BuildOptions`, `Sticks`, `FcKind::OpenLoopStabilized`, `v.state().collision_event_count`, `link_params`) and of `crates/ofs-physics/tests/mesh_perf.rs` (timing, `#[ignore]`).

- [ ] **Step 1: Write the failing tests:** `the_world_loads_with_one_mesh` (`meshes().len() == 1`, name `chernobyl`), `the_pilot_is_not_inside_geometry` (`TriMesh::within(pilot_eye, 2.0)` finds nothing), `a_quad_flies_in_through_the_west_door_untouched` (opendrone-7lr-longrange starts 100 m west of the door on the centre line at 2 m, yawed east, pitched stick forward long enough to pass 20 m inside: zero collisions), `the_same_flight_20_m_off_the_door_axis_hits_the_end_wall` (north offset beyond the door's half width: `collision_event_count > 0`, first object is the mesh), `the_flight_replays_exactly` (same seed, same digest; differs from the open field), `the_video_link_is_cut_by_the_arch_skin_and_not_through_the_door` (the hall test's method: loss at a point behind the skin >= `rf_loss_db`, loss at a point straight inside the door is 0). Plus `#[ignore = "timing record, run by hand in release"] chernobyl_timing_is_recorded` (10 s of flight inside, against the open field).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test chernobyl_world --locked` — fails until the file exists; once written, any failure is a real defect in the map (door too small, pilot clipped): fix the recipe (Task 4's files) and regenerate, not the test's expectations.
- [ ] **Step 3: Implement** the file; run the ignored timing test in release (`cargo test --release -p ofs-sim --test chernobyl_world -- --ignored --nocapture`) and write the numbers and triangle count into `docs/research/collision.md` next to the piece-1 record.
- [ ] **Step 4: Run** `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- [ ] **Step 5: Commit** — `test(sim): the chernobyl world - fly in through the door, hit the wall, cut the link`

---

### Task 6: Docs, and Stage A's PR

**Files:**
- Create: `docs/chernobyl-map.md`
- Modify: `docs/human-check.md` (a Godot look at the map), `docs/roadmap.md`, `CHANGELOG.md`, `docs/dev-setup.md` (one line: `OFS_WORLD=chernobyl`-style how-to-fly, using whatever the game's `world_path` setting / `load(..., world=...)` is), `README.md` of `tools/sitegen` if the recipe examples changed.

- [ ] **Step 1: Write `docs/chernobyl-map.md`:** what is in the map, **measured vs artistic** (a table), why VP-2 is absent (dismantled 2013, with the source), the openings and their sizes, what the OSM stage adds, how to regenerate (`python -m sitegen build sites/chernobyl/recipe.toml`), attribution.
- [ ] **Step 2: CHANGELOG** (`### Chernobyl map`, `#### Added`) and **roadmap** entries in the files' styles; **human-check.md**: the owner flies in through the door in Godot and says whether it is recognisable (spec ruling 12).
- [ ] **Step 3: Run** `python -m pytest tools/sitegen/tests -q`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- [ ] **Step 4: Commit** — `docs: the chernobyl map - what is measured, what is artistic`
- [ ] **Step 5 (controller): open Stage A's PR.** Then **STOP at the gate**: ask the repo owner whether `docs/research/sitegen-licensing.md` is confirmed (the controller never decides that). Continue to Task 7 only on an explicit yes, and only after the owner has deleted `tools/sitegen/tests/test_licence_gate.py` themselves or told the controller to.

---

### Task 7: The OSM stage (gated)

**Files:**
- Modify: `sites/chernobyl/chernobyl.osm.json` (the real snapshot), `sites/chernobyl/recipe.toml`, `sites/chernobyl/SOURCES.md`, `worlds/chernobyl.toml`, `worlds/assets/chernobyl.glb`, `tools/sitegen/tests/test_chernobyl_site.py`, `crates/ofs-sim/tests/chernobyl_world.rs` (only if a start position now collides)
- Delete (by the owner): `tools/sitegen/tests/test_licence_gate.py`

**Interfaces:**
- Consumes: Tasks 2-3 (relations, breaches, `walls` CLI), the owner's confirmation.
- Produces: the snapshot fetched once with `python -m sitegen fetch sites/chernobyl/recipe.toml` (one polite request; a 502 is retried; never overwrite a good snapshot) over the 600 m extent. Footprint overrides (ids from OSM, check each against the fetched file): **skip** `456732992` ("Укриття-2", the NSC outline: replaced by the arch landmark) and `500551950` (unit 4 ruins: overlap the shelter object); keep `500551951` (the shelter object, 65 m: the block inside the arch), `500551953` (stabilisation steel structure), `326145079` (unit 3, 70 m) and the turbine-hall relation's ring `-(8705336*100+1)` (height 50). Refine the arch's `center_ned_m` to the centre of the skipped NSC outline's bounding box (a position fact, recorded in `SOURCES.md`) and confirm the east-west axis. **One breach** in the turbine hall wall nearest unit 4, chosen with `python -m sitegen walls`, sized about 600 m2 in total (30 m wide, 20 m high, bottom 5 m; the 2013 collapse), labelled artistic.

- [ ] **Step 1: Write the failing tests** (extend `test_chernobyl_site.py`): `test_the_stale_chimney_is_not_built` (the snapshot contains no way 301362167 and no triangle stands within 8 m of north -31, east 85 above 70 m), `test_the_nsc_outline_is_replaced_by_the_arch` (way 456732992 is `skip`ped in the recipe and the report counts it), `test_the_turbine_hall_breach_is_where_the_recipe_says` (a segment through the breach crosses nothing; 1 m beside it crosses the wall), `test_the_snapshot_header_credits_openstreetmap`, `test_report_counts_are_pinned` (used / skipped by reason, exact numbers from the real build), `test_still_under_the_caps_and_5_mib`.
- [ ] **Step 2: Verify the gate**: `git ls-files tools/sitegen/tests/test_licence_gate.py` must be empty and the owner's confirmation quoted in the commit body; otherwise stop.
- [ ] **Step 3: Fetch, edit the recipe and `SOURCES.md`, rebuild** both outputs; the Task 4-5 tests must pass unchanged (if the pilot or the door line now collides with a building, move the pilot or door in the recipe, not the tests).
- [ ] **Step 4: Run** `python -m pytest tools/sitegen/tests -q`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and the release timing test again; record the new triangle count and time.
- [ ] **Step 5: Commit** — `feat(worlds): chernobyl - units 3 and 4, the turbine hall and its breach from OpenStreetMap`

---

### Task 8: Docs for Stage B and the owner's look

**Files:**
- Modify: `docs/chernobyl-map.md`, `docs/research/sitegen-licensing.md` (status line only if the owner changes it), `docs/human-check.md`, `CHANGELOG.md`, `docs/roadmap.md`, `docs/research/collision.md`

- [ ] **Step 1: Update** `chernobyl-map.md` (the measured/artistic table gains every OSM-derived building and the breach; OSM attribution line; the 600 m hall note) and the timing record.
- [ ] **Step 2: Add** to `human-check.md` the second review point: fly the finished map; recognisability is the owner's call (spec ruling 12).
- [ ] **Step 3: Run** the three verification commands of Task 7 — pass.
- [ ] **Step 4: Commit** — `docs: the chernobyl map with the OpenStreetMap stage`

---

## Self-review

- **Spec coverage:** NSC (T4), units 3/4 and turbine hall (T7), service buildings and tanks from OSM (T7, kept by the recipe unless skipped), openings to fly through (T4 door, T7 breach), flat ground (no terrain section), pilot on an open spot (T4, tested T5), a note of measured vs artistic (T6, T8), about 600 m (T2 keeps the 600 m hall whole, so the world is slightly larger than the nominal extent: ruling), the two review points (T6, T8), tests the spec lists for the map (load, openings wide enough, pilot clear, triangle count, frame time: T4, T5, T7). **Not built, on purpose:** VP-2 (gone since 2013), the replacement stack (position not sourced), a double-skin arch, roof breaches (the walls builder has wall openings only), interiors beyond the shelter block, DEM ground.
- **Stage split:** Tasks 1-6 need no OSM data and ship a complete NSC map; Tasks 7-8 are blocked on the owner's licence confirmation and say so in the task text.
- **Rulings made here:** present-day site (VP-2 dropped); relation ring ids `-(rel*100+k)`; footprints are kept when their bounding box touches the extent; `sites/chernobyl/` instead of the spec's `sites/chernobyl.toml`; west door only (east end abuts unit 3); a wall breach, not a roof breach.
