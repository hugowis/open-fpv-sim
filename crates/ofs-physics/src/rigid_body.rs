//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper contact against the
//! ground plane and the world's objects.
use glam::{DQuat, DVec3};
use crate::collision::{self, CollisionParams, TouchState};
use ofs_core::shape::{Aabb, Shape};
use ofs_core::consts::{AIR_DENSITY_KGPM3, GRAVITY_MPS2};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct MotorMount {
    pub position_frd_m: DVec3,
    /// +1: rotor angular velocity along +Z_FRD (clockwise seen from above); -1: counter-clockwise.
    pub spin: f64,
}

/// A world object the body can touch, by its name and its shape exactly as the world file gives it (not rooted).
#[derive(Debug, Clone)]
pub struct WorldObject {
    pub name: String,
    pub shape: Shape,
}

#[derive(Debug, Clone)]
pub struct GroundParams {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub friction_coeff: f64,
}

#[derive(Debug, Clone)]
pub struct AirframeParams {
    pub mass_kg: f64,
    /// Diagonal inertia about FRD axes.
    pub inertia_kgm2: DVec3,
    /// Drag area (Cd * A) per FRD axis.
    pub cda_m2: DVec3,
    pub angular_damping_nms: f64,
    /// Motor rotor + propeller inertia, for the reaction torque of spinning up.
    pub rotor_inertia_kgm2: f64,
    pub mounts: Vec<MotorMount>,
    pub contact_points_frd_m: Vec<DVec3>,
    pub ground: GroundParams,
    /// The world's objects, all of them collidable.
    pub objects: Vec<WorldObject>,
    /// The collision spheres and their contact behaviour.
    pub collision: CollisionParams,
}

#[derive(Debug, Clone, Copy)]
pub struct BodyState {
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    /// FRD body -> NED world.
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
}

pub struct RigidBody {
    p: AirframeParams,
    /// Each object's bounding box, computed once at build (the contact broad phase).
    bounds: Vec<Aabb>,
    /// The quad's bounding radius (the largest sphere centre plus radius): the broad phase's margin, handed to
    /// `collision::resolve` each step.
    bounding_radius_m: f64,
    touch: TouchState,
    last_collision: Option<(i32, f64)>,
    collision_events: u64,
    s: BodyState,
    thrust: Vec<Signal<f64>>,
    torque: Vec<Signal<f64>>,
    omega_dot: Vec<Signal<f64>>,
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    accel: Signal<DVec3>,
    collision_speed: Signal<f64>,
    collision_object: Signal<f64>,
    collision_count: Signal<f64>,
}

impl RigidBody {
    pub fn new(p: AirframeParams, initial: BodyState, bus: &mut Bus) -> Self {
        let n = p.mounts.len();
        let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect();
        let bounding_radius_m = p.collision.spheres.iter().map(|(c, r)| c.length() + r).fold(0.0, f64::max);
        let touch = TouchState::new(p.objects.len());
        let body = Self {
            thrust: (0..n).map(|i| bus.signal(&names::prop_thrust(i))).collect(),
            torque: (0..n).map(|i| bus.signal(&names::prop_torque(i))).collect(),
            omega_dot: (0..n).map(|i| bus.signal(&names::motor_omega_dot(i))).collect(),
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            accel: bus.signal(names::BODY_ACCEL_NED),
            collision_speed: bus.signal(names::BODY_COLLISION_SPEED),
            collision_object: bus.signal(names::BODY_COLLISION_OBJECT),
            collision_count: bus.signal(names::BODY_COLLISION_COUNT),
            bounds,
            bounding_radius_m,
            touch,
            last_collision: None,
            collision_events: 0,
            p,
            s: initial,
        };
        body.publish(bus, DVec3::ZERO);
        body
    }

    pub fn state(&self) -> BodyState {
        self.s
    }

    fn publish(&self, bus: &mut Bus, accel_ned: DVec3) {
        bus.set(self.pos, self.s.pos_ned_m);
        bus.set(self.vel, self.s.vel_ned_mps);
        bus.set(self.att, self.s.att);
        bus.set(self.rate, self.s.rate_frd_radps);
        bus.set(self.accel, accel_ned);
        bus.set(self.collision_speed, self.last_collision.map_or(0.0, |(_, speed)| speed));
        bus.set(
            self.collision_object,
            self.last_collision.map_or(f64::from(collision::GROUND_OBJECT_INDEX), |(object, _)| f64::from(object)),
        );
        bus.set(self.collision_count, self.collision_events as f64);
    }

    /// Total contact force and torque about the centre of mass, both in NED, and the (object index, inward
    /// speed) touch of every landing point currently inside a solid — the ground plane (`GROUND_OBJECT_INDEX`)
    /// or a world object — for the collision events. The spring-damper and friction of `[ground]` act on every
    /// landing contact point against the ground plane and against every world object; against an object the
    /// depth is minus the signed distance and the normal is the shape's gradient there.
    fn contact(&self, s: &BodyState) -> (DVec3, DVec3, Vec<(i32, f64)>) {
        let g = &self.p.ground;
        let mut force = DVec3::ZERO;
        let mut torque = DVec3::ZERO;
        let mut touches: Vec<(i32, f64)> = Vec::new();
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            let depth = s.pos_ned_m.z + lever.z; // ground plane is z = 0, NED z points down
            if depth > 0.0 {
                // The ground's normal is up, -z: the inward speed is minus the velocity along it.
                touches.push((collision::GROUND_OBJECT_INDEX, v.z.max(0.0)));
                let normal = (g.stiffness_npm * depth + g.damping_nspm * v.z).max(0.0);
                let v_t = DVec3::new(v.x, v.y, 0.0);
                let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
                let f = DVec3::new(0.0, 0.0, -normal) + friction;
                force += f;
                torque += lever.cross(f);
            }
            let point = s.pos_ned_m + lever;
            for (i, object) in self.p.objects.iter().enumerate() {
                if !self.bounds[i].contains(point) {
                    continue;
                }
                let sd = object.shape.signed_distance(point);
                if sd >= 0.0 {
                    continue;
                }
                let depth = -sd;
                let n = object.shape.normal(point);
                let v_n = v.dot(n);
                touches.push((i as i32, (-v_n).max(0.0)));
                let mag = (g.stiffness_npm * depth - g.damping_nspm * v_n).max(0.0);
                let v_t = v - n * v_n;
                let friction = -v_t * (g.friction_coeff * mag / v_t.length().max(0.05));
                let f = n * mag + friction;
                force += f;
                torque += lever.cross(f);
            }
        }
        (force, torque, touches)
    }
}

fn angular_accel(p: &AirframeParams, w: DVec3, torque: DVec3) -> DVec3 {
    let i = p.inertia_kgm2;
    (torque - w.cross(i * w) - p.angular_damping_nms * w) / i
}

impl Model for RigidBody {
    fn name(&self) -> &str {
        "body"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let s = self.s;
        let to_body = s.att.inverse();

        let mut force_body = DVec3::ZERO;
        let mut torque_body = DVec3::ZERO;
        for (k, m) in self.p.mounts.iter().enumerate() {
            let f = DVec3::new(0.0, 0.0, -bus.get(self.thrust[k]));
            force_body += f;
            torque_body += m.position_frd_m.cross(f);
            let rotor_torque = bus.get(self.torque[k]) + self.p.rotor_inertia_kgm2 * bus.get(self.omega_dot[k]);
            torque_body.z -= m.spin * rotor_torque;
        }
        let v_body = to_body * s.vel_ned_mps;
        force_body -= 0.5 * AIR_DENSITY_KGPM3 * self.p.cda_m2 * v_body.abs() * v_body;

        let (contact_force, contact_torque, point_touches) = self.contact(&s);
        torque_body += to_body * contact_torque;

        // Translation: semi-implicit Euler.
        let accel_ned = (s.att * force_body + contact_force) / self.p.mass_kg + DVec3::new(0.0, 0.0, GRAVITY_MPS2);
        let vel = s.vel_ned_mps + accel_ned * dt;
        let pos = s.pos_ned_m + vel * dt;

        // Rotation: midpoint (RK2) with torque held over the step.
        let k1 = angular_accel(&self.p, s.rate_frd_radps, torque_body);
        let k2 = angular_accel(&self.p, s.rate_frd_radps + k1 * (0.5 * dt), torque_body);
        let rate = s.rate_frd_radps + k2 * dt;
        let w_avg = 0.5 * (s.rate_frd_radps + rate);
        let att = (s.att * DQuat::from_scaled_axis(w_avg * dt)).normalize();

        self.s = BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: rate };
        // The contacts apply after integration, before publishing, so a contact this tick is visible this tick;
        // the scheduler's per-step non-finite check catches any non-finite value afterwards. The point touches
        // were collected before integration (with the forces); on the shipped quad the belly sphere usually
        // fires the event first either way.
        let events = collision::resolve(
            &self.p,
            &self.bounds,
            self.bounding_radius_m,
            &mut self.s,
            &point_touches,
            &mut self.touch,
            ctx.time_s,
        );
        if let Some((object, speed)) = events.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.last_collision = Some((*object, *speed));
            self.collision_events += 1;
        }
        self.publish(bus, accel_ned);
        Ok(())
    }
}
