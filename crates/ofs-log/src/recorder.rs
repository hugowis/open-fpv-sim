//! The recorder model and the handle the session starts and stops recordings through (the OsdHandle pattern:
//! shared state the vehicle writes and the model reads).
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use glam::{DQuat, DVec3};
use ofs_core::{Bus, Model, Signal, SignalKind, SimError, StepCtx};

use crate::sink::{quat_json, scalar_json, vec3_json, McapSink};
use crate::spec::Channel;
use crate::spec::{resolve, SignalRate};

/// Anything a recording can be written to: a file in the server, a failing writer in tests.
pub trait WriteSeek: Write + Seek + Send {}
impl<T: Write + Seek + Send> WriteSeek for T {}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordError {
    /// StartRecording while one runs.
    AlreadyRecording,
    /// StopRecording with none running.
    NotRecording,
    /// A pattern or rate the bus cannot satisfy (the message lists every problem).
    Spec(String),
    /// Creating, writing or finishing the file failed.
    Io(String),
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::AlreadyRecording => write!(f, "a recording is already running; stop it first"),
            RecordError::NotRecording => write!(f, "no recording is running"),
            RecordError::Spec(m) => write!(f, "{m}"),
            RecordError::Io(m) => write!(f, "recording I/O failed: {m}"),
        }
    }
}

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSummary {
    pub path: PathBuf,
    pub messages: u64,
    /// Simulated time of the first and last record, s (both 0 when nothing was recorded).
    pub first_s: f64,
    pub last_s: f64,
}

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Scalar(Signal<f64>),
    Vec3(Signal<DVec3>),
    Quat(Signal<DQuat>),
}

impl Source {
    /// Where channel `c`'s value lives on `bus`.
    pub(crate) fn of(bus: &Bus, c: &Channel) -> Source {
        match c.kind {
            SignalKind::Scalar => Source::Scalar(bus.lookup(&c.name).expect("listed by the bus")),
            SignalKind::Vec3 => Source::Vec3(bus.lookup(&c.name).expect("listed by the bus")),
            SignalKind::Quat => Source::Quat(bus.lookup(&c.name).expect("listed by the bus")),
        }
    }

    /// The current value.
    pub(crate) fn read(self, bus: &Bus) -> Value {
        match self {
            Source::Scalar(s) => Value::Scalar(bus.get(s)),
            Source::Vec3(s) => Value::Vec3(bus.get(s)),
            Source::Quat(s) => Value::Quat(bus.get(s)),
        }
    }

    /// The current value as the channel's JSON record, appended to `out`.
    pub(crate) fn encode(self, bus: &Bus, out: &mut Vec<u8>) {
        self.read(bus).encode(out);
    }
}

/// A signal's value, held until it is written.
#[derive(Clone, Copy)]
pub(crate) enum Value {
    Scalar(f64),
    Vec3(DVec3),
    Quat(DQuat),
}

impl Value {
    /// The channel's JSON record, appended to `out`.
    pub(crate) fn encode(self, out: &mut Vec<u8>) {
        match self {
            Value::Scalar(v) => scalar_json(out, v),
            Value::Vec3(v) => vec3_json(out, v),
            Value::Quat(q) => quat_json(out, q),
        }
    }
}

struct Active {
    sink: McapSink<Box<dyn WriteSeek>>,
    path: PathBuf,
    /// Per channel, in the sink's channel order: where its value lives and its base-tick divisor.
    sources: Vec<(Source, u64)>,
    record: Vec<u8>,
}

#[derive(Default)]
struct State {
    active: Option<Active>,
    /// Why the last recording stopped on its own (a write failed), until the session takes it for its event.
    failure: Option<String>,
    /// The same message, until the next `stop` or `start`: a StopRecording after the failure says why.
    stopped_by: Option<String>,
}

/// Shared between the vehicle (start/stop) and the recorder model (writes).
#[derive(Clone, Default)]
pub struct RecorderHandle(Arc<Mutex<State>>);

/// `<dir>/<stem>-<n>.mcap` with `n` one more than the largest already there (1 in an empty directory). Creates
/// `dir` as needed.
pub fn next_log_path(dir: &Path, stem: &str) -> std::io::Result<PathBuf> {
    next_numbered_path(dir, stem, "mcap")
}

/// `<dir>/<stem>-<n>.<ext>`, with `n` one past the largest number any `<stem>-<n>.<ext>` in `dir` has (the
/// directory is created if missing).
pub fn next_numbered_path(dir: &Path, stem: &str, ext: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let prefix = format!("{stem}-");
    let suffix = format!(".{ext}");
    let mut largest = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(&suffix)).and_then(|n| n.parse::<u64>().ok()) {
            largest = largest.max(n);
        }
    }
    Ok(dir.join(format!("{stem}-{}{suffix}", largest + 1)))
}

fn time_ns(tick_end: u64, base_hz: u32) -> u64 {
    (u128::from(tick_end) * 1_000_000_000 / u128::from(base_hz)) as u64
}

impl RecorderHandle {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts recording to a new file at `path` (refused if it exists). Returns the recorded signals' names.
    pub fn start(&self, bus: &Bus, base_hz: u32, requests: &[SignalRate], path: &Path) -> Result<Vec<String>, RecordError> {
        if self.is_recording() {
            return Err(RecordError::AlreadyRecording);
        }
        // Validate before creating the file: a refused request leaves nothing behind.
        let signals: Vec<(String, SignalKind)> = bus.signals().map(|(n, k)| (n.to_string(), k)).collect();
        resolve(requests, &signals, base_hz).map_err(RecordError::Spec)?;
        let file = File::create_new(path).map_err(|e| RecordError::Io(format!("{}: {e}", path.display())))?;
        self.start_with(bus, base_hz, requests, Box::new(BufWriter::new(file)), path)
    }

    /// `start` with the writer given (tests use a failing one). `path` is only reported back.
    pub fn start_with(
        &self,
        bus: &Bus,
        base_hz: u32,
        requests: &[SignalRate],
        writer: Box<dyn WriteSeek>,
        path: &Path,
    ) -> Result<Vec<String>, RecordError> {
        let mut state = self.lock();
        if state.active.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        let signals: Vec<(String, SignalKind)> = bus.signals().map(|(n, k)| (n.to_string(), k)).collect();
        let channels = resolve(requests, &signals, base_hz).map_err(RecordError::Spec)?;
        let sources = channels
            .iter()
            .map(|c| (Source::of(bus, c), c.divisor))
            .collect();
        let sink = McapSink::new(writer, &channels).map_err(RecordError::Io)?;
        state.active = Some(Active { sink, path: path.to_path_buf(), sources, record: Vec::with_capacity(96) });
        state.failure = None;
        state.stopped_by = None;
        Ok(channels.into_iter().map(|c| c.name).collect())
    }

    /// Finishes the running recording's file and returns what it holds. After a recording stopped on its own, the
    /// first `stop` reports its write error instead of `NotRecording`.
    pub fn stop(&self) -> Result<RecordingSummary, RecordError> {
        let mut state = self.lock();
        let Some(active) = state.active.take() else {
            return Err(state.stopped_by.take().map_or(RecordError::NotRecording, RecordError::Io));
        };
        drop(state);
        let s = active.sink.finish().map_err(RecordError::Io)?;
        let secs = |ns: u64| ns as f64 / 1e9;
        Ok(RecordingSummary { path: active.path, messages: s.messages, first_s: secs(s.first_ns), last_s: secs(s.last_ns) })
    }

    pub fn is_recording(&self) -> bool {
        self.lock().active.is_some()
    }

    /// The message of a recording that stopped because a write failed (once; `None` afterwards).
    pub fn take_failure(&self) -> Option<String> {
        self.lock().failure.take()
    }

    /// Writes the channels due at the end of tick `tick` (records carry that time: the state `GetState` reports
    /// after the tick). A failed write stops the recording and is kept for `take_failure`; it never fails the step.
    fn record(&self, tick: u64, base_hz: u32, bus: &Bus) {
        let mut state = self.lock();
        let Some(active) = state.active.as_mut() else { return };
        let end = tick + 1;
        let t = time_ns(end, base_hz);
        let mut failed = None;
        for (i, (source, divisor)) in active.sources.iter().enumerate() {
            if end % divisor != 0 {
                continue;
            }
            active.record.clear();
            source.encode(bus, &mut active.record);
            if let Err(e) = active.sink.write(i, t, &active.record) {
                failed = Some(format!("{}: {e}", active.path.display()));
                break;
            }
        }
        if let Some(message) = failed {
            state.active = None; // dropping the sink tries to finish the file; the error is already reported
            state.failure = Some(message.clone());
            state.stopped_by = Some(message);
        }
    }
}

/// The recorder model: last in the tick, it reads every recorded signal and writes nothing to the bus.
pub struct Recorder {
    handle: RecorderHandle,
    base_hz: u32,
}

impl Recorder {
    pub fn new(handle: RecorderHandle, base_hz: u32) -> Self {
        Self { handle, base_hz }
    }
}

impl Model for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        self.handle.record(ctx.tick, self.base_hz, bus);
        Ok(())
    }
}
