use std::time::Duration;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{
    fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, PilotInput, RadioLinkLoss, RunRequest, Sticks,
    StreamRequest, VideoSync,
};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
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
    let service = SimService::new(data_dir);
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(SimServer::new(service.clone()))
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
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    assert!((s.time_s - 1.0).abs() < 1e-9);
    assert!((s.position_ned_m.unwrap().z + 0.0295).abs() < 1e-3);
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
    let fault = Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})) };
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
    assert_eq!(kinds, vec![EventKind::LinkUp, EventKind::LinkDown, EventKind::LinkUp]);
    let err = c.inject_fault(Fault { kind: None }).await.unwrap_err();
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
        if !radio.unwrap().tx_enabled && !radio.unwrap().link_up {
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
