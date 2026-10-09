use ofs_video::smartaudio::*;

#[test]
fn the_crc_matches_the_get_settings_request_betaflight_sends() {
    // `aa 55 03 00 9f` is what the M0 spike saw on UART5.
    assert_eq!(crc8(&[0xAA, 0x55, 0x03, 0x00]), 0x9F);
}

#[test]
fn request_frames_have_golden_bytes() {
    assert_eq!(request_frame(CMD_GET_SETTINGS, &[]), [0xAA, 0x55, 0x03, 0x00, 0x9F]);
    assert_eq!(request_frame(CMD_SET_CHANNEL, &[0x22]), [0xAA, 0x55, 0x07, 0x01, 0x22, 0x63]);
    assert_eq!(request_frame(CMD_SET_POWER, &[0x9C]), [0xAA, 0x55, 0x05, 0x01, 0x9C, 0x01]);
}

#[test]
fn response_frames_check_the_crc_without_the_preamble() {
    assert_eq!(response_frame(CMD_SET_CHANNEL, &[0x22]), [0xAA, 0x55, 0x03, 0x01, 0x22, 0x80]);
}

#[test]
fn the_v21_settings_response_matches_the_golden_frame() {
    let payload = settings_payload(0x20, 0x10, 5658, 23, &[14, 23, 28, 30]);
    assert_eq!(payload, [0x20, 0x17, 0x10, 0x16, 0x1A, 0x17, 0x04, 0x00, 0x0E, 0x17, 0x1C, 0x1E]);
    let frame = response_frame(RESP_SETTINGS_V21, &payload);
    assert_eq!(frame, [0xAA, 0x55, 0x11, 0x0C, 0x20, 0x17, 0x10, 0x16, 0x1A, 0x17, 0x04, 0x00, 0x0E, 0x17, 0x1C, 0x1E, 0xB5]);
}

#[test]
fn the_parser_reads_every_request_kind() {
    let mut p = RequestParser::default();
    let mut bytes = Vec::new();
    bytes.extend(request_frame(CMD_GET_SETTINGS, &[]));
    bytes.extend(request_frame(CMD_SET_POWER, &[0x9C]));
    bytes.extend(request_frame(CMD_SET_CHANNEL, &[34]));
    bytes.extend(request_frame(CMD_SET_FREQUENCY, &[0x16, 0xA8])); // 5800
    bytes.extend(request_frame(CMD_SET_MODE, &[0x0C]));
    assert_eq!(
        p.push(&bytes),
        vec![
            Request::GetSettings,
            Request::SetPower(0x9C),
            Request::SetChannel(34),
            Request::SetFrequency(5800),
            Request::SetMode(0x0C)
        ]
    );
    assert_eq!(p.bad_frames(), 0);
}

#[test]
fn the_parser_handles_split_input_and_noise_between_frames() {
    let mut p = RequestParser::default();
    let frame = request_frame(CMD_SET_CHANNEL, &[7]);
    let mut got = Vec::new();
    got.extend(p.push(&[0x00, 0xFF, 0xAA])); // noise, then the first preamble byte
    got.extend(p.push(&frame[1..3]));
    got.extend(p.push(&frame[3..]));
    got.extend(p.push(&[0xAA, 0xAA, 0x55, 0x03, 0x00, 0x9F])); // a doubled 0xAA still finds the frame
    assert_eq!(got, vec![Request::SetChannel(7), Request::GetSettings]);
}

#[test]
fn bad_frames_are_ignored_and_counted() {
    let mut p = RequestParser::default();
    let mut bad_crc = request_frame(CMD_SET_CHANNEL, &[1]);
    *bad_crc.last_mut().unwrap() ^= 0xFF;
    assert!(p.push(&bad_crc).is_empty());
    assert!(p.push(&request_frame(9, &[])).is_empty(), "unknown command");
    assert!(p.push(&request_frame(CMD_SET_FREQUENCY, &[1])).is_empty(), "wrong payload length");
    assert!(p.push(&response_frame(CMD_SET_CHANNEL, &[1])).is_empty(), "a response is not a request");
    assert_eq!(p.bad_frames(), 4);
    assert_eq!(p.push(&request_frame(CMD_GET_SETTINGS, &[])), vec![Request::GetSettings], "still in sync");
}

#[test]
fn an_absurd_length_does_not_hang_or_panic() {
    let mut p = RequestParser::default();
    assert!(p.push(&[0xAA, 0x55, 0x07, 0xFF, 1, 2, 3]).is_empty());
    assert_eq!(p.push(&request_frame(CMD_GET_SETTINGS, &[])), vec![Request::GetSettings]);
}
