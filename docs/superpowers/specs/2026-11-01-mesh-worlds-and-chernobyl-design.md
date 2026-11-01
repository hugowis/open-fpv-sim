# Mesh worlds, a site generator, and a small Chernobyl map

Three pieces, built in this order, each with its own plan and PR: (1) **mesh worlds** in the engine, (2) a **site
generator** that turns real data into a world, (3) the **Chernobyl reactor-area map**, the first world made with both.
This document is the umbrella design; each piece gets a plan from it.

## Intent

Today a world is boxes and upright cylinders, flat colours, on a flat or seeded-hill ground (`worlds/*.toml`). The user
wants a map that **looks like the real Chernobyl reactor site** (the New Safe Confinement over reactor 4, its
neighbours) that you can fly **around and through openings into**, and they want it to be the **opening of a new maps
system**: later maps (an extension of this one, other sites) use the same pipeline. They have no 3D assets and cannot
model by hand, so everything is **generated, by me, from public data and published dimensions**.

Success: a recognisable Chernobyl reactor area at true scale, flyable end to end, with the sim's collision and radio links
treating the mesh as real geometry; and a generator another site can be run through by changing data, not code.

## Goals

- A world can reference one or more triangle meshes. One mesh file is the single source for what Godot draws and what the
  Rust sim collides with and blocks radio with (the terrain rule: nobody re-derives geometry).
- Flying through openings works: collision is against the **surface**, from either side; the video and ELRS links are
  blocked by walls and pass through gaps.
- Deterministic: a flight in a mesh world replays exactly; a world without meshes is bit-identical to before.
- A generator (dev tool, not shipped in the sim) turns a site recipe + open data into a mesh file and the world TOML.
- Everything shipped is legally redistributable: generated geometry, procedural materials, attributed open data only.
- A small first map (about 600 m x 600 m), recognisable, extendable by re-running the generator on a bigger area.

## Non-goals and anti-goals

- Photoreal textures, photogrammetry, scanned or purchased or AI-image-derived models. Recognisable shape and true scale
  with procedural materials is the target.
- A full plant, Pripyat, the Duga radar, the exclusion zone (later maps).
- Rotated or curved primitives in the old `[[objects]]` list: meshes replace that need; `[[objects]]` stays as is.
- Radio diffraction around mesh edges (see RF; documented limit). Dynamic or destructible meshes.
- Streaming, LOD systems, or a mesh editor. Remote (non-local) client/server asset delivery.
- Anti-goal: any change to a world without `[[meshes]]`: not a bit of its flights, bus digest, signal list or protocol
  behaviour (the flat-world pin must pass).
- Anti-goal: a map that is blocky boxes again, or a photo-scan that cannot be collided with or regenerated.

## Constraints already in the code (what the mesh must fit)

- Geometry consumers: `ofs-core::Shape` (box, cylinder) is used by physics (`ofs-physics/src/collision.rs`: sphere
  contact via `signed_distance` + `normal`, broad phase by `Aabb`), by RF (`ofs-rf/src/propagation.rs`: `Obstacle`,
  `deepest_point`, `obstruction_loss_db` knife-edge on the segment) and by the video and ELRS links. The Godot client
  only draws (`godot/world/world.gd` builds `BoxMesh`/`CylinderMesh` from the `World` message).
- The physics body is a set of small collision spheres (cm radius) stepped at 8 kHz: a few mm per step.
- The ground is `ofs-core::Terrain` (a baked height grid, sampled by physics). A real-data DEM is already anticipated by
  `Terrain::from_grid` (the terrain design left that door open). Chernobyl's site is nearly flat.
- Worlds are single TOML files (`deny_unknown_fields`, `schema_version = 1`) and repro bundles carry the world file
  (`ofs-sim/src/repro.rs`). A mesh is a second file the bundle must carry.
- The repo is GPLv3.

## Piece 1: mesh worlds (the engine)

### The world file

An opt-in, additive section; `schema_version` stays 1 (like `[terrain]`):

```toml
[[meshes]]
name = "plant"
file = "assets/chernobyl.glb"      # relative to the world file
position_ned_m = [0.0, 0.0, 0.0]   # where the file's origin sits; the file is Y-up metres, converted on load
heading_deg = 0.0

[[materials]]                       # per glTF material name; an unlisted material is 0 dB (lets the signal through)
name = "concrete"
rf_loss_db = 18.0
[[materials]]
name = "steel"
rf_loss_db = 40.0
```

Validation follows the existing style (field paths, finite numbers, unique names, file exists and parses, triangle count
at most a documented cap, every triangle finite and non-degenerate).

### The file format (my call)

**glTF binary (`.glb`)**, because Godot imports and loads it natively and the Rust side needs only positions, indices
and material names. The Rust loader is a new small module reading the GLB with the `gltf` crate, or a minimal reader if
the builder judges the dependency heavier than the format (builder's choice; the existing rule "no new dependency
for a few lines" applies, but a glTF parser is not a few lines). Collision and radio use a **collision proxy**: nodes
whose name starts with `col_` are collision/RF-only (simplified, no small detail); all other nodes are visual only. A
file with no `col_` nodes uses its visual nodes for collision. One file, two roles, so the two cannot drift.

### Collision (`ofs-physics`, `ofs-core`)

A new `ofs_core::mesh::TriMesh`: triangles in NED f64, plus a **bounding-volume hierarchy** built deterministically
(median split on a stable sort, ties by index) for closest-point queries.

- Contact is **sphere against the surface**, two-sided: the nearest point on the nearest triangle, penetration
  `radius - distance`, normal from the surface point toward the sphere centre. No inside/outside test, so meshes need not
  be watertight and a thin wall is a wall from both sides. This is what makes flying through a gap and bumping a wall
  both work.
- It plugs in beside the existing object loop in `collect`: broad phase on the mesh's `Aabb`, then BVH. Events use the
  mesh's slot in `TouchState` (one per mesh, not per triangle).
- Tunnelling bound: a sphere moves millimetres per step; the minimum collision-proxy wall is treated as a surface, not a
  volume, so any thickness works. Documented in `docs/research/collision.md`.
- Determinism: only IEEE basic operations and `sqrt` in the query (no `sin`/`exp`/`pow`, no fused multiply-add), fixed
  traversal order; a test pins the digest of a built BVH on every OS.
- Performance budget: stated in the plan; a timed `#[ignore]` test records the shipped quads in the map against a flat
  world (as the terrain work did).

### Radio and video (`ofs-rf`, `ofs-video`, `ofs-radio`)

The links ask "how much does this obstacle attenuate the segment a to b". For a mesh: the **sum of the per-material
`rf_loss_db` of every triangle the segment crosses** (segment-triangle intersection through the BVH). Consequences, all
intended: walls block, openings pass, flying into the hall loses video through the roof but not through the doorway.

Known limits, written into the docs: no diffraction around mesh edges (the shadow edge is sharp), no reflection off a
mesh, no thickness (one crossing = one loss, so a two-skin wall is two crossings). The `Obstacle` type gains a mesh
variant so the existing knife-edge path for boxes and cylinders is untouched.

### Godot client

The client must draw the same mesh the sim collides with. Chosen: the client **loads the same `.glb` itself** (no mesh
bytes over gRPC) and checks the file's digest against the one the sim reports.

- `World` message gains `meshes: [{name, file, position, heading, digest}]` and `materials`; the **protocol version is
  bumped** (as the project does for wire changes). A world without meshes sends nothing new.
- `world.gd` loads each file with Godot's runtime glTF loader, positions it, and skips `col_` nodes when drawing
  (they are collision-only). Nothing else in the client changes.
- Resolution of `file`: relative to the world file on the sim side; the sim hands the client an absolute path, valid
  because sim and client run on the same machine (non-goal: remote clients).

### Repro bundles

`repro.rs` carries the world file; it must also carry each mesh file and keep paths relative inside the bundle so a
replay finds them. The bundle manifest records the mesh digests.

### Terrain interaction

Meshes sit on top of whatever ground the world has (flat or `[terrain]`). The Chernobyl map uses a flat ground; the
generator can emit a `Terrain` grid from a DEM when a site needs it (piece 2).

## Piece 2: the site generator (a dev tool)

Lives in `tools/sitegen/` (Python, the repo already has `python/` and `tools/make_osd_font.py`), run by developers, not
by the sim. Dependencies are dev-only and pinned.

Input: a **site recipe** (TOML): site origin (lat/lon), extent, and a list of items. Item kinds:

- **footprints**: building outlines from OpenStreetMap (a snapshot fetched once and committed with its attribution),
  extruded to a height from the tag, a recipe override, or a documented default.
- **landmark builders**: parametric solids for things footprints cannot describe: the arch (a swept elliptical section
  with a double skin), a tapered striped stack, a cooling tower (hyperboloid), a hall with a pitched roof and named
  breaches. Each takes its real dimensions as parameters, with the source of each number in the recipe.
- **ground**: flat, or a DEM tile resampled to a `Terrain` grid when a licence-clean source is chosen.

Output: a `.glb` (visual + `col_` proxy nodes, named procedural materials, vertex colours and a weathering pass baked
in, no external textures) and the world TOML fragment (`[[meshes]]`, `[[materials]]`, pilot spot). Deterministic: same
recipe and snapshot give the same bytes, so the output can be checked in and regenerated.

The generator is generic by construction: **Chernobyl is a recipe, not code.** A second site is a second recipe.

Licensing handled in the tool and the docs: OSM-derived footprint snapshots stay ODbL with attribution (`docs/` and the
world header credit "(c) OpenStreetMap contributors"); the generated meshes are a produced work. The builder confirms
that ODbL data under a GPLv3 repo is acceptable before committing a snapshot, and writes the conclusion down; if not,
footprints are hand-listed from published plans instead. Every dimension is an estimate and says so, per the world-file
convention ("All values are estimates").

## Piece 3: the Chernobyl reactor-area map (first content)

`worlds/chernobyl.toml` + `worlds/assets/chernobyl.glb`, generated from `sites/chernobyl.toml`.

Contents (my call; values from published figures, to be confirmed against sources during the plan):

- **New Safe Confinement**: arch about 257 m span, 108 m high, 162 m long, over reactor 4 and the old sarcophagus.
- **Unit 3 and 4 buildings** and the **turbine hall** alongside, trimmed to the map's extent.
- **Ventilation stack VP-2**, about 150 m, red-and-white bands.
- A few service buildings and roads from OSM; flat ground; the pilot starts on an open spot with a clear view.
- **Openings to fly through** (gameplay liberties, since the real NSC is sealed): the NSC end door openings, and named
  breaches in the turbine hall roof and walls. The inside is a simplified volume, enough to give interior flights;
  no detailed interior.
- Ships with a short note listing which parts are measured and which are artistic.

Extent about 600 m x 600 m so it reads as a place without making the first mesh enormous. Extending it later means a
bigger recipe extent and re-running the generator; no engine change.

## Testing (per piece, detail in each plan)

- **Mesh worlds**: BVH closest-point and segment-crossing agree with a brute-force reference on random and adversarial
  meshes; sphere contact on a flat quad, an edge, a corner and a thin double-sided wall; a quad flies through a
  gap in a mesh wall and bounces off the wall beside it; RF loss through a wall versus through the gap; replay
  determinism and the pinned BVH digest on every OS; **the flat-world bus-digest pin passes unchanged**; the client
  loads the file and the digest handshake rejects a mismatch; repro bundle round trip; validation of every rule.
- **Generator**: same recipe twice gives identical bytes; output passes the engine's loader and validation; a small
  synthetic site as a fixture (not Chernobyl) so the tool is tested independently of the map.
- **Map**: loads, every opening is wide enough for the shipped quads (checked by a test that flies or measures it), the
  pilot is not inside geometry, triangle count under the cap, frame time on the dev machine recorded.

## What is left to the builder

Mesh format library (the `gltf` crate or a minimal reader); BVH details; the triangle cap and performance numbers;
GLB node conventions beyond `col_`; the exact set of OSM items kept; the weathering style; how openings are sized.

## Calls made without the user (rulings)

1. Everything generated from public data and published dimensions; no scans or purchased models.
2. glTF binary as the single mesh file, collision proxy by `col_` node-name prefix.
3. Sphere-vs-surface two-sided collision, no inside/outside test (open, non-watertight meshes are fine).
4. RF through a mesh = sum of per-material loss per surface crossing; no diffraction (documented limit).
5. Client loads the `.glb` itself and checks a digest; protocol bump; no mesh over gRPC.
6. The generator is a Python dev tool under `tools/sitegen/`; Chernobyl is a recipe.
7. Build order: engine, generator, then the map; one plan and PR each.
8. First map about 600 m x 600 m with openings as gameplay liberties; flat ground.
9. Interior depth: simplified volumes, not a detailed interior.
10. Collision events: meshes get their own object-index range after `world.objects` (so `HIT <name>` and `TouchState`
    resolve a mesh by its `[[meshes]]` name, not by indexing `world.objects`). Settled by the builder check.
11. The ODbL-under-GPLv3 check is scheduled in the generator's plan before any OSM snapshot is committed; the fallback
    is hand-listed footprints from published plans.
12. Two screenshot review points, after the generator and after the map, so a bad look is caught before the map is
    finished ("recognisable" is a judgement call, and it is the user's to make).
