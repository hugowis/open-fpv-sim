//! The rolling in-memory recorder: the last window of the default table, written out on demand.
use std::collections::BTreeMap;

use glam::DVec3;
use ofs_core::{Bus, Model, Scheduler, Signal, SimError, StepCtx};
use ofs_log::tail::{TailHandle, TailRecorder};
use ofs_log::{Recorder, RecorderHandle};

/// Writes `t` (simulated seconds after the step) into `x` and `t·(1, 2, 3)` into `v` every tick.
struct Clock {
    x: Signal<f64>,
    v: Signal<DVec3>,
}

impl Model for Clock {
    fn name(&self) -> &str {
        "clock"
    }
    fn rate_divisor(&self) -> u32 {
        1
    }
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let t = ctx.time_s + ctx.dt_s;
        bus.set(self.x, t);
        bus.set(self.v, DVec3::new(t, 2.0 * t, 3.0 * t));
        Ok(())
    }
}

/// An 8 kHz scheduler with the clock, a tail recorder of `window_s` and, last, a plain recorder.
fn rig(window_s: f64) -> (Scheduler, TailHandle, RecorderHandle) {
    let mut bus = Bus::new();
    let clock = Clock { x: bus.signal("body.x"), v: bus.signal("body.v") };
    let tail = TailHandle::default();
    let tail_model = TailRecorder::new(tail.clone(), &bus, 8000, window_s).unwrap();
    let plain = RecorderHandle::default();
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(clock));
    s.add(Box::new(tail_model));
    s.add(Box::new(Recorder::new(plain.clone(), 8000)));
    (s, tail, plain)
}

/// topic -> [(log_time_ns, data)] of an MCAP file's bytes.
fn messages(bytes: &[u8]) -> BTreeMap<String, Vec<(u64, Vec<u8>)>> {
    let mut out: BTreeMap<String, Vec<(u64, Vec<u8>)>> = BTreeMap::new();
    for m in mcap::MessageStream::new(bytes).unwrap() {
        let m = m.unwrap();
        out.entry(m.channel.topic.clone()).or_default().push((m.log_time, m.data.to_vec()));
    }
    out
}

#[test]
fn the_snapshot_holds_only_the_last_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tail, _) = rig(2.0);
    s.run_for(7.0).unwrap();
    let path = dir.path().join("tail.mcap");
    let summary = tail.snapshot(&path).unwrap();
    assert!(summary.first_s >= 5.0 - 1e-9 && summary.first_s < 5.1, "{summary:?}");
    assert!(summary.last_s <= 7.0 && 7.0 - summary.last_s < 0.005, "the last record of a 242 Hz channel: {summary:?}");
    let bytes = std::fs::read(&path).unwrap();
    assert!(mcap::Summary::read(&bytes).unwrap().is_some(), "a finished, indexed file");
    assert_eq!(messages(&bytes).len(), 2, "both signals");
}

#[test]
fn fewer_seconds_than_the_window_keeps_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tail, _) = rig(2.0);
    s.run_for(0.5).unwrap();
    let summary = tail.snapshot(&dir.path().join("tail.mcap")).unwrap();
    assert!(summary.first_s < 0.05, "the first record: {summary:?}");
    assert!(summary.last_s <= 0.5 && 0.5 - summary.last_s < 0.005, "{summary:?}");
}

#[test]
fn the_snapshot_matches_the_plain_recorder_over_the_same_span() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tail, plain) = rig(1.0);
    plain.start(s.bus(), 8000, &[], &dir.path().join("plain.mcap")).unwrap();
    s.run_for(3.0).unwrap();
    plain.stop().unwrap();
    let summary = tail.snapshot(&dir.path().join("tail.mcap")).unwrap();
    let tail_msgs = messages(&std::fs::read(dir.path().join("tail.mcap")).unwrap());
    let plain_msgs = messages(&std::fs::read(dir.path().join("plain.mcap")).unwrap());
    let first_ns = (summary.first_s * 1e9).round() as u64;
    assert!(first_ns > 1_900_000_000, "the window moved on: {summary:?}");
    for (topic, msgs) in &tail_msgs {
        let expected: Vec<_> = plain_msgs[topic].iter().filter(|(t, _)| *t >= first_ns).cloned().collect();
        assert!(!msgs.is_empty());
        assert_eq!(msgs, &expected, "{topic}");
    }
}

#[test]
fn two_identical_runs_give_identical_snapshot_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = Vec::new();
    for n in 0..2 {
        let (mut s, tail, _) = rig(1.0);
        s.run_for(2.5).unwrap();
        let path = dir.path().join(format!("{n}.mcap"));
        tail.snapshot(&path).unwrap();
        bytes.push(std::fs::read(path).unwrap());
    }
    assert_eq!(bytes[0], bytes[1]);
}

#[test]
fn a_snapshot_refuses_an_existing_file_and_an_unarmed_handle() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tail, _) = rig(1.0);
    s.run_for(0.1).unwrap();
    let path = dir.path().join("tail.mcap");
    tail.snapshot(&path).unwrap();
    assert!(tail.snapshot(&path).is_err(), "never truncates an earlier file");
    assert!(TailHandle::default().snapshot(&dir.path().join("none.mcap")).is_err(), "no recorder behind it");
}

#[test]
fn a_window_that_is_not_positive_is_refused() {
    let bus = Bus::new();
    for w in [0.0, -1.0, f64::NAN] {
        assert!(TailRecorder::new(TailHandle::default(), &bus, 8000, w).is_err(), "{w}");
    }
}

#[test]
fn steady_state_recording_does_not_grow_the_ring() {
    let (mut s, tail, _) = rig(1.0);
    s.run_for(1.5).unwrap(); // a full window: the first evictions have happened
    let warm = tail.ring_capacity();
    assert!(warm > 0);
    s.run_for(3.0).unwrap();
    assert_eq!(tail.ring_capacity(), warm, "after warm-up a record replaces an evicted one: no allocation");
}
