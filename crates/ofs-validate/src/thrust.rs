//! The thrust stand: the simulator's own ESC, motor and propeller held at a measured command and pack voltage until
//! they settle, as a bench with a stiff supply would hold them. Sea-level standard air, as the model assumes.
use ofs_core::{names, Bus, Scheduler};
use ofs_electrical::esc_motor::EscMotor;
use ofs_physics::propeller::Propeller;
use ofs_sim::vehicle::drivetrain_params;

use crate::submission::ThrustRow;

/// The base rate the stand runs at: the quads' own.
const BASE_HZ: u32 = 8000;
/// How long between looks at the rotor speed.
const BLOCK_S: f64 = 0.25;
/// Give up on a row that has not settled by then.
const MAX_SETTLE_S: f64 = 10.0;
/// Settled: the rotor speed moved by less than this fraction of itself in one block.
const SETTLED: f64 = 1e-6;

/// What the models settle at for one row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Predicted {
    pub rpm: f64,
    pub thrust_n: f64,
    /// Battery-side current.
    pub current_a: f64,
    /// Simulated seconds until the rotor speed stopped changing.
    pub settle_s: f64,
}

/// Runs motor `motor` of `cfg` at `row.command` and `row.voltage_v` until it settles.
pub fn simulate(cfg: &ofs_config::QuadConfig, motor: usize, row: &ThrustRow) -> Result<Predicted, String> {
    if motor >= cfg.motors.len() || motor >= cfg.props.len() || motor >= cfg.escs.len() {
        return Err(format!("motor {motor} does not exist (the quad has {})", cfg.motors.len()));
    }
    let d = drivetrain_params(cfg, motor);
    let mut bus = Bus::new();
    // Index 0 inside the stand's own bus, whichever motor of the quad it models.
    let prop = Propeller::new(0, d.prop, &mut bus);
    let esc_motor = EscMotor::new(0, d.esc, d.motor, d.prop_inertia_kgm2, &mut bus);
    let cmd = bus.signal(&names::motor_cmd(0));
    let vbus = bus.signal(names::BATTERY_VOLTAGE);
    let omega = bus.signal(&names::motor_omega(0));
    let thrust = bus.signal(&names::prop_thrust(0));
    let current = bus.signal(&names::esc_bus_current(0));
    bus.set(cmd, row.command);
    bus.set(vbus, row.voltage_v);
    let mut stand = Scheduler::new(BASE_HZ, bus);
    stand.add(Box::new(prop));
    stand.add(Box::new(esc_motor));

    let mut previous = 0.0_f64;
    let mut elapsed = 0.0_f64;
    while elapsed < MAX_SETTLE_S {
        stand.run_for(BLOCK_S).map_err(|e| e.to_string())?;
        elapsed += BLOCK_S;
        let w: f64 = stand.bus().get(omega);
        if (w - previous).abs() <= SETTLED * w.max(1.0) {
            let n = w / (2.0 * std::f64::consts::PI);
            return Ok(Predicted {
                rpm: n * 60.0,
                thrust_n: stand.bus().get(thrust),
                current_a: stand.bus().get(current),
                settle_s: elapsed,
            });
        }
        previous = w;
    }
    Err(format!("no steady state within {MAX_SETTLE_S:.0} s"))
}
