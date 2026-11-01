# How the quad collides with the world

Why a hit is an impulse and not a spring, what a contact is, and when the simulator calls it a crash. The code is
`crates/ofs-physics/src/collision.rs` (the impulses and the events) and the contact half of
`crates/ofs-physics/src/rigid_body.rs` (the landing points); the world's shapes and their signed distances,
normals and bounds are `ofs-core::shape`. The design is
`docs/superpowers/specs/2026-10-10-m3c-world-collision-elrs-design.md`.

## Two ways to stop a quad

The obvious contact model is a **penalty spring**: once a sphere is inside a solid, push back with a force
proportional to the penetration. The simulator already flies on one — the `[ground]` spring-damper of the landing
contact points (3000 N/m on the shipped quad, tuned so the quad's 6.4 N of weight sags it 2 mm at rest). Carried
to a wall it fails before any retuning is done: the energy balance `½kx² = ½mv²` gives the sink
`x = v·√(m/k)`, so at 30 m/s the ground spring lets the quad sink `30·√(0.65/3000)` = 44 cm — through the 12 cm
gate post and out the other side. The stiffness a wall needs grows with the square of the speed and the inverse
square of the allowed sink, and it is paid for twice:

- **The marginal stop has no margin.** Stopping within the post means `x = v·√(m/k) ≤ 12 cm`, i.e.
  `√(m/k) ≤ 4 ms` — the quad's own transit time through 12 cm at 30 m/s — so `k ≥ 0.65/(4 ms)² ≈ 41 kN/m`
  (14 times the ground spring). That spring is exactly marginal: its sink is the full 12 cm, the stop ends at the
  far face, and anything softer tunnels — so does this one against anything faster, since the sink scales with
  the speed (40 m/s sinks 16 cm). (A square-on hit loads the whole 0.65 kg through whichever sphere touches; an
  off-centre hit spins the quad instead and sinks it less — its effective mass is lower, see the impulse below —
  so the square-on case governs.)
- **The explicit step must fit inside the sink too.** A resting quad on an undamped spring rings on the pad, so
  the contact carries a damper (near critical, `c = 2√(km)`), and an explicit step stays stable only below
  `dt < 2m/c` — which at critical damping is exactly `√(m/k) = x/v`, the time the quad takes to cross its own
  allowed sink at the impact speed. The marginal spring's ceiling is 4 ms and fits the 8 kHz step (0.125 ms) with
  room to spare — but every step of firmness shrinks the ceiling with the sink: a 3 cm stop (a quarter of the
  post, the first honest margin) needs `k = 0.65·(30/0.03)² = 650 kN/m` and a step below 1 ms; promising a
  3.75 mm sink — the firmness an impulse gives for free — needs `k = 0.65·(30/0.00375)² ≈ 42 MN/m` and a step
  below 0.125 ms, the entire physics budget spent on stability with none left for accuracy, and anything firmer
  needs a faster clock.

An **impulse** carries none of this: it has no stiffness and no damper to integrate, and it bounds the velocity
change directly — the normal velocity is reversed (scaled by the restitution) in one step, whatever the transit
time. What limits it is plain geometry: at 8 kHz a 30 m/s quad moves 3.75 mm per step, a fifth of a prop sphere,
so nothing tunnels between steps. That is why the world's objects get impulses and the ground keeps its spring.

## The contact model

Every world object is a solid with a **signed distance**: positive outside, negative inside, zero on the surface.
A sphere of radius r whose centre sits at signed distance sd penetrates by `radius − sd`. The ground is a
solid too: the half-space below the plane z = 0 (NED, so its signed distance at a point is −z), or below a world's
terrain (see "The ground" below). Each step, after integration:

1. **Collect.** Every sphere against every near object (an object's bounding box, grown by the quad's bounding
   radius, must contain the quad's centre — the broad phase) and against the ground. The contact's normal is the
   signed distance's gradient at the sphere's centre (unit, pointing out of the solid); the contact point is the
   sphere's surface point along that normal, its deepest point into the solid.
2. **Position correction.** The body moves out of *every* penetrating contact. In the sorted order (deepest first,
   ties by object index) each contact pushes the body along its normal by the penetration it still has after the
   shifts so far, `penetration − shift·normal`, if that is above 1 nm; the sweep repeats up to 4 times, stopping when
   a sweep moves nothing. Contacts whose normals are orthogonal (a sphere in the corner of the ground and a wall)
   are out in the first sweep; contacts whose normals partly oppose (a sphere wedged between two posts) converge
   over the sweeps (the unit test pins less than 0.1 mm left in either post). A lone contact moves the body by exactly its
   penetration, as it always did. Translation only: the correction never turns the body.
3. **Normal impulses**, one contact at a time, deepest first, ties by object index, in a fixed order — so the
   same inputs give bit-identical flights. A contact that approaches (`v_n < 0`, the contact point's velocity
   along the normal) gets the impulse

   `j = −(1+e)·v_n / k`,  with `k = 1/m + n·((I⁻¹(r×n))×r)`,

   the effective mass along the normal: the impulse also turns the body through the inertia tensor (rotated into
   the body frame for the diagonal inverse), which is how a clipped gate post spins the quad. Restitution e is the
   configured value, but 0 for inward speeds below 0.2 m/s: slow contacts just stop, so a resting quad has
   nothing to jitter about. A 0.3 restitution bounces a drop back to e² = 9 % of its height (the unit test pins
   the arithmetic at e = 0.5).
4. **Friction.** Coulomb: an impulse against the contact point's tangential velocity, capped at `μ·j` — friction
   can never exceed the normal impulse times the coefficient. While the quad rests this holds it near μ·g of
   deceleration; on a sliding hit it stops the slide. The normal impulse is non-zero every step under gravity, so
   the cap is a stiction cap: a sphere that rests on a slope sticks. Its one limit is the order: the impulse
   follows the integration, so each step the body still moves `g·sinθ·Δt²` down the slope, a creep of
   `g·sinθ·Δt` — about 0.4 mm/s on a 20° slope, 1.2 mm/s at 90° — rather than exactly zero.

The impulses act at the contact points; the position correction moves the body; neither adds a force to the
integration, so there is no stability to trade against the step size.

## The landing contact points

The quad file's `contact_points_frd_m` (the legs, at z = +3 cm on the shipped quad) keep their M1 spring-damper
and friction from `[ground]`, but they now act against every world object as well as the ground: against an
object the depth is minus the signed distance of the point and the normal is the shape's gradient there. This is
how the quad lands on the launch pad and *stays* on a roof.

**Friction at a landing point is static Coulomb friction**, by an anchor spring (the Cundall–Strack contact model).
The normal force is `N = max(k·depth − c·v_n, 0)` (`v_n` positive when leaving). Across the surface each touching
point keeps a tangential displacement `d` since it stuck, integrated from its tangential velocity
(`d += v_t·Δt`, then projected onto the tangent plane, which also follows a curved surface), and the friction is
`F_t = −(k·d + c·v_t)` with the same `k` and `c` as the normal spring and damper — no new parameter. It is capped
at `μ·N`: under the cap the point sticks and the spring holds it, at the cap it slides at `μ·N` and `d` is clamped
to `μ·N/k`, so the spring part never exceeds the Coulomb limit. Losing contact (`N = 0`) drops the anchor. The
anchors are the previous step's list, in point and then object order, so the flight stays deterministic.
What it buys: a quad parked on a slope whose angle is within the friction angle (`tan θ ≤ μ`) stays put — it sags
`m·g·sinθ/(n·k)`, a millimetre, and a brief slip while the spring swings in, then nothing — and one past it slides
with `a = g(sinθ − μ·cosθ)`. The unit tests pin both on an incline (95 % and 105 % of the friction angle, within
3 %), the flat deceleration `μ·g` and the stop that stays stopped. The old law, `μ·N·v_t/max(|v_t|, 5 cm/s)`, was
viscous below 5 cm/s: a resting quad crept down any slope.

The limits: the incline tests use a synthetic ramp (the world's shapes are axis-aligned boxes and cylinders; the
slopes to land on are the terrain's, see "The ground"); the anchor lives per (point, object), so a point sliding
across an object's edge onto another starts over; the static sag is real (a millimetre or so), not a rigid hold; `friction_coeff` is one number for the
ground and every object, and the spheres' and the points' share nothing but its value (`[collision]` for the
spheres, `[ground]` for the points).

Two mechanisms, because they answer two different questions: the spring-damper carries a resting weight stably
(a landed quad must sit still for minutes, and the 3000 N/m spring sags 2 mm doing it), while the impulses bound
hits that no stable spring could (the section above). Without the points, resting weight rides on impulse
contacts — stable but hard; without the impulses, a 30 m/s hit sinks through the wall between two spring
compressions.

## The collision spheres

The quad file's `[collision].spheres_frd_m` lists its own spheres; without one, they are generated: **six spheres
of radius 2 cm evenly spaced on each prop's tip circle** (in the motor's plane) **plus one body sphere of radius
4 cm at the centre of mass** — 25 spheres on the shipped quad.

The arithmetic behind that default is a safety claim: **no gap admits a 12 cm post** — the gate posts, the
thinnest upright obstacles in the shipped world. Six spheres per tip circle put neighbours 60° apart, so their
centres are one tip radius apart (the chord `2·6.35·sin 30°` = 6.35 cm) and 2.35 cm of air is left between two
2 cm spheres. The widest ring-to-body gap is on the ring sphere farthest out: the ring's six angles straddle the
45° motor arm, and the outer one lands 15° past it, 17.5 cm from the quad's centre, leaving `17.5 − 2 − 4` =
**11.5 cm** of air to the body sphere. The spec's first figure for the prop spheres, 1.5 cm, left 12.02 cm there —
the claim failed by 0.24 mm — so the radius is 2 cm (see `docs/roadmap.md`).

And nothing is missed between steps: at 8 kHz the 30 m/s quad moves 3.75 mm per step, and a sphere is found the
step its centre crosses the surface.

## The ground

Without a `[terrain]` section the ground is the plane d = 0, and everything above holds as written. With one, the
world's loader bakes the section into a regular grid of f32 heights (`ofs_core::terrain::Terrain`, metres up, node
spacing `cell_m`) and the ground becomes the half-space below a **triangle mesh**: each cell is split along its
(south-west, north-east) diagonal, so the surface is piecewise planar, and the height and normal the physics sees are
exactly those of a mesh the client can draw from the same grid. Beyond the grid the ground is the plane d = 0. The code
is `crates/ofs-core/src/terrain.rs` (the grid and its queries), `crates/ofs-config/src/terrain.rs` (the generator) and
`ground_at` in `collision.rs`, which both ground sites call. The design is
`docs/superpowers/specs/2026-10-31-terrain-heightfield-design.md`.

**The contact maths.** For the triangle under a point (north, east) the surface has depth `d_g` there and an up unit
normal `n` (NED, so `n.z < 0`). The signed distance of a sphere's centre `c` to the triangle's plane (the exact
perpendicular distance) is `n.z·(c.z − d_g)`, so a collision sphere penetrates by `radius + (−n.z)·(c.z − d_g)` and its
contact normal is `n`. A landing point's depth is `(−n.z)·(p.z − d_g)` along `n`, handed to the same spring-damper and static-friction law as before: the spring pushes
along the slope normal and the anchor spring works in the slope's tangent plane, so a quad parked within the friction
angle of a hillside stays put and one past it slides at `g(sinθ − μ·cosθ)`. Flat (`d_g = 0`, `n` straight up) every
expression collapses to the old one (`x − 0.0` and `x·1.0` are exact), which is why a world without `[terrain]` is
bit-identical to before. The ground is still object index −1 in the events.

**One limit.** *The sphere is tested against the plane of the one triangle under its centre*, not the true distance to
the mesh. A sphere resting on triangle A with its centre exactly at the fold line misses a penetration of the neighbour
B of `r·(1 − cos θ_B / cos θ_A)`, where `θ_A` and `θ_B` are the tilts of the two normals from vertical: with A flat and
B tilted by Δθ that is `r·(1 − cos Δθ)`, and for a symmetric V fold it is 0. The miss is found when the centre crosses
the fold, which for a sphere at rest may be never. At a ridge the error is a gap (the sphere floats on A's extended
plane); it does not sink. Adjacent triangles of the shipped hills differ by a few degrees, so the error is about
`r·Δθ·θ`: well under a millimetre for a 2 cm sphere (an order-of-magnitude estimate).

The vertical-offset penetration is not a limit: `(−n.z)·(c.z − d_g)` is the exact perpendicular distance from the
centre to the triangle's plane, and `n.z` can never be 0 (the un-normalised z is −1). What the 60 degree cap on a
world's steepest triangle (checked when the world loads: hills, not cliffs) does is bound the fold miss at
`r·(1 − cos 60°) = r/2` and the sideways offset of the true contact point from the one under the centre at `r·sin θ`.

Also: a quad at rest on a slope sits about 7.6 degrees off the slope normal, because the 4 cm body sphere lets it keep its
landing lean (an existing property of the collision model, not of the terrain). A hand-built grid with a non-zero border
has a cliff at its edge, where the plane d = 0 takes over; the generator always makes a zero border and an exactly flat
disc around home (the grid's nodes within `flat_radius_m` are 0, and `ground_d` is exactly 0 within `flat_radius_m` minus
one cell diagonal), so home, the pilot and the edge are continuous with the plane.

**The generator** is seeded value noise on an integer-hashed lattice (FNV-1a of seed, octave and lattice coordinates),
a quintic fade, a fractal sum normalised to [−1, 1], times `amplitude_m` and two smooth windows, one easing the relief in
beyond the flat disc and one to zero at the grid's edge. It uses only `+ − × ÷` and `sqrt`, which IEEE 754 rounds
identically everywhere, never `sin`, `exp` or `pow`, so a baked grid is bit-identical on every OS.

**What the digests pin.** The grid of the default section has one digest constant for every OS, macOS included
(`crates/ofs-config/tests/world.rs`), because of the above. The flat-world pin (`crates/ofs-sim/tests/flat_world_pin.rs`,
recorded on `main` before the change) compares a whole-vehicle bus digest, and that includes the motor, battery and
propeller maths, which call libm and differ in the last bits between operating systems (the radio goldens keep one
digest file per OS for the same reason). So that constant is per OS: Windows and Linux have theirs, macOS has none and
the test is compiled out there. It proves "a world without `[terrain]` moved no bit where it is pinned"; the change
has no platform-specific code, so it holds on macOS unpinned. A macOS difference cannot break replay: a lockstep digest
is compared between two runs on one machine, never against another OS's constant, and the terrain adds no libm call
to the step loop or to the baked grid (load-time validation uses `hypot` and `atan` for the slope check only).

**Cost.** Each ground site samples the grid once per contact: four node reads and a few flops. Measured in release
on WSL with the ignored timing test (`terrain_costs_little_in_release` in `crates/ofs-sim/tests/terrain_flight.rs`,
run with `--release -- --ignored --nocapture`), the wall time per simulated second at 8 kHz of the whole open-loop
vehicle (motors, battery, sensors and the rest, not the contact solver alone), in 0.3 s hops, best of 5, flat
against `worlds/hills.toml`, on every shipped quad:

| Quad | Flat | Hills |
|---|---|---|
| 3t-toothpick | 3.9 ms | 7.9 ms |
| 5f-900mhz | 3.7 ms | 7.7 ms |
| 5f-fixed-link | 3.7 ms | 7.6 ms |
| 5f-freestyle | 4.0 ms | 7.9 ms |
| 5f-ibus | 3.6 ms | 7.5 ms |
| 5f-tramp | 3.9 ms | 7.9 ms |
| 5f-weak-link | 3.9 ms | 7.8 ms |
| 5r-race | 3.6 ms | 7.6 ms |
| 7lr-longrange | 3.6 ms | 7.5 ms |

Sampling roughly doubles the (small) physics cost, about +4 ms per simulated second: a hilly world's physics runs at
about 130 times real time against about 260 flat, and the budget is real time at 8 kHz. The obvious optimisation, not
done: skip the lookup for a sphere above the grid's highest node. It is a timing test, not a CI gate, because
wall-clock gates flake.

**Done in three parts.** The radio and video line of sight follow the terrain since #154 (see "Terrain" in
`docs/research/elrs-link.md`), and since #155 (protocol 16) the Godot client receives the grid (`World.terrain`) and
draws the same mesh the physics samples, so the picture is the physics' surface and a hill that blocks a link is one the
pilot sees.

## Triangle meshes

A world can carry `[[meshes]]`: `.glb` files whose triangles are solid to the quad and block the radio (the world-file
keys are in `docs/dev-setup.md`). The code is `crates/ofs-core/src/mesh.rs` (the triangles, the BVH and its two queries),
`crates/ofs-config/src/mesh.rs` (the `.glb` loader), `collision.rs` and `rigid_body.rs` (the contacts) and
`crates/ofs-rf/src/propagation.rs` (`mesh_loss_db`). The design is
`docs/superpowers/specs/2026-11-01-mesh-worlds-and-chernobyl-design.md`.

**The contact maths.** A mesh is a surface, not a solid: there is no inside or outside, so it need not be watertight, and
the contact is two-sided. A sphere of radius r at `c` touches every triangle whose closest point `q` is within r of the
centre (`within` walks a BVH that visits the left child first and uses only IEEE basic operations and `sqrt`, no fused
multiply-add). Each such triangle is one contact: the penetration is `r − |c − q|` and the normal is the unit vector from `q`
to `c` (the triangle's face normal when `c` is within 1e-12 m of `q`). Those contacts join the objects' and the ground's, so
the position correction, the normal impulses and the friction run as above, and mesh j is one more object with the index
`objects.len() + j` in the events (`HIT <mesh name>`).

**Landing points** act on a mesh as a sphere of `MESH_POINT_RADIUS_M` = 5 mm, and only the **nearest** triangle of each mesh
counts (the first in BVH order on a tie), as a shape would give one contact. Summing every triangle in reach would make the
force depend on how the surface is tessellated: a point at a shared vertex would feel the spring once per triangle around
it. The depth is `5 mm − distance`, the normal runs from the surface to the point, and the spring-damper and static
friction are the ones above, so a quad parked within the friction angle of a mesh ramp stays put (the unit test pins a 20
degree ramp). The price of the skin is a ceiling: a foot's spring force on a mesh is at most `k × 5 mm` (15 N at the
shipped 3000 N/m) before the foot is through. On the shipped quad the 4 cm body sphere reaches 1 cm below the legs, so the
spheres, which have no ceiling, catch the quad first.

**No tunnelling at racing speed.** A sphere is pushed out of a surface the step its centre comes within r of it (out the
far side if it has already crossed), so passing through needs a step longer than the sphere's radius: `v·Δt > r`, i.e.
`v > r/Δt` = 160 m/s for the 2 cm prop spheres and 320 m/s for the 4 cm body sphere at 8 kHz. Racing speeds are far
below (the unit test flies 30 m/s at a wall, which moves 3.75 mm a step).

**Limits.**
- A mesh collides with the quad's spheres and landing points only: not with objects, other meshes or any other part of the
  airframe, and nothing is damaged.
- Outside a mesh's bounding box (grown by the quad's bounding radius for the spheres, by 5 mm for the feet) it costs
  nothing.
- Determinism: the geometry uses only `+ − × ÷` and `sqrt`, so the BVH digest of a fixed mesh is pinned
  (`crates/ofs-core/tests/mesh.rs`) and the queries answer the same on every OS. A non-zero `heading_deg` turns the
  file with libm `sin` and `cos` at load time, so cross-OS bit-identity is only claimed for `heading_deg = 0` meshes. A
  world without `[[meshes]]` is bit-identical to before (the flat-world pin).

**The radio.** A mesh costs the signal the loss of every surface the straight path crosses: `mesh_loss_db` is the sum, over
the segment's crossings with the mesh (merged where one passes through a shared edge or vertex), of the loss of the crossed
triangle's material, from `[[materials]]` (0 when unlisted). It is added to the objects' and the terrain's obstruction on
the video and the ExpressLRS links alike. The limits are the model's: the loss is frequency independent; there is **no
diffraction around a mesh's edges** (the shadow behind a wall is sharp, and a path that skims a corner is untouched); there
is **no reflection**; and there is **no thickness**, a crossing is a loss, so a wall modelled as two skins costs two losses
and a wall modelled as one surface costs one. A glazed window is a surface with its own (small) loss; an opening is no
surface and passes the signal. `worlds/hall.toml` has all three.

**Cost.** Measured in release on Windows with the ignored timing test (`mesh_world_timing_is_recorded` in
`crates/ofs-physics/tests/mesh_perf.rs`, run with `cargo test --release -p ofs-physics --test mesh_perf -- --ignored --nocapture`)
on 2026-11-01, an AMD Ryzen 9 9900X3D, rustc 1.99.0: 10 s at 8 kHz of the rigid body alone (a 4 cm body sphere, four 2 cm
prop spheres and four feet, 0.65 kg), dropped 0.3 m onto a rippled grid of 100 352 triangles so it bounces and then rests in
contact every step, best of 3, against the same drop onto the plane d = 0:

| Flight | Wall time for 10 s |
|---|---|
| 100 352-triangle mesh | 0.102 s (1.3 µs a step) |
| no mesh | 0.011 s (0.14 µs a step) |

So a mesh of that size adds about 9 ms of physics per simulated second, under 1 % of real time; the rest of the vehicle
spends about 4 ms per simulated second (see "The ground", measured on another machine). The triangle cap is four times that
mesh. It is a timing test, not a CI gate, because wall-clock gates flake. The Godot rendering of meshes has been verified
headless only, by the Godot test suites; nobody has looked at it by eye yet.

The Chernobyl world (`chernobyl_timing_is_recorded` in `crates/ofs-sim/tests/chernobyl_world.rs`, run with
`cargo test --release -p ofs-sim --test chernobyl_world --locked -- --ignored --nocapture`) on 2026-11-01, same machine
(AMD Ryzen 9 9900X3D, rustc 1.99.0): 10 s of the whole vehicle (open-loop stabilised 7-inch quad flying in through the
door) against a mesh of 288 triangles takes 0.127 s, against 0.053 s for the same flight in the open field, best of 3:
about 7 ms more per simulated second.

## Events

A sphere or a landing point that *starts* touching something (it was free the step before) with an inward speed
of at least **1 m/s** raises one `COLLISION` event naming the object (`ground` for the ground) and the
speed. The same object raises nothing more until it has been out of contact for **20 ms** — one event per
touching spell, 160 steps of separation at 8 kHz. A gentle landing raises nothing: 1 m/s is the speed a fall
reaches in 5 cm, so a touchdown the pilot would call soft stays silent, and the event's speed is the inward speed
at the moment of first touch.

## Constants

| Constant | Value | Provenance |
|---|---|---|
| Restitution (default) | 0.3 | estimated, user-tunable (`[collision]` `restitution`) |
| Friction coefficient (default) | 0.5 | estimated, user-tunable (`[collision]` `friction_coeff`) |
| Landing-point friction coefficient | 0.6 on the shipped quads | estimated, user-tunable (`[ground]` `friction_coeff`); its tangential stiffness and damping are the normal spring's |
| Position correction sweeps | 4, 1 nm threshold | engineering choice: orthogonal contacts need 1, partly opposing ones converge in 2 to 4 |
| Restitution speed floor | 0.2 m/s inward | estimated |
| Event threshold | 1 m/s inward | estimated |
| Event rearm | 20 ms out of contact | estimated |
| Generated prop sphere radius | 2 cm | ruled: 1.5 cm fails the no-12-cm-post claim by 0.24 mm on the shipped quad |
| Generated body sphere radius | 4 cm | estimated (it reaches 1 cm past the shipped quad's legs — carried debt) |
| Generated spheres per prop tip | 6 | estimated |
| Terrain generator defaults (`seed` 0, `size_m` 400, `cell_m` 1, `amplitude_m` 8, `wavelength_m` 100, `octaves` 3, `persistence` 0.5, `flat_radius_m` 25) | as listed | estimates of gentle rolling hills, user-tunable (`[terrain]`); `worlds/hills.toml` uses a 600 m square, 12 m amplitude, 120 m wavelength, 30 m flat disc |
| Terrain slope cap | 60 degrees | engineering choice: bounds the fold miss at r/2 and the contact point's sideways offset at r·sin θ |
| Terrain grid limits | `cell_m` ≥ 0.25 m, at most 1025 nodes a side (4 MB), `size_m` ≤ 2000 m, `octaves` 1 to 6 | engineering choices bounding memory and bake time |
| Mesh landing-point skin | 5 mm (`MESH_POINT_RADIUS_M`) | engineering choice: caps a foot's spring force on a mesh at `k × 5 mm` and, with the nearest triangle only, keeps it independent of the tessellation |
| Mesh caps | 8 meshes, 400 000 triangles per world, 64 MiB per file | estimates bounding memory, load time and query cost (100 352 triangles measured above) |
| Mesh query tolerances | 1e-12 m (the face normal stands in), 1e-9 (barycentric and segment-end slack, crossing merge), 1 mm (BVH box slack on segment tests) | engineering choices against rounding at shared edges |

## Where it shows

- Bus signals: `body.collision_speed` (the last event's inward speed, 0 before one), `body.collision_object`
  (−1 for the ground), `body.collision_count` (events raised so far).
- The state stream: `State.collision_speed_mps` (protocol 5), and the `COLLISION` events — `HIT BuildingA 7.2 m/s`
  for an object, `HARD LANDING 3.1 m/s` for the ground.
- The game: the HUD toasts the event; Betaflight's own crash recovery (the PID loop wrestling a tumbling quad) is
  Betaflight's; nothing is damaged.
