//! Impulse contact of the collision spheres against the world's objects and the ground plane, applied after
//! integration: the body is moved out along the deepest contact's normal, then normal and friction impulses are
//! resolved one contact at a time, deepest first, in a fixed order, so the result is deterministic. At 8 kHz the
//! quad moves a few millimetres per step at racing speed, so no sphere passes through a 12 cm post between steps.
use glam::DVec3;
use ofs_core::shape::Aabb;

use crate::rigid_body::{AirframeParams, BodyState};

/// Inward contact speeds below this reverse with restitution 0, so nothing jitters at rest.
pub const RESTITUTION_SPEED_FLOOR_MPS: f64 = 0.2;
/// A contact that starts touching at least this fast inward raises a collision event.
pub const EVENT_MIN_SPEED_MPS: f64 = 1.0;
/// A touching object raises no further event until it has been out of contact for this long.
pub const EVENT_REARM_S: f64 = 0.020;
/// The ground plane's index in a collision event (objects count from 0).
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
}

impl TouchState {
    /// One slot per world object, then the ground.
    pub fn new(objects: usize) -> Self {
        Self { touching: vec![false; objects + 1], free_since: vec![-2.0 * EVENT_REARM_S; objects + 1] }
    }
}

/// One sphere's contact with one solid, in world coordinates.
struct Contact {
    /// Out of the solid, unit.
    normal: DVec3,
    /// The sphere's surface point along the normal (its deepest point into the solid).
    point: DVec3,
    penetration: f64,
    /// `GROUND_OBJECT_INDEX` for the ground, else the object's index.
    object: i32,
    /// The inward speed of the contact point when the contact was collected (for the events).
    inward: f64,
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
    let mut contacts = collect(p, bounds, bounding_radius_m, s);
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
    // Position correction: the body leaves the deepest contact along its normal, by its penetration. There may
    // be no sphere contact at all (only points touch), in which case the spring-damper does the pushing.
    if let Some(deepest) = contacts.first() {
        s.pos_ned_m += deepest.normal * deepest.penetration;
    }
    let inv_mass = 1.0 / p.mass_kg;
    let inv_inertia = DVec3::new(1.0 / p.inertia_kgm2.x, 1.0 / p.inertia_kgm2.y, 1.0 / p.inertia_kgm2.z);
    for c in &contacts {
        let lever = c.point - s.pos_ned_m;
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

/// Every sphere's contacts with every near object and with the ground. `quad_radius` is the body's bounding
/// radius, the broad phase's margin around the quad's centre.
fn collect(p: &AirframeParams, bounds: &[Aabb], quad_radius: f64, s: &BodyState) -> Vec<Contact> {
    let mut contacts = Vec::new();
    let omega = s.att * s.rate_frd_radps;
    for (centre_frd, radius) in &p.collision.spheres {
        let centre = s.pos_ned_m + s.att * *centre_frd;
        // The ground plane is the solid z >= 0: its signed distance is -z, its normal is up.
        let penetration = radius + centre.z;
        if penetration > 0.0 {
            let normal = DVec3::NEG_Z;
            let point = centre - normal * *radius;
            let inward = (-(s.vel_ned_mps + omega.cross(point - s.pos_ned_m)).dot(normal)).max(0.0);
            contacts.push(Contact { normal, point, penetration, object: GROUND_OBJECT_INDEX, inward });
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
            contacts.push(Contact { normal, point, penetration, object: i as i32, inward });
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
