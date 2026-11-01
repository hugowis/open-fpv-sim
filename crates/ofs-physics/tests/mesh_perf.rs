//! A timing record, not a CI gate (wall clocks flake): `cargo test --release -p ofs-physics --test mesh_perf -- --ignored --nocapture`.
use std::sync::Arc;
use std::time::Instant;

use glam::{DQuat, DVec3};
use ofs_core::mesh::{Triangle, TriMesh};
use ofs_core::{names, Bus, Scheduler};
use ofs_physics::collision::CollisionParams;
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, RigidBody, WorldMesh};

/// Cells a side: 224 x 224 x 2 = 100 352 triangles.
const CELLS: usize = 224;
const CELL_M: f64 = 0.5;
/// The surface sits this far above the plane d = 0, rippled by 5 cm, so the quad never touches the ground.
const SURFACE_D: f64 = -1.0;
const FLIGHT_S: f64 = 10.0;

/// A rippled grid centred on the origin.
fn synthetic_mesh() -> TriMesh {
    let half = CELLS as f64 * CELL_M / 2.0;
    let node = |i: usize, j: usize| {
        let (n, e) = (i as f64 * CELL_M - half, j as f64 * CELL_M - half);
        DVec3::new(n, e, SURFACE_D + 0.05 * (0.7 * n).sin() * (0.7 * e).sin())
    };
    let mut tris = Vec::with_capacity(CELLS * CELLS * 2);
    for i in 0..CELLS {
        for j in 0..CELLS {
            let (a, b, c, d) = (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
            tris.push(Triangle { a, b, c, material: 0 });
            tris.push(Triangle { a, b: c, c: d, material: 0 });
        }
    }
    TriMesh::new(tris).unwrap()
}

/// A 0.65 kg quad with a body sphere, four prop spheres and four feet, as in `mesh_collision.rs`.
fn params(meshes: Vec<WorldMesh>) -> AirframeParams {
    let mut spheres = vec![(DVec3::ZERO, 0.04)];
    spheres.extend([(-0.1, 0.1), (0.1, 0.1), (-0.1, -0.1), (0.1, -0.1)].iter().map(|(x, y)| (DVec3::new(*x, *y, 0.0), 0.02)));
    AirframeParams {
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
        cda_m2: DVec3::ZERO,
        angular_damping_nms: 0.0,
        rotor_inertia_kgm2: Vec::new(),
        mounts: vec![],
        contact_points_frd_m: vec![DVec3::new(-0.1, 0.1, 0.03), DVec3::new(0.1, 0.1, 0.03), DVec3::new(-0.1, -0.1, 0.03), DVec3::new(0.1, -0.1, 0.03)],
        ground: GroundParams { stiffness_npm: 3000.0, damping_nspm: 40.0, friction_coeff: 0.6 },
        objects: vec![],
        collision: CollisionParams { restitution: 0.3, friction_coeff: 0.5, spheres },
        terrain: None,
        meshes,
    }
}

/// Wall time of `FLIGHT_S` seconds at 8 kHz of a quad dropped 0.3 m onto `surface_d` (bounces, then rests on it).
fn fly(meshes: Vec<WorldMesh>, surface_d: f64) -> f64 {
    let mut bus = Bus::new();
    let start = BodyState { pos_ned_m: DVec3::new(0.0, 0.0, surface_d - 0.3), vel_ned_mps: DVec3::ZERO, att: DQuat::IDENTITY, rate_frd_radps: DVec3::ZERO };
    let body = RigidBody::new(params(meshes), start, &mut bus);
    let mut sch = Scheduler::new(8000, bus);
    sch.add(Box::new(body));
    let t = Instant::now();
    sch.run_for(FLIGHT_S).unwrap();
    let wall = t.elapsed().as_secs_f64();
    // It really landed (and so queried the mesh every step): resting on the surface, not fallen through it.
    let z = sch.bus().get(sch.bus().lookup::<DVec3>(names::BODY_POS_NED).unwrap()).z;
    assert!((z - surface_d).abs() < 0.1, "ended at d = {z}, surface at {surface_d}");
    wall
}

#[test]
#[ignore = "timing record, run by hand in release"]
fn mesh_world_timing_is_recorded() {
    let mesh = Arc::new(synthetic_mesh());
    let triangles = mesh.len();
    let best = |f: &dyn Fn() -> f64| (0..3).map(|_| f()).fold(f64::INFINITY, f64::min);
    let with = best(&|| fly(vec![WorldMesh::new("field".into(), mesh.clone())], SURFACE_D));
    let without = best(&|| fly(vec![], 0.0));
    println!("{FLIGHT_S} s at 8 kHz, best of 3: {triangles} triangles {with:.3} s, no mesh {without:.3} s");
}
