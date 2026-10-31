# Terrain in the radio and video line of sight (#154, #27 part 2 of 3)

Epic #160 (Consequence: real terrain, crash damage and sound), third child. Part 1 (#153) gave the physics a heightfield,
`ofs_core::terrain::Terrain`. This part makes the same grid hide the quad: fly behind a ridge and lose the uplink and the
picture. Part 3 (#155, the Godot mesh) and the audio lanes are not touched.

## Intent

Today the RF models know the world's boxes and cylinders (a golden-section search over their signed distance, then one
ITU-R P.526 knife-edge loss) and the flat ground (the two-ray bounce). A hill is invisible to them. After this change a
world with `[terrain]` obstructs both links with the hills, with the same gradual knife-edge physics, and a world
without `[terrain]` computes exactly the numbers it computed before.

## Goals

- A hill between the handset and the quad weakens the uplink; a hill between the quad and the goggles weakens the video
  (and the interferers' paths to the goggles): a knife-edge loss on the dominant edge, gradual as the ray sinks into or
  clears the ridge, using the existing `knife_edge_loss_db` and `fresnel_v`.
- One source of truth: the search samples the `Arc<Terrain>` the physics holds. The sim builds the grid once and hands
  the same `Arc` to the rigid body, the radio link and the video link. Nobody re-derives a height.
- Objects that stand on hilly ground are rooted to the terrain, not the plane.
- A world without `[terrain]`: same code path, same numbers, bit for bit (proved by tests, and by the radio goldens and
  the e2e suites, which stay green unedited).
- Unit tests over a synthetic ridge built with `Terrain::from_grid`.

## Non-goals and anti-goals

- No protocol change, no change to `godot/`, to `worlds/*.toml`, or to the physics.
- The ground bounce stays the two-ray bounce off the plane d = 0 (a reflection off a slope is a different, bigger model).
  Written as a limit in the research docs.
- No multi-edge diffraction (Deygout, Epstein-Peterson). One dominant edge per terrain path, like the one dominant
  point per object today. A wide hill shadow is therefore a lower bound on the real loss.
- No frequency- or ground-conductivity-dependent terrain loss; no new world-file key.
- Anti-goal: any change to the RF numbers of a world without `[terrain]`, or a slower flat world.

## Constraints

- The terrain is not convex, so the golden-section search that finds the deepest point into a convex shape cannot find
  its highest edge. The terrain gets its own search.
- Rays can be a kilometre long and the grid up to 1025 nodes a side; the link models run every packet or step. The
  search must be cheap when the ray is clear (most of the time).
- `obstruction_db` depends on the two positions only, so receivers that share a position share it. Keep that.

## The terrain obstruction

`ofs_rf::propagation::terrain_loss_db(terrain: &Terrain, a: DVec3, b: DVec3, wavelength_m: f64) -> f64`, NED metres,
dB >= 0.

The ray is `p(t) = a + (b - a) t`. The edge height at `t` is `h(t) = p(t).z - terrain.ground_d(p(t).x, p(t).y)` (positive
when the ground is above the ray), its Fresnel parameter `v(t) = fresnel_v(h(t), t L, (1 - t) L, lambda)` (the existing
function, so its 1 m end clamp applies), and the loss is `knife_edge_loss_db(max_t v(t))` capped at
`TERRAIN_RF_LOSS_DB`. The maximum of `v`, not of `h`, picks the dominant edge: the same quantity that sets a single
edge's loss (a crest near an end weighs more than a taller one in the middle).

1. **Clip** the ray to the grid's footprint (slab test on north and east); outside the grid the ground is the plane,
   which is not an obstacle (as in a flat world). No overlap: 0 dB.
2. **Clear-ray early out**: if the ray's lowest altitude over the clipped span exceeds the grid's highest point by
   `0.78 sqrt(lambda max(L, 3) / 8)` (the clearance at which `v <= -0.78` for any `t`, i.e. `J = 0`; the `max(L, 3)` because `fresnel_v` clamps d1 and d2 to 1 m, which breaks the bound for shorter paths), return 0 without
   sampling. This needs `Terrain::max_height()` (new accessor, computed in `from_grid`, not in the digest).
3. **Sample** `n = ceil(span / (cell_m / 2))` (clamped to 2..=2048) evenly spaced points of the clipped span, keep the
   best `v`.
4. **Refine** with the golden-section search already in the file, generalised to a closure (`golden_min`, which
   `deepest_point` now calls with identical arithmetic): maximise `v` over the two sample intervals around the best
   sample; keep the larger of the refined and the sampled value. The terrain is piecewise planar, so `h` is piecewise
   linear along the ray; a crest between two samples is found to well under a millimetre.
5. Clamp: `knife_edge_loss_db(v).min(TERRAIN_RF_LOSS_DB)`. `TERRAIN_RF_LOSS_DB = 60.0` dB, an estimate (a safety cap for
   a path that starts underground; the formula itself grows only logarithmically: 33 dB at v = 10, 43 dB at v = 30).

Shortcut recorded in the code: the sampling is capped at 2048 points, so a span longer than 1024 cells is sampled more
coarsely than half a cell.

### Wiring

`obstruction_db(obstacles, terrain, a, b, freq_mhz)` and `path_gain(tx, rx, freq, obstacles, terrain, bounce)` take an
`Option<&Terrain>`; the terrain's loss adds to the objects' (the existing sum). The radio's `LinkParams` and the video's
`LinkWorld` gain `terrain: Option<Arc<Terrain>>` (`None` in `ideal()`, in the test fixtures and for every world without
the section). `ofs-sim`'s `vehicle.rs` builds `Arc<Terrain>` once (`world.terrain.as_ref().map(|t| Arc::new(t.build()))`)
and passes it to the `AirframeParams`, the `LinkParams` and `link_params(...)`. `Terrain` already derives `PartialEq`,
so `LinkWorld`'s and `LinkParams`'s derives hold.

Flat worlds: `terrain: None` takes no new branch in the maths (the call adds `0` terrain loss by an early `None`), so the
objects-only sum is the same floating-point expression as before. A test builds the same world with `None` and with
the old code path's expected value; the radio goldens and `ofs-sim` radio golden compare recorded per-OS numbers.

### Behaviour worth knowing (rulings)

- A quad lying on the ground has its antenna within centimetres of the ground, and the ground-grazing path now takes a
  knife-edge loss from the terrain it sits on. Measured with `terrain_loss_db` on an all-zero grid, the quad at its
  clearance and the handset 1.2 m up, at 5.8 GHz: 4.4 dB at 3 cm, 2.3 dB at 12 cm, 1.9 dB at 15 cm, 0.5 dB at 30 cm and
  nothing from 50 cm up over 110 m (3.0, 2.6 and 1.4 dB at 12, 15 and 30 cm over 167 m); the ray rises from the quad, so
  the loss falls off slowly with the clearance, and the 1 m in the geometry is only the `MIN_DISTANCE_M` clamp on d1.
  Flat worlds do not have this term (the two-ray bounce alone models the plane). It is physical and a few dB at most;
  it is not removed to make a resting quad match a flat world.
- The ground bounce still reflects off d = 0. Over hills that ray is approximate; documented.
- Objects and terrain losses add (as objects already add). A building on a hill adds its own loss to the hill's.

## Objects root to the terrain

`Shape::rooted(self)` decides "stands on the ground" by `bottom_z >= -ON_GROUND_M`, the plane. It stays (the plane, as a
one-line delegate, so the many existing callers do not change) and a new
`Shape::rooted_on(self, terrain: Option<&Terrain>)` holds the rule: the shape stands on the ground when its bottom is at or below the
ground under its footprint's centre, within `ON_GROUND_M`: `bottom_z >= ground_d(centre) - ON_GROUND_M`, with
`ground_d = 0` without terrain (the old test exactly). Then it continues `ROOT_DEPTH_M` below its top as before, which
clears any hill (the amplitude cap is 100 m, the root 1000 m).

Ruling: the centre, not the lowest or highest ground under the footprint. A building is declared with one `z`; the
author places it on the ground at its centre. Written in the doc comment. The physics objects stay exactly as
declared (not rooted), as today.

`world_obstacles(world, terrain)` passes the terrain to `rooted`.

## Tests

- `ofs-rf` (unit, over a synthetic ridge from `Terrain::from_grid`: a 41 x 41, 1 m grid, a triangular ridge along
  the east axis, crest 10 m at north 20):
  clear ray above the crest by more than the Fresnel clearance: exactly 0; grazing the crest: about 6.0 dB; a ray
  crossing at 8 m below the crest matches `knife_edge_loss_db(fresnel_v(...))` of the closed form; loss rises
  monotonically as the ray sinks, gradually (no step); a ray along the ridge's side that never meets it: 0; the dominant of
  two ridges; an all-zero grid with both ends 2 m up equals no terrain; a ray outside the footprint: 0; the cap; the
  early-out agrees with the full search (same value with and without the early out on a sweep of heights); a crest
  between samples is found (a ridge placed at a non-node fraction).
- `ofs-rf` rooting: the plane rule is unchanged without terrain (the existing assertions, now with `None`); with a
  terrain, a building whose bottom sits on a hill is rooted, one floating above the hill is not, one declared at the
  plane's z under a hill (buried) is.
- `ofs-radio`: with a ridge between handset and quad, link quality / RSSI drop against the same link on flat ground;
  behind the ridge the link is lost, in front it is up; `terrain: None` equals the old numbers.
- `ofs-video`: a ridge between quad and goggles lowers the SNR and the interference path likewise, and a clear path is
  unchanged.
- `ofs-sim`: `link_params` and the radio chain get the terrain from `worlds/hills.toml`; `world_obstacles` roots an
  object that stands on a hill; **a world without `[terrain]` gives bit-identical `world_obstacles`, `link_params` and
  `path_gain` values**; the same `Arc` backs the rigid body and the radio (pointer equality).
- The radio goldens, the video tests, the link-state CSV and the e2e suites pass unedited.
- Performance: an `#[ignore]` timing note for the search over a 400 m span, not a CI gate.

## Docs and bookkeeping

`CHANGELOG.md` entry; `docs/research/elrs-link.md` and `docs/research/video-link.md` (the terrain term, the dominant
edge, the cap, the limits); `docs/roadmap.md` (the terrain line and the golden-section ruling note that the terrain has
its own sampled search); `docs/dev-setup.md` if it mentions the RF obstacles.

## As built

The whole-flow check (`crates/ofs-sim/tests/terrain_link.rs`, `worlds/hills.toml`, the quad hovering 1.5 m over the
ground) measured, against the same spot in a world without `[terrain]`: RSSI -58.5 dBm without the relief, -78.1 dBm
behind a hill (about 20 dB down), video SNR 18.0 dB against -3.1 dB (about 21 dB down). LQ stays at 99 there: 167 m out
the link still has 27 dB of margin over the 500 Hz sensitivity, so the picture is lost first. "Lose the uplink" is
an expectation, not measured here: it should hold farther out or with a weaker margin. The early-out margin uses
`max(L, 3)` as amended above; the ground-grazing figures are those of "Behaviour worth knowing" above.

## Calls made without the user (rulings)

1. Terrain is its own sampled-and-refined dominant-edge search, not forced into the convex-shape golden search.
2. Edge chosen by the largest Fresnel `v`, not the largest height.
3. Cap 60 dB, a constant, no world key.
4. One shared `Arc<Terrain>` built once in the sim; the RF crates take `Option<&Terrain>` / `Option<Arc<Terrain>>`.
5. Rooting measures the ground under the shape's centre.
6. The ground bounce stays on the plane d = 0; no multi-edge diffraction.
7. The builder-check subagent was skipped: the design is the issue's own, plus these rulings, and the user asked for no
   mid-plan questions.
