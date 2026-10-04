//! Open FPV Sim core: typed signal bus, models and scheduler. Deterministic, no I/O.
pub mod bus;
pub mod interp;
pub mod rng;

pub use bus::{Bus, BusValue, Signal, SignalKind};
