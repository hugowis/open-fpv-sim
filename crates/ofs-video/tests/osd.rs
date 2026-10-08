use ofs_core::{Bus, Model, StepCtx, Wire};
use ofs_video::osd::{Cell, OsdDecoder, OsdModel, PRESENT_TIMEOUT_S};

/// An MSP v1 reply frame: `$M>` len cmd payload checksum (XOR of len, cmd and the payload).
fn msp(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![b'$', b'M', b'>', payload.len() as u8, cmd];
    f.extend_from_slice(payload);
    let cks = payload.iter().fold(payload.len() as u8 ^ cmd, |a, b| a ^ b);
    f.push(cks);
    f
}

fn dp(sub: u8, rest: &[u8]) -> Vec<u8> {
    let mut payload = vec![sub];
    payload.extend_from_slice(rest);
    msp(182, &payload)
}

fn write(row: u8, col: u8, attr: u8, text: &str) -> Vec<u8> {
    let mut rest = vec![row, col, attr];
    rest.extend_from_slice(text.as_bytes());
    dp(3, &rest)
}

fn screen(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.concat()
}

#[test]
fn nothing_is_present_before_the_first_draw() {
    let d = OsdDecoder::new(30, 16);
    let f = d.frame(0.0);
    assert!(!f.present);
    assert_eq!((f.grid.cols, f.grid.rows, f.seq), (30, 16, 0));
}

#[test]
fn a_drawn_screen_is_published_as_text() {
    let mut d = OsdDecoder::new(30, 16);
    d.push(&screen(&[dp(0, &[]), dp(2, &[]), write(14, 1, 0, "24.6"), write(0, 10, 0, "OPENFPV"), dp(4, &[])]), 0.1);
    let f = d.frame(0.1);
    assert!(f.present);
    assert_eq!(f.seq, 1);
    let rows = f.grid.text_rows();
    assert_eq!(rows[14].trim_end(), " 24.6");
    assert_eq!(rows[0].trim_end(), "          OPENFPV");
    assert_eq!(rows[1].trim(), "");
}

#[test]
fn a_half_drawn_screen_is_never_shown() {
    let mut d = OsdDecoder::new(30, 16);
    d.push(&screen(&[dp(2, &[]), write(1, 1, 0, "AAA"), dp(4, &[])]), 0.0);
    d.push(&screen(&[dp(2, &[]), write(1, 1, 0, "BBB")]), 0.1); // no draw yet
    assert_eq!(d.frame(0.1).grid.text_rows()[1].trim(), "AAA");
    d.push(&dp(4, &[]), 0.2);
    assert_eq!(d.frame(0.2).grid.text_rows()[1].trim(), "BBB");
}

#[test]
fn the_sequence_number_only_moves_when_the_cells_change() {
    let mut d = OsdDecoder::new(30, 16);
    let one = screen(&[dp(2, &[]), write(2, 2, 0, "X"), dp(4, &[])]);
    d.push(&one, 0.0);
    d.push(&one, 0.1);
    assert_eq!(d.frame(0.1).seq, 1, "an identical redraw is not a new frame");
    d.push(&screen(&[dp(2, &[]), write(2, 2, 0, "Y"), dp(4, &[])]), 0.2);
    assert_eq!(d.frame(0.2).seq, 2);
}

#[test]
fn attributes_select_blink_and_the_font_page() {
    let mut d = OsdDecoder::new(30, 16);
    d.push(&screen(&[dp(2, &[]), write(0, 0, 0x41, "A"), dp(4, &[])]), 0.0);
    let cell = d.frame(0.0).grid.get(0, 0);
    assert_eq!(cell, Cell { ch: b'A', page: 1, blink: true });
    assert_eq!(Cell::unpack(cell.packed()), cell);
    assert_eq!(cell.packed(), u32::from(b'A') | 1 << 8 | 1 << 10);
}

#[test]
fn writes_outside_the_grid_are_clipped() {
    let mut d = OsdDecoder::new(30, 16);
    d.push(&screen(&[dp(2, &[]), write(15, 28, 0, "ABCDEF"), write(40, 0, 0, "Z"), write(0, 255, 0, "Z"), dp(4, &[])]), 0.0);
    let rows = d.frame(0.0).grid.text_rows();
    assert_eq!(&rows[15][26..], "  AB");
    assert!(rows.iter().all(|r| !r.contains('Z')));
}

#[test]
fn the_osd_is_gone_after_a_second_without_a_draw_or_after_release() {
    let mut d = OsdDecoder::new(30, 16);
    d.push(&screen(&[dp(2, &[]), write(0, 0, 0, "A"), dp(4, &[])]), 0.0);
    assert!(d.frame(PRESENT_TIMEOUT_S - 0.01).present);
    assert!(!d.frame(PRESENT_TIMEOUT_S + 0.01).present);
    d.push(&dp(4, &[]), 5.0);
    assert!(d.frame(5.1).present, "a new draw brings it back");
    d.push(&dp(1, &[]), 5.2);
    assert!(!d.frame(5.2).present, "released");
    d.push(&dp(4, &[]), 5.3);
    assert!(d.frame(5.3).present, "drawn again");
}

#[test]
fn bytes_split_across_pushes_still_decode() {
    let mut d = OsdDecoder::new(30, 16);
    let bytes = screen(&[dp(2, &[]), write(3, 3, 0, "SPLIT"), dp(4, &[])]);
    for chunk in bytes.chunks(3) {
        d.push(chunk, 0.0);
    }
    assert_eq!(d.frame(0.0).grid.text_rows()[3].trim(), "SPLIT");
}

#[test]
fn garbage_is_skipped_and_the_next_good_frame_still_draws() {
    let mut d = OsdDecoder::new(30, 16);
    // A deterministic pseudo-random byte stream (xorshift), a truncated frame, a bad checksum, other MSP replies
    // and DisplayPort frames this decoder does not use.
    let mut x: u32 = 0x1234_5678;
    let noise: Vec<u8> = (0..4000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect();
    d.push(&noise, 0.0);
    d.push(&write(1, 1, 0, "TRUNC")[..6], 0.0);
    let mut bad = write(1, 1, 0, "BAD");
    *bad.last_mut().unwrap() ^= 0xFF;
    d.push(&bad, 0.0);
    d.push(&msp(101, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]), 0.0); // MSP_STATUS-like reply
    d.push(&dp(6, &[1, 2, 3]), 0.0); // system element
    d.push(&dp(77, &[]), 0.0); // unknown subcommand
    d.push(&write(1, 1, 0x80, "VERSION"), 0.0); // attribute version bit set: not a v1/v2 write
    d.push(&screen(&[dp(2, &[]), write(5, 5, 0, "OK"), dp(4, &[])]), 0.1);
    let f = d.frame(0.1);
    assert!(f.present);
    assert_eq!(f.grid.text_rows()[5].trim(), "OK");
    assert!(d.unknown_frames() >= 3, "unknown frames are counted: {}", d.unknown_frames());
}

#[test]
fn the_model_publishes_frames_through_its_handle_deterministically() {
    fn run() -> u64 {
        let tap = Wire::new(4096);
        let mut model = OsdModel::new(30, 16, tap.clone(), 1);
        let handle = model.handle();
        let mut bus = Bus::new();
        tap.write(&screen(&[dp(2, &[]), write(14, 1, 0, "24.6"), dp(4, &[])]));
        model.step(&StepCtx { tick: 0, time_s: 0.25, dt_s: 0.001 }, &mut bus).unwrap();
        let frame = handle.latest();
        assert!(frame.present);
        assert_eq!(frame.grid.text_rows()[14].trim_end(), " 24.6");
        frame.digest()
    }
    assert_eq!(run(), run(), "same bytes, same frame digest");
}
