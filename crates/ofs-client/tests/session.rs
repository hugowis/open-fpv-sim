//! The client against a real (in-process) server: open-loop sessions, so no Betaflight is needed.
mod common;

use std::time::Duration;

use common::*;
use ofs_client::{Command, ErrorKind, EventKind, Phase, Settings, Sticks, Update, VtxInfo};

fn flying(server: &TestServer) -> Probe {
    let mut probe = Probe::start(server.settings());
    probe.wait_phase(Phase::Flying, LONG);
    probe
}

fn climb() -> Sticks {
    Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() }
}

#[test]
fn a_client_connects_loads_and_flies() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    assert!(probe.log.iter().any(|u| matches!(u, Update::Session { quad_name, configurator_address }
        if quad_name.contains("OpenDrone") && configurator_address.is_empty())), "{:#?}", probe.log);
    assert_eq!(&probe.phases()[..3], &[Phase::Connecting, Phase::Loading, Phase::Flying]);

    probe.client.set_sticks(climb());
    probe.wait("the drone climbs", LONG, |p| p.client.telemetry().is_some_and(|t| t.altitude_m > 0.5));
    let t = probe.client.telemetry().unwrap();
    assert!(t.tx_enabled && t.link_up && t.running && t.motors_spinning, "{t:?}");
    assert!(t.age_s < 1.0);
    let pose = probe.client.pose().expect("poses follow the states");
    assert!(pose.pos.y > 0.3, "up is +Y in Godot's frame: {pose:?}");
    assert!((pose.pos.y - t.altitude_m).abs() < 0.5, "pose {pose:?} vs altitude {}", t.altitude_m);
}

#[test]
fn a_missing_quad_fails_with_a_config_error() {
    let server = TestServer::start();
    let mut probe = Probe::start(Settings { quad_path: "does/not/exist.toml".into(), ..server.settings() });
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Config));
    assert!(detail.contains("does/not/exist.toml"), "{detail}");
}

#[test]
fn nothing_listening_fails_as_unavailable_with_a_hint() {
    let mut probe = Probe::start(Settings { server_addr: "127.0.0.1:1".into(), ..Settings::new(QUAD) });
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Unavailable));
    assert!(detail.contains("127.0.0.1:1") && detail.contains("cargo run -p ofs-sim"), "{detail}");
}

#[test]
fn pause_and_resume_stop_and_restart_simulated_time() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("states arrive", SHORT, |p| p.client.telemetry().is_some_and(|t| t.running));
    probe.client.send(Command::Pause);
    probe.wait_phase(Phase::Paused, SHORT);
    probe.wait("the session stops running", SHORT, |p| p.client.telemetry().is_some_and(|t| !t.running));
    let paused_at = probe.client.telemetry().unwrap().time_s;
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(probe.client.telemetry().unwrap().time_s, paused_at, "simulated time stands still");
    probe.client.send(Command::Resume);
    probe.wait_phase(Phase::Flying, SHORT);
    probe.wait("time advances again", SHORT, |p| p.client.telemetry().is_some_and(|t| t.running && t.time_s > paused_at + 0.1));
}

#[test]
fn cutting_the_radio_raises_link_events_and_restoring_it_clears_them() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the link is up", SHORT, |p| p.client.telemetry().is_some_and(|t| t.link_up));
    probe.client.send(Command::SetRadioLoss(true));
    probe.wait("link_down", LONG, |p| p.event_kinds().contains(&EventKind::LinkDown));
    probe.wait("the receiver reports the link down", SHORT, |p| p.client.telemetry().is_some_and(|t| !t.link_up));
    probe.client.send(Command::SetRadioLoss(false));
    probe.wait("link_up again", LONG, |p| {
        let kinds = p.event_kinds();
        kinds.iter().rposition(|k| *k == EventKind::LinkUp) > kinds.iter().rposition(|k| *k == EventKind::LinkDown)
    });
}

#[test]
fn dropping_the_client_turns_the_transmitter_off() {
    let server = TestServer::start();
    let mut raw = server.raw();
    let _keeps_the_session_alive = raw.watch(); // a watcher outlives the client, so the session stays loaded
    let mut probe = flying(&server);
    probe.wait("the transmitter is on", SHORT, |p| p.client.telemetry().is_some_and(|t| t.tx_enabled));
    assert!(raw.get_state().unwrap().radio.unwrap().tx_enabled);
    drop(probe);
    let deadline = std::time::Instant::now() + SHORT;
    loop {
        let state = raw.get_state().expect("the watched session survives");
        if !state.radio.unwrap().tx_enabled {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the transmitter stayed on after the client went away");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_second_pilot_is_refused_while_the_client_flies() {
    let server = TestServer::start();
    let _probe = flying(&server);
    let status = server.raw().try_pilot().unwrap_err();
    assert_eq!(ofs_client::ClientError::from_status(&status).kind, ErrorKind::PilotBusy);
}

#[test]
fn reload_restarts_the_session_and_the_pilot_comes_back() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.client.set_sticks(climb());
    probe.wait("some flight time", LONG, |p| p.client.telemetry().is_some_and(|t| t.time_s > 1.0 && t.altitude_m > 0.3));
    let before = probe.client.telemetry().unwrap().time_s;
    let seen = probe.log.len();
    probe.client.send(Command::Reload);
    probe.wait("loading again", SHORT, |p| p.log[seen..].iter().any(|u| matches!(u, Update::Phase { phase: Phase::Loading, .. })));
    probe.wait_phase(Phase::Flying, LONG);
    probe.wait("states of the new session", LONG, |p| p.client.telemetry().is_some_and(|t| t.time_s < before));
    assert!(probe.client.telemetry().unwrap().tx_enabled, "the pilot link was re-established");
}

#[test]
fn a_session_unloaded_underneath_the_client_fails_it_and_a_reload_recovers() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    server.raw().unload();
    probe.wait_phase(Phase::Failed, LONG);
    assert_eq!(probe.client.phase().2, Some(ErrorKind::NotLoaded), "{:?}", probe.client.phase());
    probe.client.send(Command::Reload);
    probe.wait_phase(Phase::Flying, LONG);
}

#[test]
fn a_server_that_goes_away_fails_the_client_as_unavailable() {
    let mut server = TestServer::start();
    let mut probe = flying(&server);
    server.kill();
    probe.wait_phase(Phase::Failed, LONG);
    assert_eq!(probe.client.phase().2, Some(ErrorKind::Unavailable), "{:?}", probe.client.phase());
}

#[test]
fn shutting_down_stops_the_supervisor() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.client.shutdown();
    assert_eq!(probe.client.phase().0, Phase::Stopped);
    probe.client.shutdown(); // idempotent
}

#[test]
fn a_quad_without_firmware_has_an_absent_osd_and_no_vtx() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the OSD stream delivers its first frame", SHORT, |p| p.client.osd().is_some());
    let osd = probe.client.osd().unwrap();
    assert!(!osd.present && osd.cols == 0 && osd.cells.is_empty(), "{osd:?}");
    assert!(probe.client.osd_version() >= 1);
    probe.wait("states arrive", SHORT, |p| p.client.telemetry().is_some());
    assert_eq!(probe.client.telemetry().unwrap().vtx, VtxInfo::default());
}
