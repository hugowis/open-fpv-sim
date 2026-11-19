# Analog link fidelity — design

Date: 2026-11-19. Issues: #25, #62, #64, #63. Status: approved (owner approved the selection and the two
open data questions on 2026-11-19).

## Intent

Finish the analog link model before digital video (#33): per-ray multipath instead of one combined
obstruction, frequency-dependent material loss, and a handset transmit antenna that behaves like a real
diversity handset. Then settle whether the shared RC/telemetry UART needs a contention model, with numbers
rather than a guess.

## Goals

- **#25** — `rf_loss_db` accepts a frequency curve as well as today's scalar, for world objects and mesh
  materials. The knife-edge cap uses the loss at the ray's frequency. The curve is **opt-in**: no shipped
  world changes, so no seed, digest or golden moves.
- **#64** — the ground bounce's reflected ray carries its own obstruction; one specular reflection off each
  world object's vertical box faces is added to the video path. Constants stay `estimated`.
- **#62** — the handset transmits on its antennas round-robin, one per packet; downlink diversity still
  picks the receive antenna. It is **test-only** for now: no shipped handset has a second antenna, so no
  shipped run or golden changes.
- **#63** — a time-boxed arithmetic spike measures the shared UART's occupancy from the code's real frame
  sizes and rules whether a half-duplex budget is worth building in a follow-up issue.

## Non-goals

- Digital video systems (#33), any new visual effect, or changes to the OSD/DVR.
- Full path tracing, multiple bounces, coherent object reflections, reflections off cylinders, meshes or
  terrain. One bounce, boxes only, powers summed incoherently — documented as the model's limits.
- Replacing the golden-section obstruction search (a thin post must not be stepped over).
- Converting a shipped world to a curve, or adding a dual-antenna handset part. Both are data/library
  decisions of their own; this wave only makes them possible.
- New external data: values without a source stay `estimated`.

## Constraints

- **Determinism.** No task adds or removes RNG draws. #62 changes antenna selection only in a test that
  constructs a two-antenna handset.
- **Backward compatibility.** `rf_loss_db = 25.0` stays valid; the curve form is additive. No world
  `schema_version` bump (a schema-1 reader still loads every existing file).
- **Test switches.** `ground_bounce` and `fading` stay; add `object_reflections` (off in unit-test `bare()`
  and radio, on in the simulator's video params) so the bare link-budget tests keep their meaning.
- **Goldens.** `#25` and `#62` move nothing. `#64` moves only a golden scenario whose ground-bounced path
  meets an obstacle; the PR names every changed digest, and an unexpected one is investigated, not
  regenerated. Windows locally; macOS through `macos-survey.yml`'s `write_goldens`; Linux on a Linux
  runner/WSL.
- **One PR per issue**, in the order below.

## Approaches and calls

**#25 — representation.** (a) One `rf_loss_db` field that accepts a scalar or `{ at_mhz, db }`; (b) a
second `rf_loss_curve` field beside the scalar; (c) named material profiles. **Pick (a)**: one field, the
world file stays obvious, and serde's untagged enum keeps every existing file working. (b) needs a
conflict rule; (c) is a library-wide change for a world-file concern.

**#64 — multipath.** (a) Per-ray obstruction plus one specular reflection off box vertical faces,
incoherent; (b) full image method for every shape with coherent phases; (c) per-ray obstruction only.
**Pick (a)**: it addresses both halves of the issue, matches the shipped buildings (boxes), and stays
testable and cheap. (b) turns into path tracing for cylinders/meshes and manufactures nulls the material
model cannot justify; (c) leaves "reflections off objects" unbuilt.

**#62 — handset schedule.** (a) Round-robin per packet over the handset's antennas; (b) alternate per
direction (bind); (c) keep following the best downlink. **Pick (a)**: it is the behaviour the issue names,
one counter, and the fading statistics change in the way real dual-antenna handsets do.

**#63 — contention.** (a) Measure first, then rule; (b) model a half-duplex budget now; (c) ignore it.
**Pick (a)**: the issue asks to decide; a UART occupancy calculation is cheap and the default telemetry
ratio may make this a non-issue. (b) without numbers risks a model that costs packets for no visible
reason. "Measure" here is arithmetic and code reading, not a probe: the radio model already schedules the
downlink slot, so an instrumented capture would only reproduce its own schedule.

## Design

### #25 — frequency-dependent `rf_loss_db`

World syntax (inline table; arrays keep the TOML readable):

```toml
rf_loss_db = 25.0                                              # today: the same at every frequency
rf_loss_db = { at_mhz = [900.0, 2400.0, 5800.0], db = [12.0, 18.0, 25.0] }
```

- `ofs-rf` gains `RfLoss`: `Db(f64)` or `Curve(Vec<(f64, f64)>)`, with `db_at(freq_mhz)`. Interpolation is
  **log-log** (a power law between points, the shape ITU-R P.2040 fits use), clamped at both ends (no
  extrapolation). `From<f64>` keeps the scalar construction sites readable.
- `Obstacle.rf_loss_db: f64` becomes `rf_loss: RfLoss`; `MeshObstacle.loss_db: Vec<f64>` becomes
  `Vec<RfLoss>`. `Obstacle` loses `Copy` (it keeps `Clone`); its call sites and tests are adjusted.
  `obstruction_loss_db` and `mesh_loss_db` take `freq_mhz` (the caller already has it).
- Config: `ObjectSection` and `MaterialSection` keep the field name with an untagged scalar-or-table type.
  Validation: at least two points, equal lengths, frequencies finite and strictly increasing, dB finite and
  non-negative; errors name the field and entry.
- **Opt-in data.** No shipped world changes. A test world/fixture carries the sourced curve (ITU-R P.2040
  material ratios at 900/2400/5800 MHz, worked out in the task and written up in
  `docs/research/video-link.md`). `rotated_boxes` and `flat_world_pin` keep their digests; any movement is a
  bug in the representation, not a baseline to re-record.
- **Serialized value.** The state/proto/Python/Godot scalar stays a double and carries the curve's
  **5800 MHz** value (the video band), so a curve-authored world still shows a meaningful number; no
  protocol bump. `world_obstacles` includes a curve with any positive point (`RfLoss::max_db() > 0`), and
  skips an all-zero curve, as the scalar filter does today.
- Docs: the world-object/material rows in `docs/dev-setup.md`, the material row in
  `docs/research/video-link.md`, and the roadmap entry.

### #64 — per-ray obstruction and object reflections

- **Shared path function.** `ofs-video::vtx_paths` and `interference_dbm` do not call `path_gain` today
  (they subtract one `obstruction_db` from `unobstructed_gain_db`). They switch to `path_gain` with a new
  `Multipath { ground_bounce, object_reflections }` argument; `ofs-radio::elrs` passes the same struct with
  `object_reflections` false. Object reflections never enter `obstruction_db`, whose meaning for the tests
  (the direct ray's knife-edge, including terrain) is unchanged.
- **Per-ray obstruction.** The direct ray takes `obstruction_db(tx, rx)`; the ground ray takes
  `obstruction_db(tx, bounce) + obstruction_db(bounce, rx)` for **objects and meshes only**. Terrain keeps
  today's whole-path term, applied once to the combined direct/ground power: the bounce point lies on the
  ground, so splitting the heightfield at it would double-count and blow up at the endpoint. On flat
  worlds this is invisible; `terrain_link.rs` keeps its numbers.
- **Object reflection.** For each `Obstacle` whose shape is a box, test its four vertical faces: mirror the
  transmitter across the face plane, intersect the mirrored line with the face, accept the point only if it
  lies inside the face rectangle and on/above the ground; the reflecting object is excluded from its own
  reflected ray's obstruction, every other obstacle and mesh still applies. The ray uses
  `tx.antenna.polarization.reflected()` (a circular reflection reverses hand, as the ground bounce does)
  and a constant `WALL_REFLECTION_DB` (estimated) for the surface; its power is added incoherently to the
  direct/ground power. Cylinders, meshes and terrain don't reflect (documented limits).
- **Flag.** Both `LinkParams` structs gain `object_reflections`: the simulator's video params set it true,
  the radio params and unit-test `bare()` false. The radio's values move only through the per-ray fix.
- **Cost.** The reflection point depends on the two positions, not the antenna pattern, so the geometry can
  be shared across the goggles' antennas. A handful of faces per field at 50 Hz.
- **Golden acceptance.** Before regenerating, list every changed `ofs-radio`/`ofs-sim` digest. Only a
  scenario whose bounced path meets an obstacle may move (the `obstacle-*` cases); `two-antennas-fading`
  and a bounce with a clear path must not. An unexplained digest is investigated first.
- Docs: the propagation/multipath rows and notes in `docs/research/video-link.md`, `docs/roadmap.md`.

### #62 — handset antenna per packet

- `ElrsLink` keeps a `tx_antenna` index, starting at 0. Each packet transmits on
  `handset_antennas[tx_antenna]`, then advances it round-robin. `handset_diversity.choose(downlink)` still
  chooses the receive antenna for the downlink; it no longer chooses the transmit antenna.
- No shipped handset has a second antenna, so no shipped run or golden moves and no handset part is added.
  A test constructs two handset antennas, one shadowed and one clear, asserts both are used on alternate
  packets, and that the link's quality reflects the mix. A test-only accessor exposes the schedule.
- Docs: the handset-antenna paragraph in `docs/research/elrs-link.md`, roadmap.

### #63 — UART contention spike

- No instrumented probe: the radio model already emits the downlink slot, so a capture would only
  reproduce its own schedule. The spike reads the real paths instead: the CRSF frame the link writes
  (`crates/ofs-radio/src/elrs.rs` and its tests), the UART baud in the SITL diff (115200) and the FC's
  write path (`crates/ofs-fc/src/sitl/bridge.rs`).
- Compute the occupancy: bytes per frame at the model's real frame length, the packet slot at 500 Hz
  (2 ms), the telemetry slot divisor, and the time a frame needs at 11.52 bytes/ms. Decide with a stated
  threshold (model if the worst case leaves the RC frame unable to fit its slot or drops appear at default
  settings; otherwise record the arithmetic).
- Deliverable `docs/research/uart-contention.md`: the numbers, the code paths, and either a "not modelled"
  ruling added to `docs/roadmap.md`, or a follow-up issue describing a half-duplex budget on the
  `Receiver` (baud, frame lengths, telemetry ratio, queue/delay/drop). This wave builds no model.

## Testing

- Each task adds unit tests at its seam (`ofs-rf/tests/propagation.rs`, `ofs-video/tests/link.rs`,
  `ofs-radio/tests/elrs.rs`) and runs clippy and the full workspace tests.
- #25: the scalar reproduces today's values exactly; the log-log curve interpolates and clamps; invalid
  tables are rejected; a curve with a positive point is an obstacle and an all-zero curve is not; the two
  bus-digest pins stay unchanged.
- #64: a wall on only the bounced path costs the bounce, not the direct ray; a side wall adds a ray; a
  face-outside/yaw case is rejected; `object_reflections = false` reproduces today's gain; a cylinder does
  not reflect; the terrain numbers do not move; the golden diff names only the bounced-obstacle scenarios.
- #62: the schedule test above; no golden moves.
- #63: the research note's numbers are reproducible from the frame sizes and baud it cites.
- No Godot or DVR changes, so no new manual check; the picture's only change is a few dB where a wall can
  reflect.

## Delivery order

1. **#25** (PR 1) — the representation and the sourced test fixture; no shipped values move.
2. **#62** (PR 2) — the handset schedule and its test; no golden moves.
3. **#64** (PR 3) — the shared `path_gain`, per-ray obstruction, object reflections, the video flag, the
   propagation docs and the named golden moves.
4. **#63** (PR 4) — the arithmetic spike and the ruling.
