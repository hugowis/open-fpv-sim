use glam::DVec3;
use ofs_core::{names, Bus, Wire};
use ofs_radio::crsf::{self, Decoder, Frame};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::{Delivery, Handset, LinkReport, LqWindow, Receiver, RfFrontEnd};
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};

fn front_end() -> RfFrontEnd {
    RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 }
}

fn report() -> LinkReport {
    LinkReport {
        link_up: true,
        lq_pct: 100.0,
        rssi_dbm: -50.0,
        snr_db: 40.0,
        antenna: 0,
        downlink_lq_pct: 100.0,
        uplink_rssi_dbm: [-50.0, -60.0],
        uplink_snr_db: 40.0,
        downlink_rssi_dbm: -55.0,
        downlink_snr_db: 35.0,
        rf_mode: 4,
        tx_power_index: 17,
    }
}

fn delivery(channels: Option<[u16; 16]>) -> Delivery {
    Delivery { channels, downlink_slot: false, report: report() }
}

#[test]
fn the_lq_window_is_the_share_received_of_the_last_hundred() {
    let mut w = LqWindow::new();
    assert_eq!(w.push(false), 0.0);
    let mut lq = 0.0;
    for _ in 0..99 {
        lq = w.push(true);
    }
    assert_eq!(lq, 99.0, "99 of the last 100 (the first miss is still inside)");
    assert_eq!(w.push(true), 100.0, "the miss has left the window");
}

#[test]
fn the_joystick_handset_orders_channels_aetr_and_follows_the_transmitter_signal() {
    let mut bus = Bus::new();
    let mut handset = JoystickHandset::new(front_end(), &mut bus);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_THROTTLE, 0.25), (names::RC_YAW, 0.5)] {
        let s = bus.signal::<f64>(name);
        bus.set(s, v);
    }
    let aux1 = bus.signal::<f64>(&names::rc_aux(0));
    bus.set(aux1, 1.0);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let frame = handset.sample(&bus);
    assert!(frame.transmitting);
    assert_eq!(
        (frame.channels[0], frame.channels[1], frame.channels[2], frame.channels[3], frame.channels[4]),
        (crsf::stick_ticks(0.2), crsf::stick_ticks(-0.2), crsf::throttle_ticks(0.25), crsf::stick_ticks(0.5), crsf::stick_ticks(1.0)),
        "roll, pitch, throttle, yaw (AETR), then AUX1"
    );
    assert_eq!(frame.channels[15], crsf::stick_ticks(0.0), "the unused channels sit centred");
    bus.set(tx, 0.0);
    assert!(!handset.sample(&bus).transmitting);
}

#[test]
fn the_crsf_receiver_writes_a_frame_per_delivered_packet_and_statistics_every_n() {
    let (uart, tap) = (Wire::new(4096), Wire::new(4096));
    let mut rx = CrsfReceiver::new(ReceiverParams { link_stats_interval_packets: 3 }, uart.clone(), tap);
    let channels = [crsf::stick_ticks(0.1); 16];
    for _ in 0..6 {
        rx.emit(&delivery(Some(channels)));
    }
    rx.emit(&delivery(None));
    let frames = Decoder::default().push(&uart.take(usize::MAX));
    let rc = frames.iter().filter(|f| matches!(f, Frame::RcChannels(_))).count();
    let stats: Vec<_> = frames.iter().filter_map(|f| if let Frame::LinkStatistics(s) = f { Some(*s) } else { None }).collect();
    assert_eq!(rc, 6, "none for the lost packet");
    assert_eq!(stats.len(), 2, "after the 3rd and the 6th delivered packet");
    assert_eq!((stats[0].uplink_rssi_1, stats[0].uplink_rssi_2, stats[0].rf_mode, stats[0].uplink_tx_power), (50, 60, 4, 17));
}

#[test]
fn the_receiver_queues_flight_controller_frames_and_drops_the_oldest_past_sixteen() {
    let (uart, tap) = (Wire::new(4096), Wire::new(1 << 16));
    let mut rx = CrsfReceiver::new(ReceiverParams::default(), uart, tap.clone());
    for i in 0..20u8 {
        let mut f = vec![0xC8, 3, 0x08, i];
        f.push(crsf::crc8_dvb_s2(&f[2..]));
        tap.write(&f);
    }
    rx.ingest_telemetry();
    let first = rx.next_telemetry().unwrap();
    assert_eq!((first.0, first.1), (0x08, vec![4]), "frames 0..3 were dropped: the queue holds the newest 16");
    let mut left = 1;
    while rx.next_telemetry().is_some() {
        left += 1;
    }
    assert_eq!(left, 16);
}
