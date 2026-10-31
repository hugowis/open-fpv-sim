# A heightfield ground for the physics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A world with an opt-in `[terrain]` section flies over relief in the physics; a world without it is bit-identical to today.

**Architecture:** `ofs-core::terrain::Terrain` is the one source of truth (an f32 height grid, triangle-mesh sampling). `ofs-config` bakes seeded hills from `[terrain]` into it at load. `ofs-physics` gets `Option<Arc<Terrain>>` in `AirframeParams` and both ground sites (collision spheres, landing points) ask `collision::ground_at`. A `body.terrain_d_m` signal exists only with terrain.

**Tech Stack:** Rust workspace (glam `DVec3`, serde/toml). No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-31-terrain-heightfield-design.md` (read it first; its rulings are binding).

## Global Constraints

- Run Rust tests in WSL: from `/mnt/c/dev/open-fpv-sim/.claude/worktrees/terrain-heightfield-935a87`, `CARGO_TARGET_DIR=$HOME/ofs/target-terrain ~/.cargo/bin/cargo test -p <crate> --locked` (own target dir, so other sessions do not collide). `pkill -x betaflight_SITL`, never `pkill -f`.
- Files are LF. After writing any file: `tr -cd '\r' < file | wc -c` must print 0.
- Do not touch `godot/`, any `.proto`, `ofs-rf`, `ofs-video`, `ofs-radio`. No protocol bump.
- A world without `[terrain]`: same signals, same digest, same bits as `main` (ec8f0b2).
- Generator uses only `+ - * /`, `sqrt` and integer hashing: no `sin`, `exp`, `pow`, `powi`, `mul_add`.
- Commit locally per task, message ends with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not push.

## Review Focus

- Bare `[terrain]` (all defaults) loads, passes validation and is under the 60 degree cap: Task 2 test.
- Quad starting at home with terrain: the flat disc makes `ground_d(0,0) == 0.0`: Task 2 test.
- A query exactly on the last node / a cell edge / beyond the grid does not panic or index out of range: Task 1 tests.
- Terrain with an all-zero grid flies like no terrain (same body state; only the extra signal differs): Task 3 test.
- Hill world flown twice gives the same digest; different seed gives a different one: Task 4 test.
- Sphere deep inside a steep (<= 60 degree) slope is pushed out along the slope normal, not straight up: Task 3 test.

---

### Task 1: Baseline pin of the flat world (do this BEFORE any other change)

**Files:**
- Create: `crates/ofs-sim/tests/flat_world_pin.rs`

**Interfaces:**
- Consumes: `ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle}` (see `crates/ofs-sim/tests/quads.rs` for the `vehicle()` helper pattern), `Vehicle::digest() -> u64`, `Vehicle::run_for(secs)`, `ofs_config::{load, FcKind, WorldConfig}`.
- Produces: a test `flat_world_digest_is_pinned` whose constant was recorded from unmodified `main`.

- [ ] **Step 1:** Write the test: build the shipped race quad (look in `quads/` for the 5 inch file) with `FcKind::OpenLoop`, seed 1, world `WorldConfig::open_field()`, throttle 0.7 for 1 s (it climbs), then 0 for 2 s (it falls, lands, bounces, rests on the ground: exercises the spheres, landing points and friction), `digest()` after each phase. Assert both equal `const PIN_*: u64 = 0;` first.
- [ ] **Step 2:** Run it; it fails showing the actual digests. Replace the constants with those values. Also assert that no bus signal named `body.terrain_d_m` exists is not possible yet; skip.
- [ ] **Step 3:** Run again: PASS. Commit `test(sim): pin the flat world's bus digest before the terrain work (#153)`.

### Task 2: `ofs_core::terrain::Terrain`

**Files:**
- Create: `crates/ofs-core/src/terrain.rs`, `crates/ofs-core/tests/terrain.rs`
- Modify: `crates/ofs-core/src/lib.rs` (`pub mod terrain;`), `crates/ofs-core/src/names.rs` (add `BODY_TERRAIN_D = "body.terrain_d_m"` with the doc "Depth (NED d) of the ground under the body's centre; registered only in worlds with a `[terrain]`")

**Interfaces:**
- Produces (exact, see spec for semantics):
  ```rust
  #[derive(Debug, Clone, PartialEq)] pub struct Terrain { /* private */ }
  impl Terrain {
      pub fn from_grid(origin_ne: [f64; 2], cell_m: f64, nodes_n: usize, nodes_e: usize, heights: Vec<f32>) -> Result<Terrain, String>;
      pub fn ground_d(&self, north: f64, east: f64) -> f64;
      pub fn surface(&self, north: f64, east: f64) -> (f64, DVec3);
      pub fn max_slope(&self) -> f64;
      pub fn digest(&self) -> u64;
      pub fn heights(&self) -> &[f32];
      pub fn origin_ne(&self) -> [f64; 2];
      pub fn cell_m(&self) -> f64;
      pub fn nodes(&self) -> (usize, usize);
      /// Height up at (north, east): `-ground_d`.
      pub fn height_up(&self, north: f64, east: f64) -> f64;
  }
  ```
  `from_grid` errors: fewer than 2 nodes a side, `cell_m` not finite and > 0, `heights.len() != nodes_n * nodes_e`, a non-finite height, a non-finite origin. Index: `heights[i * nodes_e + j]`, i north, j east.

- [ ] **Step 1:** Tests in `tests/terrain.rs` (all with a small grid built by `from_grid`, `origin_ne = [-2.0, -2.0]`, `cell_m = 1.0`):
  - node-exact: `height_up` at every node equals the grid value (as f64) bit for bit.
  - a 3x3 ramp `h = 0.5 * north` (f32 exact): `ground_d(1.3, 0.2) == -0.65` within 1e-12, and `surface` normal equals `normalize((-0.5, 0.0, -1.0))` within 1e-12.
  - both triangles of one cell: heights `[0, 1; 0, 0]`-style corners `h00=0,h10=1,h01=2,h11=4` give, at (u=0.75,v=0.25) (lower-right triangle, `u >= v`) `h = 0 + 0.75*1 + 0.25*(4-1) = 1.5`, and at (u=0.25,v=0.75) `h = 0 + 0.25*(4-2) + 0.75*2 = 2.0`; check the two normals differ.
  - outside: `ground_d(100.0, 0.0) == 0.0`, normal `== DVec3::NEG_Z` exactly; a query exactly on the last node and on the last cell's far edge equals the node height; `f64::NAN` query does not panic (returns the plane, `(0.0, NEG_Z)`).
  - `from_grid` rejects a wrong length, a NaN height, `cell_m = 0`, a 1-node side.
  - `digest()` differs when one f32 bit differs, equal for equal grids; `max_slope()` of the 0.5 ramp is `0.5`.
- [ ] **Step 2:** Run `cargo test -p ofs-core --test terrain`: FAIL (module missing).
- [ ] **Step 3:** Implement `terrain.rs`. Approach: locate the cell with `((n - origin_n) / cell).floor()`, clamp the cell index to `nodes - 2` so the far edge uses the last cell, fractions `u, v` from the clamped index; if the query is outside `[origin, origin + (nodes-1) * cell]` on either axis, or not finite, return the plane. Triangle choice `u >= v` uses (h00, h10, h11) with `h = h00 + u*(h10-h00) + v*(h11-h10)`, else (h00, h01, h11) with `h = h00 + u*(h11-h01) + v*(h01-h00)`. Gradients `gn`, `ge` are the u and v coefficients divided by `cell_m`; the outward normal is `DVec3::new(-gn, -ge, -1.0).normalize()` and `ground_d = -h`. `digest` is `Fnv1a` (in `ofs-core`) over the dims, `cell_m` bits and each height's bits. `max_slope` is the largest `hypot(gn, ge)` over all cells' two triangles, computed in `from_grid` and stored.
- [ ] **Step 4:** Tests PASS; `cargo clippy -p ofs-core --all-targets` clean. Commit `feat(core): a terrain height grid sampled as a triangle mesh (#153)`.

### Task 3: `[terrain]` in the world file

**Files:**
- Create: `crates/ofs-config/src/terrain.rs` (the section, `build`, the noise)
- Modify: `crates/ofs-config/src/world.rs` (`pub terrain: Option<TerrainSection>` with `#[serde(default)]` in `WorldConfig`, `terrain: None` in `open_field`, validation), `crates/ofs-config/src/lib.rs` (`mod terrain; pub use terrain::TerrainSection;`)
- Test: `crates/ofs-config/tests/world.rs` (extend; look at how it writes and loads a world TOML)

**Interfaces:**
- Consumes: `ofs_core::terrain::Terrain::from_grid`, `Terrain::max_slope`, `Terrain::digest`, `ofs_core::{fnv1a64}` (check its signature in `ofs-core/src/lib.rs` or `hash.rs`).
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Deserialize)] #[serde(default, deny_unknown_fields)]
  pub struct TerrainSection { pub seed: u64, pub size_m: f64, pub cell_m: f64, pub amplitude_m: f64, pub wavelength_m: f64, pub octaves: u32, pub persistence: f64, pub flat_radius_m: f64 }
  impl Default for TerrainSection  // seed 0, size 400, cell 1, amplitude 8, wavelength 100, octaves 3, persistence 0.5, flat_radius 25
  impl TerrainSection { pub fn build(&self) -> Terrain }   // call only on validated values
  pub const MAX_TERRAIN_SLOPE_DEG: f64 = 60.0;
  ```

- [ ] **Step 1:** Tests (in `crates/ofs-config/tests/world.rs`, style of the neighbours):
  - a world TOML with a bare `[terrain]` loads; `terrain == Some(TerrainSection::default())`; a world without it has `terrain == None`.
  - `build()` of the default: `ground_d(0.0, 0.0) == 0.0` exactly and `ground_d` within `flat_radius_m` of home is `0.0`; the grid's border heights are all `0.0`; `heights().len() == 401 * 401`; relief exists (`max |h| > 1.0`); `max_slope()` < tan(60 deg).
  - same section twice gives equal `digest()`; `seed = 1` vs `seed = 2` differ.
  - **pin:** `TerrainSection::default().build().digest() == <value recorded on first run>` (comment: "cross-OS claim: IEEE basic ops only; if this fails on one OS the generator uses a libm function").
  - one test per validation rule in the spec (non-finite value, `size_m` 0 and 2001, `cell_m` 0.1, size not a multiple of cell, > 1025 nodes a side, `amplitude_m` -1 and 101, `wavelength_m` < 4 cells, `octaves` 0 and 7, `persistence` 0 and 1.5, negative `flat_radius_m`, too steep (`amplitude_m = 100`, `wavelength_m = 8`, check the problem names `terrain.amplitude_m`), pilot below terrain: a `[terrain]` with `flat_radius_m = 0`, `amplitude_m = 50`, `wavelength_m = 100`, seed chosen so `height_up` at the pilot's spot > 1.7, and a pilot at `d = -1.7` → problem on `pilot.position_ned_m`; handset likewise). Find a seed by trying a few in a scratch test, then fix it in the test.
- [ ] **Step 2:** Run `cargo test -p ofs-config --test world`: FAIL.
- [ ] **Step 3:** Implement. Generator: node `(i, j)` at `n = origin_n + i*cell`, `e = origin_e + j*cell` with `origin = -(cells as f64) * cell / 2` per axis (`cells = (size_m / cell_m).round()`, nodes = cells + 1). `value(seed, octave, ix: i64, iy: i64) -> f64` = `fnv1a64` over the little-endian bytes of `(seed, octave, ix, iy)`, top 53 bits `as f64 * 2^-52 - 1.0`. `noise(seed, octave, x, y)` = lattice value noise with quintic fade `t*t*t*(t*(t*6.0-15.0)+10.0)` and linear mix. fbm: octave `o` samples at `(n / (wavelength / 2^o), e / ...)` (divide by `wavelength_m / (1u32 << o) as f64`), weight `persistence^o` accumulated by repeated multiplication, result divided by the weight sum. Windows: `smooth(x) = { let x = x.clamp(0.0, 1.0); x*x*(3.0-2.0*x) }`; home window `smooth((hypot_by_sqrt(n,e) - flat_radius_m) / wavelength_m)`; edge window `smooth((half - max(|n|,|e|)) / wavelength_m)` with `half = cells*cell/2`. Height `= (amplitude_m * fbm * home * edge) as f32`. Validation in `WorldConfig::validate` as in the spec, using `Checker` like the neighbours (field paths `terrain.size_m` ...); build the terrain for the slope and pilot/handset checks only when every earlier terrain check passed. Pilot/handset check: `d_pilot > terrain.ground_d(n, e)` is a problem.
- [ ] **Step 4:** Tests PASS; record the digest pin; whole `ofs-config` suite green; clippy clean. Commit `feat(config): an opt-in [terrain] section baked into a height grid (#153)`.

### Task 4: The physics samples the terrain

**Files:**
- Modify: `crates/ofs-physics/src/rigid_body.rs` (`AirframeParams.terrain: Option<std::sync::Arc<ofs_core::terrain::Terrain>>`, `contact`, signal), `crates/ofs-physics/src/collision.rs` (`ground_at`, `collect`), the 3 `AirframeParams {` literals in `crates/ofs-physics/tests/{collision,rigid_body}.rs` and any in `crates/ofs-physics/src` (`terrain: None`)
- Test: `crates/ofs-physics/tests/terrain.rs` (new; copy the rig helpers from `tests/collision.rs`)

**Interfaces:**
- Consumes: Task 2's `Terrain`, `names::BODY_TERRAIN_D`.
- Produces: `pub fn ground_at(terrain: Option<&Terrain>, north: f64, east: f64) -> (f64, DVec3)` in `collision.rs` (`None` returns `(0.0, DVec3::NEG_Z)`; `Some` returns `t.surface(..)`). `RigidBody::new` registers `BODY_TERRAIN_D` (`bus.signal`) only when `p.terrain.is_some()` and `publish` sets it to `ground_at(.., s.pos.x, s.pos.y).0`.

- [ ] **Step 1:** Tests in `tests/terrain.rs`. A helper `ramp(slope_deg)` building a `Terrain::from_grid` plane (origin `[-50,-50]`, cell 1, 101 nodes, `h = tan(slope) * north` up toward north, as f32; use slope 20 deg, border irrelevant), and `rigid_body` rigs with the 25-sphere-free configuration used in `tests/collision.rs` (one body sphere radius 0.04 at the centre, `R`), plus a 4-landing-point rig (points at `(±0.1, ±0.1, 0.03)` FRD, ground params 3000/40/0.6, copy from `tests/rigid_body.rs`'s incline test if one exists there):
  - `flat_grid_matches_no_terrain`: an all-zero grid and `None`, same drop with bounce: identical `body.pos_ned_m`/`vel` bits every tick for 1 s; the zero-grid bus has `body.terrain_d_m == 0.0`, the `None` bus has no such signal (`bus().lookup::<f64>(..)` is `None`).
  - `sphere_rests_on_a_slope_normal`: a sphere dropped at rest on the 20 degree ramp ends with `pos.z` such that its distance to the plane equals `R` (within 1e-6) after 0.5 s.
  - `landing_points_hold_inside_the_friction_angle`: quad (4 points) resting on a 20 degree ramp with `friction_coeff = 0.6` (tan 20 = 0.364 < 0.6) moves less than 5 mm horizontally in 5 s; with `friction_coeff = 0.3` it slides, and its along-slope acceleration is `g (sin - mu cos)` within 5 % (measure velocity between 0.2 s and 0.6 s after the initial settle).
  - `crest_bounce_leaves_along_the_normal`: a sphere dropped onto a 20 degree ramp with `restitution = 0.5` leaves with a horizontal velocity component away from the downhill side consistent with reflection about the normal (sign check: velocity gains a component along the downhill horizontal direction; sign asserted).
  - `ground_at_none_is_the_plane`: `ground_at(None, 3.0, 4.0) == (0.0, DVec3::NEG_Z)` exactly.
- [ ] **Step 2:** Run `cargo test -p ofs-physics`: FAIL (compile).
- [ ] **Step 3:** Implement. `collect`: per sphere `let (gd, n) = ground_at(p.terrain.as_deref(), centre.x, centre.y); let penetration = radius + (-n.z) * (centre.z - gd);` normal `n`, `point = centre - n * radius`, `inward` unchanged formula with `n`. `contact`: per landing point `let (gd, n) = ground_at(...point.x, point.y)`, `depth = (-n.z) * (point.z - gd)`; touches `(-v.dot(n)).max(0.0)`; `act(lever, k, GROUND_OBJECT_INDEX, n, depth, v)`; compute `point = s.pos_ned_m + lever` before the ground test and keep the object loop's use of it. The no-terrain arithmetic must reduce to the old expressions exactly (`x - 0.0`, `x * 1.0`).
- [ ] **Step 4:** Whole `ofs-physics` suite PASS (the existing tests are the flat regression); clippy clean. Commit `feat(physics): the ground contacts sample the terrain (#153)`.

### Task 5: Wiring, the shipped hills world, determinism and performance

**Files:**
- Create: `worlds/hills.toml`, `crates/ofs-sim/tests/terrain_flight.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs` (the `AirframeParams` build near line 959: `terrain: opts.world.terrain.as_ref().map(|t| std::sync::Arc::new(t.build()))`)

**Interfaces:**
- Consumes: Tasks 3 and 4; the pinned test of Task 1 must still pass unmodified.

- [ ] **Step 1:** `worlds/hills.toml`: model on `worlds/flat.toml`'s header and `[pilot]`/`[receiver]` (pilot beside the pad at `[-3.0, 2.0, -1.7]`, one omni + one patch antenna), no objects, no gates, `[terrain] seed = 7`, `size_m = 600.0`, `amplitude_m = 12.0`, `wavelength_m = 120.0`, `flat_radius_m = 30.0`. Header comment (copy the style of `meadow.toml`): rolling hills; the physics sees them; the Godot client draws a flat ground until #155; all values are estimates.
- [ ] **Step 2:** Tests in `terrain_flight.rs`:
  - `hills_world_loads_and_is_valid` (`ofs_config::load` of `worlds/hills.toml`).
  - `seeded_flight_over_hills_replays_exactly`: two vehicles from the hills world, race quad, open loop, seed 1, the same fixed stick script (throttle 0.7, pitch -0.3 for 4 s so it flies off the pad over the rising ground, then throttle 0 for 3 s so it falls onto a slope), run both; equal `digest()` at the end of each phase. A third vehicle with a different `terrain.seed` (modify the loaded `WorldConfig`) differs, and so does `WorldConfig::open_field()`.
  - `terrain_signal_only_with_terrain`: the `body.terrain_d_m` signal is in `v`'s bus (find how `vehicle.rs` exposes the bus, e.g. via the scheduler; adapt) for the hills world and absent for `open_field()`.
  - `quad_lands_on_a_hillside`: after the fall, the body's `pos.z` is within 0.1 m of `ground_d(pos.x, pos.y)` (landed on the surface, not the plane), measured from the world's built `Terrain` (`world.terrain.unwrap().build()`).
  - `#[ignore] terrain_costs_little_in_release`: time 10 s of simulated hover-and-bounce at 8 kHz for each shipped quad file in `quads/` on flat and on hills, `eprintln!` ms per simulated second for each and the hills/flat ratio; assert the ratio < 1.5 and both faster than real time. Run it with `--release -- --ignored --nocapture` and record the numbers for Task 6.
- [ ] **Step 3:** Run in WSL: `cargo test -p ofs-sim --test terrain_flight --test flat_world_pin` and the full `cargo test --workspace --locked`. Expected: all pass (Godot-dependent tests excluded as usual). The e2e/Godot tests are not required (no `godot/` change); if any test enumerates `worlds/`, fix its expectation.
- [ ] **Step 4:** Commit `feat(sim): fly over the terrain; worlds/hills.toml (#153)`.

### Task 6: Docs and changelog

**Files:**
- Modify: `CHANGELOG.md` (Unreleased, new section at the top in the style of the neighbours: `### A heightfield ground for the physics (#153, part 1 of 3 of #27)` with Added/Changed), `docs/dev-setup.md` (world-file list: add `[terrain]` with its keys, fix the "Nothing collides with the drone (the simulator's ground is the plane d = 0)" line, which was already stale), `docs/research/collision.md` (a "The ground" section: triangle mesh, the contact maths, the two limits, the digest signal, the generator's constants as estimates with provenance, the performance numbers from Task 5), `docs/roadmap.md` (the non-flat terrain line), `docs/architecture.md` (one line if it describes `ofs-core` shapes).

- [ ] **Step 1:** Write the docs; the CHANGELOG states: opt-in, flat worlds unchanged, no protocol change, Godot draws flat until #155, RF unchanged until #154.
- [ ] **Step 2:** `tr -cd '\r' < <file> | wc -c` is 0 for every changed file (`git diff --name-only main | xargs ...`); `git diff --stat main` shows no `godot/` or `.proto` file.
- [ ] **Step 3:** Commit `docs: the terrain heightfield (#153)`.
