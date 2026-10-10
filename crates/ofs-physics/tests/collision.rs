use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler};
use ofs_physics::collision::{CollisionParams, TouchState, GROUND_OBJECT_INDEX};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody, WorldObject};

/// A post 12 cm across, 3 m tall, standing at `north`, `east`.
fn post(north: f64, east: f64) -> Vec<WorldObject> {
    vec![WorldObject {
        name: "post".into(),
        shape: ofs_core::shape::Shape::Cylinder { center: DVec3::new(north, east, -1.5), radius: 0.06, half_height: 1.5 },
    }]
}

/// A roof to land on: a box whose top face is the plane z = -1.
fn roof() -> Vec<WorldObject> {
    vec![WorldObject {
        name: "roof".into(),
        shape: ofs_core::shape::Shape::Box { center: DVec3::new(0.0, 0.0, -0.5), half: DVec3::new(5.0, 5.0, 0.5) },
    }]
}

fn params(objects: Vec<WorldObject>, spheres: Vec<(DVec3, f64)>, restitution: f64) -> AirframeParams {
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: 0.0,
        mounts: vec![],
        contact_points_frd_m: vec![],
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects,
        collision: CollisionParams { restitution, friction_coeff: 0.5, spheres },
    }
}

fn state(pos: DVec3, vel: DVec3) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: vel, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO }
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

fn scalar(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

/// The one sphere of these rigs sits at the centre of mass.
const BODY_SPHERE: DVec3 = DVec3::ZERO;
const R: f64 = 0.04;

#[test]
fn a_dropped_sphere_bounces_to_restitution_squared_of_its_height() {
    let mut s = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.5), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    // The fall takes about 0.45 s and the rise about 0.22 s; track the highest point (the lowest z) of the
    // rebound, once the ball has first touched (the drop itself starts at the overall highest point).
    let mut touched = false;
    let mut apex = f64::MAX;
    for _ in 0..9600 {
        s.step().unwrap();
        let z = vec3(&s, names::BODY_POS_NED).z;
        touched |= z > -2.0 * R;
        if touched {
            apex = apex.min(z);
        }
    }
    // The CoM falls 0.96 m, touches at z = -R, bounces at e = 0.5 and rises e^2 * 0.96 = 0.24 m (a sphere, so
    // nothing rotates): the rebound's apex sits at z = -0.28.
    assert!((apex + 0.28).abs() < 0.0125, "apex {apex}, expected -0.28 +- 5%");
}

#[test]
fn a_quad_at_30_mps_into_a_post_never_ends_up_past_it() {
    let mut s = sim(params(post(10.0, 0.0), vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(9.0, 0.0, -1.5), DVec3::new(30.0, 0.0, 0.0)));
    s.run_for(0.5).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    assert!(pos.x < 10.0, "the body stopped before the post's centre: {pos}");
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), 0.0, "the event names the post");
    assert!(scalar(&s, names::BODY_COLLISION_SPEED) > 25.0, "at impact speed {}", scalar(&s, names::BODY_COLLISION_SPEED));
}

#[test]
fn a_quad_rests_on_a_roof_with_spheres_and_legs_for_ten_seconds() {
    let mut p = params(roof(), vec![(BODY_SPHERE, R)], 0.3);
    p.contact_points_frd_m = vec![
        DVec3::new(-0.08, 0.08, 0.03),
        DVec3::new(0.08, 0.08, 0.03),
        DVec3::new(-0.08, -0.08, 0.03),
        DVec3::new(0.08, -0.08, 0.03),
    ];
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -1.05), DVec3::ZERO));
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
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.length() < 1e-2, "at rest: {vel}");
    // Above the roof's top face, never inside it.
    let pos = samples[samples.len() - 1];
    assert!(pos.z < -1.0 && pos.z > -1.2, "resting on the roof: {pos}");
}

#[test]
fn a_prop_sphere_clipping_a_post_starts_a_rotation() {
    // A sphere out on the left front arm, a post 5 cm off its path: an off-centre hit.
    let spheres = vec![(DVec3::new(0.08, -0.08, 0.0), 0.015), (DVec3::new(-0.08, 0.08, 0.0), 0.015), (BODY_SPHERE, R)];
    let mut p = params(post(0.3, -0.03), spheres, 0.3);
    p.contact_points_frd_m = vec![DVec3::new(0.0, 0.0, 0.03)];
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -1.5), DVec3::new(10.0, 0.0, 0.0)));
    let mut worst: f64 = 0.0;
    for _ in 0..4000 {
        s.step().unwrap();
        worst = worst.max(vec3(&s, names::BODY_RATE_FRD).length());
    }
    assert!(worst > 2.0, "the clip spun the quad: worst rate {worst} rad/s");
}

#[test]
fn friction_stops_a_sliding_sphere() {
    let mut p = params(vec![], vec![(BODY_SPHERE, R)], 0.0);
    // A free sphere would spin up under the friction and roll on (Coulomb friction does no work once rolling);
    // give it a huge inertia so it cannot spin and the friction has to stop the slide.
    p.inertia_kgm2 = DVec3::splat(1.0e6);
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -R), DVec3::new(1.0, 0.0, 0.0)));
    s.run_for(0.5).unwrap();
    let vel = vec3(&s, names::BODY_VEL_NED);
    assert!(vel.x.abs() < 0.01, "sliding stopped: vel {vel}");
    assert!(vec3(&s, names::BODY_POS_NED).x < 0.15, "it slid less than mu*g would allow");
}

#[test]
fn a_bounce_raises_one_event_per_touching_spell() {
    // e = 0.3 from 1 m: impacts at 4.4 and 1.3 m/s (both >= 1), the third at 0.4 m/s is silent.
    let mut s = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    s.run_for(3.0).unwrap();
    assert_eq!(scalar(&s, names::BODY_COLLISION_COUNT), 2.0, "two hard touches");
    assert_eq!(scalar(&s, names::BODY_COLLISION_OBJECT), f64::from(GROUND_OBJECT_INDEX));
}

#[test]
fn a_new_touch_within_the_rearm_window_raises_nothing() {
    let p = params(vec![], vec![(BODY_SPHERE, R)], 0.0);
    let bounds = Vec::new();
    let mut touch = TouchState::new(0);
    // Touch hard, leave, touch hard again 10 ms later: still one event. After 20 ms free: the next touch raises.
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.0).len(), 1);
    let mut s = state(DVec3::new(0.0, 0.0, -R - 0.1), DVec3::ZERO);
    assert!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.001).is_empty(), "in the air: free");
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.011).len(), 0, "10 ms after leaving: rearmed not yet");
    let mut s = state(DVec3::new(0.0, 0.0, -R - 0.1), DVec3::ZERO);
    assert!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.012).is_empty());
    let mut s = state(DVec3::new(0.0, 0.0, -R + 0.001), DVec3::new(0.0, 0.0, 2.0));
    assert_eq!(ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.05).len(), 1, "38 ms after leaving: rearmed");
}

#[test]
fn a_rotated_hit_turns_the_body_through_the_body_frame_inertia() {
    // The wall's near face is the plane x = 0 (its contact normal is -x) and the quad is pitched 90 deg up
    // (att = from_rotation_x(FRAC_PI_2), so R maps body (x, y, z) to world (x, -z, y)). It carries one sphere
    // at body (0.1, 0.06, 0.03), r = 0.02: at pos (-0.11, 0.03, -0.16) the sphere's centre sits at
    // (-0.01, 0, -0.1), 1 cm into the face and clear of the ground, and the body slides into the wall at
    // 1 m/s with no tangential speed, so the step raises exactly one normal impulse.
    let p = params(
        vec![WorldObject {
            name: "wall".into(),
            shape: ofs_core::shape::Shape::Box { center: DVec3::new(1.0, 0.0, -5.0), half: DVec3::new(1.0, 2.0, 5.0) },
        }],
        vec![(DVec3::new(0.1, 0.06, 0.03), 0.02)],
        0.3,
    );
    let bounds = p.objects.iter().map(|o| o.shape.bounds()).collect::<Vec<_>>();
    let mut touch = TouchState::new(p.objects.len());
    let mut s = state(DVec3::new(-0.11, 0.03, -0.16), DVec3::new(1.0, 0.0, 0.0));
    s.att = DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2);
    let events = ofs_physics::collision::resolve(&p, &bounds, &mut s, &mut touch, 0.0);
    assert_eq!(events.len(), 1, "one hard touch");

    // Hand-computed, with inv_mass = 20/13, inv_inertia = (400, 400, 2000/9), e = 0.3. The position correction
    // (0.01 along -x) leaves the lever at point - pos = (0.13, -0.03, 0.06), and the normal impulse is J = n*j:
    //   lever x n = (0, -0.06, -0.03)
    //   kn = inv_mass + n . ((R diag(I^-1) R^T (lever x n)) x lever):
    //     R^T . -> (0, -0.03, 0.06); diag . -> (0, -12, 13.33); R . -> (0, -13.33, -12);
    //     . x lever -> (-1.16, -1.56, 1.73); n . -> 1.16   =>  kn = 20/13 + 1.16, j = 1.3/kn = 0.4818
    // That spin gives the contact point a tangential speed, so a Coulomb friction impulse follows at the same
    // lever; both impulses are recovered exactly from the linear velocity change (whose update carries no
    // attitude): J_total = m * dv, with J_total . n = j (the friction impulse is tangential). The stored rate is
    // body-frame, so it must be diag(I^-1) (R^T (lever x J_total)) — for the normal impulse alone that is
    // (0, -12 j, 13.33 j), while applying the diagonal in the world frame instead would give
    // R^T (diag(I^-1) (lever x J_total)) = (0, -6.67 j, 24 j): a different direction, not just a magnitude.
    let v0 = DVec3::new(1.0, 0.0, 0.0);
    let dv = s.vel_ned_mps - v0;
    let j = p.mass_kg * dv.dot(DVec3::NEG_X);
    assert!((j - 1.3 / (1.0 / 0.65 + 1.16)).abs() < 1e-9, "normal impulse {j}");
    let impulse = p.mass_kg * dv;
    let lever = DVec3::new(0.13, -0.03, 0.06);
    let expected =
        DVec3::new(400.0, 400.0, 2000.0 / 9.0) * (DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2).inverse() * lever.cross(impulse));
    assert!((s.rate_frd_radps - expected).length() < 1e-9, "rate {} expected {expected}", s.rate_frd_radps);
}

#[test]
fn contacts_with_no_objects_leave_the_body_alone() {
    // A body far from any object behaves exactly as with an empty world.
    let far = vec![WorldObject {
        name: "far".into(),
        shape: ofs_core::shape::Shape::Box { center: DVec3::new(1000.0, 0.0, -5.0), half: DVec3::new(5.0, 5.0, 5.0) },
    }];
    let mut with = sim(params(far, vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    let mut without = sim(params(vec![], vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(0.0, 0.0, -1.0), DVec3::ZERO));
    with.run_for(1.0).unwrap();
    without.run_for(1.0).unwrap();
    assert_eq!(with.bus().digest(), without.bus().digest(), "the broad phase skipped the far object entirely");
}

#[test]
fn the_same_impacts_give_bit_identical_states() {
    let scripted = || {
        let mut s = sim(params(post(10.0, 0.0), vec![(BODY_SPHERE, R)], 0.3), state(DVec3::new(9.0, 0.0, -1.5), DVec3::new(30.0, 0.0, 0.0)));
        s.run_for(0.2).unwrap();
        s.bus().digest()
    };
    assert_eq!(scripted(), scripted());
}
