//! Test stand-in for a flight controller: every motor command = throttle stick. No stabilisation.
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

pub struct OpenLoopFc {
    div: u32,
    throttle: Signal<f64>,
    cmds: Vec<Signal<f64>>,
}

impl OpenLoopFc {
    pub fn new(motor_count: usize, rate_divisor: u32, bus: &mut Bus) -> Self {
        Self {
            div: rate_divisor,
            throttle: bus.signal(names::RC_THROTTLE),
            cmds: (0..motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
        }
    }
}

impl Model for OpenLoopFc {
    fn name(&self) -> &str {
        "fc.open_loop"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let t = bus.get(self.throttle).clamp(0.0, 1.0);
        for c in &self.cmds {
            bus.set(*c, t);
        }
        Ok(())
    }
}
