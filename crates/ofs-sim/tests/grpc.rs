use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{Empty, HandshakeRequest, LoadRequest, RunRequest, Sticks};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;
use tonic::{Code, Status};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start() -> SimClient<Channel> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let data_dir = std::env::temp_dir().join("ofs-grpc-test-data");
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(SimServer::new(SimService::new(data_dir)))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    SimClient::connect(format!("http://{addr}")).await.unwrap()
}

fn kind(s: &Status) -> String {
    s.metadata().get("ofs-error-kind").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

async fn load_open_loop(c: &mut SimClient<Channel>) {
    c.load(LoadRequest { quad_path: QUAD.into(), seed: 1, mode: 0, open_loop_fc: true }).await.unwrap();
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
    assert_eq!(kind(&err), "not_loaded");
}

#[tokio::test]
async fn bad_quad_path_is_a_config_error() {
    let mut c = start().await;
    let err = c.load(LoadRequest { quad_path: "does/not/exist.toml".into(), seed: 0, mode: 0, open_loop_fc: true }).await.unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

#[tokio::test]
async fn open_loop_session_runs_and_reports_state() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    assert!((s.time_s - 1.0).abs() < 1e-9);
    assert!((s.position_ned_m.unwrap().z + 0.0295).abs() < 1e-3);
    assert_eq!(s.motor_rpm.len(), 4);
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
