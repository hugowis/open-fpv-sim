use std::io::{Cursor, Seek, SeekFrom, Write};

use glam::DVec3;
use ofs_core::{Bus, Model, Scheduler, Signal, SimError, StepCtx};
use ofs_log::recorder::{next_log_path, next_numbered_path};
use ofs_log::{RecordError, Recorder, RecorderHandle, SignalRate};

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

/// An 8 kHz scheduler with the clock and, last, a recorder.
fn rig() -> (Scheduler, RecorderHandle) {
    let mut bus = Bus::new();
    let clock = Clock { x: bus.signal("clock.x"), v: bus.signal("clock.v") };
    let handle = RecorderHandle::default();
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(clock));
    s.add(Box::new(Recorder::new(handle.clone(), 8000)));
    (s, handle)
}

fn req(pattern: &str, rate_hz: Option<f64>) -> SignalRate {
    SignalRate { pattern: pattern.into(), rate_hz }
}

#[test]
fn a_recording_carries_each_channel_at_its_rate_stamped_with_the_state_time() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, h) = rig();
    let path = dir.path().join("r.mcap");
    let names = h.start(s.bus(), 8000, &[req("clock.x", Some(1000.0)), req("clock.v", Some(100.0))], &path).unwrap();
    assert_eq!(names, ["clock.v", "clock.x"]);
    s.run_for(0.1).unwrap();
    let summary = h.stop().unwrap();
    assert_eq!(summary.path, path);
    assert_eq!(summary.messages, 100 + 10, "0.1 s at 1 kHz and at 100 Hz");
    assert!((summary.first_s - 0.001).abs() < 1e-12, "{summary:?}");
    assert!((summary.last_s - 0.1).abs() < 1e-12, "{summary:?}");
    assert!(!h.is_recording());
    // The bytes are an MCAP file: magic at both ends.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"\x89MCAP0\r\n");
    assert_eq!(&bytes[bytes.len() - 8..], b"\x89MCAP0\r\n");
}

#[test]
fn recording_never_changes_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let (mut with, h) = rig();
    h.start(with.bus(), 8000, &[], &dir.path().join("r.mcap")).unwrap();
    with.run_for(0.05).unwrap();
    let (mut without, _) = rig();
    without.run_for(0.05).unwrap();
    assert_eq!(with.bus().digest(), without.bus().digest());
}

#[test]
fn start_and_stop_refuse_the_wrong_state_and_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let (s, h) = rig();
    assert_eq!(h.stop().unwrap_err(), RecordError::NotRecording);
    let path = dir.path().join("r.mcap");
    h.start(s.bus(), 8000, &[], &path).unwrap();
    assert_eq!(h.start(s.bus(), 8000, &[], &dir.path().join("other.mcap")).unwrap_err(), RecordError::AlreadyRecording);
    h.stop().unwrap();
    assert!(matches!(h.start(s.bus(), 8000, &[], &path), Err(RecordError::Taken(m)) if m.contains("r.mcap")), "never overwrites");
    match h.start(s.bus(), 8000, &[req("gyro.*", None)], &dir.path().join("x.mcap")) {
        Err(RecordError::Spec(m)) => assert_eq!(m, "pattern 'gyro.*' matches no signal"),
        other => panic!("{other:?}"),
    }
    assert!(!dir.path().join("x.mcap").exists(), "a refused request creates no file");
}

/// Accepts `limit` bytes, then fails every write.
struct Failing {
    inner: Cursor<Vec<u8>>,
    limit: usize,
}

impl Write for Failing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.inner.get_ref().len() + buf.len() > self.limit {
            return Err(std::io::Error::other("disk full"));
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Failing {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// A file in memory that refuses exactly one write once `after` bytes are in (a transient "disk full"), then works again.
#[derive(Clone)]
struct Flaky {
    inner: std::sync::Arc<std::sync::Mutex<Cursor<Vec<u8>>>>,
    after: usize,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Write for Flaky {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        if inner.get_ref().len() >= self.after && !self.failed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::other("disk full"));
        }
        inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Flaky {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.lock().unwrap().seek(pos)
    }
}

#[test]
fn a_recording_that_stops_on_a_transient_write_error_still_has_a_summary() {
    let (mut s, h) = rig();
    let file = Flaky { inner: Default::default(), after: 1000, failed: Default::default() };
    let bytes = file.inner.clone();
    h.start_with(s.bus(), 8000, &[req("clock.*", Some(8000.0))], Box::new(file), std::path::Path::new("flaky.mcap")).unwrap();
    s.run_for(5.0).unwrap();
    assert!(!h.is_recording(), "the recording stopped itself");
    let failure = h.take_failure().expect("the failure is kept for the session");
    assert!(failure.contains("flaky.mcap") && failure.contains("disk full"), "{failure}");
    let bytes = bytes.lock().unwrap().get_ref().clone();
    let summary = mcap::Summary::read(&bytes).expect("a readable footer");
    assert!(summary.is_some(), "the file was finished: indexed readers find its summary");
    // Not claimed: that every message before the failure reads back. The record being written when the error hit can
    // be left half written inside the last chunk, so a reader can still fail there (python/ofs/reader.py calls such a
    // file "damaged" and names the recording_stopped event).
}

#[test]
fn a_failed_write_stops_the_recording_once_and_the_run_flies_on() {
    let (mut s, h) = rig();
    let writer = Box::new(Failing { inner: Cursor::new(Vec::new()), limit: 4096 });
    h.start_with(s.bus(), 8000, &[req("clock.*", Some(8000.0))], writer, std::path::Path::new("disk.mcap")).unwrap();
    // A chunk reaches the writer when it fills (1 MiB of records): about 1.3 simulated seconds at these rates.
    s.run_for(5.0).unwrap();
    assert!(!h.is_recording(), "the recording stopped itself");
    let failure = h.take_failure().expect("the failure is kept for the session");
    assert!(failure.contains("disk.mcap") && failure.contains("disk full"), "{failure}");
    assert_eq!(h.take_failure(), None, "reported once");
    match h.stop() {
        Err(RecordError::Io(m)) => assert_eq!(m, failure, "a StopRecording after the failure says why"),
        other => panic!("{other:?}"),
    }
    assert_eq!(h.stop().unwrap_err(), RecordError::NotRecording, "then there is nothing to stop");
}

#[test]
fn log_paths_count_up_per_quad() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    assert_eq!(next_log_path(&logs, "quad").unwrap(), logs.join("quad-1.mcap"));
    std::fs::write(logs.join("quad-1.mcap"), b"").unwrap();
    std::fs::write(logs.join("quad-7.mcap"), b"").unwrap();
    std::fs::write(logs.join("other-9.mcap"), b"").unwrap();
    std::fs::write(logs.join("quad-x.mcap"), b"").unwrap();
    assert_eq!(next_log_path(&logs, "quad").unwrap(), logs.join("quad-8.mcap"));
    assert_eq!(next_log_path(&logs, "other").unwrap(), logs.join("other-10.mcap"));
}

#[test]
fn numbered_paths_count_only_their_own_stem_and_extension() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    assert_eq!(next_numbered_path(&logs, "quad-inputs", "csv").unwrap(), logs.join("quad-inputs-1.csv"));
    for name in ["quad-inputs-3.csv", "quad-inputs-7.mcap", "quad-4.csv", "quad-inputs-x.csv"] {
        std::fs::write(logs.join(name), "").unwrap();
    }
    assert_eq!(next_numbered_path(&logs, "quad-inputs", "csv").unwrap(), logs.join("quad-inputs-4.csv"));
    assert_eq!(next_log_path(&logs, "quad-inputs").unwrap(), logs.join("quad-inputs-8.mcap"));
}

#[test]
fn a_number_another_process_took_in_between_gives_the_next_one() {
    use ofs_log::recorder::create_numbered;
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    let mut calls = 0;
    let (path, ()) = create_numbered(&logs, "quad", "mcap", |p| {
        calls += 1;
        if calls == 1 {
            std::fs::write(p, b"another simulator's").unwrap(); // it won the race for quad-1
        }
        std::fs::File::create_new(p).map(drop).map_err(|e| RecordError::from_create(p, &e))
    })
    .unwrap();
    assert_eq!((path, calls), (logs.join("quad-2.mcap"), 2));
    assert_eq!(std::fs::read(logs.join("quad-1.mcap")).unwrap(), b"another simulator's", "the other file is untouched");
}

#[test]
fn a_failure_that_is_not_a_taken_name_is_not_retried() {
    use ofs_log::recorder::create_numbered;
    let dir = tempfile::tempdir().unwrap();
    let mut calls = 0;
    let err = create_numbered(dir.path(), "quad", "mcap", |_| -> Result<(), RecordError> {
        calls += 1;
        Err(RecordError::Io("disk full".into()))
    })
    .unwrap_err();
    assert_eq!((err, calls), (RecordError::Io("disk full".into()), 1));
}
