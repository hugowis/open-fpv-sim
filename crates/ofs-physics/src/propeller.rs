//! Static propeller model: T = Ct·ρ·n²·D⁴, Q = Cp·ρ·n²·D⁵ / 2π, with Ct and Cp looked up by RPM.
use std::f64::consts::PI;

use ofs_core::consts::AIR_DENSITY_KGPM3;
use ofs_core::{interp, names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct PropParams {
    pub diameter_m: f64,
    /// (rpm, Ct) sorted by rpm.
    pub ct_table: Vec<(f64, f64)>,
    /// (rpm, Cp) sorted by rpm.
    pub cp_table: Vec<(f64, f64)>,
}

/// Thrust (N) and drag torque (N·m) magnitudes for a rotor speed in rad/s.
pub fn thrust_torque(p: &PropParams, omega_radps: f64) -> (f64, f64) {
    if omega_radps <= 0.0 {
        return (0.0, 0.0);
    }
    let n = omega_radps / (2.0 * PI);
    let rpm = n * 60.0;
    let ct = interp::linear(&p.ct_table, rpm);
    let cp = interp::linear(&p.cp_table, rpm);
    let d = p.diameter_m;
    let thrust = ct * AIR_DENSITY_KGPM3 * n * n * d.powi(4);
    let torque = cp * AIR_DENSITY_KGPM3 * n * n * d.powi(5) / (2.0 * PI);
    (thrust, torque)
}

pub struct Propeller {
    name: String,
    p: PropParams,
    omega: Signal<f64>,
    thrust: Signal<f64>,
    torque: Signal<f64>,
}

impl Propeller {
    pub fn new(index: usize, p: PropParams, bus: &mut Bus) -> Self {
        Self {
            name: format!("prop.{index}"),
            p,
            omega: bus.signal(&names::motor_omega(index)),
            thrust: bus.signal(&names::prop_thrust(index)),
            torque: bus.signal(&names::prop_torque(index)),
        }
    }
}

impl Model for Propeller {
    fn name(&self) -> &str {
        &self.name
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let (t, q) = thrust_torque(&self.p, bus.get(self.omega));
        bus.set(self.thrust, t);
        bus.set(self.torque, q);
        Ok(())
    }
}
