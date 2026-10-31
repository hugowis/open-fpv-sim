//! `ofs-client` starting the real `ofs-sim` binary, flying an open-loop session and stopping it again.
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use ofs_client::{Client, LaunchSpec, Phase, Settings, Sticks, Update};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn free_port_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

fn wait(client: &Client, what: &str, timeout: Duration, condition: impl Fn(&Client) -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        client.poll();
        if condition(client) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; phase {:?}", client.phase());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_client_launches_the_server_flies_and_stops_it() {
    let dir = std::env::temp_dir().join(format!("ofs-client-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let addr = free_port_addr();
    let settings = Settings {
        server_addr: addr.clone(),
        launch: Some(LaunchSpec {
            program: env!("CARGO_BIN_EXE_ofs-sim").into(),
            data_dir: dir.join("data"),
            env: vec![],
            log_file: Some(dir.join("ofs-sim.log")),
        }),
        open_loop_fc: true,
        ..Settings::new(QUAD)
    };
    let client = Client::start(settings).unwrap();
    wait(&client, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    client.set_sticks(Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() });
    wait(&client, "a climb", Duration::from_secs(15), |c| c.telemetry().is_some_and(|t| t.altitude_m > 0.3));
    assert!(TcpStream::connect(&addr).is_ok(), "the server answers while the client flies");

    drop(client); // unloads the session, then stops the server it started
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(&addr).is_ok() {
        assert!(Instant::now() < deadline, "the launched server was still running 10 s after the client went away");
        std::thread::sleep(Duration::from_millis(50));
    }
    let log = std::fs::read_to_string(dir.join("ofs-sim.log")).unwrap();
    assert!(log.contains("listening on"), "the server's stderr went to the log: {log:?}");
}

#[test]
fn an_already_running_server_is_used_and_left_running() {
    let dir = std::env::temp_dir().join(format!("ofs-client-e2e-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let addr = free_port_addr();
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_ofs-sim"))
        .args(["--listen", &addr, "--data-dir"])
        .arg(dir.join("data"))
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(&addr).is_err() {
        assert!(Instant::now() < deadline, "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A launch spec that would fail if it were used: the client must find the running server first.
    let settings = Settings {
        server_addr: addr.clone(),
        launch: Some(LaunchSpec { program: "no-such-program".into(), data_dir: dir.join("data"), env: vec![], log_file: None }),
        open_loop_fc: true,
        ..Settings::new(QUAD)
    };
    let client = Client::start(settings).unwrap();
    wait(&client, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    let updates: Vec<Update> = client.poll();
    assert!(updates.iter().all(|u| !matches!(u, Update::Error(_))));
    drop(client);
    assert!(TcpStream::connect(&addr).is_ok(), "a server the client did not start keeps running");
    let _ = server.kill();
    let _ = server.wait();
}

/// Settings that have the launcher pick the port itself (`--listen 127.0.0.1:0`): the server reports the port
/// it bound in `<data-dir>/server.port`, which is how the test reads it back.
fn port_zero_settings(dir: &Path) -> Settings {
    Settings {
        server_addr: "127.0.0.1:0".into(),
        launch: Some(LaunchSpec {
            program: env!("CARGO_BIN_EXE_ofs-sim").into(),
            data_dir: dir.join("data"),
            env: vec![],
            log_file: Some(dir.join("ofs-sim.log")),
        }),
        open_loop_fc: true,
        ..Settings::new(QUAD)
    }
}

/// The port the launched server bound, from the port file a port-0 start writes.
fn bound_port(dir: &Path) -> u16 {
    std::fs::read_to_string(dir.join("data").join("server.port")).unwrap().trim().parse().unwrap()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-client-e2e-auto-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn port_0_picks_free_ports_so_clients_fly_beside_a_held_50051() {
    let first_dir = scratch("1");
    let client = Client::start(port_zero_settings(&first_dir)).unwrap();
    wait(&client, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    let first_port = bound_port(&first_dir);
    assert!(TcpStream::connect(("127.0.0.1", first_port)).is_ok(), "the server answers on the port it reported ({first_port})");
    client.set_sticks(Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() });
    wait(&client, "a climb", Duration::from_secs(15), |c| c.telemetry().is_some_and(|t| t.altitude_m > 0.3));

    // A second client while the first lives: another server, another OS-picked port.
    let second_dir = scratch("2");
    let second = Client::start(port_zero_settings(&second_dir)).unwrap();
    wait(&second, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    let second_port = bound_port(&second_dir);
    assert_ne!(second_port, first_port, "two servers alive at once fly on two ports");
    assert!(TcpStream::connect(("127.0.0.1", second_port)).is_ok(), "the second server answers on its own port");

    // The fixed port being held no longer breaks launch: port 0 was asked. (Something else may already hold it,
    // which serves the test just as well.)
    let _held = std::net::TcpListener::bind("127.0.0.1:50051");
    let third_dir = scratch("3");
    let third = Client::start(port_zero_settings(&third_dir)).unwrap();
    wait(&third, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    assert_ne!(bound_port(&third_dir), 0, "the third client also flew on an OS-picked port");
}
