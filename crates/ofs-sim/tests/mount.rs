//! The flight controller's mount inside a whole vehicle (docs/research/rotor-dynamics.md, "Frame response").
use std::path::Path;

use ofs_config::{load, part, FcKind, FcMountSection, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const SOFT_FRAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library/frames/opendrone-5f-softmount.toml");

/// The freestyle quad flown open loop for half a second at 60 % throttle, with the frame's mount as given and,
/// optionally, a damaged prop shaking the IMU; returns the run's digest.
fn digest(mount: Option<FcMountSection>, imbalance_kgm: f64) -> u64 {
    let mut cfg = load(Path::new(QUAD)).unwrap();
    cfg.frame.fc_mount = mount;
    let opts = BuildOptions {
        seed: 1,
        data_dir: std::env::temp_dir().join("ofs-test-data"),
        fc_override: Some(FcKind::OpenLoop),
        world: WorldConfig::open_field(),
    };
    let mut v: Vehicle = build(&cfg, &opts, None).unwrap();
    if imbalance_kgm > 0.0 {
        v.set_fault(Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm }, true).unwrap();
    }
    v.set_sticks(&Sticks { throttle: 0.6, ..Sticks::default() }).unwrap();
    v.run_for(0.5).unwrap();
    v.digest()
}

fn soft() -> Option<FcMountSection> {
    Some(FcMountSection { natural_hz: 120.0, damping_ratio: 0.15 })
}

#[test]
fn a_mount_with_no_imbalance_changes_nothing() {
    assert_eq!(digest(soft(), 0.0), digest(None, 0.0));
}

#[test]
fn a_mount_changes_what_a_damaged_prop_does_to_the_imu() {
    assert_ne!(digest(soft(), 5.0e-4), digest(None, 5.0e-4));
}

#[test]
fn the_softmount_library_frame_loads() {
    let frame = part::load_frame(Path::new(SOFT_FRAME)).unwrap();
    let mount = frame.frame.fc_mount.expect("the part carries a mount");
    assert_eq!((mount.natural_hz, mount.damping_ratio), (120.0, 0.15));
}
