# Contact model: static Coulomb friction and correcting every penetrating contact (#65)

Two simplifications are recorded in `docs/research/collision.md` and `docs/roadmap.md`:

1. The landing points' friction (`rigid_body.rs`, `contact`) is `-v_t * mu*N / max(|v_t|, 0.05)`: viscous below 5 cm/s.
   It never reaches `mu*N` at rest, so a quad parked on a slope creeps; there is no static friction.
2. `collision::resolve` moves the body out along the deepest sphere contact only; every other penetrating
   contact waits for its impulse and later steps.

**Done when** (the issue): a quad at rest on a slope stays put; a multi-contact penetration resolves in fewer steps;
the existing collision tests pass; analytic friction tests (a block on an incline at the friction angle) exist;
determinism holds.

## Decisions (my calls; the user asked for no questions)

- **Static friction by a tangential anchor spring (Cundall-Strack).** Each landing point that touches a solid keeps
  one tangential displacement `d` (world frame, in the tangent plane), integrated from the point's tangential
  velocity: `d += v_t*dt`, projected onto the current tangent plane. The friction force is
  `F_t = -(k_t*d + c_t*v_t)`, capped at `mu*N` (N is the contact's normal force). When the cap binds, `d` is clamped
  to `mu*N/k_t`, so the spring part never exceeds the Coulomb limit and a slide leaves the contact "anchored" just
  behind it. Below the cap the point sticks (the spring holds it); above it the point slides at `mu*N`. It is a
  true Coulomb cone with stiction, not a regularised viscous law.
- **No new parameter, no schema bump.** `k_t` and `c_t` reuse the contact's own `stiffness_npm` and `damping_nspm`
  (the tangential spring is as stiff as the normal one, so the tangential sag of a held point is of the same order
  as its normal sag: `m*g*sin(theta)/(n*k)`, sub-millimetre). `friction_coeff` keeps its meaning. Stability is the
  normal spring's: the explicit step fits the same `dt < 2m/c` ceiling.
- **State** is the previous step's list of `(landing point, object)` anchors, rebuilt each step, in point then object
  order, so the order is fixed and the flight stays deterministic. A contact that is not touching drops its anchor.
- **Position correction of every contact.** After sorting (deepest first, ties by object), the body is shifted by a
  sequential projection: for each contact in order, move along its normal by the penetration *left* after the
  shifts so far (`penetration - shift.dot(normal)`, if positive). Up to 4 passes, stopping when a pass moves
  nothing. Orthogonal contacts (a corner: ground and a wall) resolve in the first pass; an acute V-groove needs
  more passes, so it converges across the passes and the following steps rather than exactly in one. Translation
  only, as before. A single contact behaves exactly as before (bit-identical).
- **Sphere (impulse) friction is unchanged.** Its Coulomb cap `mu*j` is already a stiction cap (the normal impulse
  is non-zero every step under gravity). Honest limit, documented: because the impulse follows integration, a body
  resting on sphere contacts on a slope creeps `g*sin(theta)*dt` (about 0.4 mm/s at 20 deg), not zero.

## Non-goals

Rolling/torsional friction, a different friction coefficient per object, rotation in the position correction, a
slope shape (objects stay axis-aligned boxes and cylinders; the incline is tested at the contact-maths level).

## Verification

- Unit tests (`crates/ofs-physics/tests/collision.rs`): the contact function directly: a block on an incline at
  95% of the friction angle holds (displacement < 1 mm over 10 s, speed ~0); at 105% it slides with
  `a = g(sin - mu cos)` within 3%; sliding on the flat decelerates at `mu*g`; at rest nothing creeps on the flat;
  a corner penetration is resolved in one step; a V-groove converges; one contact is unchanged.
- Integration: a quad on the ground with a tangential push below `mu*m*g` holds; the no-contact flights keep their
  digests (the existing `contacts_with_no_objects_leave_the_body_alone` and the flight goldens that never touch).
- Goldens change only for flights that touch down or crash; each diff is explained in the PR.
