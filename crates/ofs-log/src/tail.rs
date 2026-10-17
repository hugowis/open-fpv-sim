//! A rolling in-memory recorder: the last `window_s` of the default table, kept so a failed session can write its
//! tail into a repro bundle (parent spec §7) even if nobody started a recording. Like the plain recorder it only
//! reads the bus, so a run's digest is the same with and without it. It keeps raw values (a few words per record,
//! no allocation once the ring has grown to its window) and encodes them only when a snapshot is written.
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use ofs_core::{Bus, Model, SignalKind, SimError, StepCtx};

use crate::recorder::{RecordError, RecordingSummary, Source, Value};
use crate::sink::McapSink;
use crate::spec::{resolve, Channel};

/// How much of the past a session keeps.
pub const DEFAULT_WINDOW_S: f64 = 10.0;

struct Message {
    channel: usize,
    time_ns: u64,
    value: Value,
}

struct State {
    channels: Vec<Channel>,
    /// Per channel, in `channels` order: where its value lives.
    sources: Vec<Source>,
    window_ns: u64,
    ring: VecDeque<Message>,
}

/// Shared between the vehicle (snapshots) and the tail recorder model (writes). A default handle has no recorder
/// behind it until a `TailRecorder` is built over it.
#[derive(Clone, Default)]
pub struct TailHandle(Arc<Mutex<Option<State>>>);

fn time_ns(tick_end: u64, base_hz: u32) -> u64 {
    (u128::from(tick_end) * 1_000_000_000 / u128::from(base_hz)) as u64
}

impl TailHandle {
    fn lock(&self) -> MutexGuard<'_, Option<State>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes the retained messages to a new MCAP file at `path` (refused if it exists) and says what it holds.
    pub fn snapshot(&self, path: &Path) -> Result<RecordingSummary, RecordError> {
        let guard = self.lock();
        let Some(state) = guard.as_ref() else { return Err(RecordError::Io("no tail recorder is running".into())) };
        let file = File::create_new(path).map_err(|e| RecordError::Io(format!("{}: {e}", path.display())))?;
        let io = |e: String| RecordError::Io(format!("{}: {e}", path.display()));
        let mut sink = McapSink::new(BufWriter::new(file), &state.channels).map_err(io)?;
        let mut record = Vec::with_capacity(96);
        for m in &state.ring {
            record.clear();
            m.value.encode(&mut record);
            sink.write(m.channel, m.time_ns, &record).map_err(io)?;
        }
        let s = sink.finish().map_err(io)?;
        let secs = |ns: u64| ns as f64 / 1e9;
        Ok(RecordingSummary { path: path.to_path_buf(), messages: s.messages, first_s: secs(s.first_ns), last_s: secs(s.last_ns) })
    }

    /// The ring's allocated capacity, in records: constant once a full window has been recorded.
    #[doc(hidden)]
    pub fn ring_capacity(&self) -> usize {
        self.lock().as_ref().map_or(0, |s| s.ring.capacity())
    }

    fn record(&self, tick: u64, base_hz: u32, bus: &Bus, groups: &[(u64, Vec<usize>)]) {
        let mut guard = self.lock();
        let Some(state) = guard.as_mut() else { return };
        let end = tick + 1;
        let now = time_ns(end, base_hz);
        let oldest = now.saturating_sub(state.window_ns);
        while state.ring.front().is_some_and(|m| m.time_ns < oldest) {
            state.ring.pop_front();
        }
        // A handful of distinct divisors for a hundred channels: one test per divisor, not per channel.
        for (divisor, channels) in groups {
            if end % divisor == 0 {
                for &i in channels {
                    state.ring.push_back(Message { channel: i, time_ns: now, value: state.sources[i].read(bus) });
                }
            }
        }
    }
}

/// The tail model: last in the tick, it reads every recorded signal and writes nothing to the bus.
pub struct TailRecorder {
    handle: TailHandle,
    base_hz: u32,
    /// The channels recorded on ticks divisible by each distinct divisor, in divisor order.
    groups: Vec<(u64, Vec<usize>)>,
}

impl TailRecorder {
    /// Records every signal `bus` has now at the default table's rates, keeping `window_s` of them in `handle`.
    pub fn new(handle: TailHandle, bus: &Bus, base_hz: u32, window_s: f64) -> Result<Self, RecordError> {
        if !window_s.is_finite() || window_s <= 0.0 {
            return Err(RecordError::Spec(format!("the tail window must be finite and > 0 s, got {window_s}")));
        }
        let signals: Vec<(String, SignalKind)> = bus.signals().map(|(n, k)| (n.to_string(), k)).collect();
        let channels = resolve(&[], &signals, base_hz).map_err(RecordError::Spec)?;
        let sources = channels.iter().map(|c| Source::of(bus, c)).collect();
        let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
        for (i, c) in channels.iter().enumerate() {
            match groups.iter_mut().find(|(d, _)| *d == c.divisor) {
                Some((_, members)) => members.push(i),
                None => groups.push((c.divisor, vec![i])),
            }
        }
        groups.sort_by_key(|(d, _)| *d);
        *handle.lock() = Some(State { channels, sources, window_ns: (window_s * 1e9) as u64, ring: VecDeque::new() });
        Ok(Self { handle, base_hz, groups })
    }
}

impl Model for TailRecorder {
    fn name(&self) -> &str {
        "tail"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        self.handle.record(ctx.tick, self.base_hz, bus, &self.groups);
        Ok(())
    }
}
