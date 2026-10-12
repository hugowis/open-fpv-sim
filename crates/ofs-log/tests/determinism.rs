//! Spec §9's riskiest point, tested first: identical input writes byte-identical MCAP.
use std::io::Cursor;

use ofs_core::SignalKind;
use ofs_log::sink::{scalar_json, vec3_json, McapSink};
use ofs_log::Channel;

/// `n` channels at mixed divisors, 2 simulated seconds on an 8 kHz tick, values from IEEE + − × ÷ only (libm's
/// `sin` differs between platforms in the last bits).
fn recording(n: usize) -> Vec<u8> {
    let channels: Vec<Channel> = (0..n)
        .map(|i| Channel {
            name: format!("sig.{i:02}"),
            kind: if i % 3 == 0 { SignalKind::Vec3 } else { SignalKind::Scalar },
            divisor: [8, 33, 160, 800][i % 4],
            rate_hz: 8000.0 / [8.0, 33.0, 160.0, 800.0][i % 4],
        })
        .collect();
    let mut out = Cursor::new(Vec::new());
    let mut sink = McapSink::new(&mut out, &channels).unwrap();
    let mut record = Vec::new();
    for end in 1..=16_000u64 {
        for (i, c) in channels.iter().enumerate() {
            if end % c.divisor != 0 {
                continue;
            }
            let p = end as f64 / 8000.0 * (i as f64 + 1.0) / 7.0;
            let x = (p - p.floor()) * 3.7 / 1.3;
            record.clear();
            match c.kind {
                SignalKind::Vec3 => vec3_json(&mut record, glam::DVec3::new(x, -x, x * 0.5)),
                _ => scalar_json(&mut record, x),
            }
            sink.write(i, end * 125_000, &record).unwrap();
        }
    }
    assert!(sink.finish().unwrap().messages > 0);
    out.into_inner()
}

#[test]
fn a_two_signal_recording_is_byte_identical() {
    let (a, b) = (recording(2), recording(2));
    assert!(a.len() > 1000);
    assert_eq!(a, b);
}

/// The regression guard for the mcap pin: crates.io 0.25.0 writes the summary's channels in HashMap order, which
/// differs between two writers in one process as soon as there are a few channels.
#[test]
fn a_many_channel_recording_is_byte_identical() {
    let first = recording(24);
    for _ in 0..4 {
        assert_eq!(recording(24), first, "the summary section's order must not depend on hashing");
    }
}

#[test]
fn records_are_compact_json_objects() {
    let mut out = Vec::new();
    scalar_json(&mut out, 1.0);
    assert_eq!(out, br#"{"v":1.0}"#);
    out.clear();
    vec3_json(&mut out, glam::DVec3::new(0.1, -2.0, 3e-7));
    assert_eq!(std::str::from_utf8(&out).unwrap(), r#"{"x":0.1,"y":-2.0,"z":3e-7}"#);
    out.clear();
    ofs_log::sink::quat_json(&mut out, glam::DQuat::IDENTITY);
    assert_eq!(std::str::from_utf8(&out).unwrap(), r#"{"w":1.0,"x":0.0,"y":0.0,"z":0.0}"#);
    out.clear();
    scalar_json(&mut out, f64::NAN);
    assert_eq!(out, br#"{"v":null}"#, "a non-finite value cannot be JSON; the session fails on it anyway");
}
