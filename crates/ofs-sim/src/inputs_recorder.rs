//! Records the sticks of a live flight as a `play_inputs` CSV (spec §6): a row whenever any stick changes, in the
//! columns `inputs::parse` reads, so the file replays the flight's sticks tick for tick in lockstep.
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use ofs_core::names::RC_AUX_COUNT;
use ofs_log::RecordError;

use crate::vehicle::Sticks;

pub const HEADER: &str = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4";

const _: () = assert!(RC_AUX_COUNT == 4, "the CSV header names four aux columns");

/// What a finished recording wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct InputsRecordingSummary {
    pub path: PathBuf,
    pub rows: u64,
    /// The time of the last row, in seconds from the start of the recording.
    pub duration_s: f64,
}

pub struct InputsRecorder {
    out: BufWriter<Box<dyn Write + Send>>,
    path: PathBuf,
    base_hz: u32,
    start_tick: u64,
    /// The newest row, held until the tick moves on: a second change in the same tick replaces it.
    pending: Option<(u64, [f64; 8])>,
    /// The last values seen (written or pending): setting the sticks to the same values adds no row.
    last: Option<[f64; 8]>,
    /// The values of the newest row actually written: a same-tick change that returns to them leaves no row at all.
    committed: Option<[f64; 8]>,
    rows: u64,
    last_row_ticks: u64,
    failure: Option<String>,
}

/// The eight values of a row (roll, pitch, yaw, throttle, aux1..4), clamped to the ranges `play_inputs` accepts
/// (throttle [0, 1], every other stick [-1, 1]). A value that is not finite (`Vehicle::set_sticks` refuses those, so
/// none normally gets here) is written as that field's neutral value, so a recording always has its first row.
fn values_of(s: &Sticks) -> [f64; 8] {
    let neutral = Sticks::default();
    let pick = |v: f64, n: f64| if v.is_finite() { v } else { n };
    let clean = Sticks {
        roll: pick(s.roll, neutral.roll),
        pitch: pick(s.pitch, neutral.pitch),
        yaw: pick(s.yaw, neutral.yaw),
        throttle: pick(s.throttle, neutral.throttle),
        aux: [0, 1, 2, 3].map(|i| pick(s.aux[i], neutral.aux[i])),
    }
    .clamped();
    [clean.roll, clean.pitch, clean.yaw, clean.throttle, clean.aux[0], clean.aux[1], clean.aux[2], clean.aux[3]]
}

impl InputsRecorder {
    /// Creates `path` (never truncating an existing file), writes the header and the first row: the sticks as they
    /// stand at `tick`.
    pub fn start(path: &Path, base_hz: u32, tick: u64, sticks: &Sticks) -> Result<Self, RecordError> {
        let file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| RecordError::from_create(path, &e))?;
        Self::with_writer(BufWriter::new(Box::new(file)), path, base_hz, tick, sticks)
    }

    /// `start` over any writer; `path` names the recording in messages.
    fn with_writer(
        mut out: BufWriter<Box<dyn Write + Send>>,
        path: &Path,
        base_hz: u32,
        tick: u64,
        sticks: &Sticks,
    ) -> Result<Self, RecordError> {
        writeln!(out, "{HEADER}").map_err(|e| RecordError::Io(format!("{}: {e}", path.display())))?;
        let mut recorder =
            Self { out, path: path.to_owned(), base_hz, start_tick: tick, pending: None, last: None, committed: None, rows: 0, last_row_ticks: 0, failure: None };
        recorder.record(tick, sticks);
        Ok(recorder)
    }

    /// Notes the sticks as set at `tick`: a new row when they differ from the last values seen.
    pub fn record(&mut self, tick: u64, sticks: &Sticks) {
        let values = values_of(sticks);
        if self.last == Some(values) {
            return;
        }
        self.last = Some(values);
        let since_start = tick.saturating_sub(self.start_tick);
        match &mut self.pending {
            Some((t, v)) if *t == since_start => {
                *v = values;
                // A change that came back to the row already written within the same tick says nothing new.
                if self.committed == Some(values) {
                    self.pending = None;
                }
            }
            _ => {
                self.write_pending();
                self.pending = Some((since_start, values));
            }
        }
    }

    fn write_pending(&mut self) {
        let Some((ticks, values)) = self.pending.take() else { return };
        if self.failure.is_some() {
            return;
        }
        let t_s = ticks as f64 / f64::from(self.base_hz);
        let columns = values.map(|v| v.to_string()).join(",");
        match writeln!(self.out, "{t_s},{columns}") {
            Ok(()) => {
                self.rows += 1;
                self.last_row_ticks = ticks;
                self.committed = Some(values);
            }
            Err(e) => self.failure = Some(format!("{}: {e}", self.path.display())),
        }
    }

    /// Why writing failed, once it has; the recording is dead from then on.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Writes the pending row and flushes the file.
    pub fn finish(mut self) -> Result<InputsRecordingSummary, RecordError> {
        self.write_pending();
        let flushed = self.out.flush();
        if let Some(message) = self.failure.take() {
            return Err(RecordError::Io(message));
        }
        flushed.map_err(|e| RecordError::Io(format!("{}: {e}", self.path.display())))?;
        Ok(InputsRecordingSummary {
            path: self.path.clone(),
            rows: self.rows,
            duration_s: self.last_row_ticks as f64 / f64::from(self.base_hz),
        })
    }
}

impl Drop for InputsRecorder {
    /// A session that ends mid-recording still leaves a complete file.
    fn drop(&mut self) {
        self.write_pending();
        let _ = self.out.flush();
    }
}

#[cfg(test)]
impl InputsRecorder {
    /// A recorder over `out` instead of a new file (tests inject failing writers).
    pub(crate) fn over(out: Box<dyn Write + Send>, capacity: usize, base_hz: u32, tick: u64, sticks: &Sticks) -> Result<Self, RecordError> {
        Self::with_writer(BufWriter::with_capacity(capacity, out), Path::new("test-inputs.csv"), base_hz, tick, sticks)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Takes `budget` bytes, then every write fails; `flush_fails` makes the flush fail as well.
    pub(crate) struct Faulty {
        pub budget: usize,
        pub flush_fails: bool,
    }

    impl Write for Faulty {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.len() > self.budget {
                return Err(std::io::ErrorKind::Other.into());
            }
            self.budget -= buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.flush_fails { Err(std::io::ErrorKind::Other.into()) } else { Ok(()) }
        }
    }

    fn sticks(throttle: f64) -> Sticks {
        Sticks { throttle, ..Default::default() }
    }

    /// A recorder whose writer takes the header and nothing more (no buffering, so a failed write surfaces at once).
    fn recorder_dying_after_the_header() -> InputsRecorder {
        let out = Faulty { budget: HEADER.len() + 1, flush_fails: true };
        InputsRecorder::over(Box::new(out), 0, 500, 0, &sticks(0.1)).unwrap()
    }

    fn rows(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path).unwrap().lines().skip(1).map(str::to_owned).collect()
    }

    #[test]
    fn a_change_reverted_in_the_same_tick_writes_no_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.csv");
        let mut r = InputsRecorder::start(&path, 500, 0, &sticks(0.1)).unwrap();
        r.record(10, &sticks(0.5));
        r.record(10, &sticks(0.1)); // back to what was last written, in the same tick
        r.record(20, &sticks(0.2));
        let summary = r.finish().unwrap();
        assert_eq!(summary.rows, 2, "{:?}", rows(&path));
        assert_eq!(rows(&path), ["0,0,0,0,0.1,-1,-1,-1,-1", "0.04,0,0,0,0.2,-1,-1,-1,-1"]);
    }

    #[test]
    fn a_recording_started_on_a_non_finite_stick_still_has_its_first_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.csv");
        let r = InputsRecorder::start(&path, 500, 0, &Sticks { throttle: f64::NAN, ..Sticks::default() }).unwrap();
        r.finish().unwrap();
        assert_eq!(rows(&path), ["0,0,0,0,0,-1,-1,-1,-1"], "the neutral value stands in for the stick that is not a number");
    }

    #[test]
    fn a_writer_that_fails_at_the_header_fails_start() {
        let out = Faulty { budget: 0, flush_fails: true };
        let err = InputsRecorder::over(Box::new(out), 0, 500, 0, &sticks(0.1)).err().expect("the header write fails");
        assert!(matches!(&err, RecordError::Io(m) if m.contains("test-inputs.csv")), "{err:?}");
    }

    #[test]
    fn a_failed_row_write_kills_the_recording_and_counts_no_row() {
        let mut r = recorder_dying_after_the_header();
        assert!(r.failure().is_none(), "the first row is pending, not written yet");
        r.record(10, &sticks(0.2)); // writes the first row: fails
        let why = r.failure().expect("the failed write is recorded").to_owned();
        assert!(why.contains("test-inputs.csv"), "{why}");
        r.record(20, &sticks(0.3)); // a dead recording takes no more rows
        assert_eq!(r.rows, 0, "a failed write is not counted");
        assert!(matches!(r.finish(), Err(RecordError::Io(m)) if m == why));
    }

    #[test]
    fn a_failure_at_the_flush_is_an_io_error_from_finish() {
        let out = Faulty { budget: usize::MAX, flush_fails: true };
        let mut r = InputsRecorder::over(Box::new(out), 8192, 500, 0, &sticks(0.1)).unwrap();
        r.record(10, &sticks(0.2));
        assert!(r.failure().is_none(), "writes only reach the buffer");
        assert!(matches!(r.finish(), Err(RecordError::Io(_))));
    }

    #[test]
    fn dropping_a_failed_recorder_does_not_panic() {
        let mut r = recorder_dying_after_the_header();
        r.record(10, &sticks(0.2));
        r.record(20, &sticks(0.3));
        assert!(r.failure().is_some());
        drop(r);
    }
}
