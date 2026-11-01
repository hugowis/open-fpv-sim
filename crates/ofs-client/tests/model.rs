use glam::DVec3;
use ofs_client::{Terrain, ClientError, ErrorKind, Event, EventKind, FcTelemetry, OsdFrame, RaceMode, Sticks, Telemetry, VideoInfo, VideoSync, VtxInfo, World, WorldMesh};
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
fn telemetry_carries_the_motor_rpm() {
    let t = Telemetry::from_pb(&pb::State { motor_rpm: vec![100.0, 200.0], ..Default::default() });
    assert_eq!(t.motor_rpm, vec![100.0, 200.0]);
}

#[test]
fn telemetry_builds_the_audio_input() {
    let state = pb::State {
        motor_rpm: vec![1000.0, 2000.0],
        motor_cmd: vec![0.25, 0.5],
        velocity_ned_mps: Some(pb::Vec3 { x: 3.0, y: 4.0, z: 2.0 }),
        ..Default::default()
    };
    let t = Telemetry::from_pb(&state);
    let a = t.audio_input(true);
    assert_eq!(a.motor_rpm, vec![1000.0, 2000.0]);
    assert_eq!(a.motor_cmd, vec![0.25, 0.5]);
    assert_eq!(a.airspeed_mps, t.speed_mps);
    assert_eq!(a.descent_mps, 2.0);
    assert!(a.active);
    assert!(!t.audio_input(false).active);
}

#[test]
fn default_telemetry_age_is_infinity() {
    // No frame yet: the age must be INFINITY (the doc comment's contract), not 0.0 ("just arrived").
    assert_eq!(FcTelemetry::default().telemetry_age_s, f64::INFINITY);
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
        field_valid: true,
        latency_ms: 40,
    };
    let v = VideoInfo::from_pb(&m);
    assert_eq!(v.sync, VideoSync::Unstable);
    assert_eq!(v.sync.as_str(), "unstable");
    assert_eq!(v.rssi_dbm, vec![("omni".to_string(), -90.0), ("patch".to_string(), -85.5)]);
    assert_eq!((v.active_antenna.as_str(), v.snr_db, v.noise, v.chroma), ("patch", 7.5, 0.6, 0.8));
    assert!(v.field_valid);
    assert_eq!(v.latency_ms, 40);
    let absent = VideoInfo::from_pb(&pb::VideoLink::default());
    assert_eq!(absent.sync, VideoSync::None);
    assert_eq!(absent.sync.as_str(), "");
    assert!(!absent.field_valid);
    assert_eq!(absent.latency_ms, 0);
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
    })
    .unwrap();
    assert_eq!(w.pilot_position, DVec3::new(2.0, 1.7, 3.0), "east, up, south");
    let b = &w.objects[0];
    assert_eq!(b.center, DVec3::new(-45.0, 7.0, -110.0), "where world.gd drew building A");
    assert_eq!(b.size, DVec3::new(14.0, 14.0, 12.0), "east, height, north");
    assert_eq!(b.color, [0.62, 0.64, 0.66]);
    assert!((w.antennas[0].aim - DVec3::Y).length() < 1e-12, "the omni stands up: {}", w.antennas[0].aim);
    let up = 10f64.to_radians();
    assert!((w.antennas[1].aim - DVec3::new(0.0, up.sin(), -up.cos())).length() < 1e-12, "the patch looks north (-Z), 10 degrees up");
    assert_eq!(w.emitters[0].position, DVec3::new(-60.0, 1.0, -30.0));
    let east = World::from_pb(pb::World { pilot_facing_deg: 90.0, antennas: vec![pb::ReceiverAntenna { aim_el_deg: 0.0, ..Default::default() }], ..Default::default() }).unwrap();
    assert!((east.antennas[0].aim - DVec3::X).length() < 1e-12, "facing east, aimed ahead: +X");
}

#[test]
fn the_video_event_kinds_have_names() {
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoLost as i32, message: "video lost: SNR -2.0 dB on omni".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::VideoLost, "video_lost"));
    let e = Event::from_pb(pb::Event { time_s: 1.0, kind: pb::EventKind::VideoRestored as i32, message: String::new() });
    assert_eq!(e.kind.as_str(), "video_restored");
}

#[test]
fn a_gate_is_converted_to_godots_frame() {
    let v = |x, y, z| Some(pb::Vec3 { x, y, z });
    let w = World::from_pb(pb::World {
        gates: vec![pb::Gate { name: "Gate0".into(), center_ned_m: v(30.0, 0.0, -1.22), heading_deg: 90.0, width_m: 3.08, height_m: 2.44, pitch_deg: 10.0, roll_deg: -20.0 }],
        ..Default::default()
    })
    .unwrap();
    let g = &w.gates[0];
    assert_eq!(g.name, "Gate0");
    assert_eq!(g.center, DVec3::new(0.0, 1.22, -30.0), "east, up, south");
    assert_eq!((g.heading_deg, g.width_m, g.height_m), (90.0, 3.08, 2.44));
    assert_eq!((g.pitch_deg, g.roll_deg), (10.0, -20.0));
    assert!(World::from_pb(pb::World::default()).unwrap().gates.is_empty());
}

#[test]
fn the_worlds_courses_reach_the_client() {
    let w = World::from_pb(pb::World {
        courses: vec![
            pb::Course { name: "Line".into(), gates: vec!["A".into(), "B".into()], mode: pb::RaceMode::Laps as i32, laps: 3, time_limit_s: 0.0 },
            pb::Course { name: "Hunt".into(), gates: vec!["B".into()], mode: pb::RaceMode::TimeAttack as i32, laps: 0, time_limit_s: 45.0 },
        ],
        default_course: 1,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(w.default_course, 1);
    assert_eq!((w.courses[0].name.as_str(), w.courses[0].gates.clone(), w.courses[0].mode, w.courses[0].laps), ("Line", vec!["A".to_string(), "B".to_string()], RaceMode::Laps, 3));
    assert_eq!((w.courses[1].mode, w.courses[1].time_limit_s), (RaceMode::TimeAttack, 45.0));
    assert!(World::from_pb(pb::World::default()).unwrap().courses.is_empty());
}

#[test]
fn the_race_state_reaches_the_telemetry() {
    let state = pb::State {
        race: Some(pb::RaceState {
            present: true,
            running: true,
            next_gate: 2,
            gate_count: 3,
            laps: 1,
            current_lap_s: 4.5,
            last_lap_s: 31.2,
            best_lap_s: 30.0,
            finished: true,
            lap_limit: 3,
            course: "Hunt".into(),
            course_index: 1,
            mode: pb::RaceMode::TimeAttack as i32,
            time_limit_s: 60.0,
            time_left_s: 12.5,
        }),
        ..Default::default()
    };
    let r = Telemetry::from_pb(&state).race;
    assert_eq!((r.present, r.running, r.next_gate, r.gate_count, r.laps), (true, true, 2, 3, 1));
    assert_eq!((r.current_lap_s, r.last_lap_s, r.best_lap_s), (4.5, 31.2, 30.0));
    assert_eq!((r.finished, r.lap_limit), (true, 3));
    assert_eq!((r.course.as_str(), r.course_index, r.mode, r.time_limit_s, r.time_left_s), ("Hunt", 1, RaceMode::TimeAttack, 60.0, 12.5));
    assert!(!Telemetry::from_pb(&pb::State::default()).race.present, "a state without a race: none");
}

#[test]
fn the_race_event_kinds_have_names() {
    let e = Event::from_pb(pb::Event { time_s: 3.0, kind: pb::EventKind::GatePassed as i32, message: "Gate1 (gate 1) split 2.000 s".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::GatePassed, "gate_passed"));
    let e = Event::from_pb(pb::Event { time_s: 9.0, kind: pb::EventKind::LapCompleted as i32, message: "lap 1: 7.000 s (best)".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::LapCompleted, "lap_completed"));
    let e = Event::from_pb(pb::Event { time_s: 4.0, kind: pb::EventKind::GateHit as i32, message: "Gate1 (gate 1) hit".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::GateHit, "gate_hit"));
    let e = Event::from_pb(pb::Event { time_s: 5.0, kind: pb::EventKind::RaceFinished as i32, message: "race finished: 3 laps".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::RaceFinished, "race_finished"));
    let e = Event::from_pb(pb::Event { time_s: 6.0, kind: pb::EventKind::RaceReset as i32, message: "race reset".into() });
    assert_eq!((e.kind, e.kind.as_str()), (EventKind::RaceReset, "race_reset"));
}

fn sloped() -> Terrain {
    // Height rises 0.5 m per cell to the north and 0.25 m per cell to the east, 2 m cells, 4 x 5 nodes from (-2, 3).
    let heights = (0..4).flat_map(|i| (0..5).map(move |j| 0.5 * i as f32 + 0.25 * j as f32)).collect();
    Terrain::from_grid([-2.0, 3.0], 2.0, 4, 5, heights).unwrap()
}

fn terrain_pb(t: &Terrain) -> pb::Terrain {
    let (nodes_n, nodes_e) = t.nodes();
    pb::Terrain {
        origin_north_m: t.origin_ne()[0],
        origin_east_m: t.origin_ne()[1],
        cell_m: t.cell_m(),
        nodes_n: nodes_n as u32,
        nodes_e: nodes_e as u32,
        heights: t.heights().to_vec(),
        digest: t.digest(),
    }
}

#[test]
fn a_world_with_a_terrain_keeps_it() {
    let t = sloped();
    let w = World::from_pb(pb::World { terrain: Some(terrain_pb(&t)), ..Default::default() }).unwrap();
    let kept = w.terrain.unwrap();
    assert_eq!(kept.digest(), t.digest());
    assert_eq!(*kept, t);
}

#[test]
fn a_corrupt_terrain_is_an_error() {
    let t = sloped();
    let err = |f: fn(&mut pb::Terrain)| {
        let mut p = terrain_pb(&t);
        f(&mut p);
        World::from_pb(pb::World { terrain: Some(p), ..Default::default() }).unwrap_err()
    };
    let e = err(|p| p.digest ^= 1);
    assert!(e.contains("digest"), "{e}");
    let e = err(|p| {
        p.heights.pop();
    });
    assert!(e.contains("heights"), "{e}");
    let e = err(|p| p.cell_m = 0.0);
    assert!(e.contains("cell size"), "{e}");
}

#[test]
fn no_terrain_is_none_and_ground_y_zero() {
    let v = |x, y, z| Some(pb::Vec3 { x, y, z });
    let w = World::from_pb(pb::World {
        emitters: vec![pb::Emitter { name: "e".into(), position_ned_m: v(1.0, 4.0, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
        ..Default::default()
    })
    .unwrap();
    assert!(w.terrain.is_none());
    assert_eq!(w.emitters[0].ground_y, 0.0);
}

#[test]
fn an_emitter_stands_on_the_relief() {
    let t = sloped();
    let v = |x, y, z| Some(pb::Vec3 { x, y, z });
    let w = World::from_pb(pb::World {
        terrain: Some(terrain_pb(&t)),
        emitters: vec![pb::Emitter { name: "e".into(), position_ned_m: v(0.7, 5.1, -1.0), freq_mhz: 5695.0, power_mw: 25.0 }],
        ..Default::default()
    })
    .unwrap();
    let want = t.height_up(0.7, 5.1);
    assert!(want > 0.5, "the emitter is on a slope: {want}");
    assert_eq!(w.emitters[0].ground_y, want);
    assert_eq!(w.emitters[0].position.y, 1.0, "the position stays where the file put it");
}

#[test]
fn from_pb_puts_the_mesh_in_godots_frame() {
    let mesh = pb::Mesh { name: "hall".into(), path: "/a/hall.glb".into(), position_ned_m: Some(pb::Vec3 { x: 30.0, y: -4.0, z: -1.5 }), heading_deg: 90.0, digest: 0xabcd };
    let w = World::from_pb(pb::World { meshes: vec![mesh], ..Default::default() }).unwrap();
    let m = &w.meshes[0];
    assert_eq!((m.name.as_str(), m.heading_deg, m.digest), ("hall", 90.0, 0xabcd));
    assert_eq!(m.path, std::path::PathBuf::from("/a/hall.glb"));
    assert_eq!(m.position, DVec3::new(-4.0, 1.5, -30.0), "east, up, south");
    assert!(World::from_pb(pb::World::default()).unwrap().meshes.is_empty());
}

#[test]
fn verify_accepts_the_matching_file_and_names_the_mismatch() {
    let dir = std::env::temp_dir().join(format!("ofs-client-mesh-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hall.glb");
    let bytes = b"glTF pretend mesh bytes".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    let mesh = |path: &std::path::Path, digest| WorldMesh { name: "hall".into(), path: path.to_path_buf(), position: DVec3::ZERO, heading_deg: 0.0, digest };
    let digest = ofs_core::rng::fnv1a64(&bytes);
    assert_eq!(mesh(&path, digest).verify(), Ok(()));

    let mut edited = bytes.clone();
    edited[3] ^= 1;
    std::fs::write(&path, &edited).unwrap();
    let e = mesh(&path, digest).verify().unwrap_err();
    let got = ofs_core::rng::fnv1a64(&edited);
    assert!(e.contains("hall.glb") && e.contains(&format!("{got:016x}")) && e.contains(&format!("{digest:016x}")), "{e}");

    let gone = dir.join("gone.glb");
    let e = mesh(&gone, digest).verify().unwrap_err();
    assert!(e.contains("gone.glb") && e.contains("cannot read"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}
