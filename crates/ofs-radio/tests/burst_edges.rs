//! The burst-loss fault's windows at their edges (#13): a packet that sits exactly on an edge belongs to the window that
//! starts there, whatever the float remainder says. The reference is integer packet arithmetic.
use ofs_core::{names, Bus};
use ofs_radio::parts::FaultSignals;

/// Which of `packets` packets (one every `1 / rate_hz` s) the burst fault eats, per `FaultSignals::survives`.
fn lost(on_s: f64, off_s: f64, rate_hz: f64, packets: u64) -> Vec<bool> {
    let mut bus = Bus::new();
    let faults = FaultSignals::register(&mut bus);
    let on = bus.signal::<f64>(names::FAULT_RADIO_BURST_ON_S);
    let off = bus.signal::<f64>(names::FAULT_RADIO_BURST_OFF_S);
    bus.set(on, on_s);
    bus.set(off, off_s);
    (0..packets).map(|k| !faults.survives(&bus, k as f64 / rate_hz, 1.0)).collect()
}

#[test]
fn burst_windows_follow_integer_packet_arithmetic() {
    // 250 Hz, on = off = 0.05 s: a 25-packet period whose first 12.5 packets are lost, so packets 0..=12 of each
    // period are lost and 13..=24 survive. The period edge falls on a packet (k = 25, 50, 75, ...).
    let eaten = lost(0.05, 0.05, 250.0, 5000);
    let wrong: Vec<u64> = (0..5000u64).filter(|k| eaten[*k as usize] != (k % 25 < 13)).collect();
    assert!(wrong.is_empty(), "packets on the wrong side of a window edge: {:?}", &wrong[..wrong.len().min(10)]);
}

#[test]
fn burst_windows_with_whole_packet_edges_are_exact() {
    // 250 Hz, on = off = 0.04 s: a 20-packet period, 10 lost then 10 kept; both edges fall on packets.
    let eaten = lost(0.04, 0.04, 250.0, 5000);
    let wrong: Vec<u64> = (0..5000u64).filter(|k| eaten[*k as usize] != (k % 20 < 10)).collect();
    assert!(wrong.is_empty(), "packets on the wrong side of a window edge: {:?}", &wrong[..wrong.len().min(10)]);
}

#[test]
fn a_zero_length_burst_loses_nothing() {
    let mut bus = Bus::new();
    let faults = FaultSignals::register(&mut bus);
    assert!((0..1000).all(|k| faults.survives(&bus, f64::from(k) * 0.004, 1.0)), "no burst fault is set");
}
