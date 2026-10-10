use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn flat_world() -> WorldConfig {
    ofs_config::world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml"))).unwrap()
}

fn vehicle_in(cfg: &ofs_config::QuadConfig, world: WorldConfig) -> Vehicle {
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    build(cfg, &opts).unwrap()
}

fn throttle(t: f64) -> Sticks {
    Sticks { throttle: t, ..Sticks::default() }
}

#[test]
fn resting_quad_stays_put() {
    let mut v = vehicle(1);
    v.run_for(0.5).unwrap(); // settle onto the collision spheres (the default body sphere is 4 cm)
    let start = v.state().pos_ned_m;
    assert!((start.z + 0.04).abs() < 2e-3, "rests on the body sphere at {}", start);
    v.run_for(3.0).unwrap();
    let s = v.state();
    assert!((s.pos_ned_m - start).length() < 2e-3, "moved to {}", s.pos_ned_m);
    assert!(s.vel_ned_mps.length() < 1e-3);
    assert!((s.time_s - 3.5).abs() < 1e-12);
}

#[test]
fn full_throttle_climbs_and_sags_the_battery() {
    let mut v = vehicle(1);
    v.set_sticks(&throttle(1.0));
    v.run_for(1.0).unwrap();
    let s = v.state();
    assert!(s.pos_ned_m.z < -1.0, "altitude {}", -s.pos_ned_m.z);
    assert!(s.battery_voltage_v < 24.2, "voltage {}", s.battery_voltage_v);
    assert!(s.motor_rpm.iter().all(|r| *r > 20_000.0), "rpm {:?}", s.motor_rpm);
    assert_eq!(s.motor_cmd, vec![1.0; 4]);
}

fn scripted_digest(seed: u64) -> u64 {
    let mut v = vehicle(seed);
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        v.set_sticks(&throttle(t));
        v.run_for(secs).unwrap();
    }
    v.digest()
}

#[test]
fn same_seed_and_inputs_give_identical_runs() {
    assert_eq!(scripted_digest(1), scripted_digest(1));
    assert_ne!(scripted_digest(1), scripted_digest(2));
}

#[test]
fn a_new_vehicle_starts_with_default_sticks() {
    // Aux channels left at 0.0 would reach SITL as 1500 us until the first SetSticks.
    let built = vehicle(1);
    let mut explicit = vehicle(1);
    explicit.set_sticks(&Sticks::default());
    assert_eq!(built.digest(), explicit.digest());
}

#[test]
fn firmware_dirs_differ_for_same_named_quads_in_different_folders() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for d in [&a, &b] {
        std::fs::write(d.path().join("quad.toml"), "").unwrap();
    }
    let data = Path::new("data");
    let da = firmware_dir(data, &a.path().join("quad.toml"));
    let db = firmware_dir(data, &b.path().join("quad.toml"));
    assert_ne!(da, db);
    assert!(da.file_name().unwrap().to_string_lossy().starts_with("quad-"), "{da:?}");
    assert_eq!(da, firmware_dir(data, &a.path().join("quad.toml")), "stable");
}

#[test]
fn the_radio_link_is_up_while_the_transmitter_is_on() {
    let mut v = vehicle(1);
    v.run_for(0.5).unwrap();
    let r = v.state().radio;
    assert!(r.tx_enabled && r.link_up, "{r:?}");
    assert_eq!(r.lq_pct, 100.0);
    // RSSI comes from the geometry (M3c): 250 mW, 2 dBi dipoles, the pad about a metre and a half from the
    // open field's handset, fading on. The old fixed radio.rssi_dbm field is gone from the quad schema.
    assert!((-40.0..=-30.0).contains(&r.rssi_dbm), "rssi at the pad: {r:?}");
    v.set_transmitter(false);
    v.run_for(0.5).unwrap();
    let r = v.state().radio;
    assert!(!r.tx_enabled && !r.link_up, "{r:?}");
}

#[test]
fn step_ticks_advances_exactly_that_many_base_ticks() {
    let mut v = vehicle(1);
    v.step_ticks(80).unwrap();
    assert!((v.time_s() - 80.0 / 8000.0).abs() < 1e-12, "{}", v.time_s());
    v.step_ticks(0).unwrap();
    assert!((v.state().time_s - 0.01).abs() < 1e-12);
}

#[test]
fn the_link_loss_fault_drops_the_link_until_cleared() {
    let mut v = vehicle(1);
    v.set_fault(Fault::RadioLinkLoss, true);
    v.run_for(0.5).unwrap();
    assert!(!v.state().radio.link_up);
    v.clear_faults();
    v.run_for(0.1).unwrap();
    assert!(v.state().radio.link_up);
}

#[test]
fn a_relative_and_an_absolute_path_to_one_quad_share_a_firmware_dir() {
    // Tests run in the crate directory.
    let data = Path::new("data");
    let absolute = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert_eq!(firmware_dir(data, Path::new("Cargo.toml")), firmware_dir(data, &absolute));
}

#[test]
fn a_quad_dropped_onto_building_a_raises_one_collision_and_rests_on_the_roof() {
    // Open loop cannot steer, so the quad falls 0.55 m onto BuildingA's roof (its top is the plane z = -14). The
    // 0.3 default restitution's rebound of a 0.55 m drop stays under the 1 m/s event threshold, so the one touch
    // raises exactly one event (a 1.5 m drop bounces and raises two, and the state keeps the last one's speed).
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.initial.position_ned_m = [110.0, -45.0, -14.55];
    let building_a = flat_world().objects.iter().position(|o| o.name == "BuildingA").unwrap() as i32;
    let mut v = vehicle_in(&cfg, flat_world());
    v.run_for(5.0).unwrap();
    let s = v.state();
    assert_eq!(s.collision_object, building_a, "the event names BuildingA");
    assert!(s.collision_speed_mps > 3.0, "the fall was hard: {}", s.collision_speed_mps);
    assert!(s.vel_ned_mps.length() < 1e-2, "at rest: {}", s.vel_ned_mps);
    assert!(s.pos_ned_m.z < -14.0 && s.pos_ned_m.z > -14.2, "on the roof, never inside it: {}", s.pos_ned_m.z);
}

#[test]
fn fifty_kilometres_out_at_ten_milliwatts_the_link_never_comes_up() {
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.radio.tx_power_mw = 10;
    cfg.initial.position_ned_m = [50_000.0, 0.0, -0.03];
    let mut v = vehicle_in(&cfg, WorldConfig::open_field());
    v.run_for(1.0).unwrap();
    let s = v.state();
    assert!(!s.radio.link_up, "no packet survives 15 dB under the sensitivity");
    assert_eq!(s.radio.lq_pct, 0.0);
}
