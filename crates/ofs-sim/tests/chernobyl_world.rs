//! Flying in the shipped Chernobyl world (the New Safe Confinement arch, one door at its west end): a quad flies in
//! through the door untouched, hits the end wall when it is 30 m off the door's axis, the flight replays exactly, and
//! the arch's skin takes the video signal while the door does not.
use std::path::Path;
use std::time::Instant;

use glam::{DQuat, DVec3};

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_core::mesh::MeshHit;
use ofs_sim::vehicle::{build, link_params, BuildOptions, Sticks};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
/// The pilot's eye, 150 m west of the door.
const PILOT: DVec3 = DVec3::new(0.0, -231.0, -1.7);

fn chernobyl() -> WorldConfig {
    ofs_config::world::load(Path::new(&format!("{ROOT}/worlds/chernobyl.toml"))).unwrap()
}

fn quad() -> QuadConfig {
    load(Path::new(&format!("{ROOT}/quads/opendrone-7lr-longrange.toml"))).unwrap()
}

/// A flight: the 7-inch quad starts 100 m west of the door, `north` m off its axis, yawed east at 2 m, and flies forward.
struct Flight {
    digest: u64,
    hits: u64,
    first_object: Option<i32>,
    end: DVec3,
}

fn fly(world: WorldConfig, north: f64, seconds: f64) -> Flight {
    let mut cfg = quad();
    cfg.initial.position_ned_m = [north, -181.0, -2.0];
    cfg.initial.yaw_deg = 90.0;
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoopStabilized), world };
    let mut v = build(&cfg, &opts, None).unwrap();
    // Throttle this low and a half stick forward: the stabilised quad climbs slowly east at about 20 m/s, so it crosses
    // the door's plane (east -81) some 10 m up and is deep inside the arch after 10 s.
    v.set_sticks(&Sticks { throttle: 0.28, pitch: 0.5, ..Sticks::default() }).unwrap();
    let mut first_object = None;
    for _ in 0..(seconds * 100.0) as usize {
        v.run_for(0.01).unwrap();
        if first_object.is_none() && v.state().collision_event_count > 0 {
            first_object = Some(v.state().collision_object);
        }
    }
    Flight { digest: v.digest(), hits: v.state().collision_event_count, first_object, end: v.state().pos_ned_m }
}

const SECONDS: f64 = 10.0;

#[test]
fn the_world_loads_with_one_mesh() {
    let world = chernobyl();
    let meshes = world.load_meshes().unwrap();
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].name, "chernobyl");
}

#[test]
fn the_pilot_is_not_inside_geometry() {
    let meshes = chernobyl().load_meshes().unwrap();
    let mut hits: Vec<MeshHit> = Vec::new();
    meshes[0].mesh.within(PILOT, 2.0, &mut hits);
    assert!(hits.is_empty(), "{} triangles within 2 m of the pilot", hits.len());
}

#[test]
fn a_quad_flies_in_through_the_west_door_untouched() {
    let f = fly(chernobyl(), 0.0, SECONDS);
    assert_eq!(f.hits, 0, "first hit on object {:?}", f.first_object);
    // The door is at east -81; 20 m inside is -61. The arch's far wall is at +81.
    assert!(f.end.y > -61.0 && f.end.y < 81.0, "ended at east {}", f.end.y);
}

#[test]
fn the_same_flight_30_m_off_the_door_axis_hits_the_end_wall() {
    let f = fly(chernobyl(), 30.0, SECONDS);
    assert!(f.hits > 0, "ended at {:?}", f.end);
    // Meshes follow the objects in the collision indices: the first hit is the arch, not a ground bounce.
    assert_eq!(f.first_object, Some(chernobyl().objects.len() as i32));
    assert!(f.end.y < -61.0, "stopped outside the arch at east {}", f.end.y);
}

#[test]
fn the_flight_replays_exactly() {
    let digest = fly(chernobyl(), 0.0, SECONDS).digest;
    assert_eq!(fly(chernobyl(), 0.0, SECONDS).digest, digest, "the same seed replays the same flight");
    assert_ne!(fly(WorldConfig::open_field(), 0.0, SECONDS).digest, digest, "the arch changes the flight");
}

#[test]
fn the_video_link_is_cut_by_the_arch_skin_and_not_through_the_door() {
    let world = chernobyl();
    let meshes = world.load_meshes().unwrap();
    let params = link_params(quad().vtx.as_ref().unwrap(), &world, None, &meshes).unwrap();
    let obstruction = |north: f64, east: f64| ofs_video::link::vtx_paths(&params, DVec3::new(north, east, -2.0), DQuat::IDENTITY, 5800.0)[0].obstruction_db;
    // The pilot is at PILOT and the quad at the given point: the line to (40, -40) crosses the end wall (east -81) at
    // north 31, outside the door's +-20 m.
    let behind = obstruction(40.0, -40.0);
    assert!(behind >= 40.0, "behind the end wall {behind} dB");
    assert_eq!(obstruction(0.0, -40.0), 0.0, "straight through the door");
}

#[test]
#[ignore = "timing record, run by hand in release"]
fn chernobyl_timing_is_recorded() {
    let triangles = chernobyl().load_meshes().unwrap()[0].mesh.len();
    let best = |world: fn() -> WorldConfig| (0..3).map(|_| { let t = Instant::now(); fly(world(), 0.0, SECONDS); t.elapsed().as_secs_f64() }).fold(f64::INFINITY, f64::min);
    let inside = best(chernobyl);
    let open = best(WorldConfig::open_field);
    println!("{SECONDS} s of flight, best of 3: chernobyl ({triangles} triangles) {inside:.3} s, open field {open:.3} s");
}
