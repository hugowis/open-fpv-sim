//! The radio chain in a whole vehicle: the quad file picks the band; the band tables stay in step with the
//! config's own copy.
use std::path::Path;

use ofs_config::{load, BandKind, FcKind, WorldConfig};
use ofs_radio::band::Band;
use ofs_sim::vehicle::{build, BuildOptions, Vehicle};

fn quad(file: &str) -> Vehicle {
    let path = format!("{}/../../quads/{file}", env!("CARGO_MANIFEST_DIR"));
    let cfg = load(Path::new(&path)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts, None).unwrap()
}

#[test]
fn the_config_and_the_model_agree_on_every_bands_packet_rates() {
    for (kind, band) in [(BandKind::G2_4, Band::G2_4), (BandKind::Mhz900, Band::Mhz900)] {
        assert_eq!(kind.packet_rates(), band.packet_rates(), "{}", kind.name());
        assert_eq!(kind.name(), band.name());
    }
}

#[test]
fn the_900_mhz_quad_builds_flies_and_keeps_its_link_up() {
    let mut v = quad("opendrone-5f-900mhz.toml");
    v.run_for(1.0).unwrap();
    let radio = v.state().radio;
    assert!(radio.link_up && radio.lq_pct == 100.0, "{radio:?}");
}

#[test]
fn the_fixed_link_quad_builds_and_loses_about_a_fifth_of_its_packets() {
    let mut v = quad("opendrone-5f-fixed-link.toml");
    v.run_for(2.0).unwrap();
    let radio = v.state().radio;
    assert!(radio.link_up, "{radio:?}");
    assert!((60.0..=95.0).contains(&radio.lq_pct), "a 20 % loss fraction leaves LQ near 80: {}", radio.lq_pct);
    assert_eq!(radio.rssi_dbm, ofs_radio::fixed::NOMINAL_RSSI_DBM);
}
