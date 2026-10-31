# A heightfield ground for the physics (#153, #27 part 1 of 3)

Epic #160 (Consequence: real terrain, crash damage and sound), first child. Parts 2 and 3 of #27 (#154 radio and video
line of sight, #155 the Godot mesh) are not built here; this design leaves the seams for them.

## Intent

The ground is the plane d = 0, written as `centre.z` / `point.z` in `ofs-physics`. A world gains an opt-in `[terrain]`
section and the contact solver's ground queries, the landing points' spring-dampers and the static-friction hold sample
it, so the quad lands, rolls and slides on hills and valleys. A world without the section is the plane d = 0 exactly
as before, bit for bit.

## Goals

- `[terrain]` in a world file; relief under every ground contact (collision spheres and landing points).
- One source of truth for heights: a single baked grid (`ofs_core::terrain::Terrain`), sampled by the sim, handed to the
  client and to the RF search later. Nobody re-derives a height from parameters.
- Heights are exact: a query is a pure function of the grid, identical on every OS; no per-site approximation.
- The terrain is in the lockstep digest, and a seeded flight over hills replays exactly.
- No protocol change, no new asset file, no new dependency.
- The shipped quads stay inside the performance budget on a hilly world.

## Non-goals and anti-goals

- RF line of sight over terrain (#154), the Godot mesh and the client's ground (#155). Until #155 the Godot client draws
  a flat ground in a world with `[terrain]`: the physics is right and the picture is not. Say so in the world's header.
- A heightmap image or any binary asset; erosion, water, trees, material zones (different friction per area).
- Smooth (non-planar) surfaces. The ground is a triangle mesh and says so.
- Anti-goal: any change to a world without `[terrain]`: not one bit of its flights, its bus digest or its signal list.

## The spike: heightmap asset or analytic hills?

The issue asks for the choice, with reasons. Three candidates were weighed against the epic's ruling (heights have one
source of truth, the world data; the sim samples it and Godot renders a mesh from it):

| | Heightmap asset (PNG or raw file) | Analytic function evaluated per query | **Seeded hills baked to a grid (chosen)** |
|---|---|---|---|
| Single source | the file, read by both sides | the formula, re-implemented in GDScript and Rust (two codes, two roundings) | the grid, produced once by the sim; the client takes the grid, not the recipe |
| Exact | yes, if both sides interpolate alike | no: `sin`/`exp` differ in the last bits between libm builds (the radio goldens already need one digest file per OS) | yes: the generator uses only `+ - * /`, `sqrt` and integer hashing, which IEEE 754 rounds identically everywhere |
| Cost per query | a few loads | transcendental maths at 8 kHz x 29 contacts | 3 loads and a few flops |
| New moving parts | a decoder (PNG dependency, or a made-up format), path resolution, the repro bundle must carry it, quantization (8/16 bit) | none | none: the world stays one TOML file and the repro bundle already carries it |
| Authoring | needs an image tool | a handful of numbers | a handful of numbers (`seed`, `amplitude_m`, ...) |
| Door left open | - | - | a heightmap file is just another way to fill the same `Terrain::from_grid`; add it when someone wants real-world terrain |

**Ruling: the world file holds seeded hill parameters; the loader bakes them once into a regular f32 height grid; the grid
is the truth.** The recipe is an authoring convenience, not a second source: everything downstream (physics, #154, #155)
reads `Terrain`. A heightmap loader can be added later without touching a consumer.

## The representation

### `ofs_core::terrain::Terrain` (new, `crates/ofs-core/src/terrain.rs`)

A regular grid of heights, `nodes_n x nodes_e` f32 values in metres **up** (so a hill is positive; NED ground depth is
`-h`), row-major with north as the slow index, node (0, 0) at `origin_ne` and `cell_m` apart. f32 storage: the
grid is what a client puts in a `PackedFloat32Array`, and widening f32 to f64 is exact, so both sides read the same
numbers.

The surface is a **triangle mesh**: each cell is split along its (south-west, north-east) diagonal, so the surface is
piecewise planar and the sim's height and normal are exactly the rendered mesh's. (Bilinear patches would be
re-approximated by any mesh.)

```rust
pub struct Terrain { /* origin_ne, cell_m, nodes_n, nodes_e, heights: Vec<f32> */ }
impl Terrain {
    pub fn from_grid(origin_ne: [f64; 2], cell_m: f64, nodes_n: usize, nodes_e: usize, heights: Vec<f32>) -> Result<Terrain, String>;
    /// The ground's depth `d` (NED, down positive; a hill is negative) at (north, east).
    pub fn ground_d(&self, north: f64, east: f64) -> f64;
    /// `ground_d` and the surface's outward (up) unit normal, in NED, there.
    pub fn surface(&self, north: f64, east: f64) -> (f64, DVec3);
    pub fn max_slope(&self) -> f64;            // tan of the steepest triangle
    pub fn digest(&self) -> u64;               // FNV-1a of the exact grid
    pub fn heights(&self) -> &[f32]; pub fn origin_ne(&self) -> [f64; 2]; pub fn cell_m(&self) -> f64; pub fn nodes(&self) -> (usize, usize);
}
```

Beyond the grid the ground is the plane d = 0 with the normal straight up. The hill generator always makes a zero
border, so there is no cliff at the edge; a hand-built grid with a non-zero border has one there (documented, not
policed).

### `[terrain]` in the world file (`crates/ofs-config/src/world.rs`, generator in `crates/ofs-config/src/terrain.rs`)

```toml
[terrain]            # opt-in; every key optional, so a bare [terrain] is rolling hills
seed = 7
size_m = 400.0       # side of the square the relief covers, centred on home; flat (d = 0) beyond it
cell_m = 1.0         # grid spacing; size_m must be a whole multiple of it
amplitude_m = 8.0    # height scale; the hills span about +-amplitude
wavelength_m = 100.0 # size of the largest features
octaves = 3          # layers of detail, each half the wavelength and `persistence` times the height
persistence = 0.5
flat_radius_m = 25.0 # within this of home the ground is exactly d = 0 (the pad, the pilot); relief eases in beyond it
```

`TerrainSection::build() -> Terrain`: value noise on an integer-hashed lattice (fnv1a64 of seed, octave and lattice
coordinates, mapped to [-1, 1)), quintic fade, fractal sum normalised to [-1, 1], times `amplitude_m`, times two
smooth windows (`smoothstep`): one from the flat disc around home outward over `wavelength_m`, one to zero at the
grid's edge over `wavelength_m`. Only `+ - * /` and `sqrt` appear, never `sin`, `exp` or `pow`, so the grid is bit-identical on every OS (a test pins its digest).

Validation (in `WorldConfig::validate`, field paths like the rest): every number finite; `size_m` in (0, 2000],
`cell_m` in [0.25, size_m], size a whole multiple of the cell, at most 1025 nodes a side (4 MB); `amplitude_m` in
[0, 100]; `wavelength_m` >= 4 x `cell_m` (a feature needs cells); `octaves` 1..=6; `persistence` in (0, 1];
`flat_radius_m` >= 0; **the steepest triangle no more than 60 degrees** (hills, not cliffs: the contact maths below
is a vertical-offset approximation and wants a floor under `|n.z|`); the pilot's and the handset's positions must not
be below the terrain. Gates and objects are the author's business (they are placed in the same NED frame; the
flat radius and a low amplitude keep a course clear).

## Physics

`AirframeParams` gains `terrain: Option<Arc<Terrain>>`. One helper in `collision.rs`:

```rust
pub fn ground_at(terrain: Option<&Terrain>, north: f64, east: f64) -> (f64, DVec3)  // (d, up normal); None = (0.0, NEG_Z)
```

Both ground sites call it; with no terrain it returns exactly `(0.0, NEG_Z)` and the arithmetic below collapses to the
old expressions (`x - 0.0` and `x * 1.0` are exact), so the flat world is bit-identical. The determinism test pins it.

- **Collision spheres** (`collect`): the centre's signed distance to the triangle plane under it is
  `n.z * (c.z - d_g)`, so `penetration = radius - sd = radius + (-n.z) * (c.z - d_g)`, the contact `normal = n`. Flat:
  `radius + c.z`, `NEG_Z`, as before.
- **Landing points** (`RigidBody::contact`): `depth = (-n.z) * (p.z - d_g)` along `n`, handed to `point_force`
  unchanged: the spring-damper acts along the slope normal, and the static-friction anchor spring works in the
  slope's tangent plane (it already projects onto the contact normal's plane), so a quad parked within the friction angle
  of a hillside stays put and one past it slides, as the incline tests pin on a synthetic plane.
- **Events**: the ground is still object index -1.

Known limits (written in `docs/research/collision.md`): the sphere is tested against the plane of the one triangle
under its centre, not the true distance to the mesh. A sphere sunk into a neighbouring triangle across a ridge or a
valley fold is found a step later, off by at most `r (1 - cos(dtheta/2))`: under a millimetre for a 2 cm sphere at
the slope breaks of the shipped hills. Vertical-offset penetration understates a steep slope's depth by `|n.z|` (60 degrees:
half); the 60-degree validation bounds that.

### The digest

The lockstep digest is the bus digest (every signal's name and bits). Terrain enters it by two routes: the body state it
shapes, and, in worlds with `[terrain]` only, a new signal `body.terrain_d_m`, the ground's depth under the body's
centre, published each step (`names::BODY_TERRAIN_D`). A flat world registers no new signal, so its signal list and
digest are untouched. The signal doubles as a seam for the audio and the damage lanes (height above ground).

## Seams for #154 and #155 (not built)

- #154: the RF search samples `Terrain::ground_d` along the ray (and roots objects at the local ground instead of
  `ROOT_DEPTH_M` below d = 0). Nothing here changes `ofs-rf`.
- #155: the client needs the grid. Either the `World` message carries `Terrain::heights()`, `origin_ne()`, `cell_m()` and
  `nodes()` (a protocol bump, theirs), or the client calls the generator. The first is the single-source choice; the sim
  already holds everything it needs. `Terrain::digest()` lets the client assert it received the sim's grid.

## Tests

- `ofs-core`: node-exact heights; triangle-plane interpolation on both triangles of a cell; the normal on a tilted
  plane; zero beyond the grid; `from_grid` rejects a bad length; digest changes with one bit.
- `ofs-config`: bare `[terrain]` parses and builds; absence is `None`; every validation rule; same seed bit-identical,
  a different seed different; **the baked grid's digest is pinned** (cross-OS claim); zero grid border and flat disc;
  max slope below the limit for the defaults and the shipped world.
- `ofs-physics`: a sphere and a landing point rest on a 20-degree ramp built with `from_grid`, and a quad inside the
  friction angle stays put while one past it slides at `g (sin - mu cos)`; a drop onto a hill crest bounces off the
  slope normal; flat-grid terrain agrees with no terrain except for the extra signal.
- `ofs-sim`: **a world without `[terrain]` reproduces a pinned bus digest recorded on `main` before this change**
  (the bit-for-bit claim); a seeded flight over `worlds/hills.toml` replays to the same digest twice and differs from
  the flat world; `body.terrain_d_m` exists only with terrain.
- Performance: the shipped quads, 10 s of hover and bounce-around at 8 kHz in release, hilly against flat, recorded in
  `docs/research/collision.md` (an `#[ignore]` timing test, not a CI gate: wall-clock gates flake).

## What the pins protect, and what a macOS difference cannot break

Two kinds of digest check, with different reach:

- **The flat-world pin** (`ofs-sim/tests/flat_world_pin.rs`, constants recorded on `main` before this change): proves a
  world without `[terrain]` is bit-identical to before. It compares a whole-vehicle bus digest, which includes the
  motor, battery and propeller maths, and those call libm (`exp`, `sin`, `powf`...), whose last bits differ between
  Windows, Linux and macOS (the radio goldens already keep one digest file per OS for the same reason). So the
  constant is per OS: Windows and Linux/WSL have theirs, recorded on `main`; macOS has none, and the test is compiled
  out there. The claim it carries is "this change moved no bit on the platforms where it is pinned"; the change has no
  platform-specific code, so it holds on macOS too, unpinned.
- **The terrain grid pin** (`ofs-config/tests/world.rs`, the digest of the default section's baked grid): runs on every
  OS, macOS included, with one constant, because the generator uses only IEEE-754 basic operations and `sqrt`
  (correctly rounded everywhere), integer hashing and no fused multiply-add.

A macOS difference cannot break the lockstep digest or "a seeded flight over hills replays exactly": the lockstep
digest is compared between two runs on the same machine (a journal replay, `ofs-sim --replay`, the determinism tests
that run a flight twice and compare), never against a constant from another OS, and the terrain adds no libm call, so
it cannot make two same-platform runs differ. The test that proves the replay (`seeded_flight_over_hills_replays_exactly`)
compares run against run and carries no per-OS constant, so it runs on macOS too. Cross-platform byte-identity of
whole flights was never a property of this simulator and this change does not claim it; only the terrain grid itself
is cross-platform byte-identical.

The macOS CI job (`cargo test --workspace`, currently advisory) stays green by construction: the pin test is compiled
out there, the grid pin is platform-independent, and no new test compares a libm-dependent value with a constant.

## Docs and bookkeeping

`CHANGELOG.md` entry; `docs/dev-setup.md` world-file section (`[terrain]`, and that "nothing collides" line is stale);
`docs/research/collision.md` (the ground is a mesh; the contact maths; limits; constants with provenance: the generator's
defaults are estimates); `worlds/hills.toml` (new shipped world; header notes the flat Godot ground until #155);
`docs/roadmap.md` line on non-flat terrain.

## Calls made without the user (rulings)

1. Analytic seeded hills baked to a grid, not a heightmap file (the spike above).
2. f32 grid, triangle mesh with a fixed diagonal, not bilinear patches.
3. Zero border and zero flat disc so home, the pilot and the edge are continuous with the plane d = 0.
4. 60-degree slope cap enforced at validation; the pilot and handset checked against the terrain, gates and objects not.
5. `body.terrain_d_m` registered only with terrain, so flat worlds keep their signal list.
6. `worlds/hills.toml` ships now even though Godot draws it flat until #155; the world menu will list it.
7. The builder-check subagent was skipped: this design is the issue's own, plus rulings recorded here, and the user asked
   for no mid-plan questions.
