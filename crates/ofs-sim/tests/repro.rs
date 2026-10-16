//! A failed session leaves a repro bundle that replays (parent spec §7): through the gRPC service, and through the
//! `ofs-sim --replay` binary.
use std::path::{Path, PathBuf};
use std::process::Command;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::{fault, EventKind, Fault, LoadRequest, Mode, MotorFailure, PropDamage, RunRequest, Sticks};
use ofs_sim::repro::replay;
use ofs_sim::server::{sim_server, SimService};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start() -> (SimClient<Channel>, SimService) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let data_dir = std::env::temp_dir().join(format!("ofs-repro-test-data-{}-{n}", std::process::id()));
    std::fs::remove_dir_all(&data_dir).ok();
    let service = SimService::new(data_dir);
    tokio::spawn(tonic::transport::Server::builder().add_service(sim_server(service.clone())).serve_with_incoming(TcpListenerStream::new(listener)));
    (SimClient::connect(format!("http://{addr}")).await.unwrap(), service)
}

fn fault_at(kind: fault::Kind, at_s: f64) -> Fault {
    Fault { kind: Some(kind), at_s: Some(at_s) }
}

/// Flies open loop with stick changes and a scheduled motor fault, then a scheduled finite fault that overflows the
/// vibration model at 0.5 s: the run stops with a numerical error. Returns the bundle folder named in the error event.
async fn fail_a_session(mode: Mode) -> (PathBuf, String, SimService) {
    let (mut c, service) = start().await;
    let mut events = service.subscribe();
    c.load(LoadRequest { quad_path: QUAD.into(), seed: 9, mode: mode as i32, open_loop_fc: true, ..Default::default() }).await.unwrap();
    c.set_sticks(Sticks { throttle: 0.6, aux: vec![-1.0; 4], ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    c.set_sticks(Sticks { throttle: 0.9, roll: 0.1, aux: vec![-1.0; 4], ..Default::default() }).await.unwrap();
    c.inject_fault(fault_at(fault::Kind::MotorFailure(MotorFailure { motor: 1, scale: 0.5 }), 0.2)).await.unwrap();
    c.inject_fault(fault_at(fault::Kind::PropDamage(PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 1e305 }), 0.5)).await.unwrap();
    let err = c.run(RunRequest { seconds: 1.0 }).await.unwrap_err();
    assert_eq!(err.metadata().get("ofs-error-kind").unwrap().to_str().unwrap(), "numerical", "{err:?}");
    let mut message = None;
    while let Ok(e) = events.try_recv() {
        if e.kind() == EventKind::SimError {
            message = Some(e.message);
        }
    }
    let message = message.expect("a sim_error event");
    let path = message.split("(repro bundle: ").nth(1).and_then(|rest| rest.strip_suffix(')')).unwrap_or_else(|| panic!("no bundle in: {message}"));
    (PathBuf::from(path), message, service)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-repro-replay-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    dir
}

#[tokio::test]
async fn replaying_a_failed_run_reproduces_it() {
    let (bundle, message, service) = fail_a_session(Mode::Lockstep).await;
    assert!(message.starts_with("non-finite value in signal"), "{message}");
    assert!(bundle.starts_with(service.logs_dir()), "{bundle:?}");
    let outcome = replay(&bundle, &scratch("a")).unwrap();
    assert!(outcome.reproduced, "{outcome:?}");
    assert!(outcome.exact);
    assert_eq!(outcome.reached_tick, outcome.expected_tick);
    assert_eq!(outcome.digest, outcome.expected_digest);
    assert_eq!(outcome.failure.as_deref(), Some(outcome.expected_failure.as_str()));
    assert_eq!(outcome.expected_tick, 4000, "the scheduled fault lands at 0.5 s and the next step overflows");
}

#[tokio::test]
async fn two_replays_agree() {
    let (bundle, _, _) = fail_a_session(Mode::Lockstep).await;
    let a = replay(&bundle, &scratch("b1")).unwrap();
    let b = replay(&bundle, &scratch("b2")).unwrap();
    assert_eq!(a, b);
}

#[tokio::test]
async fn a_realtime_session_replays_inexactly() {
    let (bundle, _, _) = fail_a_session(Mode::Realtime).await;
    let outcome = replay(&bundle, &scratch("c")).unwrap();
    assert!(!outcome.exact, "SITL would run on its own clock: the bundle does not claim exactness");
    assert!(outcome.reproduced, "an open-loop run is deterministic whatever the mode: {outcome:?}");
}

/// Replaces the throttle of the first journal line that sets sticks.
fn change_the_journal(bundle: &Path) {
    let path = bundle.join("journal.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut changed = false;
    let edited: Vec<String> = text
        .lines()
        .map(|l| {
            if !changed && l.contains(" sticks ") && l.starts_with("0 ") {
                changed = true;
                l.replace(" 0.6 ", " 0.7 ")
            } else {
                l.to_string()
            }
        })
        .collect();
    assert!(changed, "no sticks entry at tick 0 in:\n{text}");
    std::fs::write(path, edited.join("\n") + "\n").unwrap();
}

#[tokio::test]
async fn a_changed_journal_is_not_reproduced() {
    let (bundle, _, _) = fail_a_session(Mode::Lockstep).await;
    change_the_journal(&bundle);
    let outcome = replay(&bundle, &scratch("d")).unwrap();
    assert!(!outcome.reproduced, "{outcome:?}");
    assert_ne!(outcome.digest, outcome.expected_digest, "both sides are reported");
}

#[tokio::test]
async fn the_binary_replays_a_bundle_and_says_so() {
    let (bundle, _, _) = fail_a_session(Mode::Lockstep).await;
    let run = |bundle: &Path| Command::new(env!("CARGO_BIN_EXE_ofs-sim")).arg("--replay").arg(bundle).output().unwrap();
    let out = run(&bundle);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    for line in ["reproduced=true", "exact=true"] {
        assert!(text.lines().any(|l| l == line), "{line} missing from:\n{text}");
    }
    assert!(text.lines().any(|l| l.starts_with("failure=non-finite value in signal")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("digest=") && l.len() == "digest=".len() + 16), "{text}");

    change_the_journal(&bundle);
    let out = run(&bundle);
    assert_eq!(out.status.code(), Some(1), "not reproduced");
    assert!(String::from_utf8_lossy(&out.stdout).lines().any(|l| l == "reproduced=false"));

    let out = run(&bundle.join("nowhere"));
    assert_eq!(out.status.code(), Some(2), "an unusable bundle");
    assert!(String::from_utf8_lossy(&out.stderr).contains("bundle.toml"), "{}", String::from_utf8_lossy(&out.stderr));
}
