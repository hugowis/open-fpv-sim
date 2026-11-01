# Collisions between objects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A world can declare a ballistic object (`[objects.motion]`); it flies, hits the ground, statics, other movers and the quad, exchanges momentum with the quad, and raises collision events — object–object pairs included — while a world without movers runs bit-for-bit as today. Closes #156.

**Architecture:** `MotionSection` in the world file (ofs-config) feeds `AirframeParams.dynamics` (ofs-physics). A new pure module `ofs-physics/src/objects.rs` integrates the movers and resolves their contacts; `collision.rs` resolves quad contacts against a mover's live pose with a mass-split impulse. The `RigidBody` model owns the movers (one owner, one intra-tick order), publishes their state as opt-in bus signals (`world.<name>.*`, digest-visible), and pushes object–object events into a handle the session drains (`OBJECT_COLLISION`, protocol 17). Godot moves the drawn node from the state stream.

**Tech Stack:** Rust workspace (MSRV 1.85, glam `DVec3`), tonic/prost proto, Python `ofs` (generated stubs), Godot 4.7.2 GDScript through the gdext bridge. No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-31-object-collisions-design.md` (read it first; its rulings and the collision matrix are binding).

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- No golden digest changes (`crates/ofs-sim/tests/golden/*`) and `crates/ofs-sim/tests/flat_world_pin.rs` passes unmodified: worlds without movers must be byte-identical. A moved golden is a bug.
- A world without `[objects.motion]` registers no new bus signal, runs the empty-list code path, and its `State`/`World` messages are byte-identical to protocol 16's.
- Do not touch `ofs-rf`, `ofs-video` or the RF obstruction search: obstruction keeps the declared pose (the spec's documented limit).
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL; for multi-line WSL commands write a script file and run `wsl -- bash <path>`. Run Rust tests from the execution worktree with an own `CARGO_TARGET_DIR`.
- Regenerate Python stubs after the proto change: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`; the three generated files change and nothing else under `python/ofs/v1`.
- Python tests need `PYTHONPATH=<worktree>/python`. Godot tests: `bash scripts/run-godot-tests.sh unit` in WSL.
- Commit locally per task. Trailers: none prescribed by this plan; if the executing session adds one it names its own identity, never a borrowed one.
- Before Task 1, post the spec's "props with each other" reading in #156 (the issue's done-when demands it); proceed unless corrected.

## Design calls (from the spec; the executor ledgers any change)

1. **Collision matrix.** Ships: quad↔mover (mutual impulse, existing `COLLISION` event), mover↔ground/terrain, mover↔static, mover↔mover. Out: prop↔prop (two aircraft — one vehicle per session), static↔static, resting/rolling/tumbling, legs↔mover, scripted motion, RF-follows-motion.
2. **Ballistic only**: gravity, semi-implicit Euler, no drag, no rotation (point mass), one throw per `Load`; `kind` is the seam for a later scripted kind.
3. **State on the bus, opt-in**: `world.<name>.pos_ned_m`, `world.<name>.vel_ned_mps`, `world.<name>.landed`, registered only when movers exist (the `body.terrain_d_m` trick).
4. **One owner**: the `RigidBody` steps the movers inside its step — integrate movers, resolve mover contacts, then the quad's contacts with the momentum exchange — in that fixed order.
5. **Contact geometry**: a mover's corners (box: 8) or rim points (cylinder: 16) against SDFs and `ground_at`, the landing-point pattern; the quad's spheres against the mover's shape at its live pose, the `collect` pattern.
6. **Pair law**: the quad's `[collision]` restitution/friction govern quad↔mover; the fixed estimates `OBJECT_RESTITUTION = 0.3`, `OBJECT_FRICTION = 0.5` govern mover pairs; `k = 1/m_eff + 1/M`, split by inverse mass.
7. **`landed`** below `LANDED_SPEED_MPS = 1e-3` m/s while touching: velocity zeroed, no further stepping, `Load` resets.
8. **Events**: quad events unchanged (same signals, same naming, index space = `[[objects]]` file order); object–object via the `WorldEventsHandle` queue → `EVENT_KIND_OBJECT_COLLISION = 27`, messages `"<mover> hit <other> <s> m/s"` / `"<mover> hit the ground <s> m/s"`, thresholds 1 m/s and 20 ms rearm reused.
9. **#157's seam**: the internal records carry the resolved normal impulse and the contact identity (sphere FRD centre for quad events); nothing of it is on the wire here.
10. **Protocol 17**: `WorldObject.motion = 9`, `State.moving_objects = 21`, `EVENT_KIND_OBJECT_COLLISION = 27`.
11. **Schema**: `mass_kg` in (0, 100], velocity finite (default zero), mover not below the terrain, `MAX_MOVING_OBJECTS = 8`.
12. **`worlds/park.toml` ships one crate** (a timed hazard across the diamond); `flat.toml` untouched.

## Review Focus

- A crate dropped on hilly ground stops at the terrain's surface, not the plane d = 0 (task 2).
- A mover-less world is bit-identical: the flat-world pin and the radio goldens pass unmodified (task 3).
- The quad hit by a crate: the existing `COLLISION` event names the crate with the relative speed, and quad momentum change + crate momentum change is zero (task 3).
- Two crates landing in one state window raise two events, not one merged (task 4).
- A mover below the terrain, a mass of 0 or 100.5, and a ninth mover are each refused at load (task 1).
- A client still on protocol 16 is refused by the handshake (task 4).
- A landed crate keeps its last pose in Godot; an empty `moving_objects` list moves nothing (task 6).

---

### Task 1: The `[objects.motion]` world section (ofs-config)

**Files:**
- Modify: `crates/ofs-config/src/world.rs`, `crates/ofs-config/src/lib.rs` (re-export `MotionSection`, `MotionKind`, `MAX_MOVING_OBJECTS` where the world types are re-exported), `worlds/park.toml`
- Test: `crates/ofs-config/tests/world.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum MotionKind { Ballistic }
  #[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
  #[serde(default, deny_unknown_fields)]
  pub struct MotionSection { pub kind: MotionKind, pub velocity_ned_mps: [f64; 3], pub mass_kg: f64 }
  impl Default for MotionSection  // kind Ballistic, velocity [0.0; 3], mass_kg 1.0
  pub const MAX_MOVING_OBJECTS: usize = 8;
  ```
  `ObjectSection` gains `#[serde(default)] pub motion: Option<MotionSection>`; `WorldConfig::open_field()` is unchanged (its `objects` is empty).

- [ ] **Step 1: Write the failing tests** in `crates/ofs-config/tests/world.rs` (extend the shipped-world test that loads `worlds/park.toml` if one exists; otherwise load it directly):
  - `a_motion_table_parses_and_its_absence_means_static`: an object with `[objects.motion]` (the sub-table after the object's keys) parses to `Some(MotionSection { .. })`; an object without it is `None`.
  - `every_motion_problem_is_reported_at_once`: one `[[objects]]` per problem, each message starting with the expected `objects[i].motion.<field>` text — a non-finite `velocity_ned_mps` entry, `mass_kg = 0`, `mass_kg = 100.5` — plus `objects[i].center_ned_m` "must not be below the terrain" for a mover inside a `[terrain]` hill (the pilot check's pattern; pick a seed/spot the way the terrain tests do), and `objects: at most 8 moving objects` for nine movers.
  - `a_motion_kind_that_is_not_ballistic_is_refused` (TOML parse error names `kind`) and `a_motion_with_an_unknown_key_is_refused` (`deny_unknown_fields`).
  - `the_park_world_declares_one_moving_crate`: `load` of `worlds/park.toml` finds exactly one object with `motion`, named `Crate`, `mass_kg == 0.5`, box `size_m` 0.4 a side.
- [ ] **Step 2: Run** `cargo test -p ofs-config --test world motion` — Expected: FAIL (no `motion` field).
- [ ] **Step 3: Implement** `MotionSection`/`MotionKind`, the `motion` field, and validation in `WorldConfig::validate` next to the object checks: every number finite; `mass_kg` in (0, 100]; the mover's centre not below the terrain (reuse `check_terrain`'s `below` closure — build the terrain only when every earlier terrain problem is absent, as `check_terrain` does); `objects[i].motion` count over 8 reports once under `objects`. Add the crate to `worlds/park.toml` after the hedge: box `Crate`, `size_m = [0.4, 0.4, 0.4]`, `center_ned_m` 30 m over the diamond's centre, `[objects.motion] kind = "ballistic"`, `velocity_ned_mps = [0.0, 4.0, 0.0]`, `mass_kg = 0.5`, and a header line saying the crate falls across the course from session start (one throw per `Load`).
- [ ] **Step 4: Run** `cargo test -p ofs-config --locked` then `cargo test --workspace --locked --no-fail-fast` — Expected: PASS (nothing consumes `motion` yet; note any test that enumerates park's objects for later tasks, do not fix here).
- [ ] **Step 5: Commit** `feat(config): [objects.motion] in the world file, a crate in the park (#156)`.

### Task 2: Dynamic objects in the physics (pure)

**Files:**
- Create: `crates/ofs-physics/src/objects.rs`; Modify: `crates/ofs-physics/src/lib.rs` (`pub mod objects;`), `crates/ofs-physics/src/rigid_body.rs` (`AirframeParams` gains `pub dynamics: Vec<DynamicObject>`, documented "empty: a world without movers, the code paths below all no-op"), `crates/ofs-core/src/shape.rs` (`Aabb::intersects`)
- Test: `crates/ofs-physics/tests/objects.rs` (new), `crates/ofs-core/tests/shape.rs` if it exists (else the assertion lives in `objects.rs`'s tests; re-verify at execution)

**Interfaces:**
- Produces (all `pub`, in `ofs_physics::objects`):
  ```rust
  pub const OBJECT_RESTITUTION: f64 = 0.3;
  pub const OBJECT_FRICTION: f64 = 0.5;
  pub const LANDED_SPEED_MPS: f64 = 1.0e-3;
  #[derive(Debug, Clone, Copy, PartialEq)] pub struct DynamicObject { pub object: usize, pub mass_kg: f64 } // index into AirframeParams.objects
  #[derive(Debug, Clone, Copy, PartialEq)] pub struct DynamicState { pub pos_ned_m: DVec3, pub vel_ned_mps: DVec3, pub landed: bool }
  #[derive(Debug, Clone, Copy, PartialEq)] pub struct WorldCollision { pub mover: usize, pub other: i32, pub inward: f64, pub impulse: f64, pub time_s: f64 }
  /// A mover's contact points, world frame, fixed order: a box its 8 corners, a cylinder 8 points on each rim.
  pub fn contact_points(shape: &Shape) -> Vec<DVec3>;
  /// One tick of the movers: integrate (gravity, semi-implicit Euler), then contacts against the ground
  /// (`collision::ground_at`), the statics and each other (broadphase `Aabb::intersects`), position correction
  /// and impulses per the spec, `landed` per the speed rule. `touching` is the pairs touching last step, kept
  /// by the caller across ticks (the rearm memory, the anchors' pattern); returns the events raised this step.
  pub fn step(p: &AirframeParams, states: &mut [DynamicState], touching: &mut Vec<(usize, i32)>, dt: f64, time_s: f64) -> Vec<WorldCollision>;
  ```
  `impl Aabb { pub fn intersects(self, other: Aabb) -> bool }` (each min <= the other's max, componentwise).
  Consumes: `crate::collision::{ground_at, GROUND_OBJECT_INDEX, EVENT_MIN_SPEED_MPS, EVENT_REARM_S, RESTITUTION_SPEED_FLOOR_MPS}`, `Shape::signed_distance/normal/bounds`, `ofs_core::terrain::Terrain`.

- [ ] **Step 1: Write the failing tests** in `tests/objects.rs` (rigs built like `tests/collision.rs`'s `params()` helper — an `AirframeParams` with one sphere, `dynamics` naming the mover's index, states hand-built; no `RigidBody` yet):
  - `a_dropped_box_lands_on_the_plane_and_stops`: released at rest 2 m up; after the fall `landed`, `vel == 0`, centre `z == half_height` within 1 mm.
  - `a_dropped_box_lands_on_the_ramps_surface` (a 20-degree `Terrain::from_grid` ramp, the rig of `tests/terrain.rs`): the resting centre sits `half_height` from the slope plane along its normal (within 1 mm).
  - `a_bouncing_box_returns_to_restitution_squared_of_its_height`: dropped 1.8 m onto the plane with `OBJECT_RESTITUTION` 0.3, the first rebound peak is `0.09 * 1.8` within 5 %.
  - `a_box_bounces_off_a_static_post`: thrown sideways into a post (`tests/collision.rs`'s `post()` world object) leaves with its normal component reversed and scaled by the restitution, tangential speed reduced by friction (sign and bound asserts).
  - `two_boxes_exchange_momentum`: head-on, equal masses, opposite velocities; after contact the centre-of-mass velocity is unchanged (within 1e-9) and both velocities flipped sign within 5 %.
  - `a_landed_box_stays_put`: after landing, 5 more seconds of `step` move it not at all and raise nothing.
  - `a_hard_landing_raises_one_event_per_touching_spell`: a bounce (rebound over the 1 m/s threshold) raises a second `WorldCollision` naming the ground; jittering at rest raises none; two boxes landing in one call return two records.
  - `contact_points_of_a_box_are_its_eight_corners_in_a_fixed_order` (and the cylinder's sixteen rim points).
- [ ] **Step 2: Run** `cargo test -p ofs-physics --test objects` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement.** `contact_points` from the shape's own local-frame corners — a box the eight sign combinations of its `half` extents (rotated by the yaw if #28 landed), never the AABB's corners (a yawed box's AABB corners are not its corners) — or centre ± radius/height (cylinder, angles at 45°+k·90°, `sin`/`cos` of those exact degrees — a libm call per point per tick is avoided by precomputing the 8 unit vectors as constants). `step`: for each not-landed mover (index order) `vel += (0, 0, g)·dt; pos += vel·dt`; collect contacts (mover index, point, depth, normal, `v_n = vel·normal`); position correction deepest-first (translate `pos` only); then per contact `j = -(1+e)·v_n / k` (`e = 0` below `RESTITUTION_SPEED_FLOOR_MPS`; `k = 1/M` against ground/statics, `1/M_a + 1/M_b` between movers), friction impulse capped `OBJECT_FRICTION·j` against `vel_t`; equal-and-opposite on the other mover. The events: a pair newly touching with inward ≥ `EVENT_MIN_SPEED_MPS` and free ≥ `EVENT_REARM_S` pushes `WorldCollision { mover, other, inward, impulse: |j|, time_s }`. Then the landed sweep. Add `dynamics: Vec::new()` to the six `AirframeParams {` literals (three in `tests/collision.rs`, two in `tests/rigid_body.rs`, one in `src/rigid_body.rs`).
- [ ] **Step 4: Run** `cargo test -p ofs-physics --locked` — Expected: PASS (the existing suites are the no-mover regression).
- [ ] **Step 5: Commit** `feat(physics): ballistic world objects with ground, static and pair contacts (#156)`.

### Task 3: The quad exchange, the RigidBody wiring and the bus signals

**Files:**
- Modify: `crates/ofs-physics/src/collision.rs` (`collect`, `resolve`), `crates/ofs-physics/src/rigid_body.rs` (`RigidBody` fields, `new`, `contact`, `step`, `publish`), `crates/ofs-core/src/names.rs`, `crates/ofs-sim/src/vehicle.rs` (`dynamic_objects`, the `AirframeParams` build at ~line 963, `Vehicle`, `VehicleState`), all `RigidBody::new` call sites in `crates/ofs-physics/tests/*`
- Create: `crates/ofs-sim/tests/moving_objects.rs`
- Test: the above new file, `crates/ofs-physics/tests/collision.rs` (extended), `crates/ofs-sim/tests/flat_world_pin.rs` (run only — unmodified)

**Interfaces:**
- Consumes: Task 2's `objects::{step, DynamicObject, DynamicState, WorldCollision}`.
- Produces:
  - `names`: `pub fn world_pos(name: &str) -> String` (`"world.{name}.pos_ned_m"`), `world_vel(name: &str) -> String` (`".vel_ned_mps"`), `world_landed(name: &str) -> String` (`".landed"`) — the pattern of `names::prop_thrust(i)`.
  - `RigidBody::new(p: AirframeParams, initial: BodyState, events: WorldEventsHandle, bus: &mut Bus) -> Self` where `#[derive(Clone, Default)] pub struct WorldEventsHandle(std::sync::Arc<std::sync::Mutex<Vec<WorldCollision>>>)` with `pub fn push(&self, e: WorldCollision)` and `pub fn take(&self) -> Vec<WorldCollision>` (the `RaceHandle` pattern, `crates/ofs-sim/src/race.rs`). Tests pass `WorldEventsHandle::default()`.
  - `collision::resolve` and `collect` gain the movers: `collect(p, bounds, quad_radius, s, dynamics: &[DynamicObject], states: &[DynamicState])` — a mover's contact uses its shape re-centred on `states[k].pos_ned_m`, the approach speed is the sphere point's velocity minus `states[k].vel_ned_mps`, and the contact record carries the mover's `k`; `resolve(...)` applies the pair impulse `k = 1/m_eff + 1/M` to the quad as today and returns the per-mover velocity deltas so the caller applies `−j·n/M`; the resolved impulse and the sphere's FRD centre join the event record internally (the #157 seam; nothing published).
  - `RigidBody::step` order, after the quad's integration: `objects::step` (movers), then `collision::resolve` with the exchange applied to the mover states, then `publish`. The landing-point loop (`contact`) skips movers (`self.p.dynamics.iter().any(|d| d.object == i)`).
  - `RigidBody::new` builds `dynamic_state` from the objects' declared centres, registers `world_pos/vel/landed` signals only when `p.dynamics` is non-empty, publishes the initial pose; `publish` refreshes them.
  - `vehicle.rs`: `pub fn dynamic_objects(world: &WorldConfig) -> Vec<DynamicObject>` (movers' indices into `world_objects`' file order + `mass_kg`); the `AirframeParams` build sets it; `Vehicle` holds the `WorldEventsHandle` clone and the mover signal handles; `VehicleState` gains `pub moving_objects: Vec<MovingObjectInfo>` with `#[derive(Debug, Clone, PartialEq)] pub struct MovingObjectInfo { pub name: String, pub pos_ned_m: DVec3, pub vel_ned_mps: DVec3, pub landed: bool }`; `pub fn take_world_collisions(&mut self) -> Vec<WorldCollision>`.
- [ ] **Step 1: Write the failing tests.** In `crates/ofs-physics/tests/collision.rs` (extend the rig with a mover):
  - `a_sphere_hitting_a_mover_exchanges_momentum`: a 0.65 kg quad body (one sphere) at 5 m/s into a 0.5 kg crate at rest; after contact `m_q·Δv_q + m_c·Δv_c == 0` within 1e-9 and the crate moves off along the contact normal.
  - `a_movers_touch_raises_the_existing_event_naming_it`: the same hit raises one event whose object index is the crate's `[[objects]]` index, inward speed the relative approach (a stationary quad struck by a moving crate raises it too, at the crate's speed).
  - `a_resting_quad_on_a_crate_rides_its_spheres` (the legs do not touch it: no leg anchor names the crate).
  In `crates/ofs-sim/tests/moving_objects.rs` (the vehicle rig of `tests/quads.rs`, world from `ofs_config::load` of `worlds/park.toml`):
  - `the_moving_object_signals_exist_only_in_a_world_with_movers`: park has `world.Crate.pos_ned_m`/`vel_ned_mps`/`landed`; the same config with `motion` cleared has none (find the bus through the vehicle's recording of a signal, or `Vehicle::state().moving_objects` empties — re-verify at execution which accessor the neighbouring tests use).
  - `a_mover_flight_replays_exactly_and_differs_from_the_moverless_twin`: two vehicles over park, throttle 0.4 for 3 s; equal `digest()`; a third over the motion-cleared config differs.
  - `a_crate_striking_the_quad_raises_the_existing_collision_event`: park with the crate's centre 2 m above the resting quad's position and zero velocity (a modified `WorldConfig`), the quad parked: `state().collision_object` names the crate's index, `collision_event_count >= 1`, `collision_speed_mps > 0`.
  - `the_crate_lands_and_stops`: after 5 s the crate's `landed` is true and its `pos_ned_m.z` is within 0.05 m of the park terrain's `ground_d` under it plus half its height.
- [ ] **Step 2: Run** `cargo test -p ofs-physics -p ofs-sim --test moving_objects --test collision --test flat_world_pin` — Expected: FAIL (compile errors first, then assertions).
- [ ] **Step 3: Implement** per the interfaces and the spec's physics section. The mover-less path must be exactly today's: `dynamics` empty means `collect`'s inner mover branch never runs and `objects::step` returns early; keep the arithmetic of the static branch untouched.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` — Expected: PASS, including the untouched `flat_world_pin` and the radio goldens. If a signal-listing test now sees park's three signals, update its expectation and say so in the commit message.
- [ ] **Step 5: Commit** `feat(sim): the quad exchanges momentum with ballistic objects; their state is on the bus (#156)`.

### Task 4: Protocol 17, the session events and the server messages

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs` (version 17, doc comment), `crates/ofs-proto/tests/messages.rs`, `python/ofs/client.py` (`PROTOCOL_VERSION = 17`), `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py` (regenerated), `crates/ofs-sim/src/session.rs` (`changes`, `world_msg`, `state_msg`), `crates/ofs-sim/src/server.rs` only if it constructs `pb::World`/`pb::State` literally
- Test: `crates/ofs-sim/src/session.rs` unit tests (next to the collision one), `crates/ofs-sim/tests/grpc.rs`, `crates/ofs-proto/tests/messages.rs`

**Interfaces:**
- Produces (proto): header comment "protocol version 17: protocol 16 plus moving objects (`WorldObject.motion`, `State.moving_objects`) and object-collision events";
  ```proto
  message Motion { string kind = 1; Vec3 velocity_ned_mps = 2; double mass_kg = 3; }
  message MovingObject { string name = 1; Vec3 position_ned_m = 2; Vec3 velocity_ned_mps = 3; bool landed = 4; }
  ```
  `WorldObject.motion = 9` (`google.protobuf`-free, plain optional message); `State.moving_objects = 21` (`repeated MovingObject`, empty without movers); `EVENT_KIND_OBJECT_COLLISION = 27` ("the message is `<mover> hit <other> <s> m/s` or `<mover> hit the ground <s> m/s`"). Sibling coordination (the umbrella's rule): #28's `WorldObject.yaw_deg` and #157's `State.damage` fields also plan 9 and 21-23 respectively — whoever lands second takes the next free numbers, re-verified against the worktree's `sim.proto` at execution.
  `Session::changes` drains `take_world_collisions` after the race events, formatting through a `pub(crate) fn world_collision(e: &WorldCollision, names: &[String]) -> pb::Event` (mover and other names from `world.objects`; `other < 0` is the ground); seconds one decimal; `Event.time_s = e.time_s`.

- [ ] **Step 1: Write the failing tests:**
  - `object_collisions_become_events_with_the_documented_text` (session unit test on `world_collision`): mover-vs-object → `"Crate hit Wall 3.4 m/s"`; mover-vs-ground → `"Crate hit the ground 2.0 m/s"`; `time_s` carried.
  - `two_crates_landing_in_one_window_raise_two_events`: a `WorldEventsHandle` fed two records, `changes()` returns two `OBJECT_COLLISION` events (and the quad's `collision_*` state fields did not move).
  - `world_msg_carries_the_motion_and_state_msg_the_moving_objects` (hand-built `WorldConfig` with a mover and a `VehicleState` with a `MovingObjectInfo`; static objects produce no `motion`, mover-less states produce an empty `moving_objects`).
  - in `grpc.rs`: `get_world_returns_the_crate_motion` (park: one object with `motion.mass_kg == 0.5`, `kind == "ballistic"`) and `a_state_carries_the_moving_crate`.
  - in `messages.rs`: the expected version becomes 17 (the failing test first).
- [ ] **Step 2: Run** `cargo test -p ofs-proto -p ofs-sim --locked protocol motion moving` — Expected: FAIL.
- [ ] **Step 3: Implement** the proto additions, bump the three version places (`sim.proto` header, `ofs-proto/src/lib.rs`, `python/ofs/client.py`), regenerate the stubs (Global Constraints command), fill the messages. Search `grep -rn "protocol 16\|PROTOCOL_VERSION" --include=*.rs --include=*.md --include=*.py --include=*.gd .` outside `docs/superpowers` and update live references.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and `PYTHONPATH=python python -m pytest python/tests -q` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(proto)!: protocol 17 - moving objects and object-collision events (#156)`.

### Task 5: Clients — Rust, the Godot bridge and Python

**Files:**
- Modify: `crates/ofs-client/src/model.rs` (`WorldObject` gains `motion: Option<Motion>`, `Telemetry` gains `moving_objects: Vec<MovingObject>` and `EventKind::ObjectCollision` with its `as_str`/`from_pb` arms), `crates/ofs-godot/src/lib.rs` (`get_world` objects gain a `motion` dictionary, `get_telemetry` gains `moving_objects`), `python/ofs/client.py` (`Motion`, `MovingObject` dataclasses, `WorldObject.motion`, `State.moving_objects`, `_event` arm), `python/README.md`
- Test: `crates/ofs-client` tests (beside the existing `World::from_pb` tests), `python/tests/test_scripting.py` or `test_client.py`

**Interfaces:**
- Produces: Rust `Motion { kind: String, velocity_ned_mps: DVec3, mass_kg: f64 }`, `MovingObject { name: String, position_ned_m: DVec3, velocity_ned_mps: DVec3, landed: bool }`, event kind string `"object_collision"`; Godot: each `get_world()["objects"]` entry gains `"motion": {kind: String, velocity: Vector3, mass: float}` when it has one, and `get_telemetry()` gains `"moving_objects"` (Array of `{name: String, position: Vector3 /* Godot frame: east, up, −north */, velocity: Vector3, landed: bool}`); Python `Motion(kind="ballistic", velocity_ned_mps=(0,0,0), mass_kg=1.0)`, `MovingObject(name, position_ned_m, velocity_ned_mps, landed)` (all defaulted, frozen), `State.moving_objects: tuple = ()`.

- [ ] **Step 1: Write the failing tests:** Rust — `a_motion_and_a_moving_object_survive_the_round_trip`, `the_object_collision_event_has_a_name`; Python — `a_state_carries_moving_objects` and `_event` names `EVENT_KIND_OBJECT_COLLISION` `"object_collision"` (unit, no server needed for the name; the state test follows the existing `test_scripting.py` launch pattern with `world="park"`).
- [ ] **Step 2: Run** the new tests — Expected: FAIL.
- [ ] **Step 3: Implement.** Python dataclasses are frozen with defaults so older code that builds a `State` keeps working; the NED→Godot frame conversion for `position`/`velocity` is the one `get_world`'s centers already use.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `PYTHONPATH=python python -m pytest python/tests -q`, `cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(client): moving objects and object-collision events in the Rust, Godot and Python clients (#156)`.

### Task 6: Godot — the crate that moves and the toast

**Files:**
- Modify: `godot/world/world.gd` (`update_moving(moving: Array)`), `godot/scripts/app.gd` (call it from the per-frame telemetry update; `_on_event` arm), `godot/tests/test_scene.gd`, and the toast test file that covers `_on_event` (find it: `test_hud.gd` or its neighbour)
- Test: `godot/tests/test_scene.gd`, the toast test

**Interfaces:**
- Consumes: Task 5's `moving_objects` telemetry key and `object_collision` event kind.
- Produces: `World.update_moving(moving)` moves each object's node (its plain `[[objects]]` name — the build sets `instance.name = node_name` unprefixed) to its state position (Godot frame; `landed` objects keep their last pose — the state keeps sending it, so a plain set is enough); unknown names are ignored (the world dictionary and the state can disagree briefly at load).

- [ ] **Step 1: Write the failing tests:** `test_scene.gd` — building the park-like world, then `update_moving` with one entry moves the `Crate` node to the entry's position (compare `global_position`), an empty array leaves every node where it was, and a name with no node is a no-op; the toast test — `_on_event("object_collision", "Crate hit the ground 2.0 m/s", …)` adds a toast with that message.
- [ ] **Step 2: Run** `bash scripts/run-godot-tests.sh unit` (WSL) — Expected: FAIL on the new assertions.
- [ ] **Step 3: Implement.** One `_on_event` arm, one method, one call site; no new nodes (the crate is an ordinary object node since the world build draws `objects`).
- [ ] **Step 4: Run** `bash scripts/run-godot-tests.sh all` — Expected: PASS (the e2e suites start `ofs-sim`; if any flies park through the crate's window and flakes, re-tune the crate's numbers in `worlds/park.toml` — centre and velocity only — and say so in the commit message).
- [ ] **Step 5: Commit** `feat(godot): the moving crate in the world and its toast (#156)`.

### Task 7: The performance row, docs and changelog

**Files:**
- Modify: `scripts/benchmark.py` (a CONFIGS row), `crates/ofs-sim/tests/moving_objects.rs` (the timing test), `docs/research/collision.md`, `docs/roadmap.md`, `docs/dev-setup.md`, `CHANGELOG.md`, `python/README.md`, `docs/architecture.md` only if it enumerates `ofs-physics` modules
- Test: the timing test (`#[ignore]`d, not a CI gate)

**Interfaces:**
- Produces: benchmark row `("open loop, moving objects", {"open_loop_fc": True, "record": False, "world": "park"}, {"throttle": 0.5}, False)`; `#[ignore] fn moving_objects_cost_little_in_release()` timing 10 simulated seconds at 8 kHz over park and over the same config with the crate's motion cleared, `eprintln!` ms per simulated second each and the ratio, asserting the ratio < 1.5 and both faster than real time (the style of `terrain_costs_little_in_release` in `crates/ofs-sim/tests/terrain_flight.rs`).

- [ ] **Step 1: Add** the row and the timing test; run the timing test in release (`--ignored --nocapture`) and record the numbers for the docs.
- [ ] **Step 2: Write the docs:** `docs/research/collision.md` gains "Moving objects" (the law, the intra-tick order, the limits — point masses, no tumbling, legs ignore movers, resting is inert, RF keeps the declared pose — and the constants table rows: `OBJECT_RESTITUTION`, `OBJECT_FRICTION`, `LANDED_SPEED_MPS`, `MAX_MOVING_OBJECTS`, all estimates/engineering choices with provenance, plus the timing numbers); `docs/roadmap.md` §Physics line becomes "objects collide (a mover is ballistic and inert at rest; no tumbling, no scripted motion)"; `docs/dev-setup.md` lists `[objects.motion]`; the changelog entry (Added: motion, events, protocol 17 breaking for 16; Changed: park ships a crate); `python/README.md` state and events.
- [ ] **Step 3: Check** the benchmark output: the new row reports "no baseline" and never gates (the m8c rule); leave `scripts/benchmark-baseline.json` alone and note in the PR that the row's baseline is recorded after five green CI runs.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo +1.85 check -p ofs-config -p ofs-core -p ofs-physics -p ofs-sim -p ofs-proto -p ofs-client -p ofs-godot`, and the WSL single-threaded full suite (goldens must not move) — Expected: PASS.
- [ ] **Step 5: Commit** `docs: moving objects - research, benchmark row, changelog (#156)`.

## Ledger

- Standing condition from the spec: Task 0 (before Task 1) posts the "props with each other" reading in #156 — two aircraft's props, ruled out of part 1 — and proceeds unless the maintainer corrects it within the run. Any correction is ledgered here with what changed.
