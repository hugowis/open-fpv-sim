# Mesh worlds (engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A world can reference triangle-mesh files (`.glb`) that the physics collides with, the video and ELRS links are blocked by, and Godot draws, from one file.

**Architecture:** A new `ofs_core::mesh::TriMesh` (triangles + deterministic BVH) answers two queries: spheres within a radius (collision) and surface crossings along a segment (radio). `ofs-config` reads the `.glb` and the `[[meshes]]`/`[[materials]]` sections; `ofs-sim` builds each mesh once and hands the same `Arc` to the physics and both links, following how `Terrain` is plumbed. Godot loads the same file itself; the sim sends only its path and a digest (protocol 17).

**Tech Stack:** Rust (`glam`, new dev-light dependency `gltf` without image decoding), gRPC/prost, Godot 4.7 GDScript + godot-rust extension, Python client stubs.

**Spec:** `docs/superpowers/specs/2026-11-01-mesh-worlds-and-chernobyl-design.md` (piece 1 of 3; the generator and the Chernobyl map are separate plans).

**Base:** work on a worktree cut from `origin/main` (8e76941, protocol **16**: the terrain RF work #168 and Godot terrain #169 are in). The checkout this plan was written in is behind; the spec and this plan are carried over to the feature branch `feat/mesh-worlds`.

## Global Constraints

- A world without `[[meshes]]` is bit-identical to before: not one bit of its flights, bus digest, signal list or protocol behaviour. `crates/ofs-sim/tests/flat_world_pin.rs` must pass unchanged.
- Deterministic: a flight in a mesh world replays exactly. Geometry queries use only IEEE basic operations and `sqrt` (no `sin`/`exp`/`pow`, no fused multiply-add), in a fixed traversal order. Trigonometry appears only when a mesh is loaded with a non-zero `heading_deg`; pins use heading 0.
- Collision is **sphere against the surface, two-sided**: no inside/outside test, meshes need not be watertight.
- Radio loss through a mesh is the **sum of the per-material `rf_loss_db` of every surface the segment crosses**; no diffraction, no reflection (documented limits).
- `.glb` is the only mesh format. A node whose name starts with `col_` is collision/RF-only (not drawn); if the file has none, the visible nodes are used for collision. File coordinates are Godot's frame (x east, y up, -z north); NED = (-z, x, -y).
- Caps (my estimates, in `ofs_config::mesh`): `MAX_MESHES = 8`, `MAX_MESH_TRIANGLES = 400_000` per world, `MAX_MESH_FILE_BYTES = 64 MiB`.
- New dependency: only `gltf` (`default-features = false`, features `utils`, `names`; verify it builds without the `image` crate). If it cannot, write a minimal GLB reader instead and say so in the commit.
- Protocol 16 -> 17 (`ofs-proto`, `python/ofs/client.py`, the messages test, docs). The mesh bytes never cross gRPC.
- The repo is GPLv3. Every dimension in a shipped world is an estimate and its header says so.
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.

## Review Focus

- A `.glb` that is missing, truncated, not a GLB, empty, over the caps, or has non-finite vertices: a clear load error naming `meshes[i].file`, never a panic or a silent empty mesh (Tasks 2, 3).
- A sphere centred exactly on a surface, an edge or a vertex: a finite unit normal, no NaN (Task 1, Task 4).
- A segment through a shared edge or vertex of two triangles, or lying in a wall's plane: counted once or not at all, never twice or NaN (Task 1).
- `heading_deg` and the axis swap: a mesh 10 m north of its origin at heading 90 is 10 m east (Task 2).
- The client's file differs from the sim's (moved, edited, missing): Godot draws nothing for that mesh and says why; the sim is unaffected (Tasks 8, 9).
- A quad spawned overlapping a wall comes out without exploding (Task 4).

---

### Task 1: `TriMesh` and its BVH

**Files:**
- Create: `crates/ofs-core/src/mesh.rs`, `crates/ofs-core/tests/mesh.rs`
- Modify: `crates/ofs-core/src/lib.rs` (add `pub mod mesh;`)

**Interfaces:**
- Produces (all `pub`, in `ofs_core::mesh`):
  - `struct Triangle { pub a: DVec3, pub b: DVec3, pub c: DVec3, pub material: u32 }`
  - `struct MeshHit { pub distance: f64, pub point: DVec3, pub normal: DVec3, pub triangle: u32 }` (`normal` is the unit vector from `point` to the query centre, the triangle's face normal when the distance is under 1e-12)
  - `TriMesh::new(triangles: Vec<Triangle>) -> Result<TriMesh, String>` (rejects empty, non-finite, zero-area)
  - `TriMesh::{len, bounds() -> Aabb, triangle(i: u32) -> &Triangle, digest() -> u64}`
  - `TriMesh::within(&self, p: DVec3, radius: f64, out: &mut Vec<MeshHit>)` appends every triangle with `distance < radius`, in BVH traversal order
  - `TriMesh::crossings(&self, a: DVec3, b: DVec3, out: &mut Vec<u32>)` appends the material of every surface the open segment crosses; crossings within 1e-9 of the segment length of each other count once
  - `closest_point_on_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3`

- [ ] **Step 1: Write the failing tests** in `crates/ofs-core/tests/mesh.rs` (NED, triangle `(0,0,0) (10,0,0) (0,10,0)` in the plane d = 0 unless stated):
  - `within_finds_the_triangle_a_sphere_touches`: p = (2,2,-0.3), r = 0.5 gives one hit, distance 0.3, point (2,2,0), normal (0,0,-1).
  - `within_is_two_sided`: p = (2,2,0.3) gives normal (0,0,1).
  - `an_edge_contact_points_away_from_the_edge`: p = (5,-0.3,0) gives distance 0.3, normal (0,-1,0).
  - `a_sphere_centred_on_the_surface_gets_a_unit_face_normal`: p = (2,2,0) gives finite normal of length 1 along Z.
  - `within_matches_a_brute_force_scan`: 500 seeded random triangles (`rand_chacha`), 200 random queries; the hit triangle indices equal those found by calling `closest_point_on_triangle` on every triangle.
  - `crossings_report_each_surface_once`: a 2-triangle wall quad at north = 10 (material 7) crossed by (0,0,-1)..(20,0,-1) gives `[7]`; two parallel walls (materials 7, 9) give both.
  - `a_crossing_on_a_shared_edge_counts_once`: the same segment aimed at the quad's diagonal gives one entry.
  - `a_segment_in_a_walls_plane_or_ending_short_crosses_nothing`.
  - `new_rejects_empty_non_finite_and_zero_area`.
  - `bounds_cover_every_vertex`.
  - `bvh_digest_is_pinned`: an 8 x 8 grid of integer-coordinate triangles (128 triangles); `const PIN: u64` (run once, paste the printed value; the claim is it is identical on every OS).

- [ ] **Step 2: Run to verify failure** — `cargo test -p ofs-core --test mesh` fails to compile (no `mesh` module).

- [ ] **Step 3: Implement** `crates/ofs-core/src/mesh.rs`. Closest point by the standard region test (Ericson). BVH: leaves of at most 4 triangles, built by median split on the longest axis of the centroid box, triangles ordered by `(centroid[axis].total_cmp, original index)`; nodes stored in a `Vec`, children visited left then right. Segment-triangle by Moller-Trumbore with a small inclusive tolerance on the barycentrics, then merge crossings with equal parameter as above. `digest` hashes (with `Fnv1a`) every triangle's vertex bits and material, then every node box's bits.

- [ ] **Step 4: Run** `cargo test -p ofs-core --test mesh` — all pass, and `cargo test -p ofs-core` still passes.

- [ ] **Step 5: Commit** — `git add crates/ofs-core && git commit -m "feat(core): TriMesh - triangles, a deterministic BVH, sphere and segment queries"`

---

### Task 2: Reading a `.glb`

**Files:**
- Modify: `crates/ofs-config/Cargo.toml` (add `gltf`, workspace-level entry in the root `Cargo.toml`), `crates/ofs-config/src/lib.rs` (`pub mod mesh;`)
- Create: `crates/ofs-config/src/mesh.rs`, `crates/ofs-config/tests/common/glb.rs` (a GLB *writer* for tests and the Task 3 example), `crates/ofs-config/tests/mesh.rs`

**Interfaces:**
- Consumes: `ofs_core::mesh::{TriMesh, Triangle}`.
- Produces in `ofs_config::mesh`: `MAX_MESHES`, `MAX_MESH_TRIANGLES`, `MAX_MESH_FILE_BYTES`; `struct LoadedGlb { pub mesh: TriMesh, pub material_names: Vec<String>, pub digest: u64 }`; `load_glb(path: &Path, position_ned_m: [f64; 3], heading_deg: f64) -> Result<LoadedGlb, String>` and `load_glb_with(path, position_ned_m, heading_deg, max_triangles: usize)`. `digest` is `ofs_core::rng::fnv1a64` of the file's bytes. `material_names` are in order of first use; a primitive with no material is named `"default"`.
- Test helper: `write_glb(nodes: &[GlbNode]) -> Vec<u8>` with `struct GlbNode { name: &str, triangles: Vec<[[f32; 3]; 3]>, material: Option<&str> }`.

- [ ] **Step 1: Write the failing tests** in `tests/mesh.rs`, files written to a `tempfile` dir:
  - `a_triangle_comes_out_in_ned`: file point (x=3, y=5, z=-10) loads as NED (10, 3, -5).
  - `heading_turns_a_mesh_clockwise_from_north`: the point 10 m north of the origin at heading 90 is (0, 10, 0); position_ned_m is added after.
  - `col_nodes_replace_the_visible_ones_for_collision`: a file with a visible node and a `col_` node yields only the `col_` triangles; a file with no `col_` node yields all.
  - `material_names_follow_first_use`.
  - `degenerate_triangles_are_dropped_and_an_all_degenerate_file_is_an_error`.
  - `a_file_over_the_triangle_cap_is_refused` (via `load_glb_with(.., 2)` on a 3-triangle file).
  - `bad_files_are_errors_not_panics`: missing file, empty file, 12 random bytes, a truncated GLB, a GLB with a NaN vertex — each `Err` whose text names the path.
  - `a_file_over_the_size_cap_is_refused` (check the length before reading; test with a sparse file or the cap constant passed through a `_with` argument if you prefer).

- [ ] **Step 2: Run** `cargo test -p ofs-config --test mesh` — fails to compile.

- [ ] **Step 3: Implement** the loader and the test writer. Walk the default scene's nodes applying each node's transform (matrix or TRS) so authors can nest nodes; use `Gltf::from_slice` and the embedded blob, no image decoding. Apply the file-to-NED map, then the heading rotation, then the position, in f64.

- [ ] **Step 4: Run** `cargo test -p ofs-config --test mesh` — pass.

- [ ] **Step 5: Commit** — `git commit -m "feat(config): read a .glb into a TriMesh in NED (gltf, no image decoding)"`

---

### Task 3: `[[meshes]]` and `[[materials]]` in the world file, and a demo hall

**Files:**
- Modify: `crates/ofs-config/src/world.rs`, `crates/ofs-config/src/lib.rs` (re-exports), `crates/ofs-config/tests/world.rs`
- Create: `crates/ofs-config/examples/make_hall.rs`, `worlds/hall.toml`, `worlds/assets/hall.glb` (generated, committed)

**Interfaces:**
- Consumes: Task 2.
- Produces:
  - `struct MeshSection { pub name: String, pub file: String, pub position_ned_m: [f64; 3], pub heading_deg: f64 }` (position and heading default 0) and `struct MaterialSection { pub name: String, pub rf_loss_db: f64 }` (default 0), both `deny_unknown_fields`; `WorldConfig` gains `pub meshes: Vec<MeshSection>` and `pub materials: Vec<MaterialSection>` (`#[serde(default)]`, empty in `open_field()`), schema_version stays 1.
  - `struct WorldMeshData { pub name: String, pub path: PathBuf, pub digest: u64, pub mesh: Arc<TriMesh>, pub loss_db: Vec<f64> }` and `WorldConfig::load_meshes(&self) -> Result<Vec<WorldMeshData>, String>`; `loss_db[m]` is the `[[materials]]` loss of the mesh's material `m` (0 if unlisted); `path` is absolute (canonicalized) because Task 8 hands it to the client.
  - `world::load` calls `load_meshes` after `validate` and reports a failure as a `Problem` with field `meshes[i].file`.

- [ ] **Step 1: Write the failing tests** in `tests/world.rs`:
  - `meshes_and_materials_parse`: a world with one mesh and two materials; `loss_db` follows material order.
  - validation, each its own assertion on the `Problem` field: duplicate mesh names (`meshes[1].name`), a bad name, an absolute `file`, a `file` containing `..`, a `file` not ending `.glb`, more than `MAX_MESHES`, non-finite position or heading, duplicate material names, negative or non-finite `rf_loss_db`, a `[[materials]]` name no loaded mesh uses (`materials[i].name`: guards typos), a missing file (`meshes[0].file`), total triangles over the cap.
  - `a_world_without_meshes_loads_exactly_as_before`: every shipped world except `hall` has empty `meshes`/`materials`; `worlds/hall.toml` loads.

- [ ] **Step 2: Run** `cargo test -p ofs-config --test world` — fails.

- [ ] **Step 3: Implement the sections, validation (existing `Checker` style and field-path strings) and `load_meshes`** (a `file` resolves against the world file's directory; a world not loaded from disk, `source_path` empty, has no meshes to load). Then write `examples/make_hall.rs` (it `#[path]`-includes `tests/common/glb.rs`) and run `cargo run -p ofs-config --example make_hall` to produce `worlds/assets/hall.glb`: surfaces only (no thickness), NED footprint north 30..50, east -6..6, 6 m tall, a flat roof, a 4 x 3 m doorway in the south wall (east -2..2, d 0..-3), a 4 x 1 m window in the east wall (north 38..42, d -3..-4), materials `concrete` and `glass`. `worlds/hall.toml`: pilot 25 m south of the doorway at 1.7 m, `concrete` 18 dB, `glass` 2 dB, a header saying all values are estimates.

- [ ] **Step 4: Run** `cargo test -p ofs-config` — pass.

- [ ] **Step 5: Commit** — `git commit -m "feat(config): [[meshes]] and [[materials]] in the world file, and a demo hall"`

---

### Task 4: Physics collision against meshes

**Files:**
- Modify: `crates/ofs-physics/src/rigid_body.rs`, `crates/ofs-physics/src/collision.rs`
- Create: `crates/ofs-physics/tests/mesh_collision.rs`

**Interfaces:**
- Consumes: `TriMesh::within`.
- Produces: `struct WorldMesh { pub name: String, pub mesh: Arc<TriMesh>, pub bounds: Aabb }` with `WorldMesh::new(name: String, mesh: Arc<TriMesh>) -> WorldMesh`; `AirframeParams.meshes: Vec<WorldMesh>` (every existing literal gets `meshes: Vec::new()`); `pub const MESH_POINT_RADIUS_M: f64 = 0.005`. A mesh's object index is `objects.len() + j`; `TouchState::new` is given `objects.len() + meshes.len()`.

- [ ] **Step 1: Write the failing tests** (build `AirframeParams` the way `tests/collision.rs` does):
  - `a_sphere_rests_on_a_mesh_floor`: a 20 x 20 m horizontal quad at d = -2, quad dropped from 0.5 m above it; after 2 s the lowest sphere is within 2 mm of the surface and `body.collision_object` equals `objects.len()`.
  - `resolve_lets_a_sphere_pass_through_a_gap_and_stops_it_at_the_wall` (hand-made `BodyState`, `collision::resolve`): a wall at north = 10 with a 2 x 2 m hole; stepping a sphere along a line through the hole leaves the state unchanged and raises no event; the same line 3 m to the east is pushed back along -north and raises one event with the mesh's object index.
  - `a_landing_point_holds_on_a_20_degree_mesh_ramp`: mirror the existing terrain incline test (friction 0.5 > tan 20 degrees): after 3 s the quad has moved less than 5 mm.
  - `a_quad_spawned_inside_a_wall_is_pushed_out`: 3 cm penetration, after one step every sphere is clear by its radius within 1e-6 and every value is finite.
  - `a_mesh_far_from_the_quad_costs_no_contact` (broad phase): contacts empty when the quad is outside the grown bounds.

- [ ] **Step 2: Run** `cargo test -p ofs-physics --test mesh_collision` — fails to compile.

- [ ] **Step 3: Implement.** In `collision.rs::collect`, after the object loop: for each mesh whose `bounds.grown(quad_radius)` contains the body, call `within(centre, radius, ..)` per sphere and push one `Contact` per hit (`penetration = radius - hit.distance`, `normal = hit.normal`, object index as above). In `RigidBody::contact`, give each landing point the same query with `MESH_POINT_RADIUS_M`: `depth = MESH_POINT_RADIUS_M - hit.distance`, handed to the existing `act(..)` and `touches` unchanged. Reuse one scratch `Vec<MeshHit>`; no allocation per step in steady state.

- [ ] **Step 4: Run** `cargo test -p ofs-physics` — all pass (the existing suites unchanged).

- [ ] **Step 5: Commit** — `git commit -m "feat(physics): spheres and landing points collide with triangle meshes"`

---

### Task 5: Radio and video through meshes

**Files:**
- Modify: `crates/ofs-rf/src/propagation.rs`, `crates/ofs-video/src/link.rs`, `crates/ofs-radio/src/elrs.rs`, and every call site and test literal the compiler flags
- Test: `crates/ofs-rf/tests/propagation.rs`, `crates/ofs-video/tests/link.rs`

**Interfaces:**
- Consumes: `TriMesh::crossings`.
- Produces: `struct MeshObstacle { pub mesh: Arc<TriMesh>, pub loss_db: Vec<f64> }`; `pub fn mesh_loss_db(m: &MeshObstacle, a: DVec3, b: DVec3) -> f64` (sum of `loss_db[material]`, 0 for a material past the end); `obstruction_db(obstacles, terrain, meshes: &[MeshObstacle], a, b, freq_mhz)` and `path_gain(tx, rx, freq_mhz, obstacles, terrain, meshes, ground_bounce)`; `LinkWorld` and the ELRS link params gain `meshes: Vec<MeshObstacle>`.

- [ ] **Step 1: Write the failing tests:**
  - `a_path_crossing_two_walls_loses_the_sum`: walls of 18 and 40 dB give 58 dB; a path through a gap in them gives 0.
  - `no_meshes_changes_nothing`: `obstruction_db` with `&[]` equals the old value on a box-and-terrain scene.
  - video: `a_mesh_wall_lowers_the_snr_by_its_loss` (`ofs-video/tests/link.rs`, pilot behind an 18 dB wall).
  - elrs: `a_mesh_wall_lowers_the_uplink_rssi` (same shape as the existing obstacle test in `ofs-radio/tests/elrs.rs`).

- [ ] **Step 2: Run** `cargo test -p ofs-rf -p ofs-video -p ofs-radio` — fails to compile.

- [ ] **Step 3: Implement.** Add the mesh sum to `obstruction_db` beside the terrain term; add the field and pass it at the four call sites (`link.rs` x2, `elrs.rs` x2). Frequency-independent, documented as a limit.

- [ ] **Step 4: Run** `cargo test -p ofs-rf -p ofs-video -p ofs-radio` — pass.

- [ ] **Step 5: Commit** — `git commit -m "feat(rf): meshes block the video and ELRS links by material"`

---

### Task 6: Wire meshes through the sim

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs`
- Test: `crates/ofs-sim/src/vehicle.rs` tests, `crates/ofs-sim/src/session.rs` tests, `crates/ofs-sim/tests/` (new `mesh_world.rs`)

**Interfaces:**
- Consumes: `WorldConfig::load_meshes`, Tasks 4 and 5.
- Produces: `vehicle::build` loads the meshes once and gives the same `Arc<TriMesh>` to `AirframeParams.meshes`, `link_params(.., meshes)` and `radio_chain(.., meshes, ..)` (as `terrain` is passed today); `Vehicle::meshes() -> &[WorldMeshData]` for Task 8; `session.rs::events_for` names a collision with object index `>= world.objects.len()` after the mesh at `index - objects.len()` (`HIT hall 7.2 m/s`).

- [ ] **Step 1: Write the failing tests:**
  - `a_mesh_world_shares_one_mesh_with_physics_and_both_links` (Arc strong counts, as the terrain test does).
  - `a_world_without_meshes_has_none_anywhere`.
  - `a_hit_on_a_mesh_names_it` (`events_for` with `collision_object = objects.len()`).
  - `mesh_world.rs`: `a_flight_in_the_hall_world_replays_exactly` (same seed twice gives the same digest, and it differs from the open field's) and `the_hall_wall_blocks_the_video_link_and_the_doorway_does_not` (video `obstruction_db` from a point behind the south wall is >= 18 dB, from the doorway axis is 0).

- [ ] **Step 2: Run** `cargo test -p ofs-sim` — fails.

- [ ] **Step 3: Implement the plumbing.** Parse each file once per `build`.

- [ ] **Step 4: Run** `cargo test -p ofs-sim` — all pass **including** `flat_world_digest_is_pinned`; if it moved, stop and fix, never re-pin.

- [ ] **Step 5: Commit** — `git commit -m "feat(sim): load a world's meshes once and share them with physics and the links"`

---

### Task 7: Repro bundles carry the meshes

**Files:**
- Modify: `crates/ofs-sim/src/repro.rs`
- Test: `crates/ofs-sim/src/repro.rs` tests

**Interfaces:**
- Consumes: `WorldConfig::meshes`.
- Produces: a bundle that holds each referenced `.glb` at `worlds/<file>` (the same relative path the world file uses), so `replay` loads it from the bundle.

- [ ] **Step 1: Write the failing tests:** `a_world_with_meshes_copies_them_into_the_bundle` (bundle contains `worlds/assets/hall.glb`, byte-identical) and `a_bundle_with_a_mesh_replays` (create from `worlds/hall.toml`, replay, same digest).
- [ ] **Step 2: Run** `cargo test -p ofs-sim repro` — fails.
- [ ] **Step 3: Implement** — load the world to learn its mesh files and push their bytes beside the world file; an unreadable mesh is the same error the world read gives.
- [ ] **Step 4: Run** `cargo test -p ofs-sim` — pass.
- [ ] **Step 5: Commit** — `git commit -m "feat(sim): repro bundles carry a world's mesh files"`

---

### Task 8: Protocol 17 and the client model

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`, `python/ofs/client.py` (`PROTOCOL_VERSION = 17`) and the Python `World` surface as terrain does, regenerated `python/ofs/v1/sim_pb2*` (recipe in `docs/dev-setup.md`), `crates/ofs-sim/src/session.rs` (`world_msg`), `crates/ofs-client/src/model.rs`, `crates/ofs-godot/src/lib.rs`
- Test: `crates/ofs-sim/src/session.rs`, `crates/ofs-client/tests/model.rs`, `python/tests/`

**Interfaces:**
- Produces: `message Mesh { string name = 1; string path = 2; Vec3 position_ned_m = 3; double heading_deg = 4; fixed64 digest = 5; }` and `repeated Mesh meshes = 12;` on `World` (`path` is the absolute path on the sim's machine; `digest` is the file's `fnv1a64`); `ofs_client::WorldMesh { pub name: String, pub path: PathBuf, pub position: DVec3 /* Godot frame */, pub heading_deg: f64, pub digest: u64 }` with `World.meshes: Vec<WorldMesh>` and `WorldMesh::verify(&self) -> Result<(), String>` (reads the file, compares the digest); `get_world()` gains `meshes`: Array of {`name`, `path`, `position`: Vector3, `heading_deg`, `digest_ok`: bool, `error`: String}.

- [ ] **Step 1: Write the failing tests:** `world_msg_carries_each_mesh_path_and_digest`; `a_world_without_meshes_sends_none`; client `verify_accepts_the_matching_file_and_names_the_mismatch` (matching, edited byte, missing file); `from_pb_puts_the_mesh_in_godots_frame`; the proto test now pins 17 and the Python client's constant; a Python test that the stub knows `meshes`.
- [ ] **Step 2: Run** `cargo test -p ofs-proto -p ofs-client -p ofs-sim` — fails.
- [ ] **Step 3: Implement** all of the above; bump 16 -> 17 in every place the old number is pinned (`grep -rn "protocol 16\|PROTOCOL_VERSION"`), and the docs line in `docs/dev-setup.md`.
- [ ] **Step 4: Run** `cargo test --workspace` and `python -m pytest python/tests` — pass.
- [ ] **Step 5: Commit** — `git commit -m "feat(proto): worlds carry their meshes (path and digest), protocol 17"`

---

### Task 9: Godot draws the meshes

**Files:**
- Modify: `godot/world/world.gd`, `godot/tests/test_scene.gd`
- Test: `godot/tests/test_scene.gd`

**Interfaces:**
- Consumes: `get_world()["meshes"]` from Task 8; `worlds/assets/hall.glb`.
- Produces: `build(world)` adds one node named after each mesh whose `digest_ok` is true, positioned at `position` and turned `-deg_to_rad(heading_deg)` about Y, with every `col_`-prefixed node removed; a mesh with `digest_ok` false adds nothing and calls `push_error` with its `error`.

- [ ] **Step 1: Write the failing tests** (`test_scene.gd`, following the file's existing style): `mesh_is_built_and_named`; `col_nodes_are_not_drawn`; `heading_90_turns_the_mesh_to_face_east` (a known vertex's global position); `a_mesh_with_a_bad_digest_draws_nothing`; `a_world_without_meshes_builds_exactly_as_before` (`built_names()` unchanged for `park`).
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` — the new tests fail.
- [ ] **Step 3: Implement** with `GLTFDocument.append_from_file` and `generate_scene` (runtime load, no editor import); materials as the file defines them.
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` — pass (the Betaflight e2e runs only with `OFS_SITL_LAUNCH`).
- [ ] **Step 5: Commit** — `git commit -m "feat(godot): draw a world's meshes from the .glb the sim collides with"`

---

### Task 10: Docs, perf record, release notes

**Files:**
- Modify: `docs/dev-setup.md` (world-file section: `[[meshes]]`, `[[materials]]`, file conventions, the limits), `docs/research/collision.md` (surface collision, the contact maths, tunnelling bound, constants with provenance), `docs/roadmap.md`, `CHANGELOG.md` (protocol 17, `meshes`, no change for worlds without them), `README.md` only if it lists worlds
- Create: `crates/ofs-physics/tests/mesh_perf.rs`

- [ ] **Step 1: Write the timing test** `mesh_world_timing_is_recorded` (`#[ignore]`, not a CI gate): 10 s of hover and bounce at 8 kHz in release next to a synthetic 100k-triangle mesh, against the same flight with no mesh; print both.
- [ ] **Step 2: Run** `cargo test --release -p ofs-physics --test mesh_perf -- --ignored --nocapture` and record the numbers in `docs/research/collision.md`.
- [ ] **Step 3: Write the docs.** State the known limits plainly: no edge diffraction or reflection, one crossing is one loss (no thickness), landing points use a 5 mm skin, sphere-nearest-triangle contact, caps are estimates, the client and sim must share a filesystem.
- [ ] **Step 4: Run** `cargo test --workspace && bash scripts/run-godot-tests.sh all` — pass.
- [ ] **Step 5: Commit** — `git commit -m "docs: mesh worlds - world file, collision maths, limits, timing"`

---

## Self-review

- **Spec coverage:** world sections (T3), format and `col_` (T2), collision and slots (T1, T4), RF sum and limits (T1, T5, T10), client loads the file and checks a digest and the protocol bump (T8, T9), repro (T7), determinism and the flat pin (T1, T6), caps (T2, T3), docs (T10). Spec call 10 (own index range) is T4 and T6.
- **Not here by design:** the generator and the Chernobyl map (plans 2 and 3); mesh-aware pilot/handset placement checks; remote clients.
- **Risks to watch:** the `gltf` feature set (T2 fallback stated); `obstruction_db` and `path_gain` signature churn touches many test literals (compiler-driven); `AirframeParams` literals gain `meshes`; the Godot runtime loader behaving headless (T9 tests prove it or the task stops).
