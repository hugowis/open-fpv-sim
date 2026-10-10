use ofs_core::Wire;

#[test]
fn bytes_come_out_in_order() {
    let w = Wire::new(16);
    w.write(&[1, 2, 3]);
    w.write(&[4]);
    assert_eq!(w.take(2), vec![1, 2]);
    assert_eq!(w.take(10), vec![3, 4]);
    assert!(w.is_empty());
}

#[test]
fn clones_share_one_fifo() {
    let a = Wire::new(16);
    let b = a.clone();
    a.write(&[7, 8]);
    assert_eq!(b.len(), 2);
    assert_eq!(b.take(usize::MAX), vec![7, 8]);
    assert!(a.is_empty());
}

#[test]
fn overflow_drops_the_oldest_bytes() {
    let w = Wire::new(4);
    w.write(&[1, 2, 3]);
    w.write(&[4, 5, 6]);
    assert_eq!(w.take(usize::MAX), vec![3, 4, 5, 6]);
    assert_eq!(w.dropped(), 2);
}

#[test]
#[should_panic(expected = "capacity")]
fn zero_capacity_is_rejected() {
    Wire::new(0);
}

#[test]
fn one_write_larger_than_the_wire_keeps_its_newest_bytes() {
    let w = Wire::new(4);
    w.write(&[1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(w.take(10), vec![4, 5, 6, 7]);
    assert_eq!(w.dropped(), 3);
}

#[test]
fn a_wire_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Wire>();
}
