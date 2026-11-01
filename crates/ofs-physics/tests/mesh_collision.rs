use std::sync::Arc;

use glam::{DQuat, DVec3};
use ofs_core::consts::GRAVITY_MPS2;
use ofs_core::mesh::{Triangle, TriMesh};
use ofs_core::shape::{Aabb, Shape};
use ofs_core::{names, Bus, Scheduler};
use ofs_physics::collision::{self, CollisionParams, TouchState};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody, WorldMesh, WorldObject};

const R: f64 = 0.04;
const MASS: f64 = 0.65;
const K: f64 = 3000.0;
/// The height (down is negative) the walls' tests fly at.
const AIR: f64 = -5.0;

/// A far-away post, so the world has one object and a mesh's index (1) differs from the ground's (-1) and 0.
fn far_post() -> Vec<WorldObject> {
    vec![WorldObject {
        name: "post".into(),
        shape: Shape::Cylinder { center: DVec3::new(500.0, 500.0, -1.5), radius: 0.06, half_height: 1.5 },
    }]
}

/// A rectangle of two triangles through the corners `a`, `b`, `c` and `a + (c - b)`... given as four corners in order.
fn quad(c: [DVec3; 4]) -> [Triangle; 2] {
    [Triangle { a: c[0], b: c[1], c: c[2], material: 0 }, Triangle { a: c[0], b: c[2], c: c[3], material: 0 }]
}

fn world_mesh(name: &str, tris: Vec<Triangle>) -> WorldMesh {
    WorldMesh::new(name.into(), Arc::new(TriMesh::new(tris).unwrap()))
}

/// A wall in the plane north = `at`, east in `e`, down in `z`.
fn wall_rect(at: f64, e: (f64, f64), z: (f64, f64)) -> [Triangle; 2] {
    quad([DVec3::new(at, e.0, z.0), DVec3::new(at, e.1, z.0), DVec3::new(at, e.1, z.1), DVec3::new(at, e.0, z.1)])
}

/// A wall at north = 10, 10 m wide and 6 m tall, with a 2 x 2 m hole centred on east = 0, down = `AIR` (well
/// above the ground, which is the plane d = 0).
fn wall_with_hole() -> WorldMesh {
    let mut tris = Vec::new();
    for (e, z) in [((1.0, 5.0), (-8.0, -2.0)), ((-5.0, -1.0), (-8.0, -2.0)), ((-1.0, 1.0), (-8.0, -6.0)), ((-1.0, 1.0), (-4.0, -2.0))] {
        tris.extend(wall_rect(10.0, e, z));
    }
    world_mesh("wall", tris)
}

fn params(spheres: Vec<(DVec3, f64)>, meshes: Vec<WorldMesh>) -> AirframeParams {
    AirframeParams {
        mass_kg: MASS,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: Vec::new(),
        mounts: vec![],
        contact_points_frd_m: vec![],
        ground: GroundParams { stiffness_npm: K, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects: far_post(),
        collision: CollisionParams { restitution: 0.3, friction_coeff: 0.5, spheres },
        terrain: None,
        meshes,
    }
}

fn state(pos: DVec3, vel: DVec3, att: DQuat) -> BodyState {
    BodyState { pos_ned_m: pos, vel_ned_mps: vel, att, rate_frd_radps: DVec3::ZERO }
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

/// One `collision::resolve` of `s` against `p`, with the touch memory `touch`.
fn resolve(p: &AirframeParams, s: &mut BodyState, touch: &mut TouchState, time_s: f64) -> Vec<(i32, f64)> {
    let bounds: Vec<Aabb> = p.objects.iter().map(|o| o.shape.bounds()).collect();
    let radius = p.collision.spheres.iter().map(|(c, r)| c.length() + r).fold(0.0, f64::max);
    collision::resolve(p, &bounds, radius, s, &[], touch, time_s)
}

#[test]
fn a_sphere_rests_on_a_mesh_floor() {
    // A 20 x 20 m floor at d = -2; the quad starts 0.5 m above it.
    let floor = world_mesh(
        "floor",
        quad([DVec3::new(-10.0, -10.0, -2.0), DVec3::new(10.0, -10.0, -2.0), DVec3::new(10.0, 10.0, -2.0), DVec3::new(-10.0, 10.0, -2.0)]).to_vec(),
    );
    let p = params(vec![(DVec3::ZERO, R)], vec![floor]);
    let objects = p.objects.len() as f64;
    let mut s = sim(p, state(DVec3::new(0.0, 0.0, -2.5), DVec3::ZERO, DQuat::IDENTITY));
    s.run_for(2.0).unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    let gap = (-2.0 - pos.z) - R;
    assert!(gap.abs() < 0.002, "the sphere's lowest point is {gap} m from the floor, at {pos}");
    let object = s.bus().get(s.bus().lookup::<f64>(names::BODY_COLLISION_OBJECT).unwrap());
    assert_eq!(object, objects, "the mesh's collision object index");
}

#[test]
fn resolve_lets_a_sphere_pass_through_a_gap_and_stops_it_at_the_wall() {
    let p = params(vec![(DVec3::ZERO, R)], vec![wall_with_hole()]);
    let mesh_index = p.objects.len() as i32;
    let fast = DVec3::new(5.0, 0.0, 0.0);

    // Through the hole (east 0, down 0): from 9 m to 11 m, 1 cm a step.
    let mut touch = TouchState::new(p.objects.len() + p.meshes.len());
    for k in 0..=200 {
        let mut s = state(DVec3::new(9.0 + 0.01 * k as f64, 0.0, AIR), fast, DQuat::IDENTITY);
        let before = s;
        let events = resolve(&p, &mut s, &mut touch, 0.01 * k as f64);
        assert!(events.is_empty(), "event at {}", s.pos_ned_m);
        assert_eq!(s.pos_ned_m, before.pos_ned_m);
        assert_eq!(s.vel_ned_mps, before.vel_ned_mps);
    }

    // The same line 3 m east hits the wall: pushed back along -north, one event for the mesh.
    let mut touch = TouchState::new(p.objects.len() + p.meshes.len());
    let mut events = Vec::new();
    // Up to 1 cm short of the wall's plane: the contact is two-sided, so a centre beyond it is pushed out the far side.
    for k in 0..=99 {
        let mut s = state(DVec3::new(9.0 + 0.01 * k as f64, 3.0, AIR), fast, DQuat::IDENTITY);
        let before = s;
        events.extend(resolve(&p, &mut s, &mut touch, 0.01 * k as f64));
        let moved = s.pos_ned_m - before.pos_ned_m;
        assert!(moved.x <= 0.0 && moved.y.abs() < 1e-12 && moved.z.abs() < 1e-12, "pushed {moved}");
        assert!(s.pos_ned_m.x <= 10.0 - R + 1e-9, "inside the wall at {}", s.pos_ned_m);
        if before.pos_ned_m.x > 10.0 - R + 1e-6 {
            assert!(moved.x < 0.0 && s.vel_ned_mps.x <= 0.0, "not pushed back: {moved}, v {}", s.vel_ned_mps);
        }
    }
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, mesh_index);
}

#[test]
fn a_landing_point_holds_on_a_20_degree_mesh_ramp() {
    // A 100 m plane rising tan(20 deg) per metre toward north through the origin; four feet 3 cm below the centre.
    let a = 20.0_f64.to_radians();
    let t = a.tan();
    let corner = |n: f64, e: f64| DVec3::new(n, e, AIR - t * n);
    let ramp = world_mesh("ramp", quad([corner(-50.0, -50.0), corner(50.0, -50.0), corner(50.0, 50.0), corner(-50.0, 50.0)]).to_vec());
    let mut p = params(vec![], vec![ramp]);
    p.contact_points_frd_m =
        vec![DVec3::new(-0.1, 0.1, 0.03), DVec3::new(0.1, 0.1, 0.03), DVec3::new(-0.1, -0.1, 0.03), DVec3::new(0.1, -0.1, 0.03)];
    p.ground.friction_coeff = 0.5;
    // Pitched to lie flat on the ramp, the feet at the static spring equilibrium: 4 k (r - d) = m g cos(a).
    let normal = DVec3::new(-a.sin(), 0.0, -a.cos());
    let hover = 0.03 + collision::MESH_POINT_RADIUS_M - MASS * GRAVITY_MPS2 * a.cos() / (4.0 * K);
    let start = DVec3::new(0.0, 0.0, AIR) + normal * hover;
    let mut s = sim(p, state(start, DVec3::ZERO, DQuat::from_rotation_y(a)));
    s.run_for(3.0).unwrap();
    let end = vec3(&s, names::BODY_POS_NED);
    let moved = (end - start).with_z(0.0).length();
    assert!(moved < 0.005, "it slid {moved} m in 3 s");
    assert!((end - start).length() < 0.005, "it sank or crept to {end} from {start}");
}

#[test]
fn a_quad_spawned_inside_a_wall_is_pushed_out() {
    let wall = world_mesh("wall", wall_rect(10.0, (-10.0, 10.0), (-10.0, 10.0)).to_vec());
    let spheres: Vec<(DVec3, f64)> =
        [(-0.1, 0.1), (0.1, 0.1), (-0.1, -0.1), (0.1, -0.1)].iter().map(|(x, y)| (DVec3::new(*x, *y, 0.0), R)).collect();
    let p = params(spheres.clone(), vec![wall]);
    // The wall front sphere's centre is 1 cm from the wall: 3 cm deep.
    let start = DVec3::new(10.0 - 0.01 - 0.1, 0.0, AIR);
    let mut s = sim(p, state(start, DVec3::ZERO, DQuat::IDENTITY));
    s.step().unwrap();
    let pos = vec3(&s, names::BODY_POS_NED);
    for v in [pos, vec3(&s, names::BODY_VEL_NED), vec3(&s, names::BODY_RATE_FRD)] {
        assert!(v.is_finite(), "{v}");
    }
    for (c, r) in &spheres {
        let clear = 10.0 - (pos + *c).x;
        assert!(clear >= r - 1e-6, "sphere at {c} is {clear} from the wall, radius {r}");
    }
}

#[test]
fn a_mesh_far_from_the_quad_costs_no_contact() {
    // The mesh's recorded box is far from the quad while its triangles touch the sphere: the broad phase believes
    // the box, so nothing is queried and nothing moves.
    let mut wall = world_mesh("wall", wall_rect(10.0, (-10.0, 10.0), (-10.0, 10.0)).to_vec());
    wall.bounds = Aabb { min: DVec3::new(100.0, 100.0, 100.0), max: DVec3::new(101.0, 101.0, 101.0) };
    let p = params(vec![(DVec3::ZERO, R)], vec![wall]);
    let mut touch = TouchState::new(p.objects.len() + p.meshes.len());
    let mut s = state(DVec3::new(10.0 - 0.01, 0.0, AIR), DVec3::new(5.0, 0.0, 0.0), DQuat::IDENTITY);
    let before = s;
    let events = resolve(&p, &mut s, &mut touch, 0.0);
    assert!(events.is_empty());
    assert_eq!(s.pos_ned_m, before.pos_ned_m);
    assert_eq!(s.vel_ned_mps, before.vel_ned_mps);

    // With the true box the same state is pushed out.
    let mut near = world_mesh("wall", wall_rect(10.0, (-10.0, 10.0), (-10.0, 10.0)).to_vec());
    near.bounds = near.mesh.bounds();
    let p = params(vec![(DVec3::ZERO, R)], vec![near]);
    let mut s = before;
    resolve(&p, &mut s, &mut touch, 0.0);
    assert!(s.pos_ned_m.x < before.pos_ned_m.x);
}
