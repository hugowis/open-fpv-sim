//! The magnetometer inside a whole vehicle (docs/superpowers/specs/2026-10-25-compass-design.md): a quad that carries
//! one publishes the field and the heading, the world's declination shows in the heading, and a quad without one is
//! untouched.
use std::f64::consts::TAU;
use std::fs;
use std::path::Path;

use ofs_config::{load, load_with_library_env, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Vehicle};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The freestyle quad at schema 7 with the shipped magnetometer and the given initial yaw, written to a temp dir that
/// sees the shipped library.
fn vehicle(yaw_deg: f64, world: WorldConfig) -> Vehicle {
    let dir = tempfile::tempdir().unwrap();
    let text = fs::read_to_string(format!("{ROOT}/quads/opendrone-5f-freestyle.toml"))
        .unwrap()
        .replace("\r\n", "\n")
        .replace("schema_version = 6", "schema_version = 7")
        .replace("baro = \"opendrone-baro\"\n", "baro = \"opendrone-baro\"\nmag = \"opendrone-mag\"\n")
        .replace("yaw_deg = 0.0", &format!("yaw_deg = {yaw_deg}"));
    let path = dir.path().join("quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.path().join("opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    let cfg = load_with_library_env(&path, Some(&format!("{ROOT}/library"))).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    build(&cfg, &opts, None).unwrap()
}

fn angle_diff(a: f64, b: f64) -> f64 {
    (a - b + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

#[test]
fn the_heading_tracks_the_initial_yaw_less_the_declination() {
    // Noise is 0.3 uT on a ~25 uT horizontal field: about a degree, so allow 2.5 degrees.
    for yaw in [0.0f64, 90.0, 215.0] {
        let mut v = vehicle(yaw, WorldConfig::open_field());
        v.run_for(0.1).unwrap();
        let heading = v.signal("mag.heading_rad").expect("the quad carries a magnetometer");
        let want = (yaw - 2.0).to_radians();
        assert!(angle_diff(heading, want).abs() < 2.5f64.to_radians(), "yaw {yaw}: heading {} deg", heading.to_degrees());
    }
}

#[test]
fn the_worlds_declination_moves_the_heading() {
    let mut world = WorldConfig::open_field();
    world.environment.magnetic_declination_deg = -10.0;
    let mut v = vehicle(0.0, world);
    v.run_for(0.1).unwrap();
    let heading = v.signal("mag.heading_rad").unwrap();
    assert!(angle_diff(heading, 10f64.to_radians()).abs() < 2.5f64.to_radians(), "{} deg", heading.to_degrees());
}

#[test]
fn a_quad_without_a_magnetometer_registers_no_compass_signal() {
    let cfg = load(Path::new(&format!("{ROOT}/quads/opendrone-5f-freestyle.toml"))).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    let v = build(&cfg, &opts, None).unwrap();
    assert!(v.signal("mag.heading_rad").is_none() && v.signal("mag.field_frd_ut").is_none());
}
