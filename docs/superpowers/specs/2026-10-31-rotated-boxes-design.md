# Rotated boxes in the world (#28) — design

Epic #160 (Consequence), the representation lane: `#160` orders representation before consequence, and the
Game-completion phase plan (`docs/superpowers/plans/2026-10-31-game-completion.md`, merged as 48c7831) names this the
wave's first unstarted item, numbering its wire work from protocol 16 (the worktree holds 16; `#169` is merged).
Planning note: the phase umbrella this spec respects
(`docs/superpowers/plans/2026-10-31-consequence-completion.md`) was still uncommitted when this was written; it ships
in the same PR beside this pair — read it; nothing here contradicts it.

## Intent

A world's box objects stand axis-aligned. A box gains a yaw — a heading about the vertical — and the three consumers
the issue names agree on it: the collision solver pushes along the rotated faces, the RF signed-distance obstruction
sees the rotated solid, and the Godot client draws the rotated box. A world that writes no yaw is bit-for-bit today:
same shapes, same flights, same digests, no golden moves.

## Goals

- `yaw_deg` on a box in the world file (absent = axis-aligned), validated like every other field.
- One geometry: `ofs_core::shape::Shape` carries the yaw, and every consumer reads the same signed distance — no
  consumer-side rotation code.
- The Godot client receives the yaw (`WorldObject.yaw_deg`, protocol 17) and turns the box by it.
- The shipped meadow gets back the gate bar its header says it is missing (`worlds/meadow.toml:2-4,64`).
- A world without a yaw on any box is bit-identical to today (the object-world digest pin proves it).

## Non-goals and anti-goals

- Pitch and roll for boxes (a tilted roof): out of scope; `[[gates]]` already carry `pitch_deg`/`roll_deg` where a
  tilt in the vertical plane is wanted (`worlds/meadow.toml`'s Gate2 frame stays unbuildable and says so).
- Rotated cylinders: a vertical cylinder is invariant under yaw; `yaw_deg` on one is a validation problem, not a
  feature.
- Anti-goal: any bit of change to a world that writes no yaw — not its shapes, its flights, its bus digest, its
  signal list, its goldens (`crates/ofs-radio/tests/golden/*`, `crates/ofs-sim/tests/golden/*`), or the flat-world
  pin (`crates/ofs-sim/tests/flat_world_pin.rs`).
- No new bus signal, no new world-file section, no new dependency.

## The rotation: yaw only

**Ruling: one angle, `yaw_deg`, degrees clockwise from north (0 = north, 90 = east) — the convention of
`pilot.facing_deg` and `gates[].heading_deg`.** The issue's "and perhaps full orientation" is declined:

- Everything the issue lists — collision, the RF obstruction, the rendering — is a box turned to a heading; no
  consumer needs a tilt.
- `Shape::rooted_on` (crates/ofs-core/src/shape.rs:31-46) roots an RF obstacle by growing it straight down from its
  top to `ROOT_DEPTH_M`, so nothing diffracts underneath. Yaw moves no vertical extent, so the rooted shape is still
  a rotated box and the guarantee is exact. A tilted box's rooted shape is not a box (it is a prism over a rotated
  footprint with a tilted top), and faking it would break the "never underneath" rule the RF search relies on.
- `#160` orders representation before consequence: the minimum that serves collisions and the link is the heading.

## The representation

### `Shape::Box` gains `yaw_rad: f64`

`Shape::Box { center, half }` becomes `Shape::Box { center, half, yaw_rad }` (radians inside the sim; degrees in the
file, like the gate). Twenty-two `Shape::Box {` construction sites across eight files gain `yaw_rad: 0.0` (shape.rs's
own five match arms are the implementation) — the compiler finds them (the pattern that shipped the terrain's
`AirframeParams.terrain`).

With `c = yaw.cos()`, `s = yaw.sin()`, `rel = p - center`, the query's local frame is the north/east pair rotated
back: `local_n = c*rel.x + s*rel.y`, `local_e = -s*rel.x + c*rel.y`, `local_d = rel.z`.

- **`signed_distance`**: the existing box formula (shape.rs:50-53) on `(local_n, local_e, local_d)` against `half`.
- **`normal`**: the existing box logic (shape.rs:69-81) on the local coordinates gives a local normal; its north and
  east components rotate forward: `world_n = c*ln - s*le`, `world_e = s*ln + c*le`; `d` passes through.
- **`bounds`**: the exact axis-aligned bounds of the rotated box —
  `half_world = (|c|*half.x + |s|*half.y, |s|*half.x + |c|*half.y, half.z)`. Exact, not merely conservative: the
  extremes of a rotated rectangle are its corners. The collision broad phase (crates/ofs-physics/src/rigid_body.rs:97)
  still works: a bigger box grown by the quad's radius still contains the quad's centre.
- **`rooted_on`**: unchanged arithmetic, and the rooted box keeps the yaw — only `half.z` and `center.z` change, so
  the prism over the rotated footprint is again a rotated box.

A rotated box is convex, so `deepest_point`'s golden-section search over the signed distance
(crates/ofs-rf/src/propagation.rs:160-179) still finds the deepest point exactly, "however thin the object and however
long the path".

### The identity fast path (the bit-for-bit rule)

Every geometry method branches first: `if yaw_rad == 0.0` runs today's expressions unchanged, else the rotated ones.
`0.0 == -0.0` is true, so a `-0.0` yaw is identity too. The guarantee is the branch itself, not an argument about
IEEE rounding — the same style as the terrain's "no terrain returns exactly `(0.0, NEG_Z)`"
(docs/superpowers/specs/2026-10-31-terrain-heightfield-design.md, "Physics").

### Canonicalization at the config boundary

`object_shape` (crates/ofs-sim/src/vehicle.rs:753) maps the file's degrees through
`yaw_deg.rem_euclid(360.0).to_radians()`. `rem_euclid` is `fmod` plus a conditional add — exact — and
`0.0f64.to_radians()` is exactly `0.0`, so a file's `0`, `-0.0`, `360` and `720` all take the identity path; the raw
file value is untouched (`ObjectSection` keeps what the author wrote). Inside `Shape` the yaw is canonical
(`[0, 2π)`), and only the exact-zero branch matters.

## The world file

`ObjectSection` (crates/ofs-config/src/world.rs:341-360) gains, with `#[serde(default)]`:

```toml
[[objects]]
name = "Gate1Bar"
shape = "box"
center_ned_m = [60.0, 45.0, -5.075]
size_m = [8.3, 0.15, 0.15]   # the box's own north, east and height axes
yaw_deg = 135.0              # the box's north axis points at this heading (0 = north, 90 = east, clockwise)
color = [0.2, 0.8, 0.35]
```

Validation, in the objects loop of `WorldConfig::validate` (world.rs:543-561), field paths like the neighbours:

- `yaw_deg` finite (`objects[i].yaw_deg: must be finite`) — the house angle rule; `gates[].heading_deg` and
  `pilot.facing_deg` check finiteness only, no range, because `sin`/`cos` take any angle.
- `yaw_deg` on a cylinder is a problem (`objects[i].yaw_deg: yaw_deg is for a box`), mirroring the existing
  cross-field rules (`world.rs:553` "radius_m and height_m are for a cylinder", `world.rs:558` "size_m is for a box").

## The consumers: which rotate, and why nothing else does

Everything downstream of `Shape` rotates because everything downstream reads the signed distance and the normal from
the one enum — shape.rs's own doc (crates/ofs-core/src/shape.rs:1-3) already says so:

- **Collision spheres**: `collect` (crates/ofs-physics/src/collision.rs:208-222) calls `signed_distance` and `normal`
  per sphere per near object.
- **Landing points**: `RigidBody::contact` (crates/ofs-physics/src/rigid_body.rs:189-193) calls the same pair per
  point.
- **Broad phase**: `bounds()` at build (rigid_body.rs:97).
- **The ELRS link and the video link**: `world_obstacles` (crates/ofs-sim/src/vehicle.rs:744-751) hands the same
  shapes, rooted, to `obstruction_loss_db` (propagation.rs:182-191), whose golden-section search stays exact on the
  convex rotated box. Both links route through `LinkParams.obstacles` and `VideoLinkParams` (vehicle.rs:703-706,
  780), so one field rotates both.
- **The Godot client**: through the wire (below); it draws, never computes.

There is no remaining consumer: the collision events name objects by index (`collision.rs:230-253`), untouched; the
Python client parses, never draws; the repro bundle already carries the world file's bytes (crates/ofs-sim/src/repro.rs:52-53).

## The wire: protocol 17

The Godot client must draw the same box the physics collides, and the `World` message is the only path, so the yaw
travels: `double yaw_deg = 9;` on `WorldObject` (proto/ofs/v1/sim.proto:429-438; fields 1-8 are taken). Zero (the
proto3 absent value) is axis-aligned. Protobuf additives are wire-compatible, but the house bumps on every message
change — protocol 15 (courses) and 16 (terrain, sim.proto:5) were additive and bumped — because the handshake
(crates/ofs-proto/src/lib.rs:12, `PROTOCOL_VERSION`) refuses mismatched clients rather than letting them misread.
Protocol 17 is a breaking change; the regeneration choreography (proto header, both version constants, the version
test, the Python stubs, the stale "protocol 16" grep) is a plan task, mirroring m8a's Task 4
(docs/superpowers/plans/2026-10-17-m8a-gates-and-laps.md).

Chain: `world_msg` fills it (crates/ofs-sim/src/session.rs:244-259) → `WorldObject` in the Rust client
(crates/ofs-client/src/model.rs:188-202, `from_pb` at :364-380) → the Godot bridge dict key `"yaw_deg"`
(crates/ofs-godot/src/lib.rs:480-491) → `_box` turns the node (godot/world/world.gd:61-66, 239-242). The Python
`WorldObject` dataclass gains `yaw_deg: float = 0.0` (python/ofs/client.py:134-138, `_world` at :339-347).

## The digest: no change

The lockstep digest is the bus digest — every registered signal's name and bits (crates/ofs-core/src/bus.rs:130-145).
Terrain entered it through the state it shapes plus its conditional `body.terrain_d_m` seam; an object's yaw, like its
position and its `rf_loss_db` before it, enters only through the signals it shapes: body state through the contacts,
collision events, the link signals through the obstruction. **Ruling: no new signal, no digest change.** No consumer
needs a yaw signal; a world with no rotated box registers nothing new, so every world's signal list — flat worlds'
included — is untouched, and the flat-world pin and the radio goldens hold by construction. The repro bundle already
carries the world file, so a rotated world's reproduction is complete.

## Determinism, and what a macOS difference cannot break

Non-zero yaw adds `sin`/`cos` inside the SDF — libm, whose last bits differ per OS. The terrain design's split
(docs/superpowers/specs/2026-10-31-terrain-heightfield-design.md, "What the pins protect") applies verbatim:

- The identity path (every world that writes no yaw) computes no trig — it is today's code — so the object-world pin
  and every existing golden are exact on every OS.
- The lockstep digest is compared run-against-run on one machine (`--replay`, the determinism tests), never against
  another OS's constant, so a yawed world replays exactly everywhere.
- The radio goldens are per-OS files already (crates/ofs-radio/tests/golden/) and run over `open_field`, which has no
  objects (crates/ofs-sim/tests/radio_golden.rs:29) — they do not move.
- No new test compares a libm-dependent value of a *yawed* world against a cross-OS constant; the yawed-world tests
  compare same-machine runs or values within a tolerance.

## Godot

The client hands Godot the box's size already reordered to Godot's frame — `(east, height, north)`
(crates/ofs-client/src/model.rs:369) — so the mesh's local +Z carries the box's north extent. A NED yaw `h` points
that axis along the Godot image of NED heading `h`: `(sin h, 0, -cos h)`; `rotation.y = PI - deg_to_rad(h)` puts a
node's local +Z exactly there (the formula m8a used for gates, pinned by `test_scene.gd`'s basis assertions ever
since). `_box` gains a defaulted `yaw_deg := 0.0` and sets `instance.rotation.y`; yaw 0 sets exactly `0.0`, so
today's drawing is unchanged.

## The demonstration: meadow's Gate1 bar

`worlds/meadow.toml:64` — "Gate1 is the diagonal (heading 45): posts across the travel, no bar (a bar would need a
rotated box)" — is this issue's own motivating note, so meadow is the shipped proof. Gate1 is at
`center_ned_m = [60.0, 45.0, -2.5]`, `heading_deg = 45.0`, opening 8 m × 5 m (meadow.toml:181-186); its posts stand
at (57.12, 47.88) and (62.88, 42.12) (meadow.toml:65-77). The bar, Gate0's proportions (meadow.toml:57-63):
`center_ned_m = [60.0, 45.0, -5.075]`, `size_m = [8.3, 0.15, 0.15]`, `color = [0.2, 0.8, 0.35]`,
`yaw_deg = 135.0` — the long axis across the travel, heading + 90. The header's line about Gate1 is rewritten; the
Gate2 (pitched) note stays. `the_meadow_world_loads` (crates/ofs-config/tests/world.rs:103-110) asserts the bar and
its yaw. No other shipped world changes.

## Tests

- **ofs-core** (`crates/ofs-core/tests/shape.rs`): a 90-degree-yawed box has the signed distance and normal of the
  swapped-extents box (within 1e-12; `cos(π/2)` is 6.1e-17, not 0); a 45-degree face normal is the rotated axis; the
  exact AABB at 30/45/90/210 degrees contains the rotated corners and matches `(|c|hₓ+|s|h_y, …)`; `rooted_on` keeps
  the yaw and the top; `yaw_rad = 0.0` and `-0.0` give today's values on the same points the existing tests pin.
- **ofs-config**: `yaw_deg` parses (default `None`), `Some(0.0)` round-trips; non-finite is a problem at
  `objects[i].yaw_deg`; a cylinder with `yaw_deg` is a problem; meadow's bar carries `Some(135.0)`.
- **ofs-physics**: a sphere driven into a 45-degree-yawed wall rebounds along the rotated face normal (gains the
  sideways component), in the rig style of `a_rotated_hit_turns_the_body_through_the_body_frame_inertia`
  (crates/ofs-physics/tests/collision.rs:242).
- **ofs-rf**: `deepest_point` bottoms out on a yawed wall where its faces are: a 90-degree wall's obstruction loss
  equals the unrotated swapped-extents wall's within 1e-12, and differs from the same wall unrotated.
- **ofs-sim**: the object-world pin (below); a scripted flight into a 45-degree wall raises the collision event and
  turns the velocity; a yawed world replays to the same digest twice and a different yaw differs;
  `world_objects`/`world_obstacles` carry the yaw (unit-test style of vehicle.rs:2292-2321); `world_msg` carries
  `yaw_deg`.
- **The pin** (`crates/ofs-sim/tests/rotated_boxes.rs`, constants recorded on `main` before any change): the race
  quad, open loop, seed 1, a synthetic world whose box stands in the climb path — the flight collides
  (`collision_event_count >= 1` pins that it exercises the box) — digest after the hit. Per-OS constants like the
  flat-world pin (crates/ofs-sim/tests/flat_world_pin.rs:14-21); macOS compiled out.
- **Godot** (`godot/tests/test_scene.gd`): a yaw-90 box node's long axis lies along +X (east); yaw 0 is exactly the
  unrotated node; an empty world still clears.

## Docs and bookkeeping

`CHANGELOG.md` (Unreleased, the neighbours' style); `docs/dev-setup.md` — the `[[objects]]` line (:118) gains
`yaw_deg`, the protocol walk (:268) moves to 17 with the version list (:284-285); `docs/research/collision.md` — a
rotated box's contact maths and the identity rule; `docs/roadmap.md` line if it lists axis-aligned boxes;
`python/README.md` protocol number.

## Calls made without the user (rulings)

1. Yaw only, not full SO(3): every consumer is served, and `rooted_on`'s straight-down RF rooting is exact only with
   the vertical axis fixed.
2. `Shape::Box { center, half, yaw_rad }` with an exact `yaw_rad == 0.0` fast path — bit-identity by construction.
3. Canonicalize at `object_shape` via `rem_euclid(360).to_radians()`; validate finite-only plus the cylinder
   cross-field rule.
4. Protocol 17 with `WorldObject.yaw_deg = 9` — the house bumps on every wire change.
5. No digest change, no new signal: rotation rides the signals it shapes, like position and `rf_loss_db`.
6. Meadow's Gate1 gets its bar (`yaw_deg = 135`); no other shipped world changes.
7. The builder-check subagent was skipped: this design is issue #28's own, plus the rulings recorded here, and the
   task asked for no mid-plan questions.

## Open questions for the maintainer

None. The declined alternative (full orientation) is recorded in the rulings; reopen it with the tilted-frame need
when a world wants one.
