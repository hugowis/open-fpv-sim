use ofs_client::fpv::{field_floor, FieldUniforms, FpvBuffer};
use ofs_client::model::{OsdFrame, VideoSync};

fn uniforms(present: bool, valid: bool) -> (FieldUniforms, bool) {
    (
        FieldUniforms { present, noise: 0.1, sparkles: 0.0, chroma: 1.0, sync: VideoSync::Locked },
        valid,
    )
}

fn push_fields(buffer: &mut FpvBuffer, times: &[f64], valid: &[bool]) {
    for (t, v) in times.iter().zip(valid) {
        let (u, _) = uniforms(true, *v);
        buffer.push_field(*t, u, *v);
    }
}

#[test]
fn the_field_boundaries_quantize_to_twenty_milliseconds() {
    assert_eq!(field_floor(0.019), 0.0);
    assert_eq!(field_floor(0.02), 0.02);
    assert_eq!(field_floor(1.037), 1.02, "37 ms of latency quantizes down to one field");
}

#[test]
fn samples_within_one_field_replace_it() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.004, 0.008], &[true, true, true]);
    let mut marker = uniforms(true, true).0;
    marker.noise = 0.9;
    b.push_field(0.0199, marker, true); // a newer sample of the same field
    let (t, u, valid) = b.field_at(0.02).unwrap();
    assert_eq!(t, 0.0);
    assert_eq!(u.noise, 0.9, "the newest sample of a field replaces its entry");
    assert!(valid);
}

#[test]
fn a_dropped_field_holds_the_previous_one() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.02, 0.04], &[true, true, false]);
    let (t, _, _) = b.field_at(0.06).unwrap();
    assert_eq!(t, 0.02, "field 0.04 dropped: the newest valid one before it shows");
}

#[test]
fn many_drops_walk_back_bounded() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0], &[true]);
    push_fields(&mut b, &[0.02, 0.04, 0.06], &[false, false, false]);
    let (t, _, _) = b.field_at(0.08).unwrap();
    assert_eq!(t, 0.0, "the walk stops at the oldest buffered valid field");
    assert!(b.field_at(field_floor(-0.01)).is_none(), "nothing before the first field");
}

#[test]
fn zero_latency_picks_the_current_field() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.02, 0.04], &[true, true, true]);
    // latency 0: the target is field_floor(sim_now) — the newest received field, nothing held back.
    let (t, _, _) = b.field_at(field_floor(0.059)).unwrap();
    assert_eq!(t, 0.04);
}

#[test]
fn the_osd_picks_the_newest_frame_at_or_before_the_instant() {
    let mut b = FpvBuffer::default();
    b.push_osd(OsdFrame { seq: 1, time_s: 0.01, present: true, cols: 1, rows: 1, cells: vec![0x41] });
    b.push_osd(OsdFrame { seq: 2, time_s: 0.031, present: true, cols: 1, rows: 1, cells: vec![0x42] });
    assert_eq!(b.osd_at(0.03).unwrap().seq, 1, "a frame drawn after the instant is not shown yet");
    assert_eq!(b.osd_at(0.05).unwrap().seq, 2, "exactly at the boundary the newer frame counts as drawn");
    assert!(b.osd_at(0.0).is_none());
}
