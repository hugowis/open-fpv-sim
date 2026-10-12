//! The bus recorder in a real vehicle (spec §4, §8.4): recording never changes a run, two lockstep runs with the
//! same seed and inputs write byte-identical files, and a record carries the state at its time.
use std::path::{Path, PathBuf};

use ofs_config::{load, FcKind, WorldConfig};
use ofs_log::SignalRate;
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle(seed: u64) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

/// The scripted flight of the digest test (throttle steps), recorded with the default table into `path`.
fn recorded_flight(seed: u64, path: &Path) -> Vehicle {
    let mut v = vehicle(seed);
    v.start_recording(&[], path).unwrap();
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        v.set_sticks(&Sticks { throttle: t, ..Sticks::default() });
        v.run_for(secs).unwrap();
    }
    v.stop_recording().unwrap();
    v
}

#[test]
fn two_lockstep_runs_write_byte_identical_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a.mcap"), dir.path().join("b.mcap"), dir.path().join("c.mcap"));
    recorded_flight(1, &a);
    recorded_flight(1, &b);
    recorded_flight(2, &c);
    let (a, b, c) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap(), std::fs::read(c).unwrap());
    assert!(a.len() > 10_000, "{} bytes", a.len());
    assert!(a == b, "same seed and inputs: the files differ");
    assert!(a != c, "another seed's noise must show in the file");
}

#[test]
fn recording_leaves_the_digest_alone() {
    let dir = tempfile::tempdir().unwrap();
    let recorded = recorded_flight(1, &dir.path().join("r.mcap"));
    let mut plain = vehicle(1);
    for (t, secs) in [(0.0, 0.5), (0.6, 1.0), (0.2, 0.5)] {
        plain.set_sticks(&Sticks { throttle: t, ..Sticks::default() });
        plain.run_for(secs).unwrap();
    }
    assert_eq!(recorded.digest(), plain.digest());
}

/// Every record of `topic` in the file as (log time ns, JSON text).
fn records(path: &PathBuf, topic: &str) -> Vec<(u64, String)> {
    let bytes = std::fs::read(path).unwrap();
    mcap::MessageStream::new(&bytes)
        .unwrap()
        .map(|m| m.unwrap())
        .filter(|m| m.channel.topic == topic)
        .map(|m| (m.log_time, String::from_utf8(m.data.into_owned()).unwrap()))
        .collect()
}

#[test]
fn a_record_carries_the_state_reported_at_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.mcap");
    let mut v = vehicle(1);
    let names = v.start_recording(&[SignalRate { pattern: "battery.voltage_v".into(), rate_hz: Some(1000.0) }], &path).unwrap();
    assert_eq!(names, ["battery.voltage_v"]);
    v.set_sticks(&Sticks { throttle: 0.8, ..Sticks::default() });
    v.run_for(0.25).unwrap();
    let state = v.state();
    let summary = v.stop_recording().unwrap();
    assert_eq!(summary.messages, 250);
    assert!((summary.last_s - state.time_s).abs() < 1e-12, "{summary:?} vs {}", state.time_s);
    let all = records(&path, "battery.voltage_v");
    let (t, json) = all.last().unwrap();
    assert_eq!(*t, 250_000_000);
    let v: f64 = json.strip_prefix(r#"{"v":"#).unwrap().strip_suffix('}').unwrap().parse().unwrap();
    assert_eq!(v, state.battery_voltage_v, "the last record is the state GetState reports at 0.25 s");
}

#[test]
fn ending_the_session_mid_recording_still_finishes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.mcap");
    let mut v = vehicle(1);
    v.start_recording(&[], &path).unwrap();
    v.run_for(0.2).unwrap();
    drop(v); // Unload, a new Load or a server shutdown: the vehicle goes, and its recording with it
    let bytes = std::fs::read(&path).unwrap();
    let summary = mcap::Summary::read(&bytes).unwrap().expect("a summary section: the file was finished");
    assert!(summary.stats.unwrap().message_count > 1000);
}
