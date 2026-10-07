//! Starting a server process, and what the client reports when that goes wrong.
mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::{Probe, QUAD, SHORT};
use ofs_client::launch::ServerProcess;
use ofs_client::{ErrorKind, LaunchSpec, Phase, Settings};

fn spec(program: PathBuf, dir: &std::path::Path) -> LaunchSpec {
    LaunchSpec { program, data_dir: dir.join("data"), env: vec![], log_file: Some(dir.join("ofs-sim.log")) }
}

fn free_port_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

#[test]
fn a_program_that_does_not_exist_is_a_launch_error_with_a_hint() {
    let dir = tempfile_dir("missing");
    let error = ServerProcess::spawn(&spec("definitely-not-ofs-sim".into(), &dir), "127.0.0.1:50999").err().expect("cannot start");
    assert_eq!(error.kind, ErrorKind::Launch);
    assert!(error.message.contains("definitely-not-ofs-sim") && error.message.contains("cargo build -p ofs-sim"), "{}", error.message);
}

#[test]
fn a_server_that_exits_during_startup_is_reported_with_its_status_and_log() {
    // This test binary stands in for a server that refuses its arguments and exits at once.
    let dir = tempfile_dir("exits");
    let settings = Settings {
        server_addr: free_port_addr(),
        launch: Some(spec(std::env::current_exe().unwrap(), &dir)),
        launch_timeout: Duration::from_secs(10),
        ..Settings::new(QUAD)
    };
    let mut probe = Probe::start(settings);
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Launch));
    assert!(detail.contains("exited during startup"), "{detail}");
    assert!(detail.contains("--listen") || detail.contains("Unrecognized option"), "the server's own message is included: {detail}");
}

fn tempfile_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-client-launch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
