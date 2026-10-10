use glam::DVec3;
use ofs_client::{ClientError, ErrorKind, Event, EventKind, OsdFrame, Sticks, Telemetry, VideoInfo, VideoSync, VtxInfo, World};
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
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0, ..Default::default() }),
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

#[test]
fn vtx_and_osd_messages_convert() {
    let vtx = VtxInfo::from_pb(&pb::Vtx { present: true, band: 5, channel: 3, freq_mhz: 5732, power_mw: 600, pit_mode: false });
    assert_eq!(vtx, VtxInfo { present: true, band: 5, channel: 3, freq_mhz: 5732, power_mw: 600, pit_mode: false });
    assert_eq!(vtx.channel_name(), "R3");
    assert_eq!(VtxInfo { band: 0, channel: 0, ..vtx }.channel_name(), "", "user-frequency mode has no channel name");
    assert_eq!(VtxInfo::default().channel_name(), "");

    let osd = OsdFrame::from_pb(pb::OsdFrame { seq: 4, time_s: 1.5, present: true, cols: 2, rows: 1, cells: vec![0x20, 0x441] });
    assert_eq!((osd.seq, osd.cols, osd.rows, osd.present), (4, 2, 1, true));
    assert_eq!(osd.cells, vec![0x20, 0x441]);
}

#[test]
fn new_event_kinds_have_names() {
    use ofs_client::{Event, EventKind};
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VtxChanged as i32, message: "R3 5732 MHz 600 mW".into() });
    assert_eq!(e.kind, EventKind::VtxChanged);
    assert_eq!(e.kind.as_str(), "vtx_changed");
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::SerialOverflow as i32, message: String::new() });
    assert_eq!(e.kind.as_str(), "serial_overflow");
}

#[test]
fn video_messages_convert() {
    let m = pb::VideoLink {
        present: true,
        snr_db: 7.5,
        interference_dbm: -100.0,
        rssi: vec![pb::AntennaRssi { name: "omni".into(), rssi_dbm: -90.0 }, pb::AntennaRssi { name: "patch".into(), rssi_dbm: -85.5 }],
        active_antenna: "patch".into(),
        noise: 0.6,
        sparkles: 0.1,
        chroma: 0.8,
        sync: pb::VideoSync::Unstable as i32,
    };
    let v = VideoInfo::from_pb(&m);
    assert_eq!(v.sync, VideoSync::Unstable);
    assert_eq!(v.sync.as_str(), "unstable");
    assert_eq!(v.rssi_dbm, vec![("omni".to_string(), -90.0), ("patch".to_string(), -85.5)]);
    assert_eq!((v.active_antenna.as_str(), v.snr_db, v.noise, v.chroma), ("patch", 7.5, 0.6, 0.8));
    let absent = VideoInfo::from_pb(&pb::VideoLink::default());
    assert_eq!(absent.sync, VideoSync::None);
    assert_eq!(absent.sync.as_str(), "");
    let t = Telemetry::from_pb(&pb::State::default());
    assert_eq!(t.video, VideoInfo::default(), "a state without video: no link, a clean picture");
    assert_eq!(t.video.chroma, 1.0);
}

#[test]
fn the_world_converts_to_godots_frame() {
    let v = |x, y, z| Some(pb::Vec3 { x, y, z });
    let w = World::from_pb(pb::World {
        name: "Flat field".into(),
        pilot_position_ned_m: v(-3.0, 2.0, -1.7),
        pilot_facing_deg: 0.0,
        antennas: vec![
            pb::ReceiverAntenna { name: "omni".into(), kind: "omni".into(), aim_el_deg: 90.0, ..Default::default() },
            pb::ReceiverAntenna { name: "patch".into(), kind: "patch".into(), aim_az_deg: 0.0, aim_el_deg: 10.0, ..Default::default() },
        ],
        objects: vec![pb::WorldObject {
            name: "BuildingA".into(),
            shape: "box".into(),
            center_ned_m: v(110.0, -45.0, -7.0),
            size_m: v(12.0, 14.0, 14.0),
            color: v(0.62, 0.64, 0.66),
            rf_loss_db: 25.0,
            ..Default::default()
        }],
        emitters: vec![pb::Emitter { name: "parked-quad".into(), position_ned_m: v(30.0, -60.0, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
        ..Default::default()
    });
    assert_eq!(w.pilot_position, DVec3::new(2.0, 1.7, 3.0), "east, up, south");
    let b = &w.objects[0];
    assert_eq!(b.center, DVec3::new(-45.0, 7.0, -110.0), "where world.gd drew building A");
    assert_eq!(b.size, DVec3::new(14.0, 14.0, 12.0), "east, height, north");
    assert_eq!(b.color, [0.62, 0.64, 0.66]);
    assert!((w.antennas[0].aim - DVec3::Y).length() < 1e-12, "the omni stands up: {}", w.antennas[0].aim);
    let up = 10f64.to_radians();
    assert!((w.antennas[1].aim - DVec3::new(0.0, up.sin(), -up.cos())).length() < 1e-12, "the patch looks north (-Z), 10 degrees up");
    assert_eq!(w.emitters[0].position, DVec3::new(-60.0, 1.0, -30.0));
    let east = World::from_pb(pb::World { pilot_facing_deg: 90.0, antennas: vec![pb::ReceiverAntenna { aim_el_deg: 0.0, ..Default::default() }], ..Default::default() });
    assert!((east.antennas[0].aim - DVec3::X).length() < 1e-12, "facing east, aimed ahead: +X");
}

#[test]
fn the_video_event_kinds_have_names() {
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoLost as i32, message: "video lost: SNR -2.0 dB on omni".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::VideoLost, "video_lost"));
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoRestored as i32, message: String::new() });
    assert_eq!(e.kind.as_str(), "video_restored");
}
