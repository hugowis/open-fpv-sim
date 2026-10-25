//! The iBUS (FlySky) frame codec and receiver (issue #8): a golden frame computed by hand, the checksum, the channel
//! mapping to and from the sim's CRSF ticks, and the receiver's pacing.
use ofs_core::Wire;
use ofs_radio::crsf;
use ofs_radio::ibus::{self, IbusError, IbusReceiver};
use ofs_radio::parts::{Delivery, LinkReport, Receiver};

/// Channels 1 to 14 in microseconds: 1500 everywhere except channel 3 (1000) and channel 5 (2000).
fn golden_us() -> [u16; 14] {
    let mut us = [1500; 14];
    us[2] = 1000;
    us[4] = 2000;
    us
}

/// Computed by hand. 1500 = 0x05DC -> bytes DC 05 (sum 220 + 5 = 225); 1000 = 0x03E8 -> E8 03 (232 + 3 = 235);
/// 2000 = 0x07D0 -> D0 07 (208 + 7 = 215). Twelve channels at 1500, one at 1000, one at 2000:
/// 12 * 225 + 235 + 215 = 3150, plus the header 0x20 + 0x40 = 96 gives 3246 = 0x0CAE.
/// The checksum is 0xFFFF - 0x0CAE = 0xF351, sent low byte first: 51 F3.
const GOLDEN: [u8; 32] = [
    0x20, 0x40, //
    0xDC, 0x05, 0xDC, 0x05, 0xE8, 0x03, 0xDC, 0x05, 0xD0, 0x07, 0xDC, 0x05, 0xDC, 0x05, //
    0xDC, 0x05, 0xDC, 0x05, 0xDC, 0x05, 0xDC, 0x05, 0xDC, 0x05, 0xDC, 0x05, 0xDC, 0x05, //
    0x51, 0xF3,
];

#[test]
fn a_frame_is_the_hand_computed_golden_frame() {
    assert_eq!(ibus::frame(&golden_us()), GOLDEN);
    assert_eq!(ibus::decode(&GOLDEN), Ok(golden_us()));
}

#[test]
fn the_checksum_is_ffff_minus_the_sum_of_the_first_thirty_bytes() {
    assert_eq!(ibus::checksum(GOLDEN[..30].try_into().unwrap()), 0xF351);
    assert_eq!(
        0xFFFF - GOLDEN[..30].iter().map(|b| u16::from(*b)).sum::<u16>(),
        0xF351
    );
    // The sum of all thirty bytes and the checksum is 0xFFFF.
    let sum: u32 = GOLDEN[..30].iter().map(|b| u32::from(*b)).sum::<u32>()
        + u32::from(u16::from_le_bytes([GOLDEN[30], GOLDEN[31]]));
    assert_eq!(sum, 0xFFFF);
}

#[test]
fn decode_refuses_a_bad_length_header_or_checksum() {
    assert_eq!(ibus::decode(&GOLDEN[..31]), Err(IbusError::Length(31)));
    let mut bad = GOLDEN;
    bad[0] = 0x21;
    assert_eq!(ibus::decode(&bad), Err(IbusError::Header([0x21, 0x40])));
    let mut bad = GOLDEN;
    bad[1] = 0x80;
    assert_eq!(ibus::decode(&bad), Err(IbusError::Header([0x20, 0x80])));
    for i in 2..32 {
        let mut bad = GOLDEN;
        bad[i] ^= 0x01;
        assert_eq!(ibus::decode(&bad), Err(IbusError::Checksum), "byte {i}");
    }
}

#[test]
fn decode_keeps_twelve_bits_as_betaflight_does() {
    // The high nibble of each channel word belongs to channels 15 to 18 (Betaflight: lo + ((hi & 0x0F) << 8)).
    let mut f = GOLDEN;
    f[3] |= 0xA0;
    let sum = 0xFFFF - f[..30].iter().map(|b| u16::from(*b)).sum::<u16>();
    f[30..].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(ibus::decode(&f), Ok(golden_us()));
}

#[test]
fn frames_round_trip_for_every_microsecond_value() {
    for us in 1000..=2000u16 {
        let channels = [us; 14];
        assert_eq!(ibus::decode(&ibus::frame(&channels)), Ok(channels));
    }
}

#[test]
fn ticks_map_linearly_to_microseconds_and_clamp_to_the_flysky_range() {
    assert_eq!(
        (
            ibus::ticks_to_us(192),
            ibus::ticks_to_us(992),
            ibus::ticks_to_us(1792)
        ),
        (1000, 1500, 2000)
    );
    assert_eq!(ibus::ticks_to_us(0), 1000, "below the range clamps");
    assert_eq!(ibus::ticks_to_us(2047), 2000, "above the range clamps");
    assert_eq!(
        (
            ibus::us_to_ticks(1000),
            ibus::us_to_ticks(1500),
            ibus::us_to_ticks(2000)
        ),
        (192, 992, 1792)
    );
    assert_eq!(
        (ibus::us_to_ticks(900), ibus::us_to_ticks(2500)),
        (192, 1792)
    );
    // The sticks the joystick handset sends survive the trip to within one tick.
    for ticks in 192..=1792u16 {
        let back = ibus::us_to_ticks(ibus::ticks_to_us(ticks));
        assert!(
            back.abs_diff(ticks) <= 1,
            "{ticks} -> {} us -> {back}",
            ibus::ticks_to_us(ticks)
        );
    }
    for us in 1000..=2000u16 {
        assert_eq!(ibus::ticks_to_us(ibus::us_to_ticks(us)), us);
    }
}

#[test]
fn the_rc_frame_carries_the_first_fourteen_sim_channels() {
    let mut ch = [crsf::stick_ticks(0.0); 16];
    ch[0] = crsf::stick_ticks(1.0);
    ch[2] = crsf::throttle_ticks(0.0);
    ch[13] = crsf::stick_ticks(-1.0);
    ch[14] = crsf::stick_ticks(1.0);
    ch[15] = crsf::stick_ticks(1.0);
    let us = ibus::decode(&ibus::rc_frame(&ch)).unwrap();
    assert_eq!((us[0], us[1], us[2], us[13]), (2000, 1500, 1000, 1000));
    assert_eq!(&us[3..13], &[1500; 10]);
}

fn delivery(channels: Option<[u16; 16]>) -> Delivery {
    Delivery {
        channels,
        downlink_slot: false,
        report: LinkReport {
            link_up: channels.is_some(),
            lq_pct: 100.0,
            rssi_dbm: -50.0,
            snr_db: 40.0,
            antenna: 0,
            downlink_lq_pct: 100.0,
            uplink_rssi_dbm: [-50.0; 2],
            uplink_snr_db: 40.0,
            downlink_rssi_dbm: -55.0,
            downlink_snr_db: 35.0,
            rf_mode: 0,
            tx_power_index: 0,
        },
    }
}

/// Feeds `slots` packet slots (every `lost_every`-th one lost when non-zero) and returns the frames written, with the
/// slot each was written in.
fn run(
    rate_hz: u32,
    slots: u32,
    lost: impl Fn(u32) -> bool,
    channels: impl Fn(u32) -> [u16; 16],
) -> Vec<(u32, [u16; 14])> {
    let (uart, tap) = (Wire::new(1 << 16), Wire::new(1 << 12));
    let mut rx = IbusReceiver::new(rate_hz, uart.clone(), tap);
    let mut out = Vec::new();
    for slot in 0..slots {
        rx.emit(&delivery(if lost(slot) {
            None
        } else {
            Some(channels(slot))
        }));
        let bytes = uart.take(usize::MAX);
        assert_eq!(bytes.len() % 32, 0, "whole frames only");
        for f in bytes.chunks(32) {
            out.push((slot, ibus::decode(f).expect("every written frame is valid")));
        }
    }
    out
}

#[test]
fn the_receiver_writes_a_frame_about_every_seven_milliseconds_whatever_the_packet_rate() {
    for rate in [500u32, 250, 150, 50] {
        let frames = run(rate, rate * 2, |_| false, |_| [crsf::stick_ticks(0.0); 16]);
        // The average is 7 ms (a packet slot longer than that sends one frame per slot).
        let expect = 2.0 * f64::from(rate).min(1000.0 / 7.0);
        assert!(
            (frames.len() as f64 - expect).abs() <= 3.0,
            "{rate} Hz: {} frames in 2 s, expected about {expect}",
            frames.len()
        );
        // Never closer than a packet slot short of 7 ms, so far from Betaflight's 500 us gap rule, and never more than a
        // slot late.
        let slot_ms = 1000.0 / f64::from(rate);
        for w in frames.windows(2) {
            let gap_ms = f64::from(w[1].0 - w[0].0) * slot_ms;
            assert!(
                gap_ms >= (7.0 - slot_ms).max(slot_ms) - 1e-6 && gap_ms <= 7.0 + slot_ms + 1e-6,
                "{rate} Hz: gap {gap_ms} ms"
            );
        }
    }
}

#[test]
fn the_first_delivered_packet_is_forwarded_at_once() {
    let frames = run(250, 10, |_| false, |_| [crsf::stick_ticks(0.0); 16]);
    assert_eq!(frames[0].0, 0);
}

#[test]
fn the_receiver_sends_the_latest_channels_and_nothing_while_the_link_is_down() {
    // 250 Hz: 4 ms slots. Lost between slots 20 and 59 (160 ms); the stick value changes at slot 60.
    let frames = run(
        250,
        100,
        |s| (20..60).contains(&s),
        |s| {
            [if s < 60 {
                crsf::stick_ticks(0.0)
            } else {
                crsf::stick_ticks(1.0)
            }; 16]
        },
    );
    assert!(
        frames.iter().all(|(s, _)| !(20..60).contains(s)),
        "no frame while nothing is delivered"
    );
    let (slot, ch) = frames.iter().find(|(s, _)| *s >= 60).unwrap();
    assert_eq!(
        (*slot, ch[0]),
        (60, 2000),
        "the first fresh packet after the loss goes out at once, with its channels"
    );
    assert_eq!(frames[0].1[0], 1500);
}

#[test]
fn the_receiver_drops_what_the_flight_controller_writes_and_queues_no_telemetry() {
    let (uart, tap) = (Wire::new(4096), Wire::new(4096));
    let mut rx = IbusReceiver::new(250, uart, tap.clone());
    tap.write(&[1, 2, 3, 4]);
    rx.ingest_telemetry();
    assert!(tap.take(usize::MAX).is_empty(), "drained");
    assert_eq!(rx.next_telemetry(), None);
}
