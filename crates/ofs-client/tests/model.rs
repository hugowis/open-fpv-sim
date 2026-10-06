use ofs_client::{ClientError, ErrorKind, Event, EventKind, Sticks, Telemetry};
use ofs_proto::pb;
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};

fn status(code: Code, kind: Option<&str>, message: &str) -> Status {
    let mut md = MetadataMap::new();
    if let Some(kind) = kind {
        md.insert("ofs-error-kind", kind.parse().unwrap());
    }
    Status::with_metadata(code, message, md)
}

#[test]
fn every_server_error_kind_maps_to_a_typed_error() {
    for (kind, expected) in [
        ("config", ErrorKind::Config),
        ("firmware", ErrorKind::Firmware),
        ("numerical", ErrorKind::Numerical),
        ("protocol", ErrorKind::Protocol),
        ("not_loaded", ErrorKind::NotLoaded),
        ("invalid_argument", ErrorKind::InvalidArgument),
        ("invalid_state", ErrorKind::InvalidState),
        ("pilot_busy", ErrorKind::PilotBusy),
        ("internal", ErrorKind::Internal),
    ] {
        let e = ClientError::from_status(&status(Code::Aborted, Some(kind), "boom"));
        assert_eq!(e.kind, expected, "{kind}");
        assert_eq!(e.kind.as_str(), kind);
        assert_eq!(e.message, "boom");
    }
}

#[test]
fn transport_failures_and_unknown_kinds_have_their_own_mapping() {
    for code in [Code::Unavailable, Code::Unknown, Code::Cancelled, Code::DeadlineExceeded] {
        let e = ClientError::from_status(&status(code, None, "h2 protocol error: error reading a body from connection"));
        assert_eq!(e.kind, ErrorKind::Unavailable, "{code:?}: a lost connection");
    }
    let e = ClientError::from_status(&status(Code::Internal, Some("from_the_future"), "x"));
    assert_eq!(e.kind, ErrorKind::Other);
    assert!(e.message.contains("Internal") && e.message.contains('x'), "{}", e.message);
}

#[test]
fn sticks_are_clamped_and_non_finite_values_become_neutral() {
    let s = Sticks { roll: 2.0, pitch: -3.0, yaw: f64::NAN, throttle: 1.5, aux: [5.0, -5.0, f64::INFINITY, 0.25] }.sanitized();
    assert_eq!((s.roll, s.pitch, s.yaw, s.throttle), (1.0, -1.0, 0.0, 1.0));
    assert_eq!(s.aux, [1.0, -1.0, -1.0, 0.25]);
    let s = Sticks { throttle: -0.5, ..Default::default() }.sanitized();
    assert_eq!(s.throttle, 0.0);
    assert_eq!(Sticks::default().aux, [-1.0; 4], "switches default to off");
}

#[test]
fn telemetry_is_derived_from_a_state_message() {
    let state = pb::State {
        time_s: 12.5,
        position_ned_m: Some(pb::Vec3 { x: 1.0, y: 2.0, z: -30.0 }),
        velocity_ned_mps: Some(pb::Vec3 { x: 3.0, y: 4.0, z: -2.0 }),
        battery_voltage_v: 24.5,
        battery_current_a: 10.0,
        motor_cmd: vec![0.0, 0.055, 0.0, 0.0],
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0 }),
        running: true,
        overruns: 3,
        fc_restarts: 1,
        ..Default::default()
    };
    let t = Telemetry::from_pb(&state);
    assert_eq!(t.altitude_m, 30.0);
    assert!((t.speed_mps - (9.0f64 + 16.0 + 4.0).sqrt()).abs() < 1e-12);
    assert_eq!(t.climb_mps, 2.0);
    assert!(t.motors_spinning, "5.5 % is armed idle");
    assert!(t.tx_enabled && t.link_up && t.running);
    assert_eq!((t.overruns, t.fc_restarts), (3, 1));
    let disarmed = Telemetry::from_pb(&pb::State { motor_cmd: vec![0.0; 4], ..Default::default() });
    assert!(!disarmed.motors_spinning);
}

#[test]
fn event_kinds_map_and_unknown_kinds_survive() {
    let e = Event::from_pb(pb::Event { time_s: 1.5, kind: pb::EventKind::LinkDown as i32, message: "down".into() });
    assert_eq!((e.kind, e.kind.as_str(), e.time_s, e.message.as_str()), (EventKind::LinkDown, "link_down", 1.5, "down"));
    assert_eq!(Event::from_pb(pb::Event { kind: 999, ..Default::default() }).kind, EventKind::Unknown);
}