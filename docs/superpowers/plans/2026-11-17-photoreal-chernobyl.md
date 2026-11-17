# Photoreal Chernobyl Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Chernobyl hero area photoreal/survey-like on Godot Forward+, with a reusable map-creation pipeline, while the physics/RF and the analog video chain stay unchanged.

**Architecture:** #134 supplies the look/quality foundation. The collision/RF mesh stays server-side; the detailed visual layer is a separate client-only `[[visual_meshes]]` file loaded asynchronously. The OSM snapshot is extended to roads/landuse; a mapgen step builds the visual GLB from Blender hero models, procedural facades/roofs, ground/roads/vegetation and CC0 textures.

**Tech Stack:** Godot 4.7.2 (Forward+/Vulkan, GDScript), Rust (`ofs-config`, `ofs-sim`, `ofs-godot`), Python 3.11+ (`sitegen`, `mapgen`), Blender, CC0 textures.

**Spec:** `docs/superpowers/specs/2026-11-17-photoreal-chernobyl-design.md`

## Global Constraints

- Godot 4.7.2, Forward+ (Vulkan); RTX 2070 development machine; 60 fps at 1080p; a low quality tier.
- No site-specific photos, drone scans or purchased models; CC0 textures; Blender-authored hero models from published dimensions and OSM outlines.
- OSM data is ODbL 1.0; the snapshot stays ODbL; the repository is GPLv3; CC0 is compatible; `art/chernobyl/LICENSES.md` records every texture.
- The collision/RF mesh stays server-side; the visual layer is client-only; the current `col_` rule stays.
- `[[visual_meshes]]` in the world file; protocol 19; the server sends path/digest but never reads the visual file for collision/RF.
- The client loads visual meshes asynchronously; no Godot editor import; the headless suites pass.
- The analog video chain, OSD, DVR and clean feed are unchanged.
- #134's `LOOKS`/quality table is the input; this plan executes it first as PR 0.
- Budgets: visual file < 100 MB, a few million LOD0 triangles, 2–4 GB texture memory.
- The owner's look is the gate for PR 3; the map is not called photoreal until they have flown it.

## Review Focus

1. A missing, corrupt or digest-mismatched visual file: the client reports it and keeps flying; the collision/RF still works.
2. A visual file over the budget or a slow load: the game shows a loading state, no stall or crash.
3. A world without visual meshes: today's behavior, no error.
4. A `col_`-only collision file: the client draws nothing from it; the visual file draws; the server uses it.
5. An OSM snapshot with open ways/roads: the building build ignores them; the mapgen reads them; no `open_way` regression.
6. The physics/RF behavior and digests unchanged by the visual layer.
7. Forward+ DVR, clean feed, OSD and screenshots still work.
8. The CC0 licences and the OSM attribution travel with the assets.
9. The hero GLB node/material names and the budget.
10. The owner's look: the hero area reads as a photoreal Chernobyl reconstruction.

## File Structure

- `crates/ofs-config/src/world.rs` — `[[visual_meshes]]` parsing, validation, digest.
- `proto/ofs/v1/sim.proto` — `World.visual_meshes = 13`; protocol 19.
- `crates/ofs-sim/src/session.rs` — `world_msg` carries the visual meshes.
- `crates/ofs-godot/src/lib.rs` — `get_world()` includes the visual meshes.
- `godot/world/world.gd` — draws the visual layer; `godot/world/visual_loader.gd` — async loader.
- `godot/scripts/app.gd` — loading state.
- `tools/sitegen/sitegen/{osm,build,glb,recipe}.py` — roads/landuse and the `col_` output.
- `tools/mapgen/` — facades, ground, vegetation, hero merge, manifest.
- `art/chernobyl/` — Blender sources, exports, textures, `LICENSES.md`.
- `worlds/chernobyl.toml`, `worlds/assets/chernobyl.glb`, `worlds/assets/chernobyl_visual.glb`.
- `docs/chernobyl-map.md`, `docs/human-check.md`, `README.md`, `CHANGELOG.md`, `docs/research/forward-plus.md`, `docs/research/assets.md`.
- Tests: `crates/ofs-config/tests/world.rs`, `crates/ofs-sim/tests/{grpc,chernobyl_world}.rs`, `godot/tests/{test_scene,e2e_open_loop}.gd`, `tools/sitegen/tests`, `tools/mapgen/tests`.

---

## PR 0 — the look/quality foundation

### Task 0: Execute #134

**Files:**
- Modify: the files named in `docs/superpowers/plans/2026-10-31-world-rendering.md`.
- Test: the same plan's tests.

**Interfaces:**
- Produces: `godot/world/world.gd::look_for(name: String) -> Dictionary`, `quality_spec(tier: String) -> Dictionary`, `set_quality(tier: String)`, and `godot/scripts/app.gd::apply_quality()`. The `LOOKS` table and the `quality` setting.

- [ ] **Step 1: Execute the #134 plan task by task**, including its tests and commits.
- [ ] **Step 2: Verify its gates**

Run: `cargo test -p ofs-config --test world` and `godot --headless --path godot -s res://tests/run_tests.gd`
Expected: PASS.

- [ ] **Step 3: Merge PR 0** before starting Task 1.

---

## PR 1 — renderer and visual layer

### Task 1: Forward+ spike

**Files:**
- Modify: `godot/project.godot` (`renderer/rendering_method = "forward_plus"`).
- Create: `docs/research/forward-plus.md`.

**Interfaces:**
- Produces: the renderer setting and the spike findings (MSAA, shadow atlas, fog mode, quality tiers) that Tasks 15–16 use.

- [ ] **Step 1: Switch the project to Forward+** and open the game on the RTX 2070.
- [ ] **Step 2: Run the spike checklist**: fly the current Chernobyl world; record a DVR clip; open the clean feed; run `godot --path godot -s res://tests/shots.gd -- --out=C:\temp\ofs-shots`; read the frame time from the HUD/`--verbose`.
- [ ] **Step 3: Record the findings** in `docs/research/forward-plus.md`: the renderer line, the DVR/clean feed/OSD result, the screenshot result, the frame time, and the settings chosen.
- [ ] **Step 4: Commit**

```bash
git add godot/project.godot docs/research/forward-plus.md
git commit -m "feat(godot): switch the game to Forward+ and record the spike"
```

### Task 2: `[[visual_meshes]]` world schema

**Files:**
- Modify: `crates/ofs-config/src/world.rs` (`WorldConfig`, `MeshSection`, `validate`, a new `visual_meshes()`).
- Test: `crates/ofs-config/tests/world.rs`.

**Interfaces:**
- Produces: `WorldConfig.visual_meshes: Vec<MeshSection>`; `VisualMeshData { name: String, path: PathBuf, digest: u64 }`; `WorldConfig::visual_meshes(&self) -> Result<Vec<VisualMeshData>, ConfigError>`; the digest is FNV-1a of the file bytes; a missing file is a load error; the visual file is not parsed as a mesh.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_world_file_may_place_visual_meshes() {
    let world = written_world(r#"schema_version = 1
name = "visual field"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[[visual_meshes]]
name = "hero"
file = "assets/hero.glb"
position_ned_m = [-53.946, -27.065, 0.0]
heading_deg = 90.0
"#).unwrap();
    assert_eq!(world.visual_meshes.len(), 1);
    assert_eq!(world.visual_meshes[0].name, "hero");
}

#[test]
fn a_visual_mesh_needs_a_file_and_finite_placement() {
    let found = problems(&world_text().replace(
        "[pilot]",
        "[[visual_meshes]]\nname = \"hero\"\nfile = \"\"\nposition_ned_m = [nan, 0.0, 0.0]\n\n[pilot]",
    ));
    assert!(found.iter().any(|p| p.starts_with("visual_meshes[0].file")), "{found:#?}");
    assert!(found.iter().any(|p| p.starts_with("visual_meshes[0].position_ned_m")), "{found:#?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-config --test world a_world_file_may_place_visual_meshes a_visual_mesh_needs_a_file_and_finite_placement`
Expected: FAIL (unknown field/type).

- [ ] **Step 3: Implement the field and validation** in `WorldConfig`: `#[serde(default)] pub visual_meshes: Vec<MeshSection>`; validate unique names, the file path rules, finite position/heading; `visual_meshes()` reads each file and hashes it with `ofs_core::rng::fnv1a64`, returning `VisualMeshData`; a missing/unreadable file is a `ConfigError` naming the field.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ofs-config --test world`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-config/src/world.rs crates/ofs-config/tests/world.rs
git commit -m "feat(config): the world file names client-only visual meshes"
```

### Task 3: Protocol 19 and the server/client plumbing

**Files:**
- Modify: `proto/ofs/v1/sim.proto` (`World.visual_meshes = 13`), the protocol version constants, `crates/ofs-sim/src/session.rs` (`world_msg`), `crates/ofs-sim/src/vehicle.rs` (store the `VisualMeshData`), `crates/ofs-godot/src/lib.rs` (`get_world`), `crates/ofs-client` if it mirrors the world.
- Test: `crates/ofs-sim/tests/grpc.rs`, `crates/ofs-sim/src/session.rs` tests.

**Interfaces:**
- Produces: `pb::World.visual_meshes: Vec<pb::Mesh>`; `world_msg(w, terrain, meshes, visual: &[VisualMeshData]) -> pb::World`; `get_world()` returns `visual_meshes` with `path`, `position`, `heading`, `digest`, `digest_ok`; the server never loads the visual file as a mesh.

- [ ] **Step 1: Write the failing tests**

The test writes a temp world with a `[[visual_meshes]]` entry and a small `.glb`; `world_with_visual_mesh() -> WorldConfig` loads it with `tempfile`.

```rust
#[test]
fn world_msg_carries_the_visual_meshes() {
    let world = world_with_visual_mesh();
    let msg = world_msg(&world, None, &[], &world.visual_meshes().unwrap());
    assert_eq!(msg.visual_meshes.len(), 1);
    assert_eq!(msg.visual_meshes[0].name, "hero");
    assert_ne!(msg.visual_meshes[0].digest, 0);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-sim --test grpc world_msg_carries_the_visual_meshes`
Expected: FAIL (no field/function).

- [ ] **Step 3: Implement** the proto field, the protocol bump to 19, the `world_msg` parameter and mapping, and the `get_world` output. The `Vehicle`/`Session` carries the `VisualMeshData` loaded at build; the repro bundle copies the visual files (Task 14).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --test grpc` and `cargo test -p ofs-config --test world`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add proto/ofs/v1/sim.proto crates/ofs-sim/src/session.rs crates/ofs-godot/src/lib.rs crates/ofs-sim/tests/grpc.rs
git commit -m "feat(proto): carry client-only visual meshes (protocol 19)"
```

### Task 4: The client async visual loader

**Files:**
- Create: `godot/world/visual_loader.gd`, `godot/tests/fixtures/visual_box.glb`.
- Modify: `godot/world/world.gd`, `godot/scripts/app.gd`.
- Test: `godot/tests/test_scene.gd`, `godot/tests/e2e_open_loop.gd`.

**Interfaces:**
- Produces: `VisualLoader.load_all(entries: Array)`, `signal finished`, `func pending() -> int`; `World.visual_loading() -> bool`; the `app.gd` shows a loading banner while pending; a missing/corrupt/digest-mismatched file is reported and skipped.

- [ ] **Step 1: Write the failing test** in `test_scene.gd`: build a world dict with a `visual_meshes` entry pointing at `godot/tests/fixtures/visual_box.glb` (a one-box GLB generated by `tools/sitegen`'s `write_glb` and committed); assert the loader reports pending, then the node appears after `finished`; a second build clears the previous visual nodes; a bad path leaves the world usable.

- [ ] **Step 2: Run it to verify it fails**

Run: `godot --headless --path godot -s res://tests/run_tests.gd`
Expected: FAIL (no `visual_meshes` handling).

- [ ] **Step 3: Implement** the loader with a `WorkerThreadPool`/`Thread` for the file read and `GLTFDocument` parse, adding the scene on the main thread via `call_deferred`; the client checks the FNV-1a digest before adding; `world.gd::build` starts the loads and clears the previous visual nodes; `app.gd` shows/hides the loading banner.

- [ ] **Step 4: Run the Godot unit suite**

Run: `godot --headless --path godot -s res://tests/run_tests.gd`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add godot/world/visual_loader.gd godot/world/world.gd godot/scripts/app.gd godot/tests/test_scene.gd
git commit -m "feat(godot): load the visual layer asynchronously"
```

### Task 5: A `col_`-only collision output

**Files:**
- Modify: `tools/sitegen/sitegen/recipe.py` (`[output] collision_only`), `build.py`, `glb.py`.
- Regenerate: `worlds/assets/chernobyl.glb`.
- Test: `tools/sitegen/tests/test_glb.py`, `test_chernobyl_site.py`, `crates/ofs-config/tests/world.rs`.

**Interfaces:**
- Produces: `[output] collision_only = true` makes every node name `col_<site>`; the server's `load_glb` uses it; the client drops it; the visual file draws.

- [ ] **Step 1: Write the failing tests**: a `collision_only` build's node name starts with `col_`; the `load_glb` mesh is the same triangles; the `world.gd` drops the node; the committed `chernobyl.glb` is the `col_` build.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement** the `collision_only` output flag and the `col_` node prefix; regenerate `worlds/assets/chernobyl.glb`; update the committed-output test.
- [ ] **Step 4: Run the tests**

Run: `.venv\Scripts\python.exe -m pytest tools/sitegen/tests -q` and `cargo test -p ofs-config --test world`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/sitegen worlds/assets/chernobyl.glb
git commit -m "feat(sitegen): a col_-only collision output for the visual split"
```

### Task 6: A test visual asset and the end-to-end split

**Files:**
- Create: `worlds/assets/chernobyl_visual.glb` (a small box for PR 1).
- Modify: `worlds/chernobyl.toml` (`[[visual_meshes]]`), `godot/tests/e2e_open_loop.gd`, `crates/ofs-sim/tests/chernobyl_world.rs`.
- Test: the e2e and the physics/RF tests.

**Interfaces:**
- Produces: the world loads with a visual mesh; the client draws it; the server's collision/RF is unchanged.

- [ ] **Step 1: Write the failing tests**: the e2e asserts the visual node exists and the collision tests still pass; the Rust test asserts the visual file is not in `vehicle.meshes()`.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Add the test visual GLB and the `[[visual_meshes]]` entry.**
- [ ] **Step 4: Run the tests**

Run: `godot --headless --path godot -s res://tests/e2e_open_loop.gd` and `cargo test -p ofs-sim --test chernobyl_world`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add worlds/assets/chernobyl_visual.glb worlds/chernobyl.toml godot/tests/e2e_open_loop.gd crates/ofs-sim/tests/chernobyl_world.rs
git commit -m "test(worlds): a visual mesh beside the col_ collision mesh"
```

---

## PR 2 — the asset and map-creation pipeline

### Task 7: Extend the OSM fetch to roads and landuse

**Files:**
- Modify: `tools/sitegen/sitegen/osm.py` (`_query`, `parse_overpass`, `write_snapshot`, `read_snapshot`), `tools/sitegen/sitegen/build.py` (read the new snapshot shape), `tools/sitegen/README.md`, `docs/research/sitegen-licensing.md` if the format note changes.
- Regenerate: `sites/chernobyl/chernobyl.osm.json`.
- Test: `tools/sitegen/tests/test_osm.py`, `test_fetch.py`, `test_build.py`, `test_chernobyl_site.py`.

**Interfaces:**
- Produces: snapshot `format = 2` with `ways` (closed building rings), `roads` (open polylines), `landuse` (open/closed polylines); `read_snapshot` accepts 1 and 2; the building build ignores roads/landuse; the mapgen reads them.

- [ ] **Step 1: Write the failing tests**: the query asks for `way["highway"]`, `way["landuse"]`, `way["railway"]`, `way["barrier"]`; an open way becomes a road, not an `open_way` skip; the snapshot round-trips format 2; a format-1 snapshot still reads.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement** the query, the parser split, the snapshot format, and the reader; re-fetch `chernobyl.osm.json`; keep the building build unchanged.
- [ ] **Step 4: Run the tests**

Run: `.venv\Scripts\python.exe -m pytest tools/sitegen/tests -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/sitegen sites/chernobyl/chernobyl.osm.json
git commit -m "feat(sitegen): fetch roads and landuse with the building snapshot"
```

### Task 8: The mapgen skeleton, manifest and licences

**Files:**
- Create: `tools/mapgen/__init__.py`, `__main__.py`, `manifest.py`, `tests/test_manifest.py`, `art/chernobyl/LICENSES.md`, `docs/research/assets.md`.
- Modify: `README.md` (the mapgen command).

**Interfaces:**
- Produces: `python -m mapgen build <recipe>`; `worlds/assets/chernobyl_visual.manifest.json` with the source files, node names, triangle counts, materials, LODs, texture licences; the CC0 sources recorded.

- [ ] **Step 1: Write the failing test**: the manifest lists every source asset with its licence and the totals; a missing source is an error.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the CLI skeleton, the manifest writer, and the licence file with the CC0 sources (Poly Haven/ambientCG) and the Blender sources (GPLv3).
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/mapgen art/chernobyl/LICENSES.md docs/research/assets.md README.md
git commit -m "feat(mapgen): the visual pipeline skeleton, manifest and licences"
```

### Task 9: Procedural facades and roofs

**Files:**
- Create: `tools/mapgen/facades.py`, `tests/test_facades.py`.
- Modify: `tools/mapgen/__main__.py`.

**Interfaces:**
- Produces: for each OSM footprint, a visual building with facade bands, windows, a flat/gabled roof, parapets and roof plant; the materials use the CC0 textures; the node names are `vis_<way id>`; the simplified `col_` base is untouched.

- [ ] **Step 1: Write the failing test**: a square footprint yields a facade with the expected window rows and a roof; the triangle count is under the per-building budget; the node name and materials are the contract.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the facade/roof generator and the material mapping.
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests/test_facades.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/mapgen/facades.py tools/mapgen/__main__.py tools/mapgen/tests/test_facades.py
git commit -m "feat(mapgen): procedural facades and roofs for the OSM buildings"
```

### Task 10: Ground, roads and landuse

**Files:**
- Create: `tools/mapgen/ground.py`, `tests/test_ground.py`.
- Modify: `tools/mapgen/__main__.py`.

**Interfaces:**
- Produces: the ground plane with the photoreal material; road/path meshes following the OSM polylines; landuse decals; the node names `vis_ground`, `vis_road_<id>`, `vis_landuse_<id>`; LODs.

- [ ] **Step 1: Write the failing test**: a road polyline becomes a mesh whose centreline follows the points; the ground/landuse nodes exist; the LODs are present; the triangle budget holds.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the ground/road/landuse meshes and materials.
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests/test_ground.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/mapgen/ground.py tools/mapgen/__main__.py tools/mapgen/tests/test_ground.py
git commit -m "feat(mapgen): ground, roads and landuse from the OSM snapshot"
```

### Task 11: Vegetation and props

**Files:**
- Create: `tools/mapgen/vegetation.py`, `tests/test_vegetation.py`.
- Modify: `tools/mapgen/__main__.py`.

**Interfaces:**
- Produces: a deterministic scatter of grass, trees, fences and debris with CC0 assets/LODs; the density is sparse and plausible; the node names and the scatter seed are in the manifest.

- [ ] **Step 1: Write the failing test**: the same seed gives the same placements; the count is within the budget; the LODs exist; no placement is inside a building footprint.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the scatter and the LODs.
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests/test_vegetation.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/mapgen/vegetation.py tools/mapgen/__main__.py tools/mapgen/tests/test_vegetation.py
git commit -m "feat(mapgen): sparse vegetation and props with LODs"
```

### Task 12: The hero Blender models

**Files:**
- Create: `art/chernobyl/nsc.blend`, `units.blend`, `turbine_hall.blend`, `shelter.blend`, `art/chernobyl/export/*.glb`, `art/chernobyl/MANIFEST.md`, `tools/mapgen/hero.py`, `tests/test_hero.py`.

**Interfaces:**
- Produces: the hero models with the node names `vis_nsc`, `vis_unit3`, `vis_unit4`, `vis_turbine_hall`, `vis_shelter`, `vis_stabilisation`; the published dimensions; 2–3 LODs each; the materials/textures; the manifest. The unit 4 ruins and the shelter/stabilisation are included.

- [ ] **Step 1: Write the failing test**: each exported GLB loads; the node names/dimensions match the manifest; the triangle budget holds; the LODs exist.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Author the models in Blender** from the published dimensions and the OSM outlines; export the GLBs; record the dimensions/materials/LODs in `MANIFEST.md`.
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests/test_hero.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add art/chernobyl tools/mapgen/hero.py tools/mapgen/tests/test_hero.py
git commit -m "feat(art): the Chernobyl hero models"
```

### Task 13: Merge the visual GLB, LODs and textures

**Files:**
- Create: `tools/mapgen/merge.py`, `tests/test_merge.py`, `worlds/assets/chernobyl_visual.glb`, `worlds/assets/chernobyl_visual.manifest.json`.
- Modify: `tools/mapgen/__main__.py`.

**Interfaces:**
- Produces: the merged visual GLB with the hero models, procedural buildings, ground/roads/vegetation; the LODs; the CC0 textures embedded/compressed; the manifest; the budget.

- [ ] **Step 1: Write the failing test**: the merged GLB loads; the manifest totals match; every node has a LOD; the file/triangle/texture budgets hold.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the merge and the texture compression.
- [ ] **Step 4: Run the test**

Run: `.venv\Scripts\python.exe -m pytest tools/mapgen/tests/test_merge.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/mapgen/merge.py tools/mapgen/__main__.py worlds/assets/chernobyl_visual.glb worlds/assets/chernobyl_visual.manifest.json
git commit -m "feat(mapgen): merge the hero visual layer with LODs and textures"
```

---

## PR 3 — the hero area, look and performance

### Task 14: Integrate the hero visual and the collision split

**Files:**
- Modify: `worlds/chernobyl.toml` (`[[visual_meshes]]`), `worlds/assets/chernobyl.glb` (the `col_` build), `crates/ofs-sim/src/repro.rs` (copy the visual files), `crates/ofs-sim/tests/chernobyl_world.rs`, `godot/tests/e2e_open_loop.gd`.
- Test: the world/e2e/physics/RF tests.

**Interfaces:**
- Produces: the hero world with the `col_` collision file and the visual file; the repro bundle carries both; the physics/RF unchanged.

- [ ] **Step 1: Write the failing tests**: the world loads with the hero visual; the repro bundle copies the visual file; the physics/RF tests pass; the client draws the hero nodes.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Integrate** the hero visual entry and the `col_` collision build; update the repro bundle.
- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --test chernobyl_world`, `cargo test -p ofs-sim --test repro`, and `godot --headless --path godot -s res://tests/e2e_open_loop.gd`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add worlds/chernobyl.toml worlds/assets/chernobyl.glb crates/ofs-sim/src/repro.rs crates/ofs-sim/tests/chernobyl_world.rs godot/tests/e2e_open_loop.gd
git commit -m "feat(worlds): the Chernobyl hero visual layer"
```

### Task 15: The Chernobyl look and lighting

**Files:**
- Modify: `godot/world/world.gd` (the Chernobyl `LOOKS` entry), `godot/project.godot` (the Forward+ settings from the spike), `godot/tests/test_scene.gd`.
- Test: `godot/tests/test_scene.gd`, the screenshot script.

**Interfaces:**
- Produces: the Chernobyl look (sun/sky/fog/ground/ambient), the Forward+ settings, the quality tiers.

- [ ] **Step 1: Write the failing test**: `look_for("Chernobyl")` returns the pinned values; the environment/sun/ground are set; the quality tiers apply.
- [ ] **Step 2: Run it to verify it fails.**
- [ ] **Step 3: Implement** the look and the settings.
- [ ] **Step 4: Run the test and the screenshots**

Run: `godot --headless --path godot -s res://tests/run_tests.gd` and `godot --path godot -s res://tests/shots.gd -- --out=C:\temp\ofs-shots`
Expected: PASS; the screenshots show the look.

- [ ] **Step 5: Commit**

```bash
git add godot/world/world.gd godot/project.godot godot/tests/test_scene.gd
git commit -m "feat(godot): the Chernobyl look on Forward+"
```

### Task 16: Performance and quality tiers

**Files:**
- Modify: `godot/world/world.gd`, `godot/scripts/app.gd`, `godot/tests/test_scene.gd`, `docs/research/forward-plus.md`.
- Test: the frame-time check and the quality-tier tests.

**Interfaces:**
- Produces: LOD/occlusion/quality tiers; the low tier holds the frame rate; the frame-time record.

- [ ] **Step 1: Write the failing tests**: the quality tiers set the expected MSAA/shadows/fog; the LOD nodes switch at the thresholds.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement** the LOD/occlusion/quality paths.
- [ ] **Step 4: Measure** the frame time on the RTX 2070 at 1080p in the hero area: run `godot --path godot -- --world=chernobyl`, read the HUD/`--verbose` frame time, and record it in `docs/research/forward-plus.md`.
- [ ] **Step 5: Commit**

```bash
git add godot/world/world.gd godot/scripts/app.gd godot/tests/test_scene.gd docs/research/forward-plus.md
git commit -m "feat(godot): LODs and quality tiers for the hero area"
```

### Task 17: Docs, credits and the human check

**Files:**
- Modify: `docs/chernobyl-map.md`, `docs/human-check.md`, `README.md`, `CHANGELOG.md`, `art/chernobyl/LICENSES.md`, `docs/research/assets.md`.

**Interfaces:**
- Produces: the visual/asset documentation, the CC0/OSM credits, the owner's look checklist.

- [ ] **Step 1: Update the docs** with the visual pipeline, the asset licences, the hero area, the look, the performance numbers, and the owner's look.
- [ ] **Step 2: Verify the links and the credits** (the README/help screen, `LICENSES.md`, the world header).
- [ ] **Step 3: Commit**

```bash
git add docs README.md CHANGELOG.md art/chernobyl/LICENSES.md
git commit -m "docs: the photoreal Chernobyl hero area, assets and look"
```

### Task 18: Final verification and the owner's look

**Files:**
- Test: the full gates.

- [ ] **Step 1: Run the full gates**

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
.venv\Scripts\python.exe -m pytest tools/sitegen/tests -q
.venv\Scripts\python.exe -m pytest tools/mapgen/tests -q
godot --headless --path godot -s res://tests/run_tests.gd
godot --headless --path godot -s res://tests/e2e_open_loop.gd
```

Expected: PASS. On Windows, put `C:\msys64\usr\bin` on PATH for the `ofs-fc` `sitl_errors` test: it needs `sh`; the failure without it is environmental.

- [ ] **Step 2: Fly the hero area on the RTX 2070**, check the look, the frame rate, the DVR/clean feed/OSD, and the credit.
- [ ] **Step 3: The owner's look** is the gate; if it fails, record what is wrong and iterate on the look/assets, not the physics.
- [ ] **Step 4: Merge PR 3** after the owner approves.

---

## Self-Review

- **Spec coverage:** every design section has a task: renderer (1), visual/collision (2–6), asset pipeline (8–13), map creation (7–14), hero area (14), look (15), performance (16), licences (8, 17), testing (each task, 18), #134 (0), OSM roads (7), unit 4/shelter (12).
- **Step scan:** each step names the test, the command, the interface or the asset contract; no "TBD"/"handle edge cases".
- **Type consistency:** `[[visual_meshes]]` → `WorldConfig.visual_meshes: Vec<MeshSection>` → `VisualMeshData { name, path, digest }` → `pb::World.visual_meshes` → `world.gd` `visual_meshes` → `VisualLoader`. The `col_` node prefix and the `LOOKS`/`quality_spec` names match the existing/#134 plans.
- **Review Focus:** each line has a task test: missing/corrupt visual (4), budget/async (4, 16), no visual (4, 6), `col_`-only (5, 6), OSM open ways (7), physics/RF (6, 14), Forward+ DVR (1, 15), licences (8, 17), hero GLB (12, 13), owner's look (18).
- **Proportion:** the plan is a task map, not a transcript; the code blocks are test assertions/signatures, and the art tasks are asset contracts.
