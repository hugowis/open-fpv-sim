use glam::{DQuat, DVec3};
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
use ofs_core::shape::Shape;
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody, WorldObject};

const A: f64 = 0.08;

fn quad_x() -> Vec<MotorMount> {
    vec![
        MotorMount { position_frd_m: DVec3::new(-A, A, 0.0), spin: 1.0 },
        MotorMount { position_frd_m: DVec3::new(A, A, 0.0), spin: -1.0 },
        MotorMount { position_frd_m: DVec3::new(-A, -A, 0.0), spin: -1.0 },
        MotorMount { position_frd_m: DVec3::new(A, -A, 0.0), spin: 1.0 },
    ]
}

fn feet() -> Vec<DVec3> {
    vec![
        DVec3::new(-A, A, 0.03),
        DVec3::new(A, A, 0.03),
        DVec3::new(-A, -A, 0.03),
        DVec3::new(A, -A, 0.03),
    ]
}

fn params(mounts: Vec<MotorMount>, contacts: Vec<DVec3>) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts,
        contact_points_frd_m: contacts,
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects: Vec::new(),
        collision: Default::default(),
    }
}

/// A box occupying x in [0, 2], y in [-2, 2], z in [-1, 0]: a wall the quad's feet can press into.
fn wall() -> Vec<WorldObject> {
    vec![WorldObject { name: "wall".into(), shape: Shape::Box { center: DVec3::new(1.0, 0.0, -0.5), half: DVec3::new(1.0, 2.0, 0.5) } }]
}

/// A roof to land on: a box whose top face is the plane z = -1.
fn roof() -> Vec<WorldObject> {
    vec![WorldObject { name: "roof".into(), shape: Shape::Box { center: DVec3::new(0.0, 0.0, -0.5), half: DVec3::new(5.0, 5.0, 0.5) } }]
}

#[test]
fn a_foot_pressed_into_a_wall_pushes_back_along_its_normal() {
    // One foot 5 mm inside the wall's near face (x = 0), level with the wall (z in [-1, 0]); the spring-damper
    // pushes it out along -x.
    let mut p = params(vec![], vec![DVec3::new(0.08, 0.0, 0.0)]);
    p.objects = wall();
    let mut s = sim(p, at(DVec3::new(-0.075, 0.0, -0.5)));
    s.step().unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    let push = 3000.0 * 0.005 / 0.65; // k * depth / mass, in m/s^2
    assert!((vel.x + push / 8000.0).abs() < push / 8000.0 * 0.2, "one tick of push: vel {vel}");
    assert!((vel.z - GRAVITY_MPS2 / 8000.0).abs() < 1e-9, "gravity still acts: vel {vel}");
}

#[test]
fn a_quad_lands_and_rests_on_a_roof() {
    let mut p = params(vec![], feet());
    p.objects = roof();
    let mut s = sim(p, at(DVec3::new(0.0, 0.0, -1.05)));
    s.run_for(3.0).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.length() < 1e-3, "settled: vel {vel}");
    let pos = vec3(&s, names::BODY_POS_NED);
    // The legs (0.03 below the CoM) carry it 2 mm into the surface (mg/k), never through it.
    let height = pos.z + 0.03;
    assert!(height > -1.0 && height < -0.995, "resting on the roof: legs at {height}, roof top at -1");
}

#[test]
fn a_quad_rests_on_a_roof_for_ten_seconds_without_drift_or_jitter() {
    let mut p = params(vec![], feet());
    p.objects = roof();
    let mut s = sim(p, at(DVec3::new(0.0, 0.0, -1.05)));
    let mut samples = Vec::new();
    for _ in 0..20 {
        s.run_for(0.5).unwrap();
        samples.push(vec3(&s, names::BODY_POS_NED));
    }
    let last = &samples[10..];
    for axis in 0..3 {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for p in last {
            min = min.min(p[axis]);
            max = max.max(p[axis]);
        }
        assert!(max - min < 1e-3, "axis {axis} jitters {} mm over the last 5 s", (max - min) * 1000.0);
    }
    let (first, end) = (last[0], last[last.len() - 1]);
    assert!((end - first).length() < 1e-3, "no drift over the last 5 s: {} -> {}", first, end);
}

fn at(pos: DVec3) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: DVec3::ZERO, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO }
}

fn sim(p: AirframeParams, s: BodyState) -> Scheduler {
    let mut bus = Bus::new();
    let body = RigidBody::new(p, s, &mut bus);
    let mut sch = Scheduler::new(8000, bus);
    sch.add(Box::new(body));
    sch
}

fn vec3(s: &Scheduler, name: &str) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(name).unwrap())
}

fn att(s: &Scheduler) -> DQuat {
    s.bus().get(s.bus().lookup::<DQuat>(names::BODY_ATT).unwrap())
}

fn set_scalar(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus().lookup::<f64>(name).unwrap();
    s.bus_mut().set(sig, v);
}

#[test]
fn free_fall_matches_kinematics() {
    let mut s = sim(params(vec![], vec![]), at(DVec3::new(0.0, 0.0, -100.0)));
    s.run_for(1.0).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    let pos = vec3(&s, names::BODY_POS_NED);
    assert!((vel.z - GRAVITY_MPS2).abs() < 1e-9, "vel {vel}");
    assert!((pos.z - (-100.0 + 0.5 * GRAVITY_MPS2)).abs() < 1e-3, "pos {pos}");
}

#[test]
fn torque_free_spin_conserves_energy_and_angular_momentum() {
    let mut p = params(vec![], vec![]);
    p.inertia_kgm2 = DVec3::new(0.002, 0.003, 0.004);
    let mut s0 = at(DVec3::new(0.0, 0.0, -1000.0));
    s0.rate_frd_radps = DVec3::new(0.3, 0.2, 5.0);
    let i = p.inertia_kgm2;
    let energy = |w: DVec3| 0.5 * (i * w * w).element_sum();
    let e0 = energy(s0.rate_frd_radps);
    let l0 = s0.att * (i * s0.rate_frd_radps);
    let mut s = sim(p, s0);
    s.run_for(2.0).unwrap();
    let w = vec3(&s, names::BODY_RATE_FRD);
    let l = att(&s) * (i * w);
    assert!(((energy(w) - e0) / e0).abs() < 1e-4, "energy drift");
    assert!((l - l0).length() / l0.length() < 1e-4, "momentum drift");
}

#[test]
fn equal_thrust_balances_gravity_and_reaction_torque_yaws() {
    let mut s = sim(params(quad_x(), vec![]), at(DVec3::new(0.0, 0.0, -10.0)));
    for k in 0..4 {
        set_scalar(&mut s, &names::prop_thrust(k), 0.65 * GRAVITY_MPS2 / 4.0);
    }
    s.run_for(1.0).unwrap();
    assert!(vec3(&s, names::BODY_VEL_NED).length() < 1e-9);
    assert!(vec3(&s, names::BODY_RATE_FRD).length() < 1e-12);

    // Drag torque on the CW rotors (mounts 0 and 3) reacts on the body as -Z (counter-clockwise).
    set_scalar(&mut s, &names::prop_torque(0), 0.01);
    set_scalar(&mut s, &names::prop_torque(3), 0.01);
    s.run_for(0.1).unwrap();
    let expected = -0.02 / 0.0045 * 0.1;
    assert!((vec3(&s, names::BODY_RATE_FRD).z - expected).abs() < 1e-3);
}

#[test]
fn dropped_quad_settles_on_its_feet() {
    let mut s = sim(params(quad_x(), feet()), at(DVec3::new(0.0, 0.0, -0.08)));
    s.run_for(3.0).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    let static_sink = 0.65 * GRAVITY_MPS2 / (4.0 * 3000.0);
    assert!((pos.z - (-0.03 + static_sink)).abs() < 1e-4, "pos {pos}");
    assert!(vec3(&s, names::BODY_VEL_NED).length() < 1e-3);
    assert!(vec3(&s, names::BODY_RATE_FRD).length() < 1e-3);
}

#[test]
fn quadratic_drag_matches_analytic_decay() {
    let mut p = params(vec![], vec![]);
    p.cda_m2 = DVec3::new(0.01, 0.01, 0.02);
    let mut s0 = at(DVec3::new(0.0, 0.0, -1000.0));
    s0.vel_ned_mps = DVec3::new(10.0, 0.0, 0.0);
    let mut s = sim(p, s0);
    s.run_for(1.0).unwrap();
    let k = 0.5 * 1.225 * 0.01;
    let expected = 10.0 / (1.0 + k * 10.0 * 1.0 / 0.65);
    let vx = vec3(&s, names::BODY_VEL_NED).x;
    assert!(((vx - expected) / expected).abs() < 1e-3, "vx {vx} expected {expected}");
}

/// Resting on its four feet at the static sink, so the ground carries exactly its weight.
fn resting() -> BodyState {
    at(DVec3::new(0.0, 0.0, -0.03 + 0.65 * GRAVITY_MPS2 / (4.0 * 3000.0)))
}

#[test]
fn sliding_on_the_ground_slows_at_mu_g() {
    let mut s0 = resting();
    s0.vel_ned_mps = DVec3::new(2.0, 0.0, 0.0);
    let mut s = sim(params(quad_x(), feet()), s0);
    s.run_for(0.2).unwrap();
    let vx = vec3(&s, names::BODY_VEL_NED).x;
    let expected = 2.0 - 0.6 * GRAVITY_MPS2 * 0.2;
    assert!((vx - expected).abs() < 0.03, "vx {vx}, Coulomb friction expects {expected}");
    s.run_for(1.0).unwrap();
    assert!(vec3(&s, names::BODY_VEL_NED).x.abs() < 0.05, "it stops (viscous below 5 cm/s)");
}

#[test]
fn a_contact_off_the_centre_of_mass_turns_the_body() {
    // One foot ahead of the centre of mass, pressed 1 mm into the ground: the ground pushes the nose up.
    let foot = DVec3::new(0.1, 0.0, 0.03);
    let mut s = sim(params(vec![], vec![foot]), at(DVec3::new(0.0, 0.0, -0.03 + 0.001)));
    s.step().unwrap();
    let normal = 3000.0 * 0.001;
    let expected = foot.x * normal / 0.0025 / 8000.0; // pitch rate after one tick: torque / inertia * dt
    let rate = vec3(&s, names::BODY_RATE_FRD);
    assert!((rate.y - expected).abs() < 1e-6 * expected.abs().max(1.0), "pitch rate {} expected {expected}", rate.y);
    assert!(rate.x.abs() < 1e-12 && rate.z.abs() < 1e-12, "{rate}");
}

#[test]
fn ground_damping_resists_the_speed_into_the_ground() {
    let mut s0 = at(DVec3::new(0.0, 0.0, -0.03 + 1e-12));
    s0.vel_ned_mps = DVec3::new(0.0, 0.0, 1.0); // 1 m/s down, just touching
    let mut s = sim(params(vec![], vec![DVec3::new(0.0, 0.0, 0.03)]), s0);
    s.step().unwrap();
    let accel = vec3(&s, names::BODY_ACCEL_NED).z;
    let expected = GRAVITY_MPS2 - 40.0 * 1.0 / 0.65;
    assert!((accel - expected).abs() < 1e-3, "accel {accel} expected {expected}");
}

#[test]
fn angular_damping_decays_a_spin_exponentially() {
    let mut p = params(vec![], vec![]);
    p.angular_damping_nms = 0.001;
    let mut s0 = at(DVec3::new(0.0, 0.0, -1000.0));
    s0.rate_frd_radps = DVec3::new(0.0, 0.0, 10.0);
    let mut s = sim(p, s0);
    s.run_for(0.5).unwrap();
    let wz = vec3(&s, names::BODY_RATE_FRD).z;
    let expected = 10.0 * (-0.5 * 0.001 / 0.0045f64).exp();
    assert!(((wz - expected) / expected).abs() < 1e-4, "wz {wz} expected {expected}");
}
