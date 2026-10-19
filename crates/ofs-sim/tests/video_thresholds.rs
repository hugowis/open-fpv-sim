//! The world file's `[video]` thresholds reach the link model (docs/research/video-link.md).
use std::path::Path;

use ofs_config::world::VideoSection;
use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, receiver_thresholds, BuildOptions, Sticks};
use ofs_video::link::ReceiverThresholds;

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

/// The freestyle quad flown open loop for 0.3 s at 40 % throttle over the given world; returns the run's digest.
fn digest(world: WorldConfig) -> u64 {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.set_sticks(&Sticks { throttle: 0.4, ..Sticks::default() }).unwrap();
    v.run_for(0.3).unwrap();
    v.digest()
}

#[test]
fn the_default_world_thresholds_equal_the_links_defaults() {
    assert_eq!(receiver_thresholds(&VideoSection::default()), ReceiverThresholds::default());
}

#[test]
fn a_stricter_clean_threshold_changes_the_flight_record() {
    let stock = digest(WorldConfig::open_field());
    let mut strict = WorldConfig::open_field();
    strict.video.clean_snr_db = 60.0;
    assert_ne!(digest(strict), stock, "a picture that needs 60 dB to be clean is not clean at 20 m");
    assert_eq!(digest(WorldConfig::open_field()), stock, "and the default world is repeatable");
}
