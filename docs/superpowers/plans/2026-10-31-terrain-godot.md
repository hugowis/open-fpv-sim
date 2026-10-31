# Terrain in the Godot world Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Godot client draws a mesh made of the exact triangles the physics samples, delivered by the simulator, and `worlds/park.toml` ships with relief.

**Architecture:** The `World` message (protocol 16) carries the baked `Terrain` grid and its digest (now including the origin). `ofs-client` rebuilds and verifies a `Terrain`, generates mesh arrays from it in Rust, `ofs-godot` hands them to GDScript, `world.gd` wraps them in an `ArrayMesh` and hides the flat ground plane.

**Tech Stack:** Rust workspace (prost/tonic, gdext), Godot 4.7.2 GDScript, Python gRPC stubs.

**Spec:** `docs/superpowers/specs/2026-10-31-terrain-godot-design.md` (rulings 1-12 are binding; the epic #160 rules too).

## Global Constraints

- Protocol 15 -> 16; `PROTOCOL_VERSION` in `ofs-proto`, `ofs-proto/tests/messages.rs`, `python/ofs/client.py` and the proto header comment move together.
- A world without `[terrain]`: `World.terrain` absent, so `get_world()` has no `terrain` key (emitters' `ground_y` is 0), `Ground` plane visible, flat-world flights/digests untouched.
- No change to the physics, to `ofs-rf`/`ofs-radio`, to `crates/ofs-config/src/world.rs`, `godot/ui/dvr.gd`, `crates/ofs-audio` (other sessions own them).
- Files stay LF: after editing check `tr -cd '\r' < file | wc -c` is 0.
- Commit locally after each task with the trailer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`; never push, no PR.
- Rust tests run in WSL from `/mnt/c/dev/open-fpv-sim/.claude/worktrees/terrain-godot-ccc1eb`: `wsl.exe -d Ubuntu -e bash -lc 'cd <that path> && CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test -p <crate> --locked'`. Godot suites run natively on Windows with `GODOT_BIN=C:/tools/godot/Godot_v4.7.2-stable_win64_console.exe` from Git Bash (`bash scripts/run-godot-tests.sh unit`), which builds `target/debug` on Windows; set `OFS_SIM_BIN` to the worktree's `target/debug/ofs-sim.exe` for e2e (a stale `target/release/ofs-sim` is otherwise preferred). The Godot runner prints "0 failed" even after a SCRIPT ERROR: read the output.
- libm differs per OS; the terrain grid itself uses only basic IEEE ops. Pin only grid-derived values on every OS.

## Review Focus

- A non-square grid with a non-zero, non-centred origin: the mesh layout test uses 3 x 5 nodes at origin (-1.5, 2.0) (rows/columns swapped would pass a square grid).
- A corrupted message (digest off by one bit, heights of the wrong length, zero cell): load fails with a protocol error, never a silent flat world.
- The largest allowed grid (1025 x 1025, 4.2 MB) crosses gRPC to the Rust client and to Python (the 4 MiB default would refuse it).
- Reloading a flat world after a hilly one: `Terrain` node gone, `Ground` visible again.
- A grid whose border is not zero: the ring still meets the mesh edge at y = 0 (a cliff, as the physics has).

---

### Task 1: Protocol 16 on the server side, digest with the origin

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`, `crates/ofs-core/src/terrain.rs`, `crates/ofs-core/tests/terrain.rs`, `crates/ofs-config/tests/world.rs` (re-pin), `crates/ofs-sim/src/session.rs` (`world_msg`), `crates/ofs-sim/tests/grpc.rs`, `python/ofs/client.py`, `python/ofs/v1/sim_pb2*.py(i)`, `python/tests/test_client.py` (if it asserts 15), `python/README.md`.

**Interfaces:**
- Produces: `pb::Terrain { origin_north_m, origin_east_m, cell_m: f64; nodes_n, nodes_e: u32; heights: Vec<f32>; digest: u64 }`, `pb::World.terrain: Option<pb::Terrain>`; `ofs_proto::MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024`; `Terrain::digest` now hashes `origin_ne` bits too.

- [ ] **Step 1: Failing tests.** `ofs-core/tests/terrain.rs`: `digest_changes_with_the_origin` (same grid, origins `[0,0]` vs `[0,1]` differ). `ofs-sim/tests/grpc.rs`: `get_world_carries_the_terrain_of_a_terrain_world` (load `worlds/hills.toml` by name `hills`, `get_world`: `terrain` is Some, `heights.len() == nodes_n * nodes_e`, rebuilding with `Terrain::from_grid` gives `digest()` equal to the message digest; a `park`-less flat world, e.g. `flat`, has `terrain == None`). The test's gRPC client must use `.max_decoding_message_size(ofs_proto::MAX_RESPONSE_BYTES)` if the harness builds the client itself. Add `get_world_carries_a_grid_over_four_mebibytes`: a temp world file `[terrain]\nsize_m = 256.0\ncell_m = 0.25\nwavelength_m = 64.0` (1025 x 1025) loads and `heights.len() == 1025*1025`.
- [ ] **Step 2: Run them; expect compile failures / assertion failures.**
- [ ] **Step 3: Implement.** Proto: add `message Terrain` and `Terrain terrain = 11;` in `World` exactly as in the spec; bump the header comment and `PROTOCOL_VERSION` to 16 (and the `messages.rs` assertion, `python/ofs/client.py`). Add `MAX_RESPONSE_BYTES` to `ofs-proto/src/lib.rs`. `Terrain::digest`: write `origin_ne[0]` and `[1]` `to_bits().to_le_bytes()` after the cell size. `world_msg`: when `w.terrain` is Some, `let t = section.build()` and fill `pb::Terrain` from `t.heights()`, `origin_ne()`, `cell_m()`, `nodes()`, `digest()`.
- [ ] **Step 4: Re-pin** `the_default_grid_digest_is_pinned_on_every_os` in `ofs-config/tests/world.rs`: run it, copy the new value it reports, say in the commit message that the pin moved because the digest now includes the origin (value computed, not guessed).
- [ ] **Step 5: Python.** Regenerate stubs with the command in `docs/dev-setup.md` (needs `grpcio-tools`; `uv run --with grpcio-tools` is fine), add `options=[("grpc.max_receive_message_length", MAX_RESPONSE_BYTES)]` to `grpc.insecure_channel` in `Sim.__init__` (module constant `_MAX_RESPONSE_BYTES = 16 * 1024 * 1024`), update `python/README.md` protocol number. Run `python -m pytest python/tests/test_client.py -q` for what runs without a SITL.
- [ ] **Step 6: Run** `cargo test -p ofs-core -p ofs-proto -p ofs-config -p ofs-sim --locked` in WSL (all pass; `flat_world_pin` unchanged) and `cargo clippy -p ofs-core -p ofs-proto -p ofs-sim --all-targets --locked -- -D warnings`.
- [ ] **Step 7: Commit** `feat(proto)!: protocol 16 - the terrain grid in the World message (#155)`.

---

### Task 2: The client model, verification and the mesh

**Files:**
- Create: `crates/ofs-client/src/terrain_mesh.rs`, `crates/ofs-client/tests/terrain_mesh.rs`
- Modify: `crates/ofs-client/src/model.rs` (`World`, `WorldEmitter`, `from_pb`), `crates/ofs-client/src/worker.rs` (store world / limit), `crates/ofs-client/src/lib.rs` (export), `crates/ofs-client/tests/model.rs` (callers of `from_pb`), `crates/ofs-client/Cargo.toml` only if a dependency is missing (ofs-core is already there).

**Interfaces:**
- Consumes: Task 1's `pb::Terrain`, `MAX_RESPONSE_BYTES`.
- Produces: `World.terrain: Option<Arc<Terrain>>`; `WorldEmitter.ground_y: f64`; `World::from_pb(w: pb::World) -> Result<World, String>`; `pub struct TerrainMesh { pub positions: Vec<[f32; 3]>, pub normals: Vec<[f32; 3]>, pub indices: Vec<u32> }`; `pub fn terrain_mesh(t: &Terrain) -> TerrainMesh` (layout: spec ruling 4; normals: central differences of the grid in Godot axes `normalize(-dh/dx, 1, -dh/dz)`, one-sided at the edge, `dz` taken with the sign of Godot's z = -north); `Terrain` re-exported as `ofs_client::Terrain`.

- [ ] **Step 1: Failing tests** (`tests/terrain_mesh.rs`, plus `tests/model.rs`):
  - `mesh_height_equals_the_terrain_everywhere`: a 3 x 5 grid at origin (-1.5, 2.0), cell 0.75, hand-picked uneven heights (not planar, so the diagonal matters), built with `Terrain::from_grid`; a helper `mesh_height(&TerrainMesh, &Terrain, x, z)` finds the cell from the node counts, reads the two triangles of that cell **from `indices`** (cell `c = i*(nodes_e-1)+j` owns indices `6c..6c+6`), picks the one containing the point by barycentric coordinates and interpolates the vertex y. Compare with `t.height_up(north, east)` (`north = -z`, `east = x`) within 1e-4 at: every node, every cell centre, 5 points on each diagonal, 5 points each side of it, the grid edges. Points outside the grid are not in the mesh: instead assert `t.height_up` there is 0 (the ring's height).
  - `every_triangle_faces_up_and_normals_are_unit`: `cross(p1-p0, p2-p0).y < 0` for every triangle; every normal length 1 within 1e-6 and `y > 0`; `positions.len() == nodes_n*nodes_e`, `indices.len() == 6*(nodes_n-1)*(nodes_e-1)`; on a flat grid every normal is `[0,1,0]`.
  - `vertex_positions_are_the_nodes`: vertex `i*nodes_e+j` is `(origin_e + j*cell, height, -(origin_n + i*cell))` (heights bit-exact).
  - model: `a_world_with_a_terrain_keeps_it` (round trip a `pb::Terrain` built from a `Terrain`, `world.terrain.unwrap().digest()` equal), `a_corrupt_terrain_is_an_error` (digest xor 1; heights short by one; `cell_m = 0`: each `Err` naming the cause), `no_terrain_is_none_and_ground_y_zero`, `an_emitter_stands_on_the_relief` (emitter at a NED position over a sloped grid gets `ground_y == height_up(n, e)`).
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement** `terrain_mesh` (one pass for vertices+normals, one for indices: triangles `(00, 10, 11)` and `(00, 01, 11)` per cell in that order, indices `u32`), and `World::from_pb` → `Result`: when `w.terrain` is Some rebuild `Terrain::from_grid([origin_north, origin_east], cell, nodes_n as usize, nodes_e as usize, heights)` (map its `Err`), compare `digest()` with the message's, `Err("the terrain grid does not match the server's digest (...)")` on mismatch; fill `ground_y` of each emitter from the NED position before the Godot-frame conversion. Update every existing `from_pb` call (tests: `.unwrap()`).
- [ ] **Step 4: Worker.** In `fly`, `World::from_pb(...)` error → `Fly::Failed(ClientError::new(ErrorKind::Protocol, message))`. In `connect_once`, `SimClient::new(channel).max_decoding_message_size(ofs_proto::MAX_RESPONSE_BYTES)`.
- [ ] **Step 5: Large grid test** `a_client_loads_the_largest_terrain` in an existing `ofs-client` integration test file that starts a real server (find the harness with `grep -rn "ofs_sim" crates/ofs-client/tests`; if none loads a world, use the `ofs-sim` gRPC harness pattern): a 1025 x 1025 world loads through `ofs_client::Client` and `world().terrain` has 1025 x 1025 nodes. If the existing harnesses cannot do this cheaply, put the test in `crates/ofs-sim/tests/grpc.rs` using a tonic client built exactly as `connect_once` does.
- [ ] **Step 6: Run** `cargo test -p ofs-client --locked` and clippy `-D warnings` in WSL; all pass.
- [ ] **Step 7: Commit** `feat(client): verify the terrain grid and generate its mesh (#155)`.

---

### Task 3: The extension and `world.gd`

**Files:**
- Modify: `crates/ofs-godot/src/lib.rs` (`get_world`, new `terrain_height`), `godot/world/world.gd`, `godot/tests/test_scene.gd`.

**Interfaces:**
- Consumes: Task 2's `World.terrain`, `terrain_mesh`, `WorldEmitter.ground_y`.
- Produces: `get_world()["terrain"]` = `{origin_x, origin_z, cell_m: float; nodes_n, nodes_e: int; vertices: PackedVector3Array; normals: PackedVector3Array; indices: PackedInt32Array; digest: String (16 hex digits)}` (key absent when flat); each emitter dict gains `ground_y: float`; `OfsClient.terrain_height(x: float, z: float) -> float` (Godot frame; `Terrain::height_up(-z, x)`; 0 without a terrain or world); in `world.gd`: `static func ring_arrays(x0: float, x1: float, z0: float, z1: float, extent: float = 3000.0) -> Array` (the 8-vertex ring around the rectangle [x0,x1] x [z0,z1], y = 0, clockwise from above), and `build()` creating node `Terrain` (MeshInstance3D, two surfaces, `material_override` = the grid shader material, hides `Ground`).

- [ ] **Step 1: Failing GDScript tests** in `test_scene.gd` (they run without a simulator): `test_a_terrain_world_draws_a_mesh_and_hides_the_flat_ground`: a dict with `terrain` of one cell (2 x 2 nodes, hand-made `vertices` `(0,0,0),(1,0.5,0)...`, `indices` `[0,2,3,0,3,1]` ordering as the Rust layout doc says — copy the layout from Task 2's doc comment), assert: node `Terrain` exists and is in `built_names()`, `(mesh as ArrayMesh).get_surface_count() == 2`, `Ground.visible == false`, surface 0's vertex array equals the dict's, the ring surface has 8 vertices and every ring triangle has `Plane(a, b, c).normal.y > 0` (Godot's clockwise-front-face rule), the ring's inner rectangle is the grid's rectangle (`x0 = origin_x`, `z1 = origin_z`, from `origin_x`, `origin_z`, `cell_m`, `nodes_*`) and its outer extent 3000. `test_a_flat_world_after_a_hilly_one_shows_the_ground_again`: build terrain world, then `build({})`: no `Terrain` node, `Ground.visible`. `test_an_emitter_post_stands_on_the_ground_below_it`: emitter dict `position (x, 4.0, z)`, `ground_y 1.5` gives marker y 1.5 and post height 2.5. Existing emitter test (no `ground_y` key) must still pass: default 0.
- [ ] **Step 2: Run the unit suite** (`bash scripts/run-godot-tests.sh unit`); expect the new tests to fail.
- [ ] **Step 3: Implement `world.gd`.** Constants for the ring extent; `_add_terrain(t: Dictionary)` builds the `ArrayMesh` with `Mesh.ARRAY_VERTEX/NORMAL/INDEX`, adds the ring as surface 1, material as `_add_ground` does (extract the shared `ShaderMaterial` creation into one helper rather than duplicating), name `Terrain`, `_add_built`; `Ground` plane referenced by a member so `build` toggles `visible = world.has("terrain") == false`. `_add_emitter` uses `e.get("ground_y", 0.0)`. Update the file's header comment (no longer "ground plane" only).
- [ ] **Step 4: Implement the extension.** In `get_world`: packed arrays from `terrain_mesh(&terrain)` (convert to `Vector3` slices; `PackedInt32Array` from `u32 as i32`), digest as `format!("{:016x}")`; emitter `ground_y`; document the keys in the doc comment on `get_world`; new `#[func] fn terrain_height(&self, x: f64, z: f64) -> f64`.
- [ ] **Step 5: Run** `cargo build -p ofs-godot --locked` and `cargo clippy -p ofs-godot --all-targets --locked -- -D warnings`, then the unit suite again: all pass (read the output for SCRIPT ERROR).
- [ ] **Step 6: Commit** `feat(godot): draw the terrain mesh the sim samples (#155)`.

---

### Task 4: The proof against a real server

**Files:**
- Create: `godot/tests/e2e_terrain.gd` (and let Godot write its `.uid`; commit it as the other tests do).
- Modify: `scripts/run-godot-tests.sh` (a `terrain` suite, also in `e2e` and `all`; usage strings), `godot/tests/shots.gd` (a `terrain.png`), `docs/dev-setup.md` mentions later in Task 6.

**Interfaces:**
- Consumes: Task 3's dict, `terrain_height`, `world.gd`.

- [ ] **Step 1: Write `e2e_terrain.gd`** modelled on `e2e_open_loop.gd` (starts the game scene or a bare `OfsClient` with `server_bin`, world path `worlds/hills.toml`, open loop): wait for `get_world()` to carry `terrain`; build `world.gd` from it into a `Node3D` tree; read the `ArrayMesh` surface 0 arrays; a local `mesh_height(arrays, t, x, z)` finds the cell from `nodes_*`, `origin_*`, `cell_m` and interpolates the cell's two triangles from `indices` by barycentric coordinates. Assert for a deterministic set of >= 300 points (cell centres and corners across the grid, 100 points on or within 1e-3 of a diagonal, nodes on the edge) that `abs(mesh_height - client.terrain_height(x, z)) < 1e-4`; points outside the grid: `terrain_height == 0`; assert the digest string is 16 hex digits and the world's pilot is not below `terrain_height`. Then reload with `worlds/park.toml` (after Task 5 park has a terrain; until then assert it is flat or skip with a message): every gate's centre `y - terrain_height(x, z)` is within 0.5 m .. 2.5 m. Time `world.build()` and print it (a human reads the load cost).
- [ ] **Step 2: Wire** the runner: `terrain)` runs `e2e_terrain.gd` with a 180 s timeout, included in `e2e` and `all`.
- [ ] **Step 3: shots.gd:** add `terrain.png` (hills world, a point above the flat disc looking north) following the file's pattern for other shots.
- [ ] **Step 4: Run** `OFS_SIM_BIN=<worktree target/debug/ofs-sim.exe> bash scripts/run-godot-tests.sh terrain` (`GODOT_BIN` set as in the constraints) and the open-loop e2e, which must stay green and untouched.
- [ ] **Step 5: Commit** `test(godot): the mesh against the sim's terrain, end to end (#155)`.

---

### Task 5: Park on the hills

**Files:**
- Modify: `worlds/park.toml`, `worlds/hills.toml` (header), `crates/ofs-config/tests/world.rs` (the park test), `godot/tests/*` only if one hard-codes park's gate heights (grep first: `test_catalog.gd`, `test_scene.gd`).

**Interfaces:**
- Consumes: part 1's `TerrainSection`, `Terrain::height_up`.

- [ ] **Step 1: Failing test** `the_park_course_stands_on_its_relief` in `ofs-config/tests/world.rs`, loading `worlds/park.toml` and baking its terrain: pilot and handset above ground and home flat (`height_up(0,0) == 0`); every gate: its centre is 0.9 to 1.6 m above the ground at the centre, and the lowest point of its opening (centre minus `height_m / 2` rotated by `roll_deg`, `cos` for the vertical extent) is not below the lowest ground over the opening's width (sample 9 points); every object's base (box: `centre.z + size.z / 2`... in NED, down positive; cylinder: `centre + height/2`) is at or below the lowest ground under its footprint (sample the rectangle/circle) and its top is above the highest ground by at least the design height recorded in the test as the pre-terrain design (walls 3.0, pillars 4.0, hedge 0.8, posts 2.2 above the lowest ground... use the exact numbers you computed); the emitter is 0.5 to 2 m above ground; `max_slope()` below `tan(30 deg)`. Fails: no `[terrain]` yet.
- [ ] **Step 2: Choose and author the relief.** `[terrain]` in `park.toml`: `seed` your pick, `size_m = 160.0`, `cell_m = 1.0`, `amplitude_m = 2.5`, `wavelength_m = 60.0`, `flat_radius_m = 8.0` (adjust the seed until slopes on the course are gentle, say under 10 degrees, and Gate0/Gate1/Gate3 do not sit in a pit). Compute grounds with a throwaway test (delete it after) that prints each object's footprint min/mean/max ground and writes the new `[[objects]]`, `[[gates]]` and emitter numbers: gates' centres at `-(ground_at_centre + design_height_above_ground)` (Gate0/1/3: 1.1 above, Gate2: 1.5 above), posts and bar as before relative to the frame (bar at centre minus 1.15, posts hang from the bar down to the lowest ground under them minus 0.2, i.e. longer than 2.2 where the ground drops), walls, pillars, hedge: top at mean ground + design height, base buried to the lowest ground under the footprint minus 0.2; emitter 1 m above its ground. Keep the file's comments true (update the header: relief, flat start, the Godot picture now matches); keep the courses and race unchanged.
- [ ] **Step 3: `hills.toml` header:** remove "the Godot client draws a flat ground until #155" and say the client draws the same mesh.
- [ ] **Step 4: Run** `cargo test -p ofs-config -p ofs-sim --locked` in WSL (the park gRPC test at `grpc.rs:765` and any gate/race tests that load park must pass; if a sim test flies park, run it) and the Godot unit suite.
- [ ] **Step 5: Fly it.** A one-off `ofs-sim` lockstep run of park in open loop from a test that already exists, or the Task 4 e2e on park, proves the start rests on the ground: assert in the Task 4 e2e (park half) that after 2 simulated seconds of no throttle the quad's `pos_ned_m.z` is within 0.2 m of 0 (it sits on the flat pad). Add that assertion there.
- [ ] **Step 6: Commit** `feat(worlds): park gets gentle relief, its course placed on the ground (#155)`.

---

### Task 6: Docs and changelog

**Files:**
- Modify: `CHANGELOG.md`, `docs/dev-setup.md` (the `[terrain]` bullet: the client now draws it; the Godot suites list gains `terrain`; the 16 MiB message limit; worlds list if park is described), `docs/architecture.md` (world message, if it names it), `docs/roadmap.md` (non-flat terrain line), `README.md` (worlds/limits lines if they say the client draws flat), `python/README.md` (checked in Task 1), `docs/research/collision.md` (one line: the picture is the physics' mesh).

- [ ] **Step 1: Edit** each, grep for "until #155", "flat ground", "protocol 15" to find stale lines: `grep -rn "until #155\|protocol 15\|Protocol 15" --include=*.md --include=*.py --include=*.toml --include=*.gd . | grep -v superpowers/`. CHANGELOG entry under the unreleased heading, matching its style: protocol 16 (World.terrain, MAX 16 MiB), `Terrain::digest` includes the origin (pin moved), park relief, `terrain_height`, Python note.
- [ ] **Step 2: Line endings** `tr -cd '\r' < file | wc -c` for every touched file is 0 (`git diff --stat` shows only intended lines).
- [ ] **Step 3: Full verification** (the verification-before-completion rules): `cargo test --workspace --locked` in WSL, clippy `-D warnings`, `bash scripts/run-godot-tests.sh all` on Windows, python tests that run offline.
- [ ] **Step 4: Commit** `docs: terrain in the Godot world (#155)`.
