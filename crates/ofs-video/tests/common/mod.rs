//! Shared scaffolding for the live tests: a level, still quad on the bus and a bridge to a fresh SITL.
#![allow(dead_code)]
use std::path::Path;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Scheduler};
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SerialTap, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;

pub fn argv(var: &str) -> Vec<String> {
    std::env::var(var).unwrap_or_default().split_whitespace().map(String::from).collect()
}

/// `source` builds a model that runs before the bridge in every tick (it gets the bus to register its signals).
pub fn still_quad(
    dir: &Path,
    diff: &str,
    serial: Vec<SerialLink>,
    taps: Vec<SerialTap>,
    source: impl FnOnce(&mut Bus) -> Option<Box<dyn Model>>,
) -> Scheduler {
    let launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    let mut cleanup = argv("OFS_SITL_CLEANUP");
    if cleanup.is_empty() {
        cleanup = net::default_cleanup(&launch);
    }
    let sitl_net = net::resolve(&launch, None, None).unwrap();
    let mut bus = Bus::new();
    let accel = bus.signal::<DVec3>(names::IMU_ACCEL);
    bus.set(accel, DVec3::new(0.0, 0.0, -9.80665));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let pressure = bus.signal::<f64>(names::BARO_PRESSURE);
    bus.set(pressure, 101_325.0);
    let source = source(&mut bus);
    let bridge = SitlBridge::start(
        BridgeConfig {
            launch: LaunchConfig {
                launch,
                cleanup,
                workdir: dir.join("fc"),
                diff_file: diff.into(),
                startup_timeout: Duration::from_secs(15),
            },
            net: sitl_net,
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
            serial,
            taps,
        },
        &mut bus,
    )
    .unwrap();
    let mut s = Scheduler::new(8000, bus);
    if let Some(m) = source {
        s.add(m);
    }
    s.add(Box::new(bridge));
    s
}
