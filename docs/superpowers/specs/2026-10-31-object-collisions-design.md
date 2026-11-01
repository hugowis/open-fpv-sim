# Collisions between objects (#156, #29 part 1 of 2)

Epic #160 (Consequence: real terrain, crash damage and sound). Part 2 (#157, crash damage) rides the seams this
design leaves; #28 (rotated boxes) is the representation sibling that may land first — everything here consumes
`ofs_core::shape::Shape` through its signed distance and normal, never an axis-aligned assumption, so rotation
composes without a change here.

The phase umbrella plan (`docs/superpowers/plans/2026-10-31-consequence-completion.md`) was still uncommitted when this
was written; it ships in the same PR beside this pair. The epic #160 body is the binding contract this design follows:
representation before consequence, one source of truth for world data, a world that does not opt in runs
bit-for-bit as today, damage on the existing fault seams, video stays analog.

## Intent

Today the quad collides — with the ground and with the static `[[objects]]`, by impulse, since M3c
(`crates/ofs-physics/src/collision.rs::resolve`), and a session test asserts a new collision count raises one
`COLLISION` event naming the object (`crates/ofs-sim/src/session.rs`, `a_new_collision_count_raises_one_collision_event_naming_the_object`).
Objects never move and never touch each other (`docs/roadmap.md` §Physics says so). This change gives the world
data **ballistic objects** — crates that fly, hit the ground, hit statics, hit each other, and hit the quad and
are hit back — and generalises the collision events to object–object pairs. The prop strike is the valuable core:
the quad's generated prop-tip spheres (2 cm, six per prop, `crates/ofs-config/src/lib.rs::collision_spheres`) are
already the contact primitives, so a crate crossing a prop disc is a crash with a speed, exactly the seam #157's
damage listens to.

## What collides with what — the ruling

| Pair | Part 1 | Law |
|---|---|---|
| Quad ↔ static object | already ships (M3c) | impulse, object infinite mass |
| Quad ↔ ground/terrain | already ships | spheres impulse, legs spring-damper |
| **Quad ↔ dynamic object** | **ships** | impulse with momentum exchange (finite object mass); existing `COLLISION` event, existing semantics |
| **Dynamic ↔ ground/terrain** | **ships** | impulse; lands, bounces at restitution, then rests inert |
| **Dynamic ↔ static object** | **ships** | impulse, static infinite mass; `OBJECT_COLLISION` event |
| **Dynamic ↔ dynamic** | **ships** | impulse split by inverse masses; `OBJECT_COLLISION` event |
| Static ↔ static | never | both infinite mass: nothing to resolve, nothing to report, no cost paid |
| Prop ↔ prop (two aircraft) | ruled out | one vehicle per session; multi-vehicle lockstep is a scheduler/journal/protocol change, not contact maths — the flagged maintainer question below |
| Objects resting, rolling, tumbling | ruled out | a landed object is inert (no per-tick resting contact for objects, no angular state): the per-tick cost and the state surface buy no game at these counts |
| Legs (landing points) ↔ dynamic object | ruled out | the quad's spring-damper legs ignore dynamic objects; the spheres carry all quad↔object contact ("hits get impulses", `docs/research/collision.md`). A quad parked on a crate rides its spheres — stable but hard, like any impulse rest |
| Scripted (kinematic) motion | ruled out of part 1 | the done-when's "hit it back" needs finite mass; a prescribed-velocity object is a second motion law with no consumer in the shipped worlds. The `motion` table's `kind` is the seam |
| RF/video obstruction after a move | ruled out of part 1 | the obstruction search keeps the declared pose (`vehicle.rs::world_obstacles` bakes it at build); the moving-object signals are the seam. A documented limit, not a silent one |
| Gate openings | never colliders | unchanged: a gate is a trigger (`docs/race-timing.md`); its posts and bar are ordinary objects and participate as such |

## The representation

### `[objects.motion]` in the world file (`crates/ofs-config/src/world.rs`)

An object that moves carries a sub-table; every object without one is exactly today's static:

```toml
[[objects]]
name = "Crate"
shape = "box"
center_ned_m = [12.0, 0.0, -30.0]
size_m = [0.4, 0.4, 0.4]
color = [0.55, 0.4, 0.2]

[objects.motion]
kind = "ballistic"          # the only kind in part 1
velocity_ned_mps = [0.0, 4.0, 0.0]
mass_kg = 0.5
```

`MotionSection { kind: MotionKind (Ballistic), velocity_ned_mps: [f64; 3] (default zero — a drop), mass_kg: f64 }`,
`deny_unknown_fields`. Validation (`WorldConfig::validate`, field paths `objects[i].motion.<field>`): every number
finite; `mass_kg` in (0, 100]; the object's centre not below the terrain (the pilot's rule, `check_terrain`'s
`below`); at most `MAX_MOVING_OBJECTS = 8` movers per world. The objects list keeps file order — an object's
index is the event naming's index space, static or moving alike.

### `ofs-physics`: the dynamic objects

`WorldObject` stays `{ name, shape }`; the params list stays **all** objects in file order (so the touch slots and
the `body.collision_object` index space are unchanged), and a new parallel list names the movers:

```rust
// crates/ofs-physics/src/objects.rs
pub const OBJECT_RESTITUTION: f64 = 0.3;      // estimated, like every collision constant
pub const OBJECT_FRICTION: f64 = 0.5;         // estimated
pub const LANDED_SPEED_MPS: f64 = 1.0e-3;     // below this while touching: inert
pub struct DynamicObject { pub object: usize, pub mass_kg: f64 }   // index into AirframeParams.objects
#[derive(Clone, Copy)]
pub struct DynamicState { pub pos_ned_m: DVec3, pub vel_ned_mps: DVec3, pub landed: bool }
```

`AirframeParams` gains `dynamics: Vec<DynamicObject>` (empty by default). The state lives in the `RigidBody` and
on the bus (below); the motion itself is a pure function `objects::step(...)` the `RigidBody` calls, so it tests
without a scheduler.

## The physics

One owner, one clock: the `RigidBody` model steps the objects inside its own step, in a fixed order, so the same
journal replays bit for bit. Per tick, after the quad's integration and before the quad's contact resolution:

1. **Integrate** every not-landed mover: gravity only, semi-implicit Euler (`vel += g·dt; pos += vel·dt`), the
   same discipline as the body. No drag (a crate's drag is a footnote at these speeds; no new constants).
2. **Object contacts.** A mover's contact points are its shape's — a box its 8 corners, a cylinder 8 points on
   each rim — in a fixed order, tested against the ground (`collision::ground_at`, terrain included), every
   static (broadphase: `Aabb::intersects`, a new `ofs_core::shape` helper), and every other mover (likewise;
   pairs in index order). Not the quad: that pair is step 3's. Each penetrating point gets depth (−signed
   distance, or the terrain expression), outward normal, and the object's inward speed (a mover has no rotation,
   so the point's velocity is the object's). Position correction translates the object out (deepest first, the
   same sweep discipline as the quad's); then one normal impulse per contact `j = −(1+e)·v_n / k` with
   `k = 1/M` against an infinite-mass solid (restitution 0 below the existing 0.2 m/s floor) and
   `k = 1/M_a + 1/M_b` between movers, split by inverse mass, plus a Coulomb friction impulse capped at `μ·j`.
   A mover whose speed falls below `LANDED_SPEED_MPS` while touching anything is **landed**: velocity zeroed, no
   further stepping, until a `Load` rebuilds the world. One throw per `Load`.
3. **Quad contacts** (`collision::collect`/`resolve`, unchanged for statics): against a mover, the sphere's
   centre tests the mover's shape at its *this-tick* pose, the approach speed is **relative** (the sphere
   surface point's velocity minus the object's velocity), the pair's restitution and friction are the quad's
   `[collision]` values (semantics preserved), and the impulse's effective mass gains the object's term:
   `k = 1/m_eff + 1/M`. The quad takes the impulse as today; the mover takes `−j·n/M` on its velocity, applied
   this tick. The touch memory is the existing `TouchState` — a mover's slot is its world-objects index — so a
   crate hitting the quad raises the existing event through the existing signals with the existing message.

## Events, and the seam for #157

- **Quad events unchanged**: `body.collision_speed/object/count`, `State.collision_speed_mps`,
  `"HIT <name> <s> m/s"` / `"HARD LANDING <s> m/s"`, one per touching spell, 1 m/s threshold, 20 ms rearm.
  A dynamic object is "the object" like any static.
- **Object–object contacts** (mover against ground, static or mover — no quad) raise `EVENT_KIND_OBJECT_COLLISION
  = 27` with the same thresholds and rearm, messages `"<mover> hit <other> <s> m/s"` and
  `"<mover> hit the ground <s> m/s"` (one decimal, the existing style). They travel the way race events do: the
  `RigidBody` pushes `WorldCollision { mover, other, inward, impulse }` records into a mutex handle
  (`crates/ofs-sim/src/race.rs`'s `RaceHandle` pattern), and `Session::changes` drains it after the race events.
  A handle, not a state diff: two crates landing in one window are two events.
- **The seam for #157 (damage, not built here)**: the internal collision records grow the resolved **normal
  impulse** and the contact's identity — which sphere (its FRD centre; the generated prop spheres map to motors
  at build) for quad events, which pair for object events. Nothing of it is on the wire in part 1;
  `collision_speed_mps` keeps its meaning. #157 reads the impulse per part where it is computed and owns its own
  protocol bump for whatever it surfaces.

## The digest and the goldens

The lockstep digest is the bus digest — every registered signal's name and bits, name order
(`crates/ofs-core/src/bus.rs::digest`). So the movers' state lives **on the bus**, and only in worlds that have
movers: three signals per moving object — `world.<name>.pos_ned_m` (Vec3), `world.<name>.vel_ned_mps` (Vec3),
`world.<name>.landed` (scalar) — registered in `RigidBody::new` only when `dynamics` is non-empty and published
with the initial pose, the same opt-in trick as `body.terrain_d_m` (`crates/ofs-physics/src/rigid_body.rs`).
The recorder's default table picks them up by pattern; a replay or a lockstep twin that disagreed on a crate
would disagree on the digest at the first divergent tick, before the crate ever touched the quad.

A world without movers registers nothing new, and every new loop is over an empty list: `world_objects()` still
returns all objects in file order (so a mover-less world's list is exactly today's), the movers' step is one
length check, and the quad's contact code is unchanged for it. The pins that prove it: the flat-world pin
(`crates/ofs-sim/tests/flat_world_pin.rs`, `WorldConfig::open_field()` — no objects) and the radio goldens
(`crates/ofs-sim/tests/golden/radio-flight.*.digests`, whose flight also uses `open_field()`), both untouched. A
mover-carrying world is new state in the digest by design. **A moved golden is a bug.**

## The wire (protocol 17) and the clients

Protocol 16 is current (`crates/ofs-proto/src/lib.rs`, `python/ofs/client.py`); this bumps to 17:

- `WorldObject.motion = 9` → `Motion { string kind = 1; Vec3 velocity_ned_mps = 2; double mass_kg = 3; }`
  (absent on static objects).
- `State.moving_objects = 21` → `MovingObject { string name = 1; Vec3 position_ned_m = 2; Vec3 velocity_ned_mps = 3;
  bool landed = 4; }`, empty when the world has no movers — a mover-less world's `State` and `World` are
  byte-identical to protocol 16's.
- `EVENT_KIND_OBJECT_COLLISION = 27` (26 is the last kind today).

The world data is the single source of truth for the motion's declaration; the `State` stream carries the truth
of where the crate is (a ballistic bounce is contact-dependent — no client may re-derive it). Clients: the Rust
client (`crates/ofs-client/src/model.rs`) gains the motion and the moving objects and the event kind; Python
(dataclasses with defaults, `_event` names `"object_collision"`) after the stubs regenerate; Godot's `world.gd`
moves the existing `Object_<name>` node from each state's moving-object entry (the NED→Godot frame the build
already uses), and `app.gd` toasts the new event.

## Performance

The broadphase is AABB pre-tests (`Aabb::intersects`) plus skipping landed movers — no spatial index: at the
shipped counts (park: 14 statics + 1 mover; the cap 8 movers × 30 statics) the sweep is a few hundred compares
per tick beside a contact solver that already runs 25 spheres × 30 objects. Evidence, not assertion: a
`scripts/benchmark.py` row over the mover-carrying shipped world (a row with no baseline entry is reported, never
gated — the m8c rule), and an `#[ignore]`d release timing test in the style of `terrain_costs_little_in_release`
comparing the mover world against its twin without the mover. Wall-clock tests are not CI gates.

## Tests

- `ofs-config`: the motion table parses; absence is static; every validation rule (non-finite, mass 0 and 100,
  mover below terrain, 9 movers); file order preserved.
- `ofs-physics` (pure `objects::step`, no scheduler): a drop lands on the plane and on a 20-degree ramp at the
  terrain's surface; a bounce restores e² of the height; a mover off a post rebounds; two movers exchange
  momentum (centre-of-mass velocity conserved); a landed mover stays put; the quad's sphere against a mover
  conserves momentum and names the touch.
- `ofs-sim`: the three signals exist only in a world with movers; the flat-world pin still passes unmodified; a
  mover flight run twice gives one digest and differs from the mover-less twin; the quad hit by a crate raises
  the existing `COLLISION` event naming the crate; two crates landing in one window raise two
  `OBJECT_COLLISION` events with the documented text.
- `ofs-proto`: version 17 in the three places; `grpc.rs` round-trips the motion, the moving objects and the event.
- Godot: `world.gd` moves the node to the state's pose; the empty list is a no-op; the toast appears.

## Docs and bookkeeping

`CHANGELOG.md` (Unreleased — this issue); `docs/research/collision.md` gains "Moving objects" (the law, the
limits — point-mass movers, legs ignore movers, resting is inert, RF keeps the declared pose — and the constants
with provenance); `docs/roadmap.md` §Physics line "objects never collide with each other" replaced; 
`docs/dev-setup.md` world-file section lists `[objects.motion]`; `python/README.md` events and state;
`worlds/park.toml` ships one crate (below).

## Calls made without the user (rulings)

1. The collision matrix above: quad↔mover, mover↔ground, mover↔static, mover↔mover ship; everything else is out
   with the stated reason.
2. Ballistic only, one throw per `Load`, translation only, no drag; a prescribed-velocity `kind` is the seam.
3. Mover state is bus state, registered only when movers exist — the digest and the recorders get it for free
   and mover-less worlds move no bit.
4. One owner: the `RigidBody` steps the movers in a fixed intra-tick order (integrate movers, resolve mover
   contacts, resolve quad contacts with exchange), not a second model — no cross-model write-back.
5. Movers are point masses: contacts at the shape's corners/rims, impulses linear only, no tumbling; the
   position-correction and restitution-floor disciplines are the quad's.
6. The quad's `[collision]` restitution/friction govern quad↔mover pairs (semantics preserved); the fixed
   estimates 0.3/0.5 govern mover pairs.
7. `landed` below 1 mm/s while touching: inert forever, `Load` resets; no resting-contact loop for objects.
8. Legs ignore movers; the spheres carry quad↔mover contact.
9. Object–object events travel a handle (the race pattern), not a state diff; the quad's event path is untouched.
10. `OBJECT_COLLISION = 27`, `State.moving_objects = 21`, `WorldObject.motion = 9` — protocol 17; mover-less
    worlds' messages are byte-identical.
11. The impulse/identity records are #157's seam and stay off the wire in part 1.
12. `MAX_MOVING_OBJECTS = 8`, `mass_kg` in (0, 100]; the object index space is the file order of `[[objects]]`.
13. RF and video obstruction keep the declared pose; documented, with the signals as the seam.
14. `worlds/park.toml` ships one crate — a timed hazard crossing the diamond (tuned at execution so the Godot
    e2e laps over park, if any fly through its window, stay honest — re-verify what `e2e_terrain.gd` flies
    before fixing the numbers); `flat.toml` is untouched.

## Open questions for the maintainer

1. **#29's "props with each other"** — the issue's own done-when asks that this be settled in #156 before the
   spec is executed. Proposed reading: two aircraft's props meeting mid-air. This design rules it out of part 1:
   a session builds one `Vehicle` (one `RigidBody`); prop-on-prop needs multi-vehicle sessions — a scheduler,
   journal and protocol change, not contact maths — and no shipped feature flies two quads in one world. If the
   phrase meant a prop striking world objects, static since M3c and movers here, it is already satisfied. The
   executing session posts this reading in #156 and proceeds unless corrected.
