use ofs_proto::pb::{PilotInput, State, Sticks, VideoLink, VideoSync};
use ofs_proto::PROTOCOL_VERSION;
use prost::Message;

#[test]
fn the_protocol_version_matches_the_python_client() {
    // python/ofs/client.py PROTOCOL_VERSION must equal this; bump both together.
    assert_eq!(PROTOCOL_VERSION, 4);
    let python = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../python/ofs/client.py")).unwrap();
    assert!(python.contains(&format!("PROTOCOL_VERSION = {PROTOCOL_VERSION}")), "python client speaks another protocol");
}

#[test]
fn a_pilot_input_survives_an_encode_decode_round_trip() {
    let input = PilotInput {
        sticks: Some(Sticks { roll: 0.25, pitch: -0.5, yaw: 1.0, throttle: 0.75, aux: vec![1.0, -1.0] }),
        state_rate_hz: 120,
    };
    let bytes = input.encode_to_vec();
    assert_eq!(PilotInput::decode(bytes.as_slice()).unwrap(), input);
}

#[test]
fn a_state_from_an_older_server_reads_as_no_video_link() {
    let bytes = State { time_s: 1.0, ..Default::default() }.encode_to_vec();
    let state = State::decode(bytes.as_slice()).unwrap();
    assert!(state.video.is_none());
    assert_eq!(VideoLink::default().sync(), VideoSync::Unspecified);
}
