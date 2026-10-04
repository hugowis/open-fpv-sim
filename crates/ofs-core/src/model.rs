//! The interface every simulated part implements.
use crate::bus::Bus;

/// Timing for one step of one model.
#[derive(Debug, Clone, Copy)]
pub struct StepCtx {
    pub tick: u64,
    pub time_s: f64,
    pub dt_s: f64,
}

/// Simulator failures. Simulated drone failures are never reported through this type.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimError {
    #[error("firmware: {0}")]
    Firmware(String),
    #[error("non-finite value in signal '{0}'")]
    NonFinite(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("{0}")]
    Other(String),
}

/// A simulated part. Reads and writes bus signals only; runs every `rate_divisor` base ticks.
pub trait Model: Send {
    fn name(&self) -> &str;
    fn rate_divisor(&self) -> u32;
    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError>;
}
