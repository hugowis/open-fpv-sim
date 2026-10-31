//! The whole chain over the shipped hills world (#154): the quad hovers 1.5 m over the ground once in clear view of the
//! handset and once behind a hill, and the ELRS link (RSSI, link quality) and the video link (SNR) see the hill. The same
//! two placements in the same world without `[terrain]` hear the same as the clear one, so the drop is the relief's.
use std::path::Path;

use glam::DVec3;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_core::terrain::Terrain;
use ofs_rf::propagation::{terrain_loss_db, wavelength_m};
use ofs_sim::vehicle::{build, BuildOptions};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const ELRS_MHZ: f64 = 2440.0;
const VIDEO_MHZ: f64 = 5800.0;

fn hills() -> WorldConfig {
    ofs_config::world::load(Path::new(&format!("{ROOT}/worlds/hills.toml"))).unwrap()
}

/// The hills world with the relief taken out: same pilot, receiver and handset, ground the plane d = 0.
fn flat() -> WorldConfig {
    WorldConfig { terrain: None, ..hills() }
}

/// What the two links report with the quad hovering at `at`: (ELRS RSSI, LQ, video SNR).
fn signals(world: WorldConfig, at: DVec3) -> (f64, f64, f64) {
    let mut cfg: QuadConfig = load(Path::new(&format!("{ROOT}/quads/opendrone-5r-race.toml"))).unwrap();
    cfg.initial.position_ned_m = [at.x, at.y, at.z];
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    let mut v = build(&cfg, &opts, None).unwrap();
    v.run_for(0.2).unwrap();
    let s = v.state();
    (s.radio.rssi_dbm, s.radio.lq_pct, s.video.snr_db)
}

/// The first hovering spot (1.5 m over the ground, 100 to 280 m from the handset) whose terrain loss at both
/// frequencies satisfies `want`, searched on a ring grid of bearings and ranges.
fn find(terrain: &Terrain, handset: DVec3, want: impl Fn(f64, f64) -> bool) -> DVec3 {
    for range in (100..=280).step_by(10) {
        for bearing in (0..360).step_by(10) {
            let (b, r) = (f64::from(bearing).to_radians(), f64::from(range));
            let (n, e) = (handset.x + r * b.cos(), handset.y + r * b.sin());
            let at = DVec3::new(n, e, terrain.ground_d(n, e) - 1.5);
            if at.z > -1.5 {
                continue; // keep it above the plane d = 0 too, for the flat world
            }
            let loss = |mhz| terrain_loss_db(terrain, handset, at, wavelength_m(mhz));
            if want(loss(ELRS_MHZ), loss(VIDEO_MHZ)) {
                return at;
            }
        }
    }
    panic!("no such spot on the hills");
}

#[test]
fn a_hill_between_the_handset_and_the_quad_lowers_both_links_and_a_flat_world_does_not() {
    let world = hills();
    let handset = DVec3::from(world.handset.position(&world.pilot));
    let terrain = world.terrain.unwrap().build();
    let clear = find(&terrain, handset, |a, b| a == 0.0 && b == 0.0);
    let behind = find(&terrain, handset, |a, b| a > 20.0 && b > 20.0);

    let (rssi_clear, lq_clear, snr_clear) = signals(hills(), clear);
    let (rssi_behind, lq_behind, snr_behind) = signals(hills(), behind);
    println!("clear  {clear:?}: RSSI {rssi_clear:.1} dBm, LQ {lq_clear}, video SNR {snr_clear:.1} dB");
    println!("behind {behind:?}: RSSI {rssi_behind:.1} dBm, LQ {lq_behind}, video SNR {snr_behind:.1} dB");
    assert!(rssi_clear - rssi_behind > 15.0, "RSSI {rssi_clear} in view, {rssi_behind} behind the hill");
    assert!(snr_clear - snr_behind > 15.0, "video SNR {snr_clear} in view, {snr_behind} behind the hill");
    assert!(lq_behind <= lq_clear);

    // The same two placements without the relief: the ground bounce is off d = 0 in both worlds, so what the
    // clear one hears is unchanged, and the hill is the whole of the other's drop.
    let (flat_clear, flat_behind) = (signals(flat(), clear), signals(flat(), behind));
    assert_eq!((rssi_clear, snr_clear), (flat_clear.0, flat_clear.2), "in clear view the hills world sounds like the flat one");
    println!("behind, no relief: RSSI {:.1} dBm, LQ {}, video SNR {:.1} dB", flat_behind.0, flat_behind.1, flat_behind.2);
    assert!(flat_behind.0 - rssi_behind > 15.0 && flat_behind.2 - snr_behind > 15.0, "{flat_behind:?} flat, behind the hill {rssi_behind} {snr_behind}");
}
