use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{Decoder, Frame};
use ofs_radio::elrs::{ElrsLink, LinkParams, NO_SIGNAL_RSSI_DBM};

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

fn frames(uart: &Wire) -> Vec<Frame> {
    Decoder::default().push(&uart.take(usize::MAX))
}

fn rc(frames: &[Frame]) -> Vec<[u16; 16]> {
    frames.iter().filter_map(|f| if let Frame::RcChannels(c) = f { Some(*c) } else { None }).collect()
}

/// Steps one radio packet (16 base ticks at 500 Hz).
fn packet(s: &mut Scheduler) {
    for _ in 0..BASE_HZ / 500 {
        s.step().unwrap();
    }
}

#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(1.0).unwrap();
    let f = frames(&uart);
    assert_eq!(rc(&f).len(), 500);
    assert_eq!(f.iter().filter(|f| matches!(f, Frame::LinkStatistics(_))).count(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    assert_eq!(get(&s, names::RADIO_RSSI), -50.0);
    let Some(Frame::LinkStatistics(stats)) = f.iter().find(|f| matches!(f, Frame::LinkStatistics(_))) else { unreachable!() };
    assert_eq!((stats.uplink_rssi_1, stats.uplink_lq, stats.uplink_snr), (50, 100, 10));
}

#[test]
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_YAW, 0.5), (names::RC_THROTTLE, 0.25)] {
        set(&mut s, name, v);
    }
    set(&mut s, &names::rc_aux(0), 1.0);
    set(&mut s, &names::rc_aux(1), -1.0);
    s.run_for(0.01).unwrap();
    let last = *rc(&frames(&uart)).last().unwrap();
    assert_eq!(&last[..6], &[1152, 832, 592, 1392, 1792, 192]);
    assert!(last[8..].iter().all(|c| *c == 992), "unused channels are centred: {last:?}");
}

#[test]
fn transmitter_off_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    set(&mut s, names::RADIO_TX_ENABLED, 0.0);
    s.run_for(0.5).unwrap();
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    assert_eq!(get(&s, names::RADIO_RSSI), NO_SIGNAL_RSSI_DBM);
}

#[test]
fn link_loss_fault_drops_and_restores_the_link() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.5).unwrap();
    uart.take(usize::MAX);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    s.run_for(0.1).unwrap(); // 50 packets: half the LQ window
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LQ), 50.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    s.run_for(0.1).unwrap(); // the whole window is lost now
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 0.0);
    s.run_for(0.1).unwrap();
    assert_eq!(rc(&frames(&uart)).len(), 50);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
}

#[test]
fn random_loss_lowers_lq_and_is_seeded() {
    let lossy = LinkParams { loss_good: 0.3, ..LinkParams::ideal(500) };
    let run = |seed| {
        let (mut s, uart) = rig(lossy.clone(), seed);
        s.run_for(2.0).unwrap();
        (uart.take(usize::MAX), get(&s, names::RADIO_LQ))
    };
    let (a, lq) = run(1);
    assert!((55.0..=85.0).contains(&lq), "LQ {lq}");
    let received = rc(&Decoder::default().push(&a)).len();
    assert!((600..=800).contains(&received), "{received} of 1000 packets");
    assert_eq!(a, run(1).0, "same seed, same bytes");
    assert_ne!(a, run(2).0, "different seed, different losses");
}

#[test]
fn a_burst_that_never_ends_loses_everything() {
    let stuck_bad = LinkParams { loss_bad: 1.0, p_good_to_bad: 1.0, p_bad_to_good: 0.0, ..LinkParams::ideal(500) };
    let (mut s, uart) = rig(stuck_bad, 1);
    s.run_for(0.5).unwrap();
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
}

#[test]
fn latency_delays_stick_changes_by_whole_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
    s.run_for(0.1).unwrap();
    uart.take(usize::MAX);
    set(&mut s, names::RC_ROLL, 1.0);
    let mut roll = Vec::new();
    for _ in 0..8 {
        packet(&mut s);
        roll.push(rc(&frames(&uart))[0][0]);
    }
    assert_eq!(roll, vec![992, 992, 992, 992, 992, 1792, 1792, 1792]);
}
