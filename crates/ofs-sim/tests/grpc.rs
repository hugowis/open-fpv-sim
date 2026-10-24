use std::time::Duration;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::{
    fault, Empty, EventKind, Fault, HandshakeRequest, InputsFile, LoadRequest, Mode, PilotInput, RadioLinkLoss, RecordingSignal,
    RunRequest, StartRecordingRequest, Sticks, StreamRequest, VideoSync,
};
use ofs_sim::server::{sim_server, SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start_with_service() -> (SimClient<Channel>, SimService) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // One data directory per server, so tests that run in parallel never share firmware state.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let data_dir = std::env::temp_dir().join(format!("ofs-grpc-test-data-{}-{n}", std::process::id()));
    // Fresh: a directory an earlier run with the same pid left behind would number this run's recordings after its own.
    std::fs::remove_dir_all(&data_dir).ok();
    let service = SimService::new(data_dir);
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(sim_server(service.clone()))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    (SimClient::connect(format!("http://{addr}")).await.unwrap(), service)
}

async fn start() -> SimClient<Channel> {
    start_with_service().await.0
}

fn kind(s: &Status) -> String {
    s.metadata().get("ofs-error-kind").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

fn open_loop(mode: Mode) -> LoadRequest {
    LoadRequest { quad_path: QUAD.into(), seed: 1, mode: mode as i32, open_loop_fc: true, ..Default::default() }
}

async fn load_open_loop(c: &mut SimClient<Channel>) {
    c.load(open_loop(Mode::Lockstep)).await.unwrap();
}

async fn time_s(c: &mut SimClient<Channel>) -> f64 {
    c.get_state(Empty {}).await.unwrap().into_inner().time_s
}

#[tokio::test]
async fn handshake_checks_protocol_version() {
    let mut c = start().await;
    let ok = c.handshake(HandshakeRequest { protocol_version: PROTOCOL_VERSION }).await.unwrap().into_inner();
    assert_eq!(ok.protocol_version, PROTOCOL_VERSION);
    let err = c.handshake(HandshakeRequest { protocol_version: 999 }).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(kind(&err), "protocol");
}

#[tokio::test]
async fn run_before_load_is_rejected() {
    let mut c = start().await;
    let err = c.run(RunRequest { seconds: 0.1 }).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(kind(&err), "not_loaded");
}

#[tokio::test]
async fn bad_quad_path_is_a_config_error() {
    let mut c = start().await;
    let req = LoadRequest { quad_path: "does/not/exist.toml".into(), open_loop_fc: true, ..Default::default() };
    let err = c.load(req).await.unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

const WORLD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml");

#[tokio::test]
async fn a_bad_world_path_is_a_config_error_and_loads_nothing() {
    let mut c = start().await;
    let req = LoadRequest { world_path: "no/such/world.toml".into(), ..open_loop(Mode::Lockstep) };
    let err = c.load(req).await.unwrap_err();
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("no/such/world.toml"), "{}", err.message());
    assert_eq!(kind(&c.get_world(Empty {}).await.unwrap_err()), "not_loaded");
}

#[tokio::test]
async fn a_session_flies_in_the_world_it_was_loaded_with() {
    let mut c = start().await;
    c.load(LoadRequest { world_path: WORLD.into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
    let world = c.get_world(Empty {}).await.unwrap().into_inner();
    assert_eq!(world.name, "Flat field");
    assert_eq!(world.objects.len(), 30);
    assert_eq!(world.antennas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["omni", "patch"]);
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    let video = s.video.unwrap();
    assert!(video.present);
    assert_eq!(video.rssi.len(), 2);
    assert_eq!(video.sync(), VideoSync::Locked, "on the launch pad: {video:?}");
    assert!(video.snr_db > 40.0, "{video:?}");
    c.load(open_loop(Mode::Lockstep)).await.unwrap();
    assert_eq!(c.get_world(Empty {}).await.unwrap().into_inner().name, "open field", "a load without a world: the open field");
}

#[tokio::test]
async fn open_loop_session_runs_and_reports_state() {
    let mut c = start().await;
    let reply = c.load(open_loop(Mode::Lockstep)).await.unwrap().into_inner();
    assert_eq!(reply.configurator_address, "", "no Betaflight, no Configurator");
    let kinds: Vec<_> = reply.parts.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds, ["frame", "motor", "prop", "esc", "battery", "vtx"]);
    let motor = &reply.parts[1];
    let kv = motor.provenance.iter().find(|p| p.path == "motor.kv_rpm_per_v").expect("the motor's kv has provenance");
    assert_eq!(kv.source, "estimated");
    assert!(!kv.note.is_empty());
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    assert!((s.time_s - 1.0).abs() < 1e-9);
    assert!((s.position_ned_m.unwrap().z + 0.04).abs() < 2e-3, "rests on the body sphere: {:?}", s.position_ned_m);
    assert_eq!(s.motor_rpm.len(), 4);
    let radio = s.radio.unwrap();
    assert!(radio.tx_enabled && radio.link_up, "{radio:?}");
    assert_eq!(radio.lq_pct, 100.0);
    assert!(!s.running);
    let again = c.get_state(Empty {}).await.unwrap().into_inner();
    assert_eq!(again.time_s, s.time_s);
}

#[tokio::test]
async fn invalid_run_durations_do_not_poison_the_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    for seconds in [-1.0, f64::NAN] {
        let err = c.run(RunRequest { seconds }).await.unwrap_err();
        assert_eq!(kind(&err), "invalid_argument");
    }
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
}

#[tokio::test]
async fn non_finite_sticks_are_rejected() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c.set_sticks(Sticks { roll: f64::NAN, ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    let err = c.set_sticks(Sticks { aux: vec![0.0; 5], ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    c.set_sticks(Sticks { throttle: 0.5, aux: vec![1.0], ..Default::default() }).await.unwrap();
}

#[tokio::test]
async fn realtime_sessions_load_paused_and_follow_the_wall_clock() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(time_s(&mut c).await, 0.0, "loads paused");
    c.start(Empty {}).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let s = c.get_state(Empty {}).await.unwrap().into_inner();
    assert!(s.running);
    assert!((0.5..=1.5).contains(&s.time_s), "{} s simulated in 1 s of wall time", s.time_s);
    c.pause(Empty {}).await.unwrap();
    let paused_at = time_s(&mut c).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(time_s(&mut c).await, paused_at);
}

#[tokio::test]
async fn run_needs_a_paused_or_lockstep_session_and_start_needs_realtime() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c.start(Empty {}).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let err = c.run(RunRequest { seconds: 0.1 }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    c.pause(Empty {}).await.unwrap();
    let before = time_s(&mut c).await;
    let s = c.run(RunRequest { seconds: 0.1 }).await.unwrap().into_inner(); // single-step while paused
    assert!((s.time_s - before - 0.1).abs() < 1e-9);
}

#[tokio::test]
async fn an_abandoned_run_stops_and_releases_the_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let mut req = Request::new(RunRequest { seconds: 86_400.0 });
    req.set_timeout(Duration::from_millis(300));
    let err = c.run(req).await.unwrap_err();
    // The client-side deadline cancels the call (tonic reports Cancelled or DeadlineExceeded).
    assert!(matches!(err.code(), Code::DeadlineExceeded | Code::Cancelled), "{err:?}");
    let t = tokio::time::timeout(Duration::from_secs(5), time_s(&mut c)).await.expect("the session stayed locked");
    assert!(t < 86_400.0);
}

#[tokio::test]
async fn faults_drop_the_radio_link_and_events_report_it() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    let fault = Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})), at_s: None };
    c.inject_fault(fault).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    assert!(!s.radio.unwrap().link_up);
    c.clear_faults(Empty {}).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    assert!(s.radio.unwrap().link_up);
    let mut kinds = Vec::new();
    while let Ok(e) = events.try_recv() {
        kinds.push(e.kind());
    }
    // The fault injection and the clear are events now (M4b): injected before the link drops, cleared as the
    // link comes back.
    assert_eq!(kinds, vec![EventKind::LinkUp, EventKind::FaultInjected, EventKind::LinkDown, EventKind::LinkUp, EventKind::FaultCleared]);
    let err = c.inject_fault(Fault { kind: None, at_s: None }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
}

#[tokio::test]
async fn state_streams_at_the_requested_rate() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let err = c.stream_state(StreamRequest { rate_hz: 1000 }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    let mut stream = c.stream_state(StreamRequest { rate_hz: 50 }).await.unwrap().into_inner();
    let started = std::time::Instant::now();
    let mut times = Vec::new();
    while times.len() < 10 {
        times.push(stream.next().await.unwrap().unwrap().time_s);
    }
    let elapsed = started.elapsed().as_secs_f64();
    assert!((0.15..=0.6).contains(&elapsed), "10 states at 50 Hz took {elapsed} s");
    assert!(times.windows(2).all(|w| w[1] >= w[0]), "{times:?}");
}

#[tokio::test]
async fn a_vanished_pilot_switches_the_transmitter_off() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks { throttle: 0.2, ..Default::default() }), state_rate_hz: 50 }).await.unwrap();
    let mut states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    let s = states.next().await.unwrap().unwrap();
    assert!(s.radio.unwrap().tx_enabled);

    let (tx2, rx2) = tokio::sync::mpsc::channel(1);
    tx2.send(PilotInput::default()).await.unwrap();
    let err = c.pilot(ReceiverStream::new(rx2)).await.unwrap_err();
    assert_eq!(kind(&err), "pilot_busy");

    drop(tx); // the pilot's input stream ends: the client is gone
    drop(states);
    let mut radio = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        radio = c.get_state(Empty {}).await.unwrap().into_inner().radio;
        if !radio.as_ref().unwrap().tx_enabled && !radio.as_ref().unwrap().link_up {
            break;
        }
    }
    let radio = radio.unwrap();
    assert!(!radio.tx_enabled && !radio.link_up, "{radio:?}");
    assert!(time_s(&mut c).await > 0.0, "the session keeps running for the pilot to reconnect");
}

#[tokio::test]
async fn the_last_watcher_leaving_ends_a_session_unless_keep_alive() {
    let mut c = start().await;
    for keep_alive in [false, true] {
        c.load(LoadRequest { keep_alive, ..open_loop(Mode::Lockstep) }).await.unwrap();
        let watch = c.watch(Empty {}).await.unwrap().into_inner();
        drop(watch);
        let mut loaded = true;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            loaded = c.get_state(Empty {}).await.is_ok();
            if !loaded {
                break;
            }
        }
        assert_eq!(loaded, keep_alive, "keep_alive = {keep_alive}");
    }
}

#[tokio::test]
async fn watchers_receive_events() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let mut watch = c.watch(Empty {}).await.unwrap().into_inner();
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    let e = tokio::time::timeout(Duration::from_secs(5), watch.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(e.kind(), EventKind::LinkUp);
}

#[tokio::test]
async fn a_pilot_does_not_outlive_its_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
    let mut old_states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    assert!(old_states.next().await.unwrap().unwrap().radio.unwrap().tx_enabled);

    // The old pilot keeps its input stream open across an Unload and a new Load.
    c.unload(Empty {}).await.unwrap();
    load_open_loop(&mut c).await;

    // The old pilot's slot is released once its state feed has ended, so a new pilot is not refused.
    let mut new_pilot = None;
    for _ in 0..100 {
        let (tx2, rx2) = tokio::sync::mpsc::channel(8);
        tx2.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
        match c.pilot(ReceiverStream::new(rx2)).await {
            Ok(r) => {
                new_pilot = Some((tx2, r.into_inner()));
                break;
            }
            Err(e) => {
                assert_eq!(kind(&e), "pilot_busy", "{e:?}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    let (_tx2, mut new_states) = new_pilot.expect("a new pilot was refused for the whole wait");
    assert!(new_states.next().await.unwrap().unwrap().radio.unwrap().tx_enabled);

    // The old pilot's later input and its end touch neither the new session's sticks nor its transmitter.
    let _ = tx.send(PilotInput { sticks: Some(Sticks { throttle: 0.9, ..Default::default() }), state_rate_hz: 50 }).await;
    drop(tx);
    drop(old_states);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let s = c.run(RunRequest { seconds: 0.1 }).await.unwrap().into_inner();
    assert!(s.radio.unwrap().tx_enabled, "the old pilot switched the new session's transmitter off");
    assert!(s.motor_cmd.iter().all(|m| *m == 0.0), "the old pilot's sticks reached the new session: {:?}", s.motor_cmd);
}

#[tokio::test]
async fn shutdown_stops_a_long_lockstep_run() {
    let (mut c, service) = start_with_service().await;
    load_open_loop(&mut c).await;
    let mut runner = c.clone();
    let run = tokio::spawn(async move { runner.run(RunRequest { seconds: 86_400.0 }).await });
    tokio::time::sleep(Duration::from_millis(300)).await; // the Run is under way
    let stop = tokio::task::spawn_blocking(move || service.shutdown());
    tokio::time::timeout(Duration::from_secs(2), stop).await.expect("shutdown waited for the whole Run").unwrap();
    let err = tokio::time::timeout(Duration::from_secs(2), run).await.expect("the Run did not return").unwrap().unwrap_err();
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = c.get_state(Empty {}).await.unwrap_err();
    assert_eq!(kind(&err), "not_loaded", "the session slot is empty");
}

async fn next_kinds(events: &mut tokio::sync::broadcast::Receiver<ofs_sim::pb::Event>, until: EventKind) -> Vec<EventKind> {
    let mut kinds = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !kinds.contains(&until) {
        let e = tokio::time::timeout_at(deadline, events.recv()).await.expect("event not published").unwrap();
        kinds.push(e.kind());
    }
    kinds
}

#[tokio::test]
async fn pilot_and_session_events_are_published_in_order() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    let watch = c.watch(Empty {}).await.unwrap().into_inner();
    load_open_loop(&mut c).await;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
    let states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    assert_eq!(next_kinds(&mut events, EventKind::PilotConnected).await.last(), Some(&EventKind::PilotConnected));
    drop(tx);
    drop(states);
    let kinds = next_kinds(&mut events, EventKind::PilotDisconnected).await;
    assert!(!kinds.contains(&EventKind::PilotConnected), "{kinds:?}");
    drop(watch); // the last watcher leaves a watched session without keep_alive
    next_kinds(&mut events, EventKind::SessionEnded).await;
    assert_eq!(kind(&c.get_state(Empty {}).await.unwrap_err()), "not_loaded");
}

#[tokio::test]
async fn a_state_stream_without_a_session_fails_at_once() {
    let mut c = start().await;
    let started = std::time::Instant::now();
    let mut states = c.stream_state(StreamRequest { rate_hz: 1 }).await.unwrap().into_inner();
    let first = tokio::time::timeout(Duration::from_secs(5), states.next()).await.unwrap().unwrap();
    assert_eq!(kind(&first.unwrap_err()), "not_loaded");
    assert!(started.elapsed() < Duration::from_millis(500), "waited {:?} (a whole 1 Hz period)", started.elapsed());
}

#[tokio::test]
async fn a_pilot_that_never_reads_its_states_frees_the_slot_when_its_session_ends() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 240 }).await.unwrap();
    let _unread_states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    tokio::time::sleep(Duration::from_millis(1000)).await; // its state channel and the transport fill up
    c.unload(Empty {}).await.unwrap();
    load_open_loop(&mut c).await;
    let (tx2, rx2) = tokio::sync::mpsc::channel(8);
    tx2.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
    let mut states = c.pilot(ReceiverStream::new(rx2)).await.expect("the idle pilot still holds the slot").into_inner();
    assert!(states.next().await.unwrap().unwrap().radio.unwrap().tx_enabled);
    drop(tx);
}

/// The events published since `events` subscribed, as (kind, message), keeping only `wanted` kinds.
fn drain(events: &mut tokio::sync::broadcast::Receiver<ofs_sim::pb::Event>, wanted: &[EventKind]) -> Vec<(EventKind, String)> {
    let mut out = Vec::new();
    while let Ok(e) = events.try_recv() {
        if wanted.contains(&e.kind()) {
            out.push((e.kind(), e.message));
        }
    }
    out
}

#[tokio::test]
async fn the_handshake_names_the_logs_directory() {
    let (mut c, service) = start_with_service().await;
    let reply = c.handshake(HandshakeRequest { protocol_version: PROTOCOL_VERSION }).await.unwrap().into_inner();
    assert_eq!(std::path::PathBuf::from(&reply.logs_dir), service.logs_dir());
    assert!(std::path::Path::new(&reply.logs_dir).is_absolute(), "{}", reply.logs_dir);
    assert!(reply.logs_dir.ends_with("logs"), "{}", reply.logs_dir);
}

#[tokio::test]
async fn the_server_names_recordings_and_reports_them_as_events() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    let signals = vec![
        RecordingSignal { pattern: "battery.*".into(), rate_hz: 0.0 },
        RecordingSignal { pattern: "imu.gyro_frd_radps".into(), rate_hz: 500.0 },
    ];
    let started = c.start_recording(StartRecordingRequest { signals }).await.unwrap().into_inner();
    assert_eq!(std::path::PathBuf::from(&started.path), service.logs_dir().join("opendrone-5f-freestyle-1.mcap"));
    // `battery.*` is the pack's four signals and, since M6b, two per cell (six cells on the shipped pack).
    let mut expected: Vec<String> = (0..6).flat_map(|k| [format!("battery.cell.{k}.soc"), format!("battery.cell.{k}.voltage_v")]).collect();
    expected.extend(["battery.consumed_mah", "battery.current_a", "battery.soc", "battery.voltage_v", "imu.gyro_frd_radps"].map(String::from));
    assert_eq!(started.signals, expected);
    assert_eq!(kind(&c.start_recording(StartRecordingRequest::default()).await.unwrap_err()), "invalid_state", "one at a time");
    c.run(RunRequest { seconds: 1.0 }).await.unwrap();
    let summary = c.stop_recording(Empty {}).await.unwrap().into_inner();
    assert_eq!(summary.path, started.path);
    assert_eq!(summary.messages, 16 * 10 + 500, "sixteen battery signals at the default 10 Hz, the gyro at 500 Hz");
    assert_eq!((summary.first_s, summary.last_s), (0.002, 1.0));
    assert_eq!(kind(&c.stop_recording(Empty {}).await.unwrap_err()), "invalid_state");
    let next = c.start_recording(StartRecordingRequest::default()).await.unwrap().into_inner();
    assert!(next.path.ends_with("opendrone-5f-freestyle-2.mcap"), "{}", next.path);
    let got = drain(&mut events, &[EventKind::RecordingStarted, EventKind::RecordingStopped]);
    assert_eq!(
        got,
        [
            (EventKind::RecordingStarted, started.path.clone()),
            (EventKind::RecordingStopped, format!("{}: 660 messages", started.path)),
            (EventKind::RecordingStarted, next.path.clone()),
        ]
    );
}

#[tokio::test]
async fn unloading_mid_recording_publishes_the_stop_event_and_finishes_the_file() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    c.run(RunRequest { seconds: 0.05 }).await.unwrap();
    c.set_sticks(Sticks { throttle: 0.5, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.05 }).await.unwrap();
    c.unload(Empty {}).await.unwrap();
    let got = drain(&mut events, &[EventKind::InputsRecordingStopped]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].1.starts_with(&started.path) && got[0].1.contains("rows over"), "{got:?}");
    let text = std::fs::read_to_string(&started.path).unwrap();
    assert!(text.lines().count() >= 3, "the header, the first row and the throttle change: {text}");
}

#[tokio::test]
async fn unloading_mid_mcap_recording_publishes_the_stop_event() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    let started = c.start_recording(StartRecordingRequest::default()).await.unwrap().into_inner();
    c.run(RunRequest { seconds: 0.2 }).await.unwrap();
    c.unload(Empty {}).await.unwrap();
    let got = drain(&mut events, &[EventKind::RecordingStopped]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].1.starts_with(&started.path), "{got:?}");
    assert!(std::path::Path::new(&started.path).is_file(), "the file is finished, not half-written");
}

#[tokio::test]
async fn loading_another_quad_mid_mcap_recording_publishes_the_stop_event() {
    let (mut c, _service) = start_with_service().await;
    let mut events = _service.subscribe();
    load_open_loop(&mut c).await;
    let started = c.start_recording(StartRecordingRequest::default()).await.unwrap().into_inner();
    load_open_loop(&mut c).await;
    let got = drain(&mut events, &[EventKind::RecordingStopped]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].1.starts_with(&started.path), "{got:?}");
}

#[tokio::test]
async fn loading_another_quad_mid_recording_publishes_the_stop_event() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    load_open_loop(&mut c).await;
    let got = drain(&mut events, &[EventKind::InputsRecordingStopped]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].1.starts_with(&started.path), "{got:?}");
}

#[tokio::test]
async fn unloading_without_a_recording_publishes_no_stop_event() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    c.unload(Empty {}).await.unwrap();
    load_open_loop(&mut c).await;
    assert!(drain(&mut events, &[EventKind::InputsRecordingStopped]).is_empty());
}

#[tokio::test]
async fn the_server_records_a_sessions_sticks_names_the_file_and_reports_it() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    assert_eq!(kind(&c.stop_inputs_recording(Empty {}).await.unwrap_err()), "invalid_state", "none running");
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert_eq!(std::path::PathBuf::from(&started.path), service.logs_dir().join("opendrone-5f-freestyle-inputs-1.csv"));
    assert_eq!(kind(&c.start_inputs_recording(Empty {}).await.unwrap_err()), "invalid_state", "one at a time");
    c.set_sticks(Sticks { throttle: 0.5, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    c.set_sticks(Sticks { throttle: 0.8, roll: 0.25, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    let summary = c.stop_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert_eq!((summary.path.as_str(), summary.rows, summary.duration_s), (started.path.as_str(), 2, 0.5));
    let text = std::fs::read_to_string(&started.path).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ["t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4,tx,link_loss", "0,0,0,0,0.5,-1,-1,-1,-1,1,0", "0.5,0.25,0,0,0.8,-1,-1,-1,-1,1,0"]
    );
    let next = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert!(next.path.ends_with("opendrone-5f-freestyle-inputs-2.csv"), "{}", next.path);
    let got = drain(&mut events, &[EventKind::InputsRecordingStarted, EventKind::InputsRecordingStopped]);
    assert_eq!(
        got,
        [
            (EventKind::InputsRecordingStarted, started.path.clone()),
            (EventKind::InputsRecordingStopped, format!("{}: 2 rows over 0.5 s", started.path)),
            (EventKind::InputsRecordingStarted, next.path.clone()),
        ]
    );
}

#[tokio::test]
async fn an_inputs_recording_is_refused_while_a_file_plays_and_without_a_session() {
    let mut c = start().await;
    assert_eq!(kind(&c.start_inputs_recording(Empty {}).await.unwrap_err()), "not_loaded");
    load_open_loop(&mut c).await;
    c.play_inputs(InputsFile { name: "climb.csv".into(), csv: "t_s,throttle\n0,0.3\n30,1\n".into() }).await.unwrap();
    let err = c.start_inputs_recording(Empty {}).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    assert!(err.message().contains("climb.csv is playing"), "{}", err.message());
}

#[tokio::test]
async fn playing_inputs_is_refused_while_an_inputs_recording_runs() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    c.start_inputs_recording(Empty {}).await.unwrap();
    let err = c.play_inputs(InputsFile { name: "climb.csv".into(), csv: "t_s,throttle
0,0.3
30,1
".into() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    assert!(err.message().contains("an inputs recording is running"), "{}", err.message());
    c.stop_inputs_recording(Empty {}).await.unwrap();
    c.play_inputs(InputsFile { name: "climb.csv".into(), csv: "t_s,throttle
0,0.3
30,1
".into() }).await.unwrap();
}

#[tokio::test]
async fn unloading_mid_recording_leaves_a_complete_file() {
    let (mut c, service) = start_with_service().await;
    load_open_loop(&mut c).await;
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    c.set_sticks(Sticks { throttle: 0.3, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    c.unload(Empty {}).await.unwrap();
    let text = std::fs::read_to_string(&started.path).unwrap();
    assert_eq!(text.lines().collect::<Vec<_>>(), ["t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4,tx,link_loss", "0,0,0,0,0.3,-1,-1,-1,-1,1,0"]);
    let _ = service;
}

#[tokio::test]
async fn a_recording_pattern_matching_nothing_is_refused_with_its_near_misses() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let signals = vec![RecordingSignal { pattern: "gyro.*".into(), rate_hz: 0.0 }];
    let err = c.start_recording(StartRecordingRequest { signals }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    assert_eq!(err.message(), "pattern 'gyro.*' matches no signal (similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps)");
}

#[tokio::test]
async fn inputs_play_refuse_set_sticks_and_report_events() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    let file = |name: &str, csv: &str| InputsFile { name: name.into(), csv: csv.into() };
    let bad = c.play_inputs(file("bad.csv", "t_s,throttle\n0,2\n")).await.unwrap_err();
    assert_eq!(kind(&bad), "config");
    assert_eq!(bad.message(), "bad.csv line 2: throttle 2 is outside [0, 1]");
    c.play_inputs(file("climb.csv", "t_s,throttle\n0,0.3\n0.5,1\n")).await.unwrap();
    let refused = c.set_sticks(Sticks::default()).await.unwrap_err();
    assert_eq!(kind(&refused), "invalid_argument");
    assert!(refused.message().contains("climb.csv is playing"), "{}", refused.message());
    assert_eq!(kind(&c.play_inputs(file("other.csv", "t_s,yaw\n0,0\n")).await.unwrap_err()), "invalid_state");
    let s = c.run(RunRequest { seconds: 0.6 }).await.unwrap().into_inner();
    assert_eq!(s.motor_cmd, [1.0; 4]);
    c.set_sticks(Sticks::default()).await.unwrap(); // finished: the sticks are the script's again
    c.play_inputs(file("hold.csv", "t_s,throttle\n0,0.2\n9,0.2\n")).await.unwrap();
    c.stop_inputs(Empty {}).await.unwrap();
    c.stop_inputs(Empty {}).await.unwrap(); // nothing playing: nothing to do
    c.set_sticks(Sticks::default()).await.unwrap();
    let got = drain(&mut events, &[EventKind::InputsStarted, EventKind::InputsEnded]);
    assert_eq!(
        got,
        [
            (EventKind::InputsStarted, "climb.csv: 2 rows over 0.5 s".to_string()),
            (EventKind::InputsEnded, "climb.csv: finished".to_string()),
            (EventKind::InputsStarted, "hold.csv: 2 rows over 9 s".to_string()),
            (EventKind::InputsEnded, "hold.csv: stopped".to_string()),
        ]
    );
}

#[tokio::test]
async fn an_inputs_file_larger_than_grpcs_default_limit_plays() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let mut csv = String::from("t_s,throttle,roll,pitch,yaw\n");
    for i in 0..200_000 {
        csv.push_str(&format!("{:.3},0.5,0.1,-0.1,0.0\n", f64::from(i) * 0.001));
    }
    assert!(csv.len() > 4 * 1024 * 1024, "{} bytes", csv.len());
    c.play_inputs(InputsFile { name: "long.csv".into(), csv }).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.1 }).await.unwrap().into_inner();
    assert_eq!(s.motor_cmd, [0.5; 4]);
}

#[tokio::test]
async fn scripted_inputs_and_a_pilot_exclude_each_other() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.play_inputs(InputsFile { name: "hold.csv".into(), csv: "t_s,throttle\n0,0.2\n9,0.2\n".into() }).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
    assert_eq!(kind(&c.pilot(ReceiverStream::new(rx)).await.unwrap_err()), "invalid_state");
    c.stop_inputs(Empty {}).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks::default()), state_rate_hz: 50 }).await.unwrap();
    let _states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    let err = c.play_inputs(InputsFile { name: "hold.csv".into(), csv: "t_s,throttle\n0,0.2\n".into() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    drop(tx);
}

#[tokio::test]
async fn a_fault_can_wait_for_its_time_but_not_for_a_passed_one() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let at = |at_s: f64| Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})), at_s: Some(at_s) };
    c.inject_fault(at(0.5)).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.4 }).await.unwrap().into_inner();
    assert!(s.radio.unwrap().link_up, "not yet");
    let s = c.run(RunRequest { seconds: 0.6 }).await.unwrap().into_inner();
    assert!(!s.radio.unwrap().link_up, "lost since 0.5 s");
    let err = c.inject_fault(at(0.2)).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    assert_eq!(err.message(), "fault time 0.2 s has passed (the session is at 1 s)");
}

#[tokio::test]
async fn a_stabilize_load_without_open_loop_fc_is_refused_and_keeps_the_old_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    let err = c.load(LoadRequest { open_loop_fc: false, stabilize: true, ..open_loop(Mode::Lockstep) }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    assert!(err.message().contains("stabilize needs open_loop_fc"), "{}", err.message());
    assert!((time_s(&mut c).await - 0.1).abs() < 1e-9, "the previous session is still loaded");
}

/// The roll angle in degrees of a state's attitude (FRD to NED, aerospace Z-Y-X).
fn roll_deg(q: &ofs_sim::pb::Quat) -> f64 {
    (2.0 * (q.w * q.x + q.y * q.z)).atan2(1.0 - 2.0 * (q.x * q.x + q.y * q.y)).to_degrees()
}

#[tokio::test]
async fn a_stabilized_session_obeys_a_roll_stick_and_a_plain_open_loop_one_ignores_it() {
    let mut roll_after = Vec::new();
    for stabilize in [true, false] {
        let mut c = start().await;
        c.load(LoadRequest { stabilize, ..open_loop(Mode::Lockstep) }).await.unwrap();
        c.set_sticks(Sticks { roll: 0.2, throttle: 0.4, aux: vec![-1.0; 4], ..Default::default() }).await.unwrap();
        let state = c.run(RunRequest { seconds: 1.5 }).await.unwrap().into_inner();
        roll_after.push(roll_deg(&state.attitude.unwrap()));
    }
    assert!((roll_after[0] - 12.0).abs() < 3.0, "a 0.2 stick leans a stabilized quad to about 12 degrees: {}", roll_after[0]);
    assert!(roll_after[1].abs() < 1.0, "a throttle-only quad stays level: {}", roll_after[1]);
}

#[tokio::test]
async fn a_world_can_be_named() {
    let mut c = start().await;
    c.load(LoadRequest { world_path: "flat".into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
    assert_eq!(c.get_world(Empty {}).await.unwrap().into_inner().name, "Flat field");
    let world = c.get_world(Empty {}).await.unwrap().into_inner();
    let gates: Vec<&str> = world.gates.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(gates, ["Gate0", "Gate1", "Gate2"]);
    let race = c.get_state(Empty {}).await.unwrap().into_inner().race.expect("a race message");
    assert_eq!((race.present, race.gate_count, race.running), (true, 3, false));
    let err = c.load(LoadRequest { world_path: "nowhere".into(), ..open_loop(Mode::Lockstep) }).await.unwrap_err();
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("nowhere.toml"), "{}", err.message());
}

#[tokio::test]
async fn a_race_can_only_be_reset_in_a_world_with_gates() {
    let mut c = start().await;
    let err = c.reset_race(Empty {}).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition, "nothing loaded: {err}");
    c.load(open_loop(Mode::Lockstep)).await.unwrap();
    let err = c.reset_race(Empty {}).await.unwrap_err();
    assert_eq!((err.code(), kind(&err).as_str()), (Code::FailedPrecondition, "invalid_state"), "the open field has no gates: {err}");
    assert!(err.message().contains("no gates"), "{}", err.message());
    c.load(LoadRequest { world_path: "flat".into(), ..open_loop(Mode::Lockstep) }).await.unwrap();
    c.reset_race(Empty {}).await.unwrap();
    let race = c.run(RunRequest { seconds: 0.01 }).await.unwrap().into_inner().race.unwrap();
    assert_eq!((race.running, race.finished, race.lap_limit, race.next_gate), (false, false, 0, 0));
}

#[tokio::test]
async fn a_downgrade_override_on_a_stabilized_quad_is_refused() {
    // The library resolves beside the quads directory, so the quad goes in `quads/` next to a copy of the repo's.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("quads")).unwrap();
    let quad = dir.path().join("quads").join("stabilized.toml");
    std::fs::write(&quad, std::fs::read_to_string(QUAD).unwrap().replace("kind = \"sitl\"", "kind = \"open_loop_stabilized\"")).unwrap();
    copy_library(dir.path());
    let mut c = start().await;
    let err = c
        .load(LoadRequest { quad_path: quad.display().to_string(), open_loop_fc: true, stabilize: false, ..open_loop(Mode::Lockstep) })
        .await
        .unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    assert!(err.message().contains("open_loop_stabilized"), "{}", err.message());
    // Asking for what the file already is stays fine.
    c.load(LoadRequest { quad_path: quad.display().to_string(), open_loop_fc: true, stabilize: true, ..open_loop(Mode::Lockstep) })
        .await
        .unwrap();
}

/// Copies the repo's parts library into `<dir>/library`, so a quad written under `<dir>/quads/` resolves its parts.
fn copy_library(dir: &std::path::Path) {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"));
    fn walk(root: &std::path::Path, rel: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            let sub = rel.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &sub, out);
            } else {
                out.push(sub);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root, std::path::Path::new(""), &mut files);
    for f in files {
        let to = dir.join("library").join(&f);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(root.join(&f), to).unwrap();
    }
}
