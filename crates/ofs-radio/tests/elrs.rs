use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{stick_ticks, Decoder, Frame};
use ofs_radio::elrs::{
    packet_error_rate, rf_mode_index, sensitivity_dbm, tx_power_index_mw, ElrsLink, LinkParams, NOISE_FLOOR_DBM,
    NO_SIGNAL_RSSI_DBM,
};
use ofs_rf::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let div = BASE_HZ / params.packet_rate_hz;
    let link = ElrsLink::new(params, div, seed, uart.clone(), &mut bus);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(link));
    (s, uart)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

fn frames(uart: &Wire) -> Vec<Frame> {
    Decoder::default().push(&uart.take(usize::MAX))
}

fn rc(frames: &[Frame]) -> Vec<[u16; 16]> {
    frames.iter().filter_map(|f| if let Frame::RcChannels(c) = f { Some(*c) } else { None }).collect()
}

fn statistics(frames: &[Frame]) -> Vec<ofs_radio::crsf::LinkStatistics> {
    frames.iter().filter_map(|f| if let Frame::LinkStatistics(s) = f { Some(*s) } else { None }).collect()
}

/// Steps `packets` radio packets at 500 Hz.
fn packets(s: &mut Scheduler, count: u32) {
    for _ in 0..count * (BASE_HZ / 500) {
        s.step().unwrap();
    }
}

fn close(actual: f64, expected: f64, eps: f64, what: &str) {
    assert!((actual - expected).abs() <= eps, "{what}: expected {expected} +- {eps}, got {actual}");
}

#[test]
fn the_sensitivity_table_is_elrs_published() {
    assert_eq!(sensitivity_dbm(50), Some(-117.0));
    assert_eq!(sensitivity_dbm(150), Some(-112.0));
    assert_eq!(sensitivity_dbm(250), Some(-108.0));
    assert_eq!(sensitivity_dbm(500), Some(-105.0));
    assert_eq!(sensitivity_dbm(1000), None);
    assert_eq!(rf_mode_index(500), Some(4));
    assert_eq!(rf_mode_index(50), Some(1));
    assert_eq!(tx_power_index_mw(250), 17);
    assert_eq!(tx_power_index_mw(10), 1);
    assert_eq!(tx_power_index_mw(1000), 23);
}

#[test]
fn per_is_half_at_the_sensitivity_and_small_five_db_up() {
    let s = sensitivity_dbm(500).unwrap();
    close(packet_error_rate(s, s), 0.5, 1e-9, "at the sensitivity");
    assert!(packet_error_rate(s + 5.0, s) < 0.01, "5 dB up: {}", packet_error_rate(s + 5.0, s));
    assert!(packet_error_rate(s - 15.0, s) > 0.999_999, "15 dB down: effectively everything is lost");
}

#[test]
fn the_rssi_reaches_the_sensitivity_at_the_published_range() {
    // 250 mW and 10 mW, 2 dBi dipoles at peak gain and co-polarized, fading, bounce and shadow off.
    for (power_mw, range_m) in [(250.0, 43_600.0), (10.0, 8_700.0)] {
        let mut p = LinkParams::ideal(500);
        p.tx_power_mw = power_mw;
        let (mut s, _uart) = rig(p, 1);
        set_pos(&mut s, DVec3::new(range_m, 0.0, -1.7));
        packets(&mut s, 2);
        let rssi = get(&s, names::RADIO_RSSI);
        // +-5 % of range is +-0.42 dB; give it a little slack.
        close(rssi, sensitivity_dbm(500).unwrap(), 0.55, &format!("{power_mw} mW at {range_m} m"));
    }
}

#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    set_pos(&mut s, DVec3::new(0.0, 0.0, -1.7)); // level with the handset, 2 m away: a horizontal path
    s.run_for(1.0).unwrap();
    let f = frames(&uart);
    assert_eq!(rc(&f).len(), 500);
    assert_eq!(statistics(&f).len(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    let expected = 10.0 * 250f64.log10() + 4.0 - fspl_db(2.0, 2440.0);
    close(get(&s, names::RADIO_RSSI), expected, 0.1, "the bare budget at 2 m");
    close(get(&s, names::RADIO_SNR), expected - NOISE_FLOOR_DBM, 0.1, "SNR is RSSI minus the LoRa noise floor");
    let stats = statistics(&f);
    assert_eq!((stats[0].rf_mode, stats[0].uplink_tx_power, stats[0].active_antenna), (4, 17, 0));
    assert_eq!(stats[0].uplink_lq, 100);
    assert_eq!(stats[0].uplink_rssi_1, (-expected).round() as u8);
    assert_eq!(stats[0].uplink_rssi_2, stats[0].uplink_rssi_1, "one antenna: both fields report it");
}

#[test]
fn a_second_antenna_is_picked_when_it_is_clearly_better() {
    let mut p = LinkParams::ideal(500);
    p.quad_antennas.push(Antenna { kind: AntennaKind::Patch { beamwidth_deg: 60.0 }, gain_dbi: 8.0, polarization: Polarization::Linear, axis: DVec3::NEG_X });
    let (mut s, uart) = rig(p, 1);
    set_pos(&mut s, DVec3::new(20.0, 0.0, -1.7)); // the handset is due west of the quad: the patch's boresight
    s.run_for(1.0).unwrap();
    let stats = statistics(&frames(&uart));
    assert_eq!(stats[0].active_antenna, 1, "the 8 dBi patch aimed at the handset wins");
    assert_eq!(get(&s, names::RADIO_ANTENNA), 1.0);
    assert!(stats[0].uplink_rssi_2 + 4 <= stats[0].uplink_rssi_1, "the patch hears it at least 4 dB better: {stats:?}");
}

#[test]
fn a_dipole_crossed_to_the_handset_loses_20_db() {
    let (mut aligned, _) = rig(LinkParams::ideal(500), 1);
    set_pos(&mut aligned, DVec3::new(0.0, 0.0, -1.7)); // level with the handset: a horizontal path
    packets(&mut aligned, 2);
    let mut crossed_params = LinkParams::ideal(500);
    crossed_params.quad_antennas[0].axis = DVec3::Y; // broadside to the path, its field crossed with the handset's
    let (mut crossed, _) = rig(crossed_params, 1);
    set_pos(&mut crossed, DVec3::new(0.0, 0.0, -1.7));
    packets(&mut crossed, 2);
    let loss = get(&aligned, names::RADIO_RSSI) - get(&crossed, names::RADIO_RSSI);
    close(loss, 20.0, 0.1, "crossed linear dipoles");
}

#[test]
fn behind_a_building_the_rssi_drops_by_its_capped_knife_edge_loss() {
    let mut blocked_params = LinkParams::ideal(500);
    let shape = Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(10.0, 10.0, 10.0) };
    blocked_params.obstacles = vec![Obstacle { shape: shape.rooted(), rf_loss_db: 25.0 }];
    let (mut open, _) = rig(LinkParams::ideal(500), 1);
    let (mut blocked, _) = rig(blocked_params, 1);
    set_pos(&mut open, DVec3::new(100.0, 0.0, -1.7));
    set_pos(&mut blocked, DVec3::new(100.0, 0.0, -1.7));
    packets(&mut open, 2);
    packets(&mut blocked, 2);
    let drop = get(&open, names::RADIO_RSSI) - get(&blocked, names::RADIO_RSSI);
    assert!(drop > 24.0 && drop <= 25.0 + 1e-9, "the building costs its cap of 25 dB: {drop}");
}

#[test]
fn a_hovering_quad_in_the_open_keeps_lq_at_100() {
    let mut p = LinkParams::ideal(500);
    p.fading = true;
    p.ground_bounce = true;
    let (mut s, _uart) = rig(p, 7);
    set_pos(&mut s, DVec3::new(30.0, 0.0, -1.7));
    s.run_for(1.0).unwrap();
    assert_eq!(get(&s, names::RADIO_LQ), 100.0, "60 dB of margin survives any fade");
}

#[test]
fn the_same_seed_gives_the_same_packet_stream() {
    let scripted = |seed: u64| {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        p.ground_bounce = true;
        let (mut s, uart) = rig(p, seed);
        for north in [0.0, 500.0, 1000.0] {
            set_pos(&mut s, DVec3::new(north, 0.0, -1.7));
            s.run_for(0.2).unwrap();
        }
        uart.take(usize::MAX)
    };
    assert_eq!(scripted(11), scripted(11), "same seed: identical bytes");
    assert_ne!(scripted(11), scripted(12), "a different seed fades differently");
}

#[test]
fn the_loss_draws_do_not_depend_on_the_transmitter() {
    // At the sensitivity edge (PER near one half), the received/not pattern of the 250 packets after the
    // switch-on must be identical whether the handset transmitted from the start or mid-run: the draws never
    // depend on the inputs.
    let pattern = |tx_from: u64| {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        let (mut s, uart) = rig(p, 3);
        set_pos(&mut s, DVec3::new(42_000.0, 0.0, -1.7));
        let mut seen = Vec::new();
        for t in 0..8000 {
            set(&mut s, names::RADIO_TX_ENABLED, if t >= tx_from { 1.0 } else { 0.0 });
            s.step().unwrap();
            if t >= tx_from {
                seen.push(!Decoder::default().push(&uart.take(usize::MAX)).is_empty());
            }
        }
        seen
    };
    assert_eq!(&pattern(0)[4000..], &pattern(4000));
}

#[test]
fn with_the_transmitter_off_the_receiver_goes_silent_and_lq_drains() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.2).unwrap(); // LQ 100
    frames(&uart); // discard the connected period's frames, still buffered on the wire
    set(&mut s, names::RADIO_TX_ENABLED, 0.0);
    s.run_for(0.25).unwrap(); // 100 packets of silence drain the window
    assert!(rc(&frames(&uart)).is_empty(), "no frames while the handset is off");
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    assert_eq!(get(&s, names::RADIO_RSSI), NO_SIGNAL_RSSI_DBM);
}

#[test]
fn the_radio_link_loss_fault_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.1).unwrap();
    let before = rc(&frames(&uart)).len();
    assert!(before > 0);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    s.run_for(0.25).unwrap(); // 125 packets of fault: the 100-packet window drains fully
    let after = rc(&frames(&uart)).len();
    assert_eq!(after, 0, "every packet lost while the fault is active");
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
}

#[test]
fn the_sticks_reach_the_receiver_after_latency_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
    packets(&mut s, 2); // prime the pipeline with the neutral sticks
    set(&mut s, names::RC_ROLL, 0.5);
    s.run_for(0.012).unwrap(); // 6 packets at 500 Hz
    let seen = rc(&frames(&uart));
    assert_eq!(seen.len(), 8);
    assert!(
        seen[..7].iter().all(|f| f[0] == stick_ticks(0.0)),
        "the first frames carry the sticks from before the move: {seen:?}"
    );
    assert_eq!(seen.last().unwrap()[0], stick_ticks(0.5), "the move arrives five packets late");
}

#[test]
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_YAW, 0.5), (names::RC_THROTTLE, 0.25)] {
        set(&mut s, name, v);
    }
    s.run_for(0.004).unwrap();
    let frame = rc(&frames(&uart)).pop().unwrap();
    assert_eq!((frame[0], frame[1], frame[2], frame[3]), (stick_ticks(0.2), stick_ticks(-0.2), ofs_radio::crsf::throttle_ticks(0.25), stick_ticks(0.5)));
}
