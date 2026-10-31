# Terrain in the Godot world (#155, #27 part 3 of 3)

Epic #160 (Consequence: real terrain, crash damage and sound), third child of #27. Part 1 (#153, PR #161) gave the physics
a baked height grid, `ofs_core::terrain::Terrain`, and `worlds/hills.toml`; until now the Godot client draws a flat
ground in a world with `[terrain]`: the physics is right and the picture is not. This part makes the picture the physics.
Part 2 (#154, RF line of sight) and the audio items are not built here.

## Intent

The client shows the hills the sim enforces. The mesh comes from the grid the simulator samples, never from a free-hand
scene, and one shipped world (`park`) gains relief so the feature arrives with something worth flying.

## Goals

- `[terrain]` world: `godot/world/world.gd` draws a mesh whose triangles are exactly the triangles `Terrain::surface`
  samples (same diagonal, same heights), delivered by the simulator in the `World` message, not re-derived.
- The proof obligation: a test compares heights sampled on the mesh with `Terrain` heights at points (Rust, exact
  against the same `Terrain`; Godot, against the real extension and a real server).
- `worlds/park.toml` gets `[terrain]`; its gates, objects and emitter are placed on the ground; the start is on the ground.
  A test pins that the shipped course is playable on the relief.
- The menu-less e2e path and the flat worlds (no `[terrain]`) are untouched: same dictionary keys (one added to an
  emitter, zero in a flat world's terrain), same nodes, same ground plane.

## Non-goals and anti-goals

- RF line of sight (#154); physics changes (part 1 is the truth and is not touched, except `Terrain::digest`, below).
- Textures, vegetation, water, LOD, mesh collision in Godot (the physics collides, Godot only draws), the Python client
  exposing the grid (it still receives the message; its `World` dataclass does not carry the terrain).
- A free-hand terrain scene, or a GDScript copy of the hill generator or of the height rule. Anti-goal: the client
  and the sim disagreeing about the ground in silence (a corrupted or mismatched grid is an error, not a flat world).

## Rulings (calls made without the user)

1. **Delivery: the grid rides in `World` (protocol 15 -> 16).** `World.terrain` (field 11, absent in a flat world) carries
   origin (north, east), cell size, node counts, the exact `repeated float heights` and the grid's digest. This is the
   "single source" choice part 1 recommended. A bake of the parameters in the client was rejected: it would be a second
   copy of the generator (and the hand-built-grid door of part 1 would have no way in).
2. **Digest hashes the origin.** `Terrain::digest` now also hashes `origin_ne` (part 1's handover said the handshake must).
   The client rebuilds a `Terrain` with `from_grid` from the message and recomputes the digest; a mismatch, or a grid
   `from_grid` refuses, fails the load with a protocol error. Re-pinned: the default section's baked-grid pin in
   `ofs-config/tests/world.rs` (computed, not guessed). Nothing else pins `Terrain::digest`; the lockstep (bus) digest
   does not contain it and is unchanged.
3. **The mesh is generated in Rust, in the client library, from the verified `Terrain`.** A 601 x 601 grid is 361 000
   vertices and 720 000 triangles; building that in GDScript every world load would stall the main thread, and a Rust
   function is tested natively on every OS against `Terrain::surface`. `ofs_client::terrain_mesh(&Terrain)` returns
   positions, normals and a triangle index list in Godot's frame; `OfsClient.get_world()` hands them over as packed
   arrays under `terrain`; `world.gd` wraps them in an `ArrayMesh`. GDScript does no height maths.
4. **Mesh layout is the sim's rule.** Vertex `i * nodes_e + j` is node (i, j) at Godot position
   `(origin_east + j * cell, height, -(origin_north + i * cell))`. Each cell is two triangles on the (i, j)-(i+1, j+1)
   diagonal: (00, 10, 11) and (00, 11, 01), the sim's lower and upper triangle (the upper one reordered so both are), wound clockwise seen from above (Godot's
   front face). Heights are the grid's f32s unchanged; only the horizontal coordinates round to f32 (at most about 1.5e-5 m
   at 300 m, so a height off by under 3e-5 m on the steepest allowed slope: the tests use 1e-4 m).
5. **Shading normals are smooth, geometry is exact.** Vertex normals are central differences of the grid (for lighting
   only); the triangles are the physics' triangles. A faceted look (face normals) was rejected: at 1 m cells it reads as
   noise, and the shader derivative trick cannot be checked headless.
6. **The ground.** With a terrain, `world.gd` hides the flat `Ground` plane (the node stays, so tests and callers that
   look it up still find it) and draws the grid mesh plus a flat ring (surface 2 of the same mesh, y = 0) out to the
   plane's 6000 m extent, with the same grid-shader material, so the metre lines run on across the hills and beyond.
   Beyond the grid the sim's ground is the plane d = 0 (and part 1's generator leaves a zero border), so ring and
   mesh meet at y = 0. A hand-built grid with a non-zero border has a visible cliff, as in the physics.
7. **Emitter posts stand on the ground.** `WorldEmitter` gains `ground_y` (the terrain height under it, 0 when flat); the
   post runs from there up to the emitter. The pilot figure and the handset are left as they were: the config
   validation already refuses a pilot or handset below the terrain, and the shipped worlds keep both on the flat disc.
8. **Objects, gates and the emitter are placed by the author**, in the same NED frame, as part 1 ruled. There is no
   auto-rooting in Godot. `park.toml` is authored against the baked relief (the numbers come from sampling `Terrain`;
   the walls, posts and pillars are buried down to the lowest ground under their footprint so none floats), and a test
   re-derives the rules from the file so a changed terrain section cannot silently break the course.
9. **Message size.** The grid is up to 1025 x 1025 x 4 B = 4.2 MB, above tonic's and grpc-python's 4 MiB default receive
   limit. The Rust client and the Python client raise it to 16 MiB; the server's sending is unlimited.
10. **Python.** Protocol constant 16, regenerated stubs, the receive limit. The `World` dataclass does not carry the grid
    (non-goal); the message does, so a later change can expose it without another protocol bump.
11. **The Godot proof test needs a reference.** `OfsClient.terrain_height(x, z)` (Godot-frame metres, the extension's one
    new method) returns `Terrain::height_up` of the verified grid, 0 in a flat world. It is the same Rust the physics
    runs, so it is the ground truth the mesh is compared with.
12. **The builder-check subagent is skipped**, as in part 1: the issue is the design intent, the rulings are here, and
    the instruction was not to stop and ask.

## The design

### Protocol 16

```proto
message Terrain {
  double origin_north_m = 1;  // node (0, 0)
  double origin_east_m = 2;
  double cell_m = 3;
  uint32 nodes_n = 4;
  uint32 nodes_e = 5;
  repeated float heights = 6;  // metres up, row-major, north the slow index: nodes_n * nodes_e values
  fixed64 digest = 7;          // ofs_core::terrain::Terrain::digest of the grid (includes the origin)
}
message World { ...; Terrain terrain = 11; }   // absent: the plane d = 0
```

`session::world_msg` fills it from `WorldConfig::terrain.build()` (the same bake the vehicle samples; the generator is
deterministic, so building twice is the same grid, and a test pins the digest equality with `vehicle`'s terrain).
The proto header comment, `python/ofs/client.py` and `ofs-proto/tests/messages.rs` move to 16.

### `ofs-client`

`World.terrain: Option<Arc<Terrain>>` (an `Arc`: `OfsClient.world()` clones the model). `World::from_pb` becomes
`Result<World, String>` (every existing caller either unwraps in tests or maps to a `Protocol` error in the worker).
New `terrain_mesh.rs`:

```rust
pub struct TerrainMesh { pub positions: Vec<[f32; 3]>, pub normals: Vec<[f32; 3]>, pub indices: Vec<u32> }
pub fn terrain_mesh(t: &Terrain) -> TerrainMesh;   // Godot frame, layout as ruling 4
```

### `ofs-godot`

`get_world()` adds `terrain` (only when present): `origin_x`, `origin_z` (Godot frame of node (0, 0)), `cell_m`,
`nodes_n`, `nodes_e`, `vertices` (PackedVector3Array), `normals` (PackedVector3Array), `indices` (PackedInt32Array),
`digest` (a 16-digit hex String); emitters gain `ground_y`. New `#[func] terrain_height(x, z) -> f64`.

### `godot/world/world.gd`

`build(world)` with `terrain` makes the node `Terrain` (a `MeshInstance3D` through `_add_built`, so the next build removes
it): surface 0 the grid arrays, surface 1 the ring (8 vertices, 8 triangles) from the grid's rectangle out to 3000 m each
way; the grid shader material; the `Ground` plane hidden. Without a terrain `Ground` is visible again. Static helpers:
`terrain_arrays(t)` and `ring_arrays(x0, x1, z0, z1)`, so unit tests build meshes without a simulator.

### `worlds/park.toml` and `hills.toml`

`park.toml` gains `[terrain]`: gentle relief (amplitude 2.5 m, wavelength about 60 m: slopes of a few degrees, the
validation cap is 60), a flat disc around home for the pad, the pilot and the start gate approach. Every object, gate
and the emitter are re-placed on the ground (see ruling 8). `hills.toml` header loses "the Godot client draws a flat
ground until #155".

## Tests

- `ofs-core`: digest changes with the origin (and still with one bit of height).
- `ofs-config`: the default section's grid digest re-pinned; park loads with a terrain; **the park course on the relief**:
  the pilot and handset above ground, home flat, every gate's opening clears the ground with the bottom edge no lower
  than the ground at its centre and the centre no more than 2 m above it, each box or cylinder's base at or below the
  lowest ground under its footprint (nothing floats) and its top above the highest, the emitter 0.5 to 2 m above ground,
  and the steepest slope under the gentle cap (30 degrees).
- `ofs-sim`: `world_msg` carries the grid of a `[terrain]` world, equal to the vehicle's terrain, digest equal; none for
  a flat world; a `grpc.rs` test loads `hills` over gRPC and rebuilds the same digest from `get_world`.
- `ofs-client`: **the mesh against the sim's `Terrain`**: at nodes, cell centres, points on and either side of every
  diagonal, along the grid edge and outside it (mesh height by barycentric interpolation of the triangle holding the
  point, the triangle taken from the index buffer) equals `Terrain::height_up` within 1e-4 m; every triangle is
  clockwise from above (cross product y < 0) and every normal a unit vector with y > 0; counts; a corrupted digest, a
  wrong heights length and a bad cell are errors; a flat world has no terrain, an emitter's `ground_y` follows the relief.
- Godot unit (`test_scene.gd`): a world dict with a tiny terrain builds `Terrain` with two surfaces, hides `Ground`, the
  ring winds upward (`Plane(a, b, c).normal.y > 0`), a rebuild replaces it, a flat world shows `Ground` again; the emitter
  post stands on `ground_y`.
- Godot e2e (`e2e_terrain.gd`, run by `run-godot-tests.sh` in the `terrain` suite and in `e2e`/`all`): loads
  `worlds/hills.toml` through a real server, builds `world.gd` from `get_world()`, and for a grid of sample points
  (including a diagonal and points outside the grid) compares the height read off the `ArrayMesh`'s triangles with
  `OfsClient.terrain_height`; it also loads `park` and checks the gate centres sit above `terrain_height`.
- Python: the protocol constant; `grpc.max_receive_message_length`.

## What a human must look at

Godot shows nothing in headless tests: the tests cover mesh data, node structure and numbers. A person must run
`godot --path godot`, pick Hills and Park, and look: the relief reads (lighting, grid lines running over it, the horizon
joining the ground ring without a seam), the gates stand on the ground, the quad lands on the ground it sees (not
sinking or floating), and the frame rate holds with the 361 000-vertex mesh in both the FPV and the DVR/second view.
`tests/shots.gd` is extended to save a `terrain.png` from the hills world for that check.

## Docs and bookkeeping

`CHANGELOG.md` (protocol 16, the digest change, park, `terrain_height`); `docs/dev-setup.md` (`[terrain]` no longer a
flat picture; the Godot suites; the message limit); `docs/architecture.md` if it names the world message;
`python/README.md` (protocol 16); `README.md` if it lists worlds; `docs/roadmap.md`.
