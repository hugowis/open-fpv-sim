//! Impulse contact of the collision spheres against the world's objects and the ground, applied after
//! integration: the body is moved out of every penetrating contact, then normal and friction impulses are
//! resolved one contact at a time, deepest first, in a fixed order, so the result is deterministic. At 8 kHz the
//! quad moves a few millimetres per step at racing speed, so no sphere passes through a 12 cm post between steps.
use glam::DVec3;
use ofs_core::mesh::MeshHit;
use ofs_core::shape::Aabb;
use ofs_core::terrain::Terrain;

use crate::rigid_body::{AirframeParams, BodyState, GroundParams};

/// Inward contact speeds below this reverse with restitution 0, so nothing jitters at rest.
pub const RESTITUTION_SPEED_FLOOR_MPS: f64 = 0.2;
/// A contact that starts touching at least this fast inward raises a collision event.
pub const EVENT_MIN_SPEED_MPS: f64 = 1.0;
/// A touching object raises no further event until it has been out of contact for this long.
pub const EVENT_REARM_S: f64 = 0.020;
/// The position correction sweeps the contacts at most this many times: one pass resolves contacts whose normals
/// are orthogonal, and the passes after it converge the ones that partly oppose.
const CORRECTION_PASSES: usize = 4;
/// A contact with less penetration than this left is resolved (and a lone contact is moved exactly once, so a
/// single penetration is corrected bit-identically to a single sweep).
const CORRECTION_EPS_M: f64 = 1.0e-9;
/// A landing point touches a mesh within this radius of it (a 5 mm sphere), the depth its spring-damper sees.
pub const MESH_POINT_RADIUS_M: f64 = 0.005;
/// The ground's index in a collision event (objects count from 0).
pub const GROUND_OBJECT_INDEX: i32 = -1;

/// The collision spheres of `[collision]`, in the body frame.
#[derive(Debug, Clone)]
pub struct CollisionParams {
    pub restitution: f64,
    pub friction_coeff: f64,
    /// Each sphere's centre (FRD) and radius.
    pub spheres: Vec<(DVec3, f64)>,
}

impl Default for CollisionParams {
    fn default() -> Self {
        Self { restitution: 0.3, friction_coeff: 0.5, spheres: Vec::new() }
    }
}

/// Per-object contact memory for the events: who is touching (a sphere or a landing point), and who went free
/// when.
#[derive(Debug, Default)]
pub struct TouchState {
    touching: Vec<bool>,
    free_since: Vec<f64>,
    /// Scratch for the spheres' mesh queries, reused so a step does not allocate.
    hits: Vec<MeshHit>,
}

impl TouchState {
    /// One slot per world object and mesh (objects first), then the ground.
    pub fn new(objects: usize) -> Self {
        Self { touching: vec![false; objects + 1], free_since: vec![-2.0 * EVENT_REARM_S; objects + 1], hits: Vec::new() }
    }
}

/// One sphere's contact with one solid, in world coordinates.
struct Contact {
    /// Out of the solid, unit.
    normal: DVec3,
    /// The sphere's centre in the body frame and its radius: the impulse loop recomputes the surface point
    /// from the post-correction centre.
    centre_frd: DVec3,
    radius: f64,
    penetration: f64,
    /// `GROUND_OBJECT_INDEX` for the ground, else the object's index.
    object: i32,
    /// The inward speed of the contact point when the contact was collected (for the events).
    inward: f64,
}

/// The force of one landing point `depth` inside a solid whose outward unit `normal` is given, moving at `v` (world,
/// the point's own velocity): the spring-damper along the normal and static-and-sliding Coulomb friction across it.
/// `anchor` is the point's tangential displacement since it stuck (world, in the tangent plane), the state a
/// held point needs: friction is a spring-damper of the same stiffness and damping as the normal one on that
/// displacement, capped at `friction_coeff` times the normal force. Under the cap the point sticks and the spring
/// holds it (a slope below the friction angle holds a quad forever); at the cap it slides at `mu*N` and the
/// anchor stays at the cap. A point that pulls away (no normal force) loses its anchor.
pub fn point_force(g: &GroundParams, normal: DVec3, depth: f64, v: DVec3, anchor: &mut DVec3, dt: f64) -> DVec3 {
    let v_n = v.dot(normal);
    let n = (g.stiffness_npm * depth - g.damping_nspm * v_n).max(0.0);
    if n == 0.0 {
        *anchor = DVec3::ZERO;
        return DVec3::ZERO;
    }
    let v_t = v - normal * v_n;
    let d = *anchor + v_t * dt;
    let d = d - normal * d.dot(normal);
    let friction = -(d * g.stiffness_npm + v_t * g.damping_nspm);
    let cap = g.friction_coeff * n;
    // shortcut: the cap compares the whole tangential force, so a slide's anchor is clamped to cap/k, not to the
    // exact Coulomb cone of the spring alone; fine at these stiffnesses, upgrade if k_t ever decouples from k.
    *anchor = d.clamp_length_max(cap / g.stiffness_npm);
    normal * n + friction.clamp_length_max(cap)
}

/// Applies the sphere contacts of one step to `s` and returns the (object index, inward speed) pairs of the
/// collision events raised this step, hardest last in the vector. `bounding_radius_m` is the quad's bounding
/// radius (the largest sphere centre plus radius), the broad phase's margin. `point_touches` are the landing
/// contact points' touches, (object index, inward speed >= 0), as `RigidBody::contact` saw them; they feed the
/// same per-object touch memory as the spheres, so the rearm is shared across both mechanisms.
pub fn resolve(
    p: &AirframeParams,
    bounds: &[Aabb],
    bounding_radius_m: f64,
    s: &mut BodyState,
    point_touches: &[(i32, f64)],
    touch: &mut TouchState,
    time_s: f64,
) -> Vec<(i32, f64)> {
    let mut contacts = collect(p, bounds, bounding_radius_m, s, &mut touch.hits);
    if contacts.is_empty() && point_touches.is_empty() {
        release_everything(touch, time_s);
        return Vec::new();
    }
    let events = touch_events(&contacts, point_touches, touch, time_s);
    // One order for everything: deepest first, ties by object index (the ground, -1, sorts first), then the
    // order the spheres were collected in (the sort is stable), so the result is deterministic.
    contacts.sort_by(|a, b| {
        b.penetration
            .total_cmp(&a.penetration)
            .then(a.object.cmp(&b.object))
    });
    // Position correction: the body leaves every penetrating contact, in the sorted order, each by the
    // penetration still left after the shifts so far (a corner resolves in one pass; contacts whose normals
    // oppose partly converge over the passes). There may be no sphere contact at all (only points touch), in
    // which case the spring-damper does the pushing.
    let mut shift = DVec3::ZERO;
    for _ in 0..CORRECTION_PASSES {
        let mut moved = false;
        for c in &contacts {
            let left = c.penetration - shift.dot(c.normal);
            if left > CORRECTION_EPS_M {
                shift += c.normal * left;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    s.pos_ned_m += shift;
    let inv_mass = 1.0 / p.mass_kg;
    let inv_inertia = DVec3::new(1.0 / p.inertia_kgm2.x, 1.0 / p.inertia_kgm2.y, 1.0 / p.inertia_kgm2.z);
    for c in &contacts {
        // The lever arm runs from the post-correction centre to the surface point, recomputed for the
        // corrected body position (the collection-time point is up to one penetration stale).
        let point = s.pos_ned_m + s.att * c.centre_frd - c.normal * c.radius;
        let lever = point - s.pos_ned_m;
        let mut j = 0.0;
        let v_point = s.vel_ned_mps + (s.att * s.rate_frd_radps).cross(lever);
        let v_n = v_point.dot(c.normal);
        if v_n < 0.0 {
            let e = if -v_n < RESTITUTION_SPEED_FLOOR_MPS { 0.0 } else { p.collision.restitution };
            let rn = lever.cross(c.normal);
            // Effective mass along the normal: the impulse also turns the body through the inertia tensor.
            let kn = inv_mass + c.normal.dot((s.att * (inv_inertia * (s.att.inverse() * rn))).cross(lever));
            if kn > 0.0 {
                j = -(1.0 + e) * v_n / kn;
                apply(s, inv_mass, inv_inertia, lever, c.normal * j);
            }
        }
        // Coulomb friction, at most mu times the normal impulse, against the tangential velocity.
        let v_point = s.vel_ned_mps + (s.att * s.rate_frd_radps).cross(lever);
        let v_t = v_point - c.normal * v_point.dot(c.normal);
        let speed = v_t.length();
        if speed > 1e-9 && j > 0.0 {
            let t = v_t / speed;
            let rt = lever.cross(t);
            let kt = inv_mass + t.dot((s.att * (inv_inertia * (s.att.inverse() * rt))).cross(lever));
            if kt > 0.0 {
                let jt = (speed / kt).min(p.collision.friction_coeff * j);
                apply(s, inv_mass, inv_inertia, lever, -t * jt);
            }
        }
    }
    events
}

/// An impulse at a world point: linear velocity along the impulse, body-frame angular velocity through the
/// body-frame (diagonal) inverse inertia — the torque is rotated into the body frame before the diagonal acts.
fn apply(s: &mut BodyState, inv_mass: f64, inv_inertia: DVec3, lever: DVec3, impulse: DVec3) {
    s.vel_ned_mps += impulse * inv_mass;
    s.rate_frd_radps += inv_inertia * (s.att.inverse() * lever.cross(impulse));
}

/// The ground's depth `d` (NED) and its outward (up) unit normal at (north, east): the terrain's, or the plane
/// d = 0 with the normal up when there is none.
pub fn ground_at(terrain: Option<&Terrain>, north: f64, east: f64) -> (f64, DVec3) {
    terrain.map_or((0.0, DVec3::NEG_Z), |t| t.surface(north, east))
}

/// Every sphere's contacts with every near object and with the ground. `quad_radius` is the body's bounding
/// radius, the broad phase's margin around the quad's centre. `hits` is the mesh queries' scratch.
fn collect(p: &AirframeParams, bounds: &[Aabb], quad_radius: f64, s: &BodyState, hits: &mut Vec<MeshHit>) -> Vec<Contact> {
    let mut contacts = Vec::new();
    let omega = s.att * s.rate_frd_radps;
    for (centre_frd, radius) in &p.collision.spheres {
        let centre = s.pos_ned_m + s.att * *centre_frd;
        // The ground is the solid below the plane of the triangle under the centre, whose signed distance there is
        // n.z * (c.z - d). Flat (d = 0, n = NEG_Z) this is exactly radius + c.z.
        let (ground_d, normal) = ground_at(p.terrain.as_deref(), centre.x, centre.y);
        let penetration = radius + (-normal.z) * (centre.z - ground_d);
        if penetration > 0.0 {
            let point = centre - normal * *radius;
            let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
            contacts.push(Contact { normal, centre_frd: *centre_frd, radius: *radius, penetration, object: GROUND_OBJECT_INDEX, inward });
        }
        for (i, object) in p.objects.iter().enumerate() {
            // Broad phase: the quad's centre inside the object's box, grown by the quad's bounding radius.
            if !bounds[i].grown(quad_radius).contains(s.pos_ned_m) {
                continue;
            }
            let sd = object.shape.signed_distance(centre);
            let penetration = radius - sd;
            if penetration <= 0.0 {
                continue;
            }
            let normal = object.shape.normal(centre);
            let point = centre - normal * *radius;
            let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
            contacts.push(Contact { normal, centre_frd: *centre_frd, radius: *radius, penetration, object: i as i32, inward });
        }
        for (j, wm) in p.meshes.iter().enumerate() {
            if !wm.bounds.grown(quad_radius).contains(s.pos_ned_m) {
                continue;
            }
            hits.clear();
            wm.mesh.within(centre, *radius, hits);
            let object = (p.objects.len() + j) as i32;
            for hit in hits.iter() {
                let normal = hit.normal;
                let point = centre - normal * *radius;
                let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
                contacts.push(Contact { normal, centre_frd: *centre_frd, radius: *radius, penetration: radius - hit.distance, object, inward });
            }
        }
    }
    contacts
}

/// The events of the contacts — spheres and landing points alike — against the per-object touch memory: one
/// per touching spell that starts hard enough, and only once the object has been out of contact for
/// `EVENT_REARM_S`. Where both mechanisms touch the same object, the faster inward touch raises the event.
fn touch_events(contacts: &[Contact], point_touches: &[(i32, f64)], touch: &mut TouchState, time_s: f64) -> Vec<(i32, f64)> {
    let mut events = Vec::new();
    let n = touch.touching.len();
    for idx in 0..n {
        let object = if idx + 1 == n { GROUND_OBJECT_INDEX } else { idx as i32 };
        let touching_now = contacts.iter().any(|c| c.object == object) || point_touches.iter().any(|(o, _)| *o == object);
        if touching_now && !touch.touching[idx] {
            let inward = contacts
                .iter()
                .filter(|c| c.object == object)
                .map(|c| c.inward)
                .chain(point_touches.iter().filter(|(o, _)| *o == object).map(|(_, inward)| *inward))
                .fold(0.0, f64::max);
            if inward >= EVENT_MIN_SPEED_MPS && time_s - touch.free_since[idx] >= EVENT_REARM_S {
                events.push((object, inward));
            }
        }
        if !touching_now && touch.touching[idx] {
            touch.free_since[idx] = time_s;
        }
        touch.touching[idx] = touching_now;
    }
    events
}

fn release_everything(touch: &mut TouchState, time_s: f64) {
    for idx in 0..touch.touching.len() {
        if touch.touching[idx] {
            touch.free_since[idx] = time_s;
            touch.touching[idx] = false;
        }
    }
}
