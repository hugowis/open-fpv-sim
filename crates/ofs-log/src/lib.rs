//! Recording the signal bus to MCAP (M4c). The recorder is a model appended last to the scheduler: it only reads
//! the bus, so a run's digest is the same with and without it, and two lockstep runs with the same seed and inputs
//! write byte-identical files.
pub mod glob;
pub mod recorder;
pub mod sink;
pub mod spec;
pub mod tail;

pub use recorder::{RecordError, Recorder, RecorderHandle, RecordingSummary};
pub use spec::{Channel, SignalRate, DEFAULT_RATES};
