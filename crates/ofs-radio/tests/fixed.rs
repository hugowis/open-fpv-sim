use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::chain::RadioChain;
use ofs_radio::crsf::{self, Decoder, Frame};
use ofs_radio::elrs::NO_SIGNAL_RSSI_DBM;
use ofs_radio::fixed::{FixedLink, FixedParams, NOMINAL_RSSI_DBM};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::RfFrontEnd;
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};
use ofs_radio::telemetry::FcTelemetryHandle;

const BASE_HZ: u32 = 8000;
const TICKS_PER_PACKET: u32 = BASE_HZ / 500;

fn params() -> FixedParams {
    FixedParams { packet_rate_hz: 500, latency_packets: 1, loss_fraction: 0.0, burst_on_s: 0.0, burst_off_s: 0.0 }
}

/// The scheduler, the receiver's UART output, the flight controller's telemetry input and the handset's telemetry.
fn rig_full(params: FixedParams, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let front_end = RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 };
    let handset = JoystickHandset::new(front_end, &mut bus);
    let link = FixedLink::new(params, seed, &mut bus);
    let receiver = CrsfReceiver::new(ReceiverParams::default(), uart.clone(), telemetry_in.clone());
    let chain = RadioChain::new(Box::new(handset), Box::new(link), Box::new(receiver), BASE_HZ, &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
}

fn rig(params: FixedParams, seed: u64) -> (Scheduler, Wire) {
    let (s, uart, _telemetry_in, _handle) = rig_full(params, seed);
    (s, uart)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

/// Runs `packets` packet slots, one frame-decode per slot: `Some(roll ticks)` when the slot's RC frame arrived.
fn slots(s: &mut Scheduler, uart: &Wire, packets: u32, roll_of: impl Fn(u32) -> f64) -> Vec<Option<u16>> {
    let mut seen = Vec::new();
    for k in 0..packets {
        set(s, names::RC_ROLL, roll_of(k));
        for _ in 0..TICKS_PER_PACKET {
            s.step().unwrap();
        }
        let frames = Decoder::default().push(&uart.take(usize::MAX));
        seen.push(frames.iter().find_map(|f| if let Frame::RcChannels(c) = f { Some(c[0]) } else { None }));
    }
    seen
}

#[test]
fn the_latency_is_exactly_that_many_packets() {
    let (mut s, uart) = rig(FixedParams { latency_packets: 3, ..params() }, 1);
    let roll = |k: u32| f64::from(k) / 100.0;
    let seen = slots(&mut s, &uart, 10, roll);
    for (k, ticks) in seen.iter().enumerate() {
        let sampled = (k as u32).saturating_sub(3);
        assert_eq!(*ticks, Some(crsf::stick_ticks(roll(sampled))), "packet {k} carries the sample from packet {sampled}");
    }
}

#[test]
fn the_loss_fraction_is_seeded_and_close_to_its_value() {
    let delivered = |seed: u64| {
        let (mut s, uart) = rig(FixedParams { loss_fraction: 0.25, ..params() }, seed);
        slots(&mut s, &uart, 4000, |_| 0.0).iter().map(Option::is_some).collect::<Vec<_>>()
    };
    let a = delivered(5);
    let share = a.iter().filter(|d| **d).count() as f64 / a.len() as f64;
    assert!((0.72..0.78).contains(&share), "delivered {share}");
    assert_eq!(a, delivered(5), "the same seed loses the same packets");
    assert_ne!(a, delivered(6), "another seed loses others");
}

#[test]
fn no_loss_delivers_everything_and_total_loss_goes_silent_with_the_floor_published() {
    let (mut s, uart) = rig(params(), 1);
    assert!(slots(&mut s, &uart, 300, |_| 0.0).iter().all(Option::is_some));
    assert_eq!((get(&s, names::RADIO_LINK_UP), get(&s, names::RADIO_LQ), get(&s, names::RADIO_RSSI)), (1.0, 100.0, NOMINAL_RSSI_DBM));
    let (mut s, uart) = rig(FixedParams { loss_fraction: 1.0, ..params() }, 1);
    assert!(slots(&mut s, &uart, 300, |_| 0.0).iter().all(Option::is_none));
    assert_eq!((get(&s, names::RADIO_LINK_UP), get(&s, names::RADIO_LQ), get(&s, names::RADIO_RSSI)), (0.0, 0.0, NO_SIGNAL_RSSI_DBM));
}

#[test]
fn bursts_follow_integer_packet_arithmetic_at_their_edges() {
    // 500 Hz, on = off = 0.05 s: a 50-packet period, the first 25 lost. The edges sit on packets, where
    // `time_s % period` can land a few ulps either side (#13).
    for (on, off, lost_per_period, period) in [(0.05, 0.05, 25, 50), (0.03, 0.07, 15, 50)] {
        let (mut s, uart) = rig(FixedParams { burst_on_s: on, burst_off_s: off, ..params() }, 1);
        let seen = slots(&mut s, &uart, 1000, |_| 0.0);
        let wrong: Vec<usize> = (0..1000).filter(|k| seen[*k].is_none() != (k % period < lost_per_period)).collect();
        assert!(wrong.is_empty(), "on {on} off {off}: wrong side of an edge at packets {:?}", &wrong[..wrong.len().min(10)]);
    }
}

#[test]
fn bursts_lose_every_packet_of_their_on_window() {
    let (mut s, uart) = rig(FixedParams { burst_on_s: 0.1, burst_off_s: 0.1, ..params() }, 1);
    let seen = slots(&mut s, &uart, 100, |_| 0.0); // 0.2 s: on for the first 50 packets, off for the next 50
    assert!(seen[..50].iter().all(Option::is_none), "the on window loses everything");
    assert!(seen[50..].iter().all(Option::is_some), "the off window delivers everything");
}

#[test]
fn the_radio_faults_apply_to_a_fixed_link_too() {
    let (mut s, uart) = rig(params(), 1);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_none), "link loss silences it");
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 0.0);
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 1.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_none), "a loss fraction of 1 loses all");
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 0.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_some), "and clearing the faults restores it");
}

#[test]
fn a_telemetry_slot_flies_every_fourth_heard_packet() {
    let (mut s, _uart, telemetry_in, handle) = rig_full(params(), 1);
    // A CRSF battery frame: 24.6 V, 3.2 A, the mAh bytes carrying an index, 100 % remaining.
    let mut battery = vec![0xC8, 10, crsf::FRAMETYPE_BATTERY_SENSOR, 0, 246, 0, 32, 0, 0, 7, 100];
    battery.push(crsf::crc8_dvb_s2(&battery[2..]));
    for _ in 0..8 {
        telemetry_in.write(&battery);
    }
    for _ in 0..(8 * TICKS_PER_PACKET) {
        s.step().unwrap();
    }
    let t = handle.latest();
    assert_eq!(t.frame_count, 2, "8 heard packets: slots on the 4th and 8th");
    assert!((t.battery_v - 24.6).abs() < 1e-9 && (t.battery_a - 3.2).abs() < 1e-9, "{t:?}");
}

#[test]
fn the_burst_fault_loses_exactly_the_packets_of_its_on_window() {
    // 0.05 s at 500 Hz is 25 packets: 25 lost, 25 delivered, repeating.
    let (mut s, uart) = rig(params(), 1);
    set(&mut s, names::FAULT_RADIO_BURST_ON_S, 0.05);
    set(&mut s, names::FAULT_RADIO_BURST_OFF_S, 0.05);
    let seen = slots(&mut s, &uart, 100, |_| 0.0);
    // A packet exactly on a window edge (k % 25 == 0) lands on either side of it by float remainder (`time_s % period`
    // is a few ulp off at some edges: packet 75 is lost although it starts the off window). The ELRS golden digests
    // exercise the same formula, so it is frozen; every packet inside a window is asserted.
    for (k, got) in seen.iter().enumerate().filter(|(k, _)| k % 25 != 0) {
        assert_eq!(got.is_none(), k % 50 < 25, "packet {k}: the on window is the first 25 packets of each 50");
    }
    let lost = seen.iter().filter(|d| d.is_none()).count();
    assert!((50..=52).contains(&lost), "{lost} of 100 lost: the on windows, give or take an edge packet each");
}

#[test]
fn a_fault_does_not_shift_the_links_random_stream() {
    let delivered = |faulted: bool| {
        let (mut s, uart) = rig(FixedParams { loss_fraction: 0.25, ..params() }, 9);
        let mut seen = slots(&mut s, &uart, 100, |_| 0.0);
        if faulted {
            set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 1.0);
        }
        let during = slots(&mut s, &uart, 100, |_| 0.0);
        set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 0.0);
        if faulted {
            assert!(during.iter().all(Option::is_none), "the fault loses packets 100-199");
        }
        seen.extend(during);
        seen.extend(slots(&mut s, &uart, 200, |_| 0.0));
        seen.iter().map(Option::is_some).collect::<Vec<_>>()
    };
    let (clean, faulted) = (delivered(false), delivered(true));
    assert_eq!(clean[..100], faulted[..100]);
    assert_eq!(clean[200..], faulted[200..], "after the fault the same packets are lost as in the clean run");
}
