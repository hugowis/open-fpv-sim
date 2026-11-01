//! Flying in the shipped hall world: a seeded flight into its south wall replays exactly (and differs from the open
//! field's), and the wall takes the video signal while the doorway does not.
use std::path::Path;

use glam::{DQuat, DVec3};

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, link_params, BuildOptions, Sticks};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn hall() -> WorldConfig {
    ofs_config::world::load(Path::new(&format!("{ROOT}/worlds/hall.toml"))).unwrap()
}

fn quad() -> QuadConfig {
    load(Path::new(&format!("{ROOT}/quads/opendrone-5f-freestyle.toml"))).unwrap()
}

/// The quad starts on the ground 6 m south of the wall, off to the side of the doorway, and flies north into it.
/// Returns the digest, the number of collisions and the number of meshes the vehicle holds.
fn fly(world: WorldConfig) -> (u64, u64, usize) {
    let mut cfg = quad();
    cfg.initial.position_ned_m = [24.0, 4.0, -0.03];
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoopStabilized), world };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.set_sticks(&Sticks { throttle: 0.45, pitch: 1.0, ..Sticks::default() }).unwrap();
    v.run_for(4.0).unwrap();
    (v.digest(), v.state().collision_event_count, v.meshes().len())
}

#[test]
fn a_flight_in_the_hall_world_replays_exactly() {
    let (digest, hits, meshes) = fly(hall());
    assert_eq!(meshes, 1);
    assert!(hits > 0, "the quad must reach the wall for this test to exercise the mesh");
    assert_eq!(fly(hall()).0, digest, "the same seed replays the same flight");
    assert_ne!(fly(WorldConfig::open_field()).0, digest, "the wall changes the flight");
}

#[test]
fn the_hall_wall_blocks_the_video_link_and_the_doorway_does_not() {
    let world = hall();
    let meshes = world.load_meshes().unwrap();
    let params = link_params(quad().vtx.as_ref().unwrap(), &world, None, &meshes).unwrap();
    let obstruction = |north: f64, east: f64| ofs_video::link::vtx_paths(&params, DVec3::new(north, east, -1.7), DQuat::IDENTITY, 5800.0)[0].obstruction_db;
    // The pilot is at (5, 0): the line to (40, 4) crosses the south wall (north 30) at east 2.9, outside the doorway.
    let behind = obstruction(40.0, 4.0);
    assert!(behind >= 18.0, "behind the wall {behind} dB");
    assert_eq!(obstruction(40.0, 0.0), 0.0, "through the doorway");
}
