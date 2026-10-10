//! Open FPV Sim core: typed signal bus, models and scheduler. Deterministic, no I/O.
pub mod bus;
pub mod consts;
pub mod interp;
pub mod model;
pub mod names;
pub mod rng;
pub mod scheduler;
pub mod shape;
pub mod wire;

pub use bus::{Bus, BusValue, Signal, SignalKind};
pub use model::{Model, SimError, StepCtx};
pub use scheduler::Scheduler;
pub use wire::Wire;
