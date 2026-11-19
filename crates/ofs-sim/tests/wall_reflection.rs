//! The video link's wall reflection through the simulator's own world: the obstacles are the ones [`world_obstacles`]
//! builds from a world file (rooted on the ground, so a standing box runs 1000 m below it), not hand-built shapes.
use std::path::Path;

use glam::{DQuat, DVec3};
use ofs_config::world::{ObjectSection, Shape as WorldShape};
use ofs_config::{load, QuadConfig, WorldConfig};
use ofs_core::shape::Shape;
use ofs_sim::vehicle::{link_params, world_obstacles};
use ofs_video::link::{vtx_paths, LinkParams};

/// A 1 m thick, 10 m long (east-west), 3 m tall box standing 5.5 m north of the quad.
fn wall() -> ObjectSection {
    ObjectSection {
        name: "Wall".into(),
        shape: WorldShape::Box,
        center_ned_m: [5.5, 0.0, -1.5],
        size_m: Some([1.0, 10.0, 3.0]),
        radius_m: None,
        height_m: None,
        yaw_deg: None,
        color: [0.6, 0.6, 0.6],
        rf_loss_db: 25.0.into(),
    }
}

#[test]
fn a_world_wall_beside_a_clear_path_reflects_in_the_simulators_link() {
    let mut world = WorldConfig::open_field();
    // The quad is at north 0, east 0 and the goggles 8 m east of it (north 0, east 8), the wall's length east-west
    // between them: the wall is beside the path, never on it.
    world.pilot.position_ned_m = [0.0, 8.0, -1.7];
    world.objects.push(wall());
    let quad: QuadConfig = load(Path::new(&format!("{}/../../quads/opendrone-5r-race.toml", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let vtx = quad.vtx.as_ref().unwrap();
    let params = link_params(vtx, &world, None, &[]).unwrap();
    assert!(params.object_reflections, "the simulator's video link has the reflections on");
    let Shape::Box { half, .. } = world_obstacles(&world, None)[0].shape else { panic!("a box") };
    assert!(half.z > 499.0, "the obstacle is rooted, else the test proves nothing");
    let pos = DVec3::new(0.0, 0.0, -1.7);
    let with = LinkParams { object_reflections: true, ..params.clone() };
    let without = LinkParams { object_reflections: false, ..params };
    let (on, off) = (vtx_paths(&with, pos, DQuat::IDENTITY, 5800.0)[0].gain_db, vtx_paths(&without, pos, DQuat::IDENTITY, 5800.0)[0].gain_db);
    assert!(on > off, "the wall adds a ray: {on} vs {off}");
    let mut clear = world.clone();
    clear.objects.clear();
    assert_eq!(off, vtx_paths(&link_params(vtx, &clear, None, &[]).unwrap(), pos, DQuat::IDENTITY, 5800.0)[0].gain_db, "flag off: the open field's gain");
}
