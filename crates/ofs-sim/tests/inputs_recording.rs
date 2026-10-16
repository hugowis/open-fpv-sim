//! Recording a live flight's sticks to a `play_inputs` CSV (spec §6): rows only on change, one row per tick, only
//! playable values, a file that is complete however the session ends, and a lockstep replay that reproduces the
//! flight.
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_log::RecordError;
use ofs_sim::inputs;
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const HEADER: &str = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4";

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap().lines().map(String::from).collect()
}

fn sticks(throttle: f64, roll: f64) -> Sticks {
    Sticks { throttle, roll, ..Sticks::default() }
}

#[test]
fn rows_are_written_only_when_a_stick_changes_and_the_first_row_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.set_sticks(&sticks(0.5, 0.0)); // the same tick as the start: it replaces the initial row
    v.run_for(0.02).unwrap();
    v.set_sticks(&sticks(0.5, 0.0)); // unchanged: no row
    v.run_for(0.02).unwrap();
    v.set_sticks(&sticks(0.5, 0.25));
    v.run_for(0.01).unwrap();
    let summary = v.stop_inputs_recording().unwrap();
    assert_eq!(
        lines(&path),
        [HEADER, "0,0,0,0,0.5,-1,-1,-1,-1", "0.04,0.25,0,0,0.5,-1,-1,-1,-1"],
        "the initial sticks, then the change at tick 320 (0.04 s)"
    );
    assert_eq!((summary.rows, summary.duration_s, summary.path), (2, 0.04, path));
}

#[test]
fn two_changes_in_one_tick_leave_the_later_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.run_for(0.01).unwrap();
    v.start_inputs_recording(&path).unwrap();
    v.set_sticks(&sticks(0.1, 0.0));
    v.set_sticks(&sticks(0.2, 0.0));
    v.set_sticks(&sticks(0.3, 0.0));
    v.stop_inputs_recording().unwrap();
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0.3,-1,-1,-1,-1"], "t_s counts from the start of the recording");
}

#[test]
fn a_non_finite_stick_adds_no_row_and_an_out_of_range_one_is_clamped_so_the_file_stays_playable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.01).unwrap();
    v.set_sticks(&sticks(f64::NAN, 0.0));
    v.set_sticks(&Sticks::default()); // the server never lets a NaN reach the bus (it would abort the run): put the sticks back
    v.run_for(0.01).unwrap();
    v.set_sticks(&Sticks { roll: 3.0, throttle: -2.0, ..Sticks::default() });
    v.run_for(0.01).unwrap();
    v.stop_inputs_recording().unwrap();
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0,-1,-1,-1,-1", "0.02,1,0,0,0,-1,-1,-1,-1"]);
    inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).expect("every value is inside the playable ranges");
}

#[test]
fn a_recording_that_never_changes_is_one_row_and_still_parses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.5).unwrap();
    let summary = v.stop_inputs_recording().unwrap();
    assert_eq!((summary.rows, summary.duration_s), (1, 0.0));
    assert_eq!(inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).unwrap().rows.len(), 1);
}

#[test]
fn a_vehicle_dropped_mid_recording_leaves_a_complete_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    {
        let mut v = vehicle();
        v.start_inputs_recording(&path).unwrap();
        v.run_for(0.01).unwrap();
        v.set_sticks(&sticks(0.7, 0.0)); // the newest row is still pending when the session ends
    }
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0,-1,-1,-1,-1", "0.01,0,0,0,0.7,-1,-1,-1,-1"]);
}

#[test]
fn recordings_start_once_stop_once_and_refuse_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    assert_eq!(v.stop_inputs_recording().unwrap_err(), RecordError::NotRecording);
    v.start_inputs_recording(&path).unwrap();
    assert!(v.is_recording_inputs());
    assert_eq!(v.start_inputs_recording(&dir.path().join("other.csv")).unwrap_err(), RecordError::AlreadyRecording);
    v.stop_inputs_recording().unwrap();
    assert!(!v.is_recording_inputs());
    assert!(matches!(v.start_inputs_recording(&path), Err(RecordError::Io(_))), "it never truncates an earlier file");
    assert!(v.take_inputs_recording_failure().is_none());
}

#[test]
fn floats_round_trip_exactly_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let awkward = 0.1 + 0.2; // 0.30000000000000004
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.01).unwrap();
    v.set_sticks(&sticks(1.0 / 3.0, awkward));
    v.stop_inputs_recording().unwrap();
    let parsed = inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).unwrap();
    let last = parsed.rows.last().unwrap();
    assert_eq!(last.1[0], awkward, "roll");
    assert_eq!(last.1[3], 1.0 / 3.0, "throttle");
}

/// The flight of the replay tests: three stick changes, 2.2 s.
fn flown(record_to: Option<&Path>) -> Vehicle {
    let mut v = vehicle();
    if let Some(path) = record_to {
        v.start_inputs_recording(path).unwrap();
    }
    for (throttle, roll, secs) in [(0.0, 0.0, 0.5), (0.6, 0.0, 1.0), (0.4, 0.5, 0.7)] {
        v.set_sticks(&sticks(throttle, roll));
        v.run_for(secs).unwrap();
    }
    if record_to.is_some() {
        v.stop_inputs_recording().unwrap();
    }
    v
}

fn replayed(csv: &str) -> Vehicle {
    let mut v = vehicle();
    v.play_inputs(inputs::parse("recorded.csv", csv).unwrap()).unwrap();
    v.run_for(2.2).unwrap();
    v
}

#[test]
fn a_lockstep_replay_reproduces_the_recorded_flight_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flight.csv");
    let original = flown(Some(&path));
    let csv = std::fs::read_to_string(&path).unwrap();
    assert_eq!(csv.lines().count(), 1 + 3, "the header and the three changes: {csv}");
    assert_eq!(replayed(&csv).digest(), original.digest(), "the same sticks on the same ticks: the same flight");
    assert_eq!(flown(None).digest(), original.digest(), "recording the sticks never changes the flight");
}

#[test]
fn two_replays_write_byte_identical_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let flight = dir.path().join("flight.csv");
    flown(Some(&flight));
    let csv = std::fs::read_to_string(&flight).unwrap();
    let mcap = |name: &str| {
        let path = dir.path().join(name);
        let mut v = vehicle();
        v.start_recording(&[], &path).unwrap();
        v.play_inputs(inputs::parse("recorded.csv", &csv).unwrap()).unwrap();
        v.run_for(2.2).unwrap();
        v.stop_recording().unwrap();
        std::fs::read(path).unwrap()
    };
    let (a, b) = (mcap("a.mcap"), mcap("b.mcap"));
    assert!(a.len() > 10_000 && a == b, "{} vs {} bytes", a.len(), b.len());
}
