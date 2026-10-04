use std::path::Path;

use ofs_config::{load, FcKind};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop) };
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
