use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn throttle(t: f64) -> Sticks {
    Sticks { throttle: t, ..Sticks::default() }
}

#[test]
fn resting_quad_stays_put() {
    let mut v = vehicle(1);
    let start = v.state().pos_ned_m;
    v.run_for(3.0).unwrap();
    let s = v.state();
    assert!((s.pos_ned_m - start).length() < 2e-3, "moved to {}", s.pos_ned_m);
    assert!(s.vel_ned_mps.length() < 1e-3);
    assert!((s.time_s - 3.0).abs() < 1e-12);
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
    assert_eq!(r.rssi_dbm, -50.0, "the quad file's radio.rssi_dbm");
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
