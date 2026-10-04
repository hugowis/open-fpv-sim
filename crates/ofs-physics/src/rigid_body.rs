//! 6-DOF rigid body: prop thrust and reaction torque, quadratic body drag, spring-damper ground contact.
use glam::{DQuat, DVec3};
use ofs_core::consts::{AIR_DENSITY_KGPM3, GRAVITY_MPS2};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct MotorMount {
    pub position_frd_m: DVec3,
    /// +1: rotor angular velocity along +Z_FRD (clockwise seen from above); -1: counter-clockwise.
    pub spin: f64,
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
    s: BodyState,
    thrust: Vec<Signal<f64>>,
    torque: Vec<Signal<f64>>,
    omega_dot: Vec<Signal<f64>>,
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    accel: Signal<DVec3>,
}

impl RigidBody {
    pub fn new(p: AirframeParams, initial: BodyState, bus: &mut Bus) -> Self {
        let n = p.mounts.len();
        let body = Self {
            thrust: (0..n).map(|i| bus.signal(&names::prop_thrust(i))).collect(),
            torque: (0..n).map(|i| bus.signal(&names::prop_torque(i))).collect(),
            omega_dot: (0..n).map(|i| bus.signal(&names::motor_omega_dot(i))).collect(),
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            accel: bus.signal(names::BODY_ACCEL_NED),
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
    }

    /// Total contact force and torque about the centre of mass, both in NED.
    fn contact(&self, s: &BodyState) -> (DVec3, DVec3) {
        let g = &self.p.ground;
        let mut force = DVec3::ZERO;
        let mut torque = DVec3::ZERO;
        for c in &self.p.contact_points_frd_m {
            let lever = s.att * *c;
            let depth = s.pos_ned_m.z + lever.z; // ground plane is z = 0, NED z points down
            if depth <= 0.0 {
                continue;
            }
            let v = s.vel_ned_mps + s.att * s.rate_frd_radps.cross(*c);
            let normal = (g.stiffness_npm * depth + g.damping_nspm * v.z).max(0.0);
            let v_t = DVec3::new(v.x, v.y, 0.0);
            let friction = -v_t * (g.friction_coeff * normal / v_t.length().max(0.05));
            let f = DVec3::new(0.0, 0.0, -normal) + friction;
            force += f;
            torque += lever.cross(f);
        }
        (force, torque)
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

        let (contact_force, contact_torque) = self.contact(&s);
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
        self.publish(bus, accel_ned);
        Ok(())
    }
}
