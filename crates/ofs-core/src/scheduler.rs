//! Fixed-rate, multi-rate, deterministic stepping of models over one bus.
use crate::bus::Bus;
use crate::model::{Model, SimError, StepCtx};

pub struct Scheduler {
    base_hz: u32,
    tick: u64,
    bus: Bus,
    models: Vec<Box<dyn Model>>,
}

impl Scheduler {
    pub fn new(base_hz: u32, bus: Bus) -> Self {
        assert!(base_hz > 0, "base_hz must be > 0");
        Self { base_hz, tick: 0, bus, models: Vec::new() }
    }

    pub fn add(&mut self, model: Box<dyn Model>) {
        assert!(model.rate_divisor() >= 1, "model '{}' has rate divisor 0", model.name());
        self.models.push(model);
    }

    pub fn base_hz(&self) -> u32 {
        self.base_hz
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn time_s(&self) -> f64 {
        self.tick as f64 / f64::from(self.base_hz)
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    /// Runs every model due on this tick, in registration order, then checks all signals are finite.
    /// On error the tick is not advanced.
    pub fn step(&mut self) -> Result<(), SimError> {
        let time_s = self.time_s();
        for model in self.models.iter_mut() {
            let div = u64::from(model.rate_divisor());
            if self.tick % div == 0 {
                let ctx = StepCtx { tick: self.tick, time_s, dt_s: div as f64 / f64::from(self.base_hz) };
                model.step(&ctx, &mut self.bus)?;
            }
        }
        if let Some(name) = self.bus.first_non_finite() {
            return Err(SimError::NonFinite(name.to_string()));
        }
        self.tick += 1;
        Ok(())
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(SimError::InvalidArgument(format!("seconds must be finite and >= 0 (got {seconds})")));
        }
        let ticks = (seconds * f64::from(self.base_hz)).round() as u64;
        for _ in 0..ticks {
            self.step()?;
        }
        Ok(())
    }
}
