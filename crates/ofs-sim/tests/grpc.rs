use std::time::Duration;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, RadioLinkLoss, RunRequest, Sticks};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start_with_service() -> (SimClient<Channel>, SimService) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let data_dir = std::env::temp_dir().join("ofs-grpc-test-data");
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
    assert!((0.6..=1.4).contains(&s.time_s), "{} s simulated in 1 s of wall time", s.time_s);
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
    let mut req = Request::new(RunRequest { seconds: 100_000.0 });
    req.set_timeout(Duration::from_millis(300));
    let err = c.run(req).await.unwrap_err();
    // The client-side deadline cancels the call (tonic reports Cancelled or DeadlineExceeded).
    assert!(matches!(err.code(), Code::DeadlineExceeded | Code::Cancelled), "{err:?}");
    let t = tokio::time::timeout(Duration::from_secs(5), time_s(&mut c)).await.expect("the session stayed locked");
    assert!(t < 100_000.0);
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
