use ofs_core::Wire;
use ofs_fc::sitl::bridge::{route_reply, SerialTap};
use ofs_fc::sitl::codec::ReplySerial;

#[test]
fn blocks_reach_the_tap_wired_to_their_uart_and_others_are_discarded() {
    let osd = Wire::new(64);
    let sa = Wire::new(64);
    let taps = [SerialTap { uart_index: 3, tx: osd.clone() }, SerialTap { uart_index: 4, tx: sa.clone() }];
    let serial = ReplySerial { dropped: 0, blocks: vec![(3, vec![1, 2, 3]), (1, vec![9]), (4, vec![7]), (3, vec![4])] };
    route_reply(&taps, &serial);
    assert_eq!(osd.take(64), vec![1, 2, 3, 4]);
    assert_eq!(sa.take(64), vec![7]);
}

#[test]
fn a_full_tap_drops_its_oldest_bytes_and_counts_them() {
    let osd = Wire::new(4);
    let taps = [SerialTap { uart_index: 3, tx: osd.clone() }];
    route_reply(&taps, &ReplySerial { dropped: 0, blocks: vec![(3, vec![1, 2, 3, 4, 5, 6])] });
    assert_eq!(osd.take(8), vec![3, 4, 5, 6]);
    assert_eq!(osd.dropped(), 2);
}
