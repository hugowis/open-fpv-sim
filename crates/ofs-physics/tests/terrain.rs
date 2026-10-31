use std::sync::Arc;

use glam::{DQuat, DVec3};
use ofs_core::consts::GRAVITY_MPS2;
use ofs_core::terrain::Terrain;
use ofs_core::{names, Bus, Scheduler};
use ofs_physics::collision::{ground_at, CollisionParams};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody};

const SLOPE_DEG: f64 = 20.0;
const R: f64 = 0.04;
const MASS: f64 = 0.65;
const K: f64 = 3000.0;

/// A plane rising `tan(slope)` per metre toward north through the origin, 100 m a side, as a 1 m grid.
fn ramp(slope_deg: f64) -> Arc<Terrain> {
    let t = slope_deg.to_radians().tan();
    let heights = (0..101 * 101).map(|k| (t * (-50.0 + (k / 101) as f64)) as f32).collect();
    Arc::new(Terrain::from_grid([-50.0, -50.0], 1.0, 101, 101, heights).unwrap())
}

/// The ramp's outward (up) unit normal, in NED: uphill is north, so it leans south.
fn ramp_normal(slope_deg: f64) -> DVec3 {
    let a = slope_deg.to_radians();
    DVec3::new(-a.sin(), 0.0, -a.cos())
}

/// The signed distance of `p` from the ramp's plane through the origin, positive above it.
fn above_ramp(slope_deg: f64, p: DVec3) -> f64 {
    p.dot(ramp_normal(slope_deg))
}

/// One body sphere of radius `R` at the centre of mass, as in `tests/collision.rs`.
fn sphere(terrain: Option<Arc<Terrain>>, restitution: f64) -> AirframeParams {
    AirframeParams {
        mass_kg: MASS,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: Vec::new(),
        mounts: vec![],
        contact_points_frd_m: vec![],
        ground: GroundParams { stiffness_npm: K, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects: vec![],
        collision: CollisionParams { restitution, friction_coeff: 0.5, spheres: vec![(DVec3::ZERO, R)] },
        terrain,
    }
}

/// Four landing points 3 cm below the centre of mass, no spheres.
fn legs(terrain: Option<Arc<Terrain>>, friction_coeff: f64) -> AirframeParams {
    let mut p = sphere(terrain, 0.3);
    p.collision.spheres.clear();
    p.ground.friction_coeff = friction_coeff;
    p.contact_points_frd_m =
        vec![DVec3::new(-0.1, 0.1, 0.03), DVec3::new(0.1, 0.1, 0.03), DVec3::new(-0.1, -0.1, 0.03), DVec3::new(0.1, -0.1, 0.03)];
    p
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

fn state(pos: DVec3, vel: DVec3, att: DQuat) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: DVec3::ZERO }
}

fn bits(v: DVec3) -> [u64; 3] {
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

#[test]
fn ground_at_none_is_the_plane() {
    assert_eq!(ground_at(None, 3.0, 4.0), (0.0, DVec3::NEG_Z));
}

/// The same 1 s run with no terrain and with an all-zero grid: bit-identical body state every tick.
fn flat_and_zero_grid_agree(rig: impl Fn(Option<Arc<Terrain>>) -> AirframeParams, s0: BodyState) {
    let zero = Arc::new(Terrain::from_grid([-50.0, -50.0], 1.0, 101, 101, vec![0.0; 101 * 101]).unwrap());
    let mut flat = sim(rig(None), s0);
    let mut grid = sim(rig(Some(zero)), s0);
    for tick in 0..8000 {
        flat.step().unwrap();
        grid.step().unwrap();
        for name in [names::BODY_POS_NED, names::BODY_VEL_NED, names::BODY_RATE_FRD] {
            assert_eq!(bits(vec3(&flat, name)), bits(vec3(&grid, name)), "{name} differs at tick {tick}");
        }
    }
    assert!(vec3(&flat, names::BODY_POS_NED).z > -0.1, "it reached the ground");
    assert!(flat.bus().lookup::<f64>(names::BODY_TERRAIN_D).is_none(), "no terrain, no signal");
    let d = grid.bus().get(grid.bus().lookup::<f64>(names::BODY_TERRAIN_D).unwrap());
    assert_eq!(d, 0.0);
}

#[test]
fn flat_grid_matches_no_terrain() {
    // A sphere thrown down and sideways (it bounces and its friction spins it) and a quad landing on its legs.
    flat_and_zero_grid_agree(|t| sphere(t, 0.5), state(DVec3::new(1.3, -2.7, -1.0), DVec3::new(0.8, 0.3, 0.5), DQuat::IDENTITY));
    flat_and_zero_grid_agree(|t| legs(t, 0.6), state(DVec3::new(0.4, 0.6, -0.3), DVec3::new(0.5, -0.2, 0.0), DQuat::from_rotation_x(0.1)));
}

#[test]
fn sphere_rests_on_a_slope_normal() {
    // Released at rest 1 mm above the ramp (it lands at 0.14 m/s, under the restitution floor): the position
    // correction keeps its centre exactly R from the plane along the plane's normal, wherever it rolls. On the
    // flat plane it would sit at z = -R instead, 0.35 * x lower.
    let n = ramp_normal(SLOPE_DEG);
    let start = DVec3::new(0.3, 0.2, 0.0) - DVec3::new(0.3, 0.2, 0.0).dot(n) * n + n * (R + 0.001);
    let mut s = sim(sphere(Some(ramp(SLOPE_DEG)), 0.3), state(start, DVec3::ZERO, DQuat::IDENTITY));
    s.run_for(0.5).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    assert!((above_ramp(SLOPE_DEG, pos) - R).abs() < 1e-6, "centre {} from the plane, expected {R}: {pos}", above_ramp(SLOPE_DEG, pos));
    assert!((pos - start).length() > 0.01, "it rolled downhill: {start} -> {pos}");
    let d = s.bus().get(s.bus().lookup::<f64>(names::BODY_TERRAIN_D).unwrap());
    assert!((d + SLOPE_DEG.to_radians().tan() * pos.x).abs() < 1e-6, "body.terrain_d_m {d} under {pos}");
}

/// A quad pitched to lie flat on the ramp, its four feet sunk to the static normal sink `m g cos / (4 k)`.
fn parked(friction_coeff: f64) -> Scheduler {
    let a = SLOPE_DEG.to_radians();
    let sink = MASS * GRAVITY_MPS2 * a.cos() / (4.0 * K);
    // Pitching nose up by the slope puts the body's down axis along minus the ramp's normal.
    let centre = ramp_normal(SLOPE_DEG) * (0.03 - sink);
    sim(legs(Some(ramp(SLOPE_DEG)), friction_coeff), state(centre, DVec3::ZERO, DQuat::from_rotation_y(a)))
}

#[test]
fn landing_points_hold_inside_the_friction_angle() {
    // tan 20 deg = 0.364 < mu = 0.6: static friction holds. The springs then carry the weight along the ramp's
    // normal: the centre sits 0.03 - m g cos / (4 k) above the plane (0.499 mm of sink). A spring along the
    // vertical instead would sink m g / (4 k) = 0.531 mm, 32 um deeper.
    let mut s = parked(0.6);
    let start = vec3(&s, names::BODY_POS_NED);
    s.run_for(2.5).unwrap();
    let mid = vec3(&s, names::BODY_POS_NED);
    s.run_for(2.5).unwrap();
    let end = vec3(&s, names::BODY_POS_NED);
    let moved = (end - start).with_z(0.0).length();
    assert!(moved < 0.005, "it slid {moved} m in 5 s");
    assert!((end - mid).length() < 1e-5, "it crept {} m between 2.5 s and 5 s", (end - mid).length());
    let a = SLOPE_DEG.to_radians();
    let expected = 0.03 - MASS * GRAVITY_MPS2 * a.cos() / (4.0 * K);
    let above = above_ramp(SLOPE_DEG, end);
    assert!((above - expected).abs() < 5e-6, "centre {above} above the plane, expected {expected}");

    // mu = 0.3 < tan 20 deg: it slides at g (sin - mu cos) = 0.59 m/s^2.
    let mut s = parked(0.3);
    let downhill = DVec3::new(-a.cos(), 0.0, a.sin());
    s.run_for(0.2).unwrap();
    let v1 = vec3(&s, names::BODY_VEL_NED).dot(downhill);
    s.run_for(0.4).unwrap();
    let v2 = vec3(&s, names::BODY_VEL_NED).dot(downhill);
    let accel = (v2 - v1) / 0.4;
    let expected = GRAVITY_MPS2 * (a.sin() - 0.3 * a.cos());
    assert!((accel - expected).abs() < 0.05 * expected, "slides at {accel} m/s^2, expected {expected}");
}

#[test]
fn slope_bounce_leaves_along_the_normal() {
    // Dropped from 0.5 m onto the ramp with e = 0.5: the normal impulse reverses half the normal speed, so the
    // sphere leaves with a horizontal component downhill (south, -x), -(1 + e) v cos sin before friction, which
    // takes back at most its sliding share. On the flat plane it would bounce straight up.
    let n = ramp_normal(SLOPE_DEG);
    let start = DVec3::new(0.3, 0.2, -0.5 - SLOPE_DEG.to_radians().tan() * 0.3);
    let mut s = sim(sphere(Some(ramp(SLOPE_DEG)), 0.5), state(start, DVec3::ZERO, DQuat::IDENTITY));
    let mut before = DVec3::ZERO;
    for _ in 0..8000 {
        s.step().unwrap();
        let v = vec3(&s, names::BODY_VEL_NED);
        if v.dot(n) > 0.0 {
            // The bounce tick: leaving at e times the arrival's normal speed (one tick of gravity aside).
            let arrival = -before.dot(n);
            assert!(arrival > 2.0, "it fell onto the ramp at {arrival} m/s");
            assert!((v.dot(n) - 0.5 * arrival).abs() < 0.01 * arrival, "leaves at {} along n, arrived at {arrival}", v.dot(n));
            assert!(v.x < -0.2 * arrival, "leaves downhill (south): {v}");
            assert!(v.y.abs() < 1e-9, "no sideways kick: {v}");
            return;
        }
        before = v;
    }
    panic!("it never bounced");
}
