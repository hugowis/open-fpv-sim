use glam::{DQuat, DVec3};
use ofs_core::{consts::GRAVITY_MPS2, names, Bus, Scheduler};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};

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
    }
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
