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
    /// The advance ratio `V/(nD)` at which the torque vanishes, for windmilling; 0 turns the effect off.
    pub windmill_j0: f64,
}

/// Thrust (N) and drag torque (N·m) magnitudes for a rotor speed in rad/s, in still air.
pub fn thrust_torque(p: &PropParams, omega_radps: f64) -> (f64, f64) {
    thrust_torque_in_flow(p, omega_radps, 0.0)
}

/// As `thrust_torque`, with `inflow_mps` of air coming up through the disc (the aircraft descending). With
/// `windmill_j0 > 0` the torque coefficient becomes `Cp·(1 − J/j0)` for `J = inflow/(nD) > 0`, so it vanishes at
/// `J = j0` and drives the rotor beyond it; thrust is unchanged. Climb (`inflow <= 0`) is unchanged too.
pub fn thrust_torque_in_flow(p: &PropParams, omega_radps: f64, inflow_mps: f64) -> (f64, f64) {
    if omega_radps <= 0.0 {
        return (0.0, 0.0);
    }
    let n = omega_radps / (2.0 * PI);
    let rpm = n * 60.0;
    let ct = interp::linear(&p.ct_table, rpm);
    let cp = interp::linear(&p.cp_table, rpm);
    let d = p.diameter_m;
    let thrust = ct * AIR_DENSITY_KGPM3 * n * n * d.powi(4);
    let torque = if p.windmill_j0 > 0.0 && inflow_mps > 0.0 {
        // Cp (1 − V/(n D j0)) ρ n² D⁵ / 2π, multiplied out so nothing divides by n.
        cp * AIR_DENSITY_KGPM3 * d.powi(4) * n * (d * n - inflow_mps / p.windmill_j0) / (2.0 * PI)
    } else {
        cp * AIR_DENSITY_KGPM3 * n * n * d.powi(5) / (2.0 * PI)
    };
    (thrust, torque)
}

pub struct Propeller {
    name: String,
    p: PropParams,
    omega: Signal<f64>,
    inflow: Signal<f64>,
    thrust: Signal<f64>,
    torque: Signal<f64>,
    thrust_scale: Signal<f64>,
}

impl Propeller {
    pub fn new(index: usize, p: PropParams, bus: &mut Bus) -> Self {
        for (table, what) in [(&p.ct_table, "prop.ct_table"), (&p.cp_table, "prop.cp_table")] {
            if let Err(e) = interp::check_table(table, what) {
                panic!("{e}");
            }
        }
        let thrust_scale = bus.signal(&names::fault_prop_thrust_scale(index));
        bus.set(thrust_scale, 1.0); // the multiplicative fault seam starts at its identity (M4b)
        Self {
            name: format!("prop.{index}"),
            p,
            omega: bus.signal(&names::motor_omega(index)),
            inflow: bus.signal(&names::prop_inflow(index)),
            thrust: bus.signal(&names::prop_thrust(index)),
            torque: bus.signal(&names::prop_torque(index)),
            thrust_scale,
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
        let (t, q) = thrust_torque_in_flow(&self.p, bus.get(self.omega), bus.get(self.inflow));
        let scale = bus.get(self.thrust_scale);
        bus.set(self.thrust, t * scale);
        bus.set(self.torque, q * scale);
        Ok(())
    }
}
