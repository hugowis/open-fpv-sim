use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::band::{Band, ELRS_900_FREQ_MHZ, NOISE_FLOOR_900_DBM};
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{rf_mode_index, sensitivity_dbm, LinkParams};
use ofs_radio::receiver::ReceiverParams;
use ofs_rf::propagation::fspl_db;

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams) -> Scheduler {
    let mut bus = Bus::new();
    let chain = RadioChain::elrs(params, ReceiverParams::default(), BASE_HZ, 1, Wire::new(1 << 20), Wire::new(1 << 16), &mut bus);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

#[test]
fn the_band_tables_are_the_published_rf_modes() {
    assert_eq!(Band::G2_4.packet_rates(), [50, 150, 250, 500]);
    assert_eq!(Band::Mhz900.packet_rates(), [25, 50, 100, 200, 250]);
    for (rate, dbm) in [(25, -123.0), (50, -120.0), (100, -117.0), (200, -112.0), (250, -111.0)] {
        assert_eq!(Band::Mhz900.sensitivity_dbm(rate), Some(dbm), "{rate} Hz");
    }
    assert_eq!(Band::Mhz900.sensitivity_dbm(500), None, "500 Hz is a 2.4 GHz mode");
    assert_eq!(Band::G2_4.sensitivity_dbm(25), None);
}

#[test]
fn the_2_4_ghz_band_is_what_the_free_functions_always_said() {
    for rate in [50, 150, 250, 500] {
        assert_eq!(Band::G2_4.sensitivity_dbm(rate), sensitivity_dbm(rate));
        assert_eq!(Band::G2_4.rf_mode_index(rate), rf_mode_index(rate));
    }
    assert_eq!(Band::G2_4.freq_mhz(), 2440.0);
    assert_eq!(Band::G2_4.noise_floor_dbm(), ofs_radio::elrs::NOISE_FLOOR_DBM);
}

#[test]
fn the_rf_mode_is_the_rates_place_in_the_band_plus_one() {
    assert_eq!(Band::Mhz900.rf_mode_index(25), Some(1));
    assert_eq!(Band::Mhz900.rf_mode_index(250), Some(5));
    assert_eq!(Band::Mhz900.rf_mode_index(300), None);
}

#[test]
fn the_900_mhz_noise_floor_is_the_lora_formula() {
    let formula = -174.0 + 10.0 * 500_000f64.log10() + 6.0;
    assert!((NOISE_FLOOR_900_DBM - formula).abs() < 1e-9, "{NOISE_FLOOR_900_DBM} vs {formula}");
    assert_eq!(Band::Mhz900.noise_floor_dbm(), NOISE_FLOOR_900_DBM);
    assert_eq!(Band::Mhz900.freq_mhz(), ELRS_900_FREQ_MHZ);
}

#[test]
fn names_round_trip_and_unknown_names_are_none() {
    for band in Band::ALL {
        assert_eq!(Band::from_name(band.name()), Some(band));
    }
    assert_eq!(Band::from_name("2g4"), Some(Band::G2_4));
    assert_eq!(Band::from_name("900"), Some(Band::Mhz900));
    assert_eq!(Band::from_name("5g8"), None);
}

#[test]
fn the_900_mhz_budget_at_two_metres_is_free_space_at_915_mhz() {
    let mut s = rig(LinkParams { band: Band::Mhz900, ..LinkParams::ideal(200) });
    set_pos(&mut s, DVec3::new(0.0, 0.0, -1.7)); // level with the handset, 2 m away: dipoles broadside
    s.run_for(0.05).unwrap();
    let expected = 10.0 * 250f64.log10() + 4.0 - fspl_db(2.0, ELRS_900_FREQ_MHZ);
    assert!((get(&s, names::RADIO_RSSI) - expected).abs() < 0.1, "{} vs {expected}", get(&s, names::RADIO_RSSI));
    assert!((get(&s, names::RADIO_SNR) - (expected - NOISE_FLOOR_900_DBM)).abs() < 0.1, "SNR uses the 900 MHz noise floor");
}

#[test]
fn at_40_km_the_900_mhz_link_holds_while_the_2_4_ghz_link_drops_packets() {
    let at_40_km = |params: LinkParams| {
        let mut s = rig(params);
        set_pos(&mut s, DVec3::new(40_000.0, 0.0, -1.7));
        s.run_for(2.0).unwrap();
        get(&s, names::RADIO_LQ)
    };
    let lq_900 = at_40_km(LinkParams { band: Band::Mhz900, ..LinkParams::ideal(100) });
    let lq_2g4 = at_40_km(LinkParams::ideal(500));
    assert_eq!(lq_900, 100.0, "21 dB of margin at 100 Hz");
    assert!(lq_2g4 < 95.0, "the 2.4 GHz link is at its sensitivity edge there: {lq_2g4}");
}
