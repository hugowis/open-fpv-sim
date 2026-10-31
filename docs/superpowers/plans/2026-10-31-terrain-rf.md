# Terrain in the radio and video line of sight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A hill between the handset and the quad, or between the quad and the goggles, obstructs the link with a knife-edge loss sampled from the physics' own `Terrain`; objects root to the terrain.

**Architecture:** `ofs-rf` gains `terrain_loss_db` (clip, early-out, sample, refine by golden section on the Fresnel parameter, cap), `obstruction_db`/`path_gain` take `Option<&Terrain>`. `ofs-radio`'s `LinkParams` and `ofs-video`'s `LinkWorld` carry `Option<Arc<Terrain>>`. `ofs-sim` builds the `Arc<Terrain>` once and hands the same one to the rigid body and both links, and roots objects with `Shape::rooted_on`.

**Tech Stack:** Rust workspace (glam, ofs-core/-rf/-radio/-video/-sim/-config).

**Spec:** `docs/superpowers/specs/2026-10-31-terrain-rf-design.md` (and part 1: `docs/superpowers/specs/2026-10-31-terrain-heightfield-design.md`).

## Global Constraints

- Rust tests run in WSL from `/mnt/c/dev/open-fpv-sim/.claude/worktrees/terrain-rf-896803`: `wsl.exe -d Ubuntu -e bash -lc 'cd <worktree> && CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test -p <crate> --locked'` (the `docs/dev-setup.md` command; Windows `cargo` also works for pure-Rust crates but the radio goldens are per OS, run them in WSL and on Windows).
- Files are LF: `tr -cd '\r' < file | wc -c` must print 0 for every file touched.
- No protocol change, nothing under `godot/`, no `worlds/*.toml` edit, no physics change.
- Flat worlds (no `[terrain]`) must keep bit-identical RF numbers: the radio goldens (`crates/ofs-radio/tests/golden.rs`, `crates/ofs-sim/tests/radio_golden.rs`) and video tests stay unedited and green.
- `TERRAIN_RF_LOSS_DB = 60.0`; sampling step `cell_m / 2`, clamped to 2..=2048 points; early-out clearance `0.78 * sqrt(lambda * L / 8)`.
- Commits end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not push; no PR.
- Out of scope files: `godot/**`, `crates/ofs-audio/**`, `worlds/park.toml`.

## Review Focus

- A ray that starts or ends below the ground (a crashed quad, penetrating a few cm): finite loss, never NaN, never above the 60 dB cap. Pinned in Task 2.
- Ray parallel to a ridge or exactly vertical (a == b horizontally): no division by zero in the clip, 0 dB or a sane value. Pinned in Task 2.
- Zero-length ray (a == b): 0 dB, no NaN. Pinned in Task 2.
- A terrain whose grid is entirely off the ray's side (ray outside the footprint) costs 0 and does not sample. Pinned in Task 2.
- A thin ridge between sample points is found (a crest at a non-node fraction). Pinned in Task 2.

---

### Task 1: `Terrain::max_height` and `Shape::rooted_on`

**Files:**
- Modify: `crates/ofs-core/src/terrain.rs`, `crates/ofs-core/src/shape.rs`
- Test: `crates/ofs-core/tests/shape.rs`, `crates/ofs-core/tests/terrain.rs` (add to the existing file if present, otherwise to the file the existing terrain tests live in: `grep -rn "Terrain::from_grid" crates/ofs-core/tests`)

**Interfaces:**
- Produces: `Terrain::max_height(&self) -> f64` (highest node, metres up, at least 0.0 is NOT forced: the raw maximum of the grid); `Shape::rooted_on(self, terrain: Option<&Terrain>) -> Shape`; `Shape::rooted(self)` stays and equals `rooted_on(None)`.

- [ ] **Step 1: Write failing tests.** In the terrain tests: `max_height_is_the_highest_node` (a 3 x 3 grid with one 4.5 node and one -2 node gives 4.5; an all-negative grid gives its largest, negative, value). In `shape.rs` tests, with a 5 x 5 1 m grid raised uniformly to 8 m (ground_d = -8 everywhere inside): `rooted_on_a_hill_roots_a_box_standing_on_it` (a box with bottom at z = -8 gets `rooted_on(Some(&t))` extended: its signed distance 100 m below its bottom is negative, as `rooted()` would give for a plane-standing box); `rooted_on_leaves_a_floating_box_alone` (bottom at z = -12, 4 m above the hill: unchanged, `== original`); `rooted_on_roots_a_buried_box` (bottom at z = 0, 8 m under the hill's surface: rooted); `rooted_on_none_is_rooted` (for a box on the plane and a floating one, `rooted_on(None) == rooted()`).

- [ ] **Step 2: Run them (`-p ofs-core`) and see them fail to compile / fail.**

- [ ] **Step 3: Implement.** In `terrain.rs`: add a `max_height: f64` field set in `from_grid` (fold of `node` over the heights, starting at `f64::NEG_INFINITY`) and the accessor; keep it out of `digest` (derived `PartialEq` may include it). In `shape.rs`: move the body of `rooted` into `rooted_on(self, terrain: Option<&Terrain>)`, replacing the constant `-ON_GROUND_M` threshold by `ground_d - ON_GROUND_M` with `ground_d = terrain.map_or(0.0, |t| t.ground_d(center.x, center.y))`; with `None` the expression `0.0 - ON_GROUND_M` is the old `-ON_GROUND_M` exactly. `rooted(self)` becomes `self.rooted_on(None)`. Doc comment: the ground under the footprint centre decides (ruling 5). `shape.rs` needs `use crate::terrain::Terrain;`.

- [ ] **Step 4: Run `cargo test -p ofs-core --locked`; all pass, the existing `rooted` assertions in `crates/ofs-rf/tests/propagation.rs` still compile unedited.**

- [ ] **Step 5: Commit** `feat(core): Terrain::max_height and Shape::rooted_on (#154)`.

### Task 2: The terrain obstruction in `ofs-rf`

**Files:**
- Modify: `crates/ofs-rf/src/propagation.rs`
- Test: `crates/ofs-rf/tests/propagation.rs`

**Interfaces:**
- Consumes: `Terrain::{ground_d, max_height, origin_ne, cell_m, nodes}` (Task 1 and part 1), `knife_edge_loss_db`, `fresnel_v`.
- Produces:
  - `pub const TERRAIN_RF_LOSS_DB: f64 = 60.0;`
  - `pub fn terrain_loss_db(terrain: &Terrain, a: DVec3, b: DVec3, wavelength_m: f64) -> f64`
  - `pub fn obstruction_db(obstacles: &[Obstacle], terrain: Option<&Terrain>, a: DVec3, b: DVec3, freq_mhz: f64) -> f64` (new second parameter)
  - `pub fn path_gain(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, obstacles: &[Obstacle], terrain: Option<&Terrain>, ground_bounce: bool) -> PathGain` (new fifth parameter)
  - private `golden_min(f: impl Fn(f64) -> f64, lo: f64, hi: f64) -> f64` (returns the argmin); `deepest_point` calls it with `(0.0, 1.0)`.

- [ ] **Step 1: Write failing tests** in `crates/ofs-rf/tests/propagation.rs`. Fixture `ridge(crest_row: usize, crest_m: f32) -> Terrain`: a 41 x 41 grid, cell 1.0, origin `[0.0, -20.0]`, height at row `crest_row` equal to `crest_m`, falling linearly to 0 at `crest_row - 10` / `+ 10` (clamped, zero elsewhere), constant along east; so north is the slow axis and the crest line is north = `crest_row`. Existing `path_gain(...)` and `obstruction_db(...)` calls in this file gain `None` for the terrain. `lambda = wavelength_m(5800.0)`. New tests:
  - `a_ray_well_above_the_ridge_loses_nothing`: crest 10 m at row 20, ray (0,0,-40) to (40,0,-40): `terrain_loss_db == 0.0` exactly (early-out) AND equals the loss with the early-out bypassed is covered by the sweep test below.
  - `grazing_the_crest_costs_about_6_db`: ray at the crest's altitude, (0,0,-10) to (40,0,-10): `close(.., 6.03, 0.3)` (`knife_edge_loss_db(0)`, the sampled v is at most slightly off 0).
  - `a_ray_below_the_crest_matches_the_knife_edge_formula`: ray (0,0,-2) to (40,0,-2), crest 10 m at row 20: `h = 8`, `d1 = d2 = 20`: expected `knife_edge_loss_db(fresnel_v(8.0, 20.0, 20.0, lambda))` (about 31 dB, below the cap), tolerance 0.05.
  - `the_loss_rises_gradually_as_the_ray_sinks`: heights -14 down to -2 in 1 m steps: each loss >= the previous (monotone), no step larger than 3 dB between 1 m steps near the crest, and the loss at clearance 0 is between those at +1 m and -1 m.
  - `a_ray_that_never_meets_the_ridge_loses_nothing`: both ends at north 30..40 (past the ridge's foot at row 30): 0.
  - `the_dominant_of_two_ridges_sets_the_loss`: a grid with two crests, 6 m at row 12 and 10 m at row 28; the loss equals the max of the two single-edge formulas (computed in the test with `fresnel_v` at each crest) within 0.05 dB, not their sum.
  - `a_flat_terrain_changes_nothing_for_a_raised_ray`: an all-zero 41 x 41 grid, ends at 2 m up (z = -2) 40 m apart: `terrain_loss_db == 0.0`, and `path_gain(.., &[], Some(&flat), true) == path_gain(.., &[], None, true)` field for field.
  - `a_ray_outside_the_footprint_loses_nothing`: ends at east = 500.
  - `a_thin_crest_between_samples_is_found`: build a 41 x 41 grid with cell 1.0 whose ridge is a single node row of 10 m (so the crest is a 1 m wide tent); ray along north at z = -2 offset by 0.5 east so it runs on the cell-diagonal; the loss is within 0.5 dB of the formula for `h` computed from `ground_d` at the maximum of `v` found by a brute-force 100000-point scan in the test.
  - `a_ray_sampled_with_the_early_out_agrees_with_without`: sweep ray altitudes from -9 m (below crest) up to -30 m; the loss is monotone non-increasing and reaches exactly 0 at -30 m; at the first altitude where it is 0, `v <= -0.78` for the crest (assert via `fresnel_v`).
  - `a_ray_starting_underground_is_finite_and_capped`: a at z = +0.5 under the ridge's foot, b = (40,0,-2): finite, `0.0 <= loss <= TERRAIN_RF_LOSS_DB`; and an a == b ray: `0.0`.
  - `a_vertical_ray_over_the_ridge_is_finite`: a = (20,0,-1), b = (20,0,-30): finite, in `[0, cap]`.
  - `obstruction_db_adds_terrain_to_the_objects`: with the existing building obstacle from `obstruction_db` tests plus the ridge, the total equals the objects' sum plus `terrain_loss_db`; with `None` equal to the old value.

- [ ] **Step 2: Run `cargo test -p ofs-rf --locked`; fails to compile.**

- [ ] **Step 3: Implement** in `propagation.rs` (`use ofs_core::terrain::Terrain;`):
  - Refactor `deepest_point` onto `golden_min` keeping the loop arithmetic identical (same `ratio`, `m1`/`m2`, `f(m1) < f(m2)` tie rule, 60 steps, returns `(lo + hi) * 0.5`), so object numbers do not move a bit.
  - `terrain_loss_db`: `L = a.distance(b)`; return 0 if `L < 1e-9`. Clip: the parameter interval `[t0, t1]` within the grid footprint `[origin_ne, origin_ne + (nodes - 1) * cell]` in north and east (slab test; a zero horizontal component is inside or outside, no division); empty interval returns 0. Early-out: with `zmax = max(p(t0).z, p(t1).z)` (the ray's lowest altitude is `-zmax`), return 0 when `-zmax > terrain.max_height() + 0.78 * (wavelength_m * L / 8.0).sqrt()`. `v(t) = fresnel_v(p(t).z - terrain.ground_d(p(t).x, p(t).y), t * L, (1.0 - t) * L, wavelength_m)`. Sample `n = ((t1 - t0) * L / (cell_m / 2.0)).ceil().clamp(2.0, 2048.0)` points (include both ends), take the index of the best `v`; refine with `golden_min(|t| -v(t), t_{k-1}, t_{k+1})` (indices clamped to the ends); `v_max` = the larger of the sampled best and `v(refined)`; return `knife_edge_loss_db(v_max).min(TERRAIN_RF_LOSS_DB)`. Add the comment `// shortcut: at most 2048 samples, so a span over 1024 cells is sampled coarser than half a cell; refine, or lower the cap, if a world needs it.`
  - `obstruction_db` and `path_gain` take the terrain; the objects' sum is unchanged and the terrain term is added only when `Some` (`+ terrain.map_or(0.0, |t| terrain_loss_db(t, a, b, lambda))` is NOT allowed to run for `None` in a way that alters the sum's bits: add it as `let objects = ...; match terrain { Some(t) => objects + terrain_loss_db(..), None => objects }`).
  - Update the module doc: the ground is the plane z = 0 *or* the terrain the caller passes.

- [ ] **Step 4: Run `cargo test -p ofs-rf --locked`; all pass.** (The other crates do not build until Task 3: run only `-p ofs-rf` here.)

- [ ] **Step 5: Commit** `feat(rf): terrain knife-edge obstruction by a sampled dominant edge (#154)`.

### Task 3: Radio and video links carry the terrain

**Files:**
- Modify: `crates/ofs-radio/src/elrs.rs` (`LinkParams` + `ideal()` + the two `path_gain` calls at ~231 and ~262), `crates/ofs-video/src/link.rs` (`LinkWorld`, `vtx_paths`, `interference_dbm`), `crates/ofs-radio/Cargo.toml` / `crates/ofs-video/Cargo.toml` only if `ofs-core` is not already a dependency.
- Modify (compile fixes): every `LinkWorld { .. }` literal in `crates/ofs-video/tests/link.rs` and any `LinkParams { .. }` literal without `..` in the radio tests; every `path_gain(...)` call in `crates/ofs-rf/tests/propagation.rs` if Task 2 left any.
- Test: `crates/ofs-radio/tests/elrs.rs`, `crates/ofs-video/tests/link.rs`

**Interfaces:**
- Consumes: Task 2's `obstruction_db(.., terrain, ..)` and `path_gain(.., terrain, ..)`.
- Produces: `pub terrain: Option<std::sync::Arc<ofs_core::terrain::Terrain>>` on `ofs_radio::elrs::LinkParams` and `ofs_video::link::LinkWorld`; `None` in `LinkParams::ideal`.

- [ ] **Step 1: Write failing tests.**
  - Radio (`tests/elrs.rs`, following the existing blocked-by-a-building test at ~line 155 for the rig and assertions): `a_ridge_between_the_handset_and_the_quad_cuts_the_uplink` with a 41 x 41 ridge terrain (10 m crest) at the handset-to-quad line, the quad 2 m up and 200 m away so the ridge sits mid-path (scale the fixture's grid with `cell_m` to cover it: build the ridge fixture with a `cell_m` parameter; do not copy-paste it, put the helper in a `tests/common`-style helper only if the crate already has one, otherwise duplicate the 10-line fixture locally): the uplink RSSI (or the link quality the building test asserts) is lower than the same link with `terrain: None`, by roughly the knife-edge value; `the_same_link_without_a_ridge_in_the_way_is_unchanged` (ray well above the ridge: equal to `None`, bit for bit); the telemetry downlink is cut by the same ridge (both directions use the one terrain).
  - Video (`tests/link.rs`, following the building test at ~line 200): `a_ridge_between_the_quad_and_the_goggles_lowers_the_snr` (SNR / `present` as that test asserts) and `a_ridge_between_an_emitter_and_the_goggles_lowers_the_interference` (`interference_dbm` with a terrain between an emitter and the pilot is lower than without), `a_clear_path_over_the_ridge_is_unchanged`.

- [ ] **Step 2: Run `cargo test -p ofs-radio -p ofs-video --locked`; fails (field missing).**

- [ ] **Step 3: Implement.** Add the field (doc comment: "the physics' own grid, shared: never a copy of the heights"), pass `self.params.terrain.as_deref()` to both `path_gain` calls; in the video link pass `params.world.terrain.as_deref()` to both `obstruction_db` calls. Fix every struct literal and call the compiler flags.

- [ ] **Step 4: Run `cargo test -p ofs-rf -p ofs-radio -p ofs-video --locked` in WSL; the goldens and old tests pass unedited except for the added `terrain: None`.** Also run the radio goldens on Windows (`cargo test -p ofs-radio --test golden` in PowerShell).

- [ ] **Step 5: Commit** `feat(radio,video): links obstructed by the terrain (#154)`.

### Task 4: One shared `Arc<Terrain>` in the sim, objects rooted to it

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (`radio_chain`, `world_obstacles`, `link_params`, the `AirframeParams` construction ~line 970, the vehicle build at ~1014 and ~1108, and the test at ~2227/2236).
- Test: a new `crates/ofs-sim/tests/terrain_rf.rs` (or an existing integration file for world/RF if one fits: `ls crates/ofs-sim/tests`), and the unit tests in `vehicle.rs` where `world_obstacles` is already tested.

**Interfaces:**
- Consumes: `LinkParams.terrain`, `LinkWorld.terrain` (Task 3), `Shape::rooted_on` (Task 1), `TerrainSection::build() -> Terrain`.
- Produces: `pub fn world_obstacles(world: &WorldConfig, terrain: Option<&Terrain>) -> Vec<Obstacle>`; `pub fn link_params(vtx: &VtxSection, world: &WorldConfig, terrain: Option<Arc<Terrain>>) -> Result<VideoLinkParams, SimError>`; `radio_chain(..)` gets a `terrain: Option<Arc<Terrain>>` parameter. The vehicle build creates `let terrain = opts.world.terrain.as_ref().map(|t| Arc::new(t.build()));` once, before the radio chain, and clones the `Arc` into the airframe, the radio chain and `link_params`.

- [ ] **Step 1: Write failing tests.**
  - `objects_on_hilly_ground_root_to_the_terrain`: a `WorldConfig` (from a TOML string with `[terrain] amplitude_m = 8.0` and one building with `rf_loss_db > 0` whose bottom sits on the hill at its centre, computed in the test from the baked `Terrain::ground_d`): `world_obstacles(&world, Some(&terrain))` roots it (signed distance 100 m under its bottom is negative) whereas the same object with bottom at the plane z = 0 in the same hill zone, floating where the ground is lower than the plane, is not (use `ground_d > 0` spot: a valley, or build the terrain with `Terrain::from_grid` negative heights).
  - `a_world_without_terrain_roots_exactly_as_before`: `world_obstacles(&flat_world, None)` equals the objects mapped with `object_shape(o).rooted()` (the old expression), element for element.
  - `the_links_and_the_body_share_one_grid`: build the vehicle on `worlds/hills.toml`; the radio link's and the video link's `Arc<Terrain>` and the airframe's are `Arc::ptr_eq`. If the built types hide those fields, expose them through the narrowest existing test seam (a `#[cfg(test)]` accessor), do not widen the public API.
  - `a_world_without_terrain_has_no_terrain_in_the_links`: on `worlds/flat.toml` both are `None`.
  - `a_hill_between_the_pilot_and_the_quad_cuts_the_video_in_the_real_world`: `link_params` on a hilly world (a `WorldConfig` from TOML with `[terrain]` and the pilot at home) with a synthetic ridge terrain passed in: `vtx_paths` at a quad position behind the ridge has a larger `obstruction_db` than one beside it.

- [ ] **Step 2: Run `cargo test -p ofs-sim --locked` (WSL); fails.**

- [ ] **Step 3: Implement** the parameters above; `world_obstacles` uses `object_shape(o).rooted_on(terrain)`. The airframe's `terrain:` field takes the shared `Arc` (replacing the inline `Arc::new(t.build())`). Update the existing call sites and tests (`world_obstacles(&world, None)` in the existing flat tests, the test at ~2227 passes `None`).

- [ ] **Step 4: Run the full suites in WSL: `cargo test --workspace --locked`; Windows: `cargo test -p ofs-radio -p ofs-sim --locked`; then the e2e suites that exist for a link-state CSV (`grep -rln "link_state\|link-state" crates python godot --include=* | head`) and the Python tests that do not need Godot or SITL. Everything that passed on `main` passes; the radio goldens are unedited.** Also run `cargo clippy --workspace --all-targets --locked -- -D warnings` if the repo CI does (check `.github/workflows`).

- [ ] **Step 5: Commit** `feat(sim): one shared terrain for the physics and both links, objects rooted to it (#154)`.

### Task 5: Docs, changelog, final proof

**Files:**
- Modify: `CHANGELOG.md`, `docs/research/elrs-link.md`, `docs/research/video-link.md`, `docs/roadmap.md` (lines ~141 and ~174), `docs/dev-setup.md` (only if it mentions RF obstacles or terrain), `docs/superpowers/specs/2026-10-31-terrain-rf-design.md` (as built, if anything moved).

- [ ] **Step 1: CHANGELOG.** Under "Unreleased" add "### Terrain in the radio and video line of sight (#154, part 2 of 3 of #27)" with Added/Changed bullets: the terrain knife-edge term (dominant Fresnel edge, 60 dB cap, the shared grid), objects rooted on hills, flat worlds unchanged (tests), no protocol change, Godot still draws flat until #155, the limits (plane ground bounce, single edge).
- [ ] **Step 2: Research docs.** `elrs-link.md` and `video-link.md`: a "Terrain" paragraph with the maths (`v` maximised along the ray, `J(v)`, the cap as an estimate, clearance early-out), the numbers (31 dB for an 8 m ridge at 20 m/20 m at 5.8 GHz; compute the 2.4 GHz equivalent with the test's formula and write it), the limits (ground bounce off d = 0, one edge, a grounded quad takes ground-grazing loss from the terrain under it, objects add to terrain). Roadmap: the terrain line (#154 done, #155 remains) and the golden-section ruling note ("terrain has its own sampled search because it is not convex").
- [ ] **Step 3: Whole-flow proof.** Run an end-to-end sanity check of the real chain: `ofs-sim` on `worlds/hills.toml`, quad flown behind a hill (use whichever scripted/replay harness `crates/ofs-sim/tests` offers; if none sets positions, a test that builds the vehicle and steps with the quad teleported behind the hill via the initial position in `QuadConfig` is enough): the radio's RSSI/LQ signal behind the hill is below the signal in front. Record the two numbers in the changelog or the research doc.
- [ ] **Step 4: Check LF** (`tr -cd '\r'` on every touched file), `git status` clean, `git diff main --stat` limited to the in-scope files.
- [ ] **Step 5: Commit** `docs: terrain in the radio and video line of sight (#154)`.
