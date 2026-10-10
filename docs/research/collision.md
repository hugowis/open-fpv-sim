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
A sphere of radius r whose centre sits at signed distance sd penetrates by `radius − sd`. The ground plane is a
solid too: the half-space below z = 0 (NED, so its signed distance at a point is −z). Each step, after
integration:

1. **Collect.** Every sphere against every near object (an object's bounding box, grown by the quad's bounding
   radius, must contain the quad's centre — the broad phase) and against the ground. The contact's normal is the
   signed distance's gradient at the sphere's centre (unit, pointing out of the solid); the contact point is the
   sphere's surface point along that normal, its deepest point into the solid.
2. **Position correction.** The body moves out along the *deepest* contact's normal, by that contact's
   penetration.
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
   deceleration; on a sliding hit it stops the slide.

The impulses act at the contact points; the position correction moves the body; neither adds a force to the
integration, so there is no stability to trade against the step size.

## The landing contact points

The quad file's `contact_points_frd_m` (the legs, at z = +3 cm on the shipped quad) keep their M1 spring-damper
and friction from `[ground]`, but they now act against every world object as well as the ground plane: against an
object the depth is minus the signed distance of the point and the normal is the shape's gradient there. This is
how the quad lands on the launch pad and *stays* on a roof.

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
the claim failed by 0.24 mm — so the radius is 2 cm (see `docs/superpowers/m3c-carried-debt.md`).

And nothing is missed between steps: at 8 kHz the 30 m/s quad moves 3.75 mm per step, and a sphere is found the
step its centre crosses the surface.

## Events

A sphere or a landing point that *starts* touching something (it was free the step before) with an inward speed
of at least **1 m/s** raises one `COLLISION` event naming the object (`ground` for the ground plane) and the
speed. The same object raises nothing more until it has been out of contact for **20 ms** — one event per
touching spell, 160 steps of separation at 8 kHz. A gentle landing raises nothing: 1 m/s is the speed a fall
reaches in 5 cm, so a touchdown the pilot would call soft stays silent, and the event's speed is the inward speed
at the moment of first touch.

## Constants

| Constant | Value | Provenance |
|---|---|---|
| Restitution (default) | 0.3 | estimated, user-tunable (`[collision]` `restitution`) |
| Friction coefficient (default) | 0.5 | estimated, user-tunable (`[collision]` `friction_coeff`) |
| Restitution speed floor | 0.2 m/s inward | estimated |
| Event threshold | 1 m/s inward | estimated |
| Event rearm | 20 ms out of contact | estimated |
| Generated prop sphere radius | 2 cm | ruled: 1.5 cm fails the no-12-cm-post claim by 0.24 mm on the shipped quad |
| Generated body sphere radius | 4 cm | estimated (it reaches 1 cm past the shipped quad's legs — carried debt) |
| Generated spheres per prop tip | 6 | estimated |

## Where it shows

- Bus signals: `body.collision_speed` (the last event's inward speed, 0 before one), `body.collision_object`
  (−1 for the ground), `body.collision_count` (events raised so far).
- The state stream: `State.collision_speed_mps` (protocol 5), and the `COLLISION` events — `HIT BuildingA 7.2 m/s`
  for an object, `HARD LANDING 3.1 m/s` for the ground.
- The game: the HUD toasts the event; Betaflight's own crash recovery (the PID loop wrestling a tumbling quad) is
  Betaflight's; nothing is damaged.
