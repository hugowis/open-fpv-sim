//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-fc --test sitl_live -- --ignored
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler};
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;

const DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.betaflight.diff");

#[test]
#[ignore]
fn still_quad_exchanges_packets_and_stays_disarmed() {
    let launch: Vec<String> = std::env::var("OFS_SITL_LAUNCH").expect("set OFS_SITL_LAUNCH").split_whitespace().map(String::from).collect();
    let mut cleanup: Vec<String> = std::env::var("OFS_SITL_CLEANUP").unwrap_or_default().split_whitespace().map(String::from).collect();
    if cleanup.is_empty() {
        cleanup = net::default_cleanup(&launch);
    }
    let sitl_net = net::resolve(&launch, None, None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut bus = Bus::new();
    let accel = bus.signal::<DVec3>(names::IMU_ACCEL);
    bus.set(accel, DVec3::new(0.0, 0.0, -9.80665));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let pressure = bus.signal::<f64>(names::BARO_PRESSURE);
    bus.set(pressure, 101_325.0);
    let bridge = SitlBridge::start(
        BridgeConfig {
            launch: LaunchConfig { launch, cleanup, workdir: dir.path().join("fc"), diff_file: DIFF.into(), startup_timeout: Duration::from_secs(15) },
            net: sitl_net,
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
        },
        &mut bus,
    )
    .unwrap();
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(bridge));
    s.run_for(2.0).unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 0.0);
    }
}
