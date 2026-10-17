use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{stick_ticks, Decoder, Frame};
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{
    packet_error_rate, rf_mode_index, sensitivity_dbm, tx_power_index_mw, LinkParams, NOISE_FLOOR_DBM, NO_SIGNAL_RSSI_DBM,
};
use ofs_radio::receiver::ReceiverParams;
use ofs_radio::telemetry::FcTelemetryHandle;
use ofs_rf::propagation::{fspl_db, Antenna, AntennaKind, Obstacle, Polarization};

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let chain = RadioChain::elrs(params, ReceiverParams::default(), BASE_HZ, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
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
    assert_eq!(sensitivity_dbm(50), Some(-115.0), "ExpressLRS's table lists the 2.4 GHz 50 Hz mode at -115 dBm (#12)");
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
        let (mut s, _uart, _telemetry_in, _fc) = rig(p, 1);
        set_pos(&mut s, DVec3::new(range_m, 0.0, -1.7));
        packets(&mut s, 2);
        let rssi = get(&s, names::RADIO_RSSI);
        // +-5 % of range is +-0.42 dB; give it a little slack.
        close(rssi, sensitivity_dbm(500).unwrap(), 0.55, &format!("{power_mw} mW at {range_m} m"));
    }
}

#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
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
    let (mut s, uart, _telemetry_in, _fc) = rig(p, 1);
    set_pos(&mut s, DVec3::new(20.0, 0.0, -1.7)); // the handset is due west of the quad: the patch's boresight
    s.run_for(1.0).unwrap();
    let stats = statistics(&frames(&uart));
    assert_eq!(stats[0].active_antenna, 1, "the 8 dBi patch aimed at the handset wins");
    assert_eq!(get(&s, names::RADIO_ANTENNA), 1.0);
    assert!(stats[0].uplink_rssi_2 + 4 <= stats[0].uplink_rssi_1, "the patch hears it at least 4 dB better: {stats:?}");
}

#[test]
fn a_dipole_crossed_to_the_handset_loses_20_db() {
    let (mut aligned, _, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
    set_pos(&mut aligned, DVec3::new(0.0, 0.0, -1.7)); // level with the handset: a horizontal path
    packets(&mut aligned, 2);
    let mut crossed_params = LinkParams::ideal(500);
    crossed_params.quad_antennas[0].axis = DVec3::Y; // broadside to the path, its field crossed with the handset's
    let (mut crossed, _, _telemetry_in, _fc) = rig(crossed_params, 1);
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
    let (mut open, _, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
    let (mut blocked, _, _telemetry_in, _fc) = rig(blocked_params, 1);
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
    let (mut s, _uart, _telemetry_in, _fc) = rig(p, 7);
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
        let (mut s, uart, _telemetry_in, _fc) = rig(p, seed);
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
        let (mut s, uart, _telemetry_in, _fc) = rig(p, 3);
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
    let (mut s, uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
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
    let (mut s, uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
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
fn zero_fraction_is_a_no_op() {
    let (mut clean, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 11);
    let (mut faulted, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 11);
    set(&mut faulted, names::FAULT_RADIO_LOSS_FRACTION, 0.0);
    packets(&mut clean, 500);
    packets(&mut faulted, 500);
    // With fading off the only draws are loss decisions, so this pins that a zero fraction loses nothing; that the
    // model's own stream never moves is pinned by `the_models_own_stream_ignores_the_fault_fraction`.
    close(get(&clean, names::RADIO_LQ), get(&faulted, names::RADIO_LQ), 0.0, "LQ identical");
}

#[test]
fn full_fraction_equals_radio_link_loss() {
    let (mut cut, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 3);
    set(&mut cut, names::FAULT_RADIO_LINK_LOSS, 1.0);
    packets(&mut cut, 300);
    let (mut full, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 3);
    set(&mut full, names::FAULT_RADIO_LOSS_FRACTION, 1.0);
    packets(&mut full, 300);
    assert_eq!(get(&cut, names::RADIO_LQ), 0.0);
    assert_eq!(get(&full, names::RADIO_LQ), 0.0, "every packet lost: the same silence");
    assert_eq!(get(&full, names::RADIO_LINK_UP), 0.0);
}

#[test]
fn half_fraction_drops_about_half() {
    let (mut s, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 5);
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 0.5);
    packets(&mut s, 2000);
    let lq = get(&s, names::RADIO_LQ);
    assert!(lq > 35.0 && lq < 65.0, "binomial 3-sigma band around 50: {lq}");
}

#[test]
fn burst_window_boundaries_are_deterministic() {
    // 100 ms on / 300 ms off at 500 Hz: 50 lost packets per on-phase, 150 delivered per off-phase.
    // LQ is the last 100 packets, so the off-phase's tail reads a full clean window.
    let (mut s, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 9);
    set(&mut s, names::FAULT_RADIO_BURST_ON_S, 0.1);
    set(&mut s, names::FAULT_RADIO_BURST_OFF_S, 0.3);
    packets(&mut s, 50);
    assert_eq!(get(&s, names::RADIO_LQ), 0.0, "the on-window lost every packet so far");
    packets(&mut s, 150);
    assert!(get(&s, names::RADIO_LQ) > 90.0, "the 150-packet off-window flushed the history");
    packets(&mut s, 50);
    close(get(&s, names::RADIO_LQ), 50.0, 1.0, "the second on-window mixes 50 lost into the last 100");
}

#[test]
fn a_long_burst_failsafes_and_recovers() {
    let (mut s, _uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 13);
    set(&mut s, names::FAULT_RADIO_BURST_ON_S, 2.0);
    set(&mut s, names::FAULT_RADIO_BURST_OFF_S, 1.0);
    packets(&mut s, 900); // 1.8 s: inside the 2 s on-window
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0, "LQ hit zero during the burst");
    packets(&mut s, 500); // 1.0 s more: 2.8 s, inside the off-window
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0, "the off-window delivered again");
}

#[test]
fn the_sticks_reach_the_receiver_after_latency_packets() {
    let (mut s, uart, _telemetry_in, _fc) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
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
    let (mut s, uart, _telemetry_in, _fc) = rig(LinkParams::ideal(500), 1);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_YAW, 0.5), (names::RC_THROTTLE, 0.25)] {
        set(&mut s, name, v);
    }
    s.run_for(0.004).unwrap();
    let frame = rc(&frames(&uart)).pop().unwrap();
    assert_eq!((frame[0], frame[1], frame[2], frame[3]), (stick_ticks(0.2), stick_ticks(-0.2), ofs_radio::crsf::throttle_ticks(0.25), stick_ticks(0.5)));
}

mod fc_telemetry {
    use super::*;
    use ofs_radio::crsf::{FRAMETYPE_ATTITUDE, FRAMETYPE_BATTERY_SENSOR, FRAMETYPE_FLIGHT_MODE};
    use ofs_radio::telemetry::FcTelemetry;

    /// One CRC-valid CRSF frame: [address 0xC8][len][type][payload][crc8 poly 0xD5].
    fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0xC8, (payload.len() + 2) as u8, frame_type];
        f.extend_from_slice(payload);
        f.push(ofs_radio::crsf::crc8_dvb_s2(&f[2..]));
        f
    }

    fn battery_frame(voltage_v: f64, current_a: f64, _mah: u32, index: u8) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&((voltage_v * 10.0) as u16).to_be_bytes()); // 0.1 V units
        p.extend_from_slice(&((current_a * 10.0) as u16).to_be_bytes()); // 0.1 A units
        p.extend_from_slice(&[0, 0, index]); // u24 mAh: the index rides in the low byte
        p.push(100);
        frame(FRAMETYPE_BATTERY_SENSOR, &p)
    }

    fn attitude_frame(pitch_deg: f64, roll_deg: f64, yaw_deg: f64) -> Vec<u8> {
        let rad = |d: f64| (d.to_radians() * 10_000.0).round() as i16;
        let mut p = Vec::new();
        for v in [rad(pitch_deg), rad(roll_deg), rad(yaw_deg)] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        frame(FRAMETYPE_ATTITUDE, &p)
    }

    #[test]
    fn a_frame_flies_every_fourth_downlink_packet_and_updates_the_handle() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 7));
        packets(&mut s, 3);
        assert_eq!(fc.latest().frame_count, 0, "no slot before the 4th packet");
        packets(&mut s, 1);
        let t = fc.latest();
        assert_eq!(t.frame_count, 1);
        assert!((t.battery_v - 24.6).abs() < 1e-9, "{:?}", t);
        assert!((t.battery_a - 3.2).abs() < 1e-9, "{:?}", t);
        assert_eq!(t.battery_mah, 7.0, "the u24's low byte rode through: {:?}", t);
        packets(&mut s, 4);
        assert_eq!(fc.latest().frame_count, 1, "the FIFO is empty: later slots fly idle");
    }

    #[test]
    fn attitude_and_flight_mode_decode() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&attitude_frame(-4.5, 12.3, 90.0));
        telemetry_in.write(&frame(FRAMETYPE_FLIGHT_MODE, b"ACRO\0"));
        packets(&mut s, 8);
        let t = fc.latest();
        assert!((t.pitch_deg + 4.5).abs() < 0.01 && (t.roll_deg - 12.3).abs() < 0.01, "{:?}", t);
        assert!((t.yaw_deg - 90.0).abs() < 0.01, "yaw wrapped to ±180 by Betaflight decodes back: {:?}", t);
        assert_eq!(t.flight_mode, "ACRO");
        assert_eq!(t.frame_count, 2);
    }

    #[test]
    fn a_full_fifo_drops_the_oldest_frame() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        for i in 0..20u32 {
            telemetry_in.write(&battery_frame(20.0 + f64::from(i), 0.0, 0, i as u8));
        }
        packets(&mut s, 4); // the first slot: 16 kept of 20 -> the 5th offered frame (index 4) flies
        let t = fc.latest();
        assert_eq!(t.frame_count, 1);
        assert!((t.battery_v - 24.0).abs() < 1e-9, "oldest dropped: {:?}", t);
    }

    #[test]
    fn no_downlink_packets_means_no_telemetry() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        s.run_for(0.1).unwrap();
        let after_connected = fc.latest().frame_count;
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
        set(&mut s, names::RADIO_TX_ENABLED, 0.0);
        s.run_for(0.25).unwrap(); // 125 packets of silence drain the 100-packet LQ window
        assert_eq!(fc.latest().frame_count, after_connected, "the receiver is silent: no slots");
        assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    }

    #[test]
    fn corrupt_bytes_are_skipped_and_the_stream_stays_aligned() {
        let (mut s, _uart, telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        telemetry_in.write(&[0xDE, 0xAD, 0xC8, 0xFF]);
        telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
        packets(&mut s, 4);
        assert_eq!(fc.latest().frame_count, 1, "the garbage never reached the handle");
    }

    #[test]
    fn the_telemetry_path_draws_no_numbers() {
        // The downlink byte stream is identical whether or not frames were offered: the scheduler
        // consumes no randomness.
        let run = |offer: bool| {
            let (mut s, uart, telemetry_in, _fc) = rig(
                LinkParams { fading: true, ground_bounce: true, ..LinkParams::ideal(500) },
                5,
            );
            if offer {
                telemetry_in.write(&battery_frame(24.6, 3.2, 0, 1));
            }
            set_pos(&mut s, DVec3::new(30.0, 0.0, -1.7));
            s.run_for(0.2).unwrap();
            uart.take(usize::MAX)
        };
        assert_eq!(run(true), run(false), "the uplink bytes must not depend on the telemetry offered");
    }

    #[test]
    fn an_open_link_reports_no_telemetry_until_frames_arrive() {
        let (mut s, _uart, _telemetry_in, fc) = rig(LinkParams::ideal(500), 1);
        s.run_for(0.05).unwrap();
        assert_eq!(fc.latest(), FcTelemetry::default(), "frame_count 0, everything zero, no mode");
    }
}

/// m4b debt: `zero_fraction_is_a_no_op` runs with fading off, where a draw leaking from the fault stream into the
/// model's would shift both rigs alike. Here fading is on, at the downlink's sensitivity edge, and only the uplink
/// fault fraction differs: the downlink's loss history comes from the model's own stream alone, so whatever the
/// fault draws, it must not move.
#[test]
fn the_models_own_stream_ignores_the_fault_fraction() {
    let downlink_lq = |fraction: f64, north: f64| -> Vec<f64> {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        let (mut s, _uart, _telemetry_in, _fc) = rig(p, 21);
        set_pos(&mut s, DVec3::new(north, 0.0, -1.7));
        set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, fraction);
        (0..400)
            .map(|_| {
                packets(&mut s, 1);
                get(&s, names::RADIO_DOWNLINK_LQ)
            })
            .collect()
    };
    let edge = [5_000.0, 10_000.0, 20_000.0, 30_000.0, 42_000.0, 60_000.0, 90_000.0]
        .into_iter()
        .find(|&north| downlink_lq(0.0, north).iter().any(|&lq| lq > 0.0 && lq < 100.0))
        .expect("a distance where the downlink sits at its sensitivity edge");
    let clean = downlink_lq(0.0, edge);
    for fraction in [0.3, 0.9] {
        assert_eq!(downlink_lq(fraction, edge), clean, "fault fraction {fraction} moved the model's own draws");
    }
}
