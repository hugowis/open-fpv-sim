//! Every built vehicle keeps the last ten seconds of its default channel table (the repro bundle's MCAP).
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

#[test]
fn a_built_vehicle_has_a_tail_of_its_last_ten_seconds() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 3, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    let mut v = build(&cfg, &opts).unwrap();
    v.set_sticks(&Sticks { throttle: 0.4, ..Sticks::default() });
    v.run_for(12.0).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tail.mcap");
    let summary = v.snapshot_tail(&path).unwrap();
    assert!(summary.first_s >= 2.0 - 1e-9 && summary.first_s < 2.1, "{summary:?}");
    assert!(summary.last_s <= 12.0 && 12.0 - summary.last_s < 0.05, "{summary:?}");
    let bytes = std::fs::read(&path).unwrap();
    let topics: std::collections::BTreeSet<String> =
        mcap::MessageStream::new(&bytes).unwrap().map(|m| m.unwrap().channel.topic.clone()).collect();
    for wanted in ["body.pos_ned_m", "battery.voltage_v", "motor.0.omega_radps"] {
        assert!(topics.contains(wanted), "{wanted} missing from {topics:?}");
    }
}

#[test]
fn the_tail_does_not_change_the_run() {
    // The same vehicle with and without a snapshot taken in the middle ends on the same digest.
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 3, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    let mut a = build(&cfg, &opts).unwrap();
    let mut b = build(&cfg, &opts).unwrap();
    let dir = tempfile::tempdir().unwrap();
    a.run_for(1.0).unwrap();
    b.run_for(0.5).unwrap();
    b.snapshot_tail(&dir.path().join("mid.mcap")).unwrap();
    b.run_for(0.5).unwrap();
    assert_eq!(a.digest(), b.digest());
}
