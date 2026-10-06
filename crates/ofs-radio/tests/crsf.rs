use ofs_radio::crsf::*;
use proptest::prelude::*;

#[test]
fn crc_matches_the_crc8_dvb_s2_check_value() {
    assert_eq!(crc8_dvb_s2(b"123456789"), 0xBC);
}

#[test]
fn channels_pack_11_bits_each_lowest_bits_first() {
    let mut ch = [0u16; CHANNEL_COUNT];
    ch[0] = 0x7FF;
    assert_eq!(&pack_channels(&ch)[..3], &[0xFF, 0x07, 0x00]);
    let mut ch = [0u16; CHANNEL_COUNT];
    ch[1] = 0x7FF;
    let p = pack_channels(&ch);
    assert_eq!(&p[..3], &[0x00, 0xF8, 0x3F]);
    assert!(p[3..].iter().all(|b| *b == 0));
}

#[test]
fn stick_mapping_matches_betaflight_microseconds() {
    assert_eq!(stick_ticks(0.0), 992);
    assert_eq!(stick_ticks(-1.0), 192);
    assert_eq!(stick_ticks(1.0), 1792);
    assert_eq!(stick_ticks(5.0), 1792, "clamped");
    assert_eq!(throttle_ticks(0.0), 192);
    assert_eq!(throttle_ticks(1.0), 1792);
    // What M0 read back through SITL's MSP_RC for CRSF input (docs/research/sitl-interface.md §5).
    assert_eq!(ticks_to_us(stick_ticks(0.2)), 1600);
    assert_eq!(ticks_to_us(stick_ticks(-0.2)), 1400);
    assert_eq!(ticks_to_us(stick_ticks(0.0)), 1500);
    assert_eq!(ticks_to_us(throttle_ticks(0.0)), 1000);
}

#[test]
fn rc_frame_layout() {
    let f = rc_channels_frame(&[992; CHANNEL_COUNT]);
    assert_eq!(f.len(), 26);
    assert_eq!(&f[..3], &[ADDRESS_FLIGHT_CONTROLLER, 24, FRAMETYPE_RC_CHANNELS_PACKED]);
    assert_eq!(f[25], crc8_dvb_s2(&f[2..25]));
}

#[test]
fn decoder_round_trips_rc_and_link_statistics() {
    let ch: [u16; CHANNEL_COUNT] = std::array::from_fn(|i| 172 + 100 * i as u16);
    let stats = LinkStatistics { uplink_rssi_1: 60, uplink_rssi_2: 61, uplink_lq: 100, uplink_snr: -5, rf_mode: 7, ..Default::default() };
    let mut bytes = rc_channels_frame(&ch);
    bytes.extend(link_statistics_frame(&stats));
    assert_eq!(Decoder::default().push(&bytes), vec![Frame::RcChannels(ch), Frame::LinkStatistics(stats)]);
}

#[test]
fn decoder_resynchronises_after_garbage_and_split_input() {
    let mut input = vec![0x00, 0xFF, 0xC8]; // noise, including a stray sync byte
    input.extend(rc_channels_frame(&[992; CHANNEL_COUNT]));
    let (a, b) = input.split_at(10);
    let mut d = Decoder::default();
    assert!(d.push(a).is_empty());
    assert_eq!(d.push(b), vec![Frame::RcChannels([992; CHANNEL_COUNT])]);
}

#[test]
fn corrupt_crc_is_counted_and_skipped() {
    let mut input = rc_channels_frame(&[0; CHANNEL_COUNT]);
    *input.last_mut().unwrap() ^= 0xFF;
    input.extend(rc_channels_frame(&[1000; CHANNEL_COUNT]));
    let mut d = Decoder::default();
    assert_eq!(d.push(&input), vec![Frame::RcChannels([1000; CHANNEL_COUNT])]);
    assert_eq!(d.crc_errors(), 1);
}

proptest! {
    #[test]
    fn decoder_never_panics_and_holds_at_most_one_frame(
        chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..80), 0..20)
    ) {
        let mut d = Decoder::default();
        for c in &chunks {
            let _ = d.push(c);
            prop_assert!(d.buffered() < FRAME_SIZE_MAX);
        }
    }

    #[test]
    fn any_channels_round_trip(ch in proptest::array::uniform16(0u16..2048)) {
        prop_assert_eq!(Decoder::default().push(&rc_channels_frame(&ch)), vec![Frame::RcChannels(ch)]);
    }
}
