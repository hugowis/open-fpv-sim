//! Lumped thermal model: one RC node per ESC, motor and video transmitter, heated by the losses already on the bus.
//! Each node is integrated exactly (`T ← T + (T_ss − T)·(1 − e^(−dt/(R·C)))`, `T_ss = ambient + P·R`, power held over
//! the step), so the step size does not change the answer. A part without thermal data sits at ambient. Nothing
//! derates: the temperatures are observable (the bus, recordings, Betaflight's ESC temperature, Tramp's status).
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

/// One node's heat capacity (J/K) and thermal resistance to the ambient (K/W).
#[derive(Debug, Clone, Copy)]
pub struct NodeParams {
    pub heat_capacity_jpk: f64,
    pub resistance_kpw: f64,
}

/// An ESC: conduction loss `I²·r_on` plus switching loss `k_sw·V·|I|` (phase current `I`, bus voltage `V`).
#[derive(Debug, Clone, Copy)]
pub struct EscThermalParams {
    pub node: NodeParams,
    pub r_on_ohm: f64,
    pub switching_loss_coeff: f64,
}

/// A motor: winding loss `I²·R` plus the friction (no-load) torque times the speed.
#[derive(Debug, Clone, Copy)]
pub struct MotorThermalParams {
    pub node: NodeParams,
    pub winding_resistance_ohm: f64,
    pub friction_torque_nm: f64,
}

/// A video transmitter: `P_rf·(1/η − 1) + idle_w` while it is present.
#[derive(Debug, Clone, Copy)]
pub struct VtxThermalParams {
    pub node: NodeParams,
    pub efficiency: f64,
    pub idle_w: f64,
}

#[derive(Debug, Clone)]
pub struct ThermalParams {
    pub ambient_c: f64,
    /// One entry per ESC and per motor; `None` = no thermal data (the temperature stays at ambient).
    pub esc: Vec<Option<EscThermalParams>>,
    pub motor: Vec<Option<MotorThermalParams>>,
    pub vtx: Option<VtxThermalParams>,
}

pub struct ThermalNode {
    p: NodeParams,
    temp_c: f64,
}

impl ThermalNode {
    pub fn new(p: NodeParams, start_c: f64) -> Self {
        Self { p, temp_c: start_c }
    }

    /// Advances the node by `dt_s` with `power_w` going in; returns the new temperature.
    pub fn step(&mut self, power_w: f64, ambient_c: f64, dt_s: f64) -> f64 {
        let steady = ambient_c + power_w * self.p.resistance_kpw;
        let k = 1.0 - (-dt_s / (self.p.resistance_kpw * self.p.heat_capacity_jpk)).exp();
        self.temp_c += (steady - self.temp_c) * k;
        self.temp_c
    }
}

pub struct Thermal {
    p: ThermalParams,
    div: u32,
    esc_nodes: Vec<Option<ThermalNode>>,
    motor_nodes: Vec<Option<ThermalNode>>,
    vtx_node: Option<ThermalNode>,
    current: Vec<Signal<f64>>,
    omega: Vec<Signal<f64>>,
    vbus: Signal<f64>,
    vtx_present: Signal<f64>,
    vtx_power: Signal<f64>,
    vtx_pit: Signal<f64>,
    esc_temp: Vec<Signal<f64>>,
    motor_temp: Vec<Signal<f64>>,
    vtx_temp: Signal<f64>,
}

impl Thermal {
    pub fn new(p: ThermalParams, rate_divisor: u32, bus: &mut Bus) -> Self {
        let motors = p.motor.len().max(p.esc.len());
        let t = Self {
            div: rate_divisor,
            esc_nodes: p.esc.iter().map(|e| e.map(|e| ThermalNode::new(e.node, p.ambient_c))).collect(),
            motor_nodes: p.motor.iter().map(|m| m.map(|m| ThermalNode::new(m.node, p.ambient_c))).collect(),
            vtx_node: p.vtx.map(|v| ThermalNode::new(v.node, p.ambient_c)),
            current: (0..motors).map(|i| bus.signal(&names::motor_current(i))).collect(),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            vbus: bus.signal(names::BATTERY_VOLTAGE),
            vtx_present: bus.signal(names::VTX_PRESENT),
            vtx_power: bus.signal(names::VTX_POWER_MW),
            vtx_pit: bus.signal(names::VTX_PIT),
            esc_temp: (0..p.esc.len()).map(|i| bus.signal(&names::esc_temp(i))).collect(),
            motor_temp: (0..p.motor.len()).map(|i| bus.signal(&names::motor_temp(i))).collect(),
            vtx_temp: bus.signal(names::VTX_TEMP_C),
            p,
        };
        for s in t.esc_temp.iter().chain(&t.motor_temp).chain([&t.vtx_temp]) {
            bus.set(*s, t.p.ambient_c);
        }
        t
    }
}

impl Model for Thermal {
    fn name(&self) -> &str {
        "thermal"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let (dt, ambient) = (ctx.dt_s, self.p.ambient_c);
        let vbus = bus.get(self.vbus);
        for (i, node) in self.esc_nodes.iter_mut().enumerate() {
            let t = match (node, self.p.esc[i]) {
                (Some(node), Some(e)) => {
                    let amps = bus.get(self.current[i]);
                    node.step(amps * amps * e.r_on_ohm + e.switching_loss_coeff * vbus * amps.abs(), ambient, dt)
                }
                _ => ambient,
            };
            bus.set(self.esc_temp[i], t);
        }
        for (i, node) in self.motor_nodes.iter_mut().enumerate() {
            let t = match (node, self.p.motor[i]) {
                (Some(node), Some(m)) => {
                    let amps = bus.get(self.current[i]);
                    node.step(amps * amps * m.winding_resistance_ohm + m.friction_torque_nm * bus.get(self.omega[i]), ambient, dt)
                }
                _ => ambient,
            };
            bus.set(self.motor_temp[i], t);
        }
        let t = match (&mut self.vtx_node, self.p.vtx) {
            (Some(node), Some(v)) => {
                // `vtx.power_mw` keeps the selected level in pit mode, where the output is a few mW: idle draw only.
                let power = if bus.get(self.vtx_present) <= 0.5 {
                    0.0
                } else if bus.get(self.vtx_pit) > 0.5 {
                    v.idle_w
                } else {
                    bus.get(self.vtx_power) / 1000.0 * (1.0 / v.efficiency - 1.0) + v.idle_w
                };
                node.step(power, ambient, dt)
            }
            _ => ambient,
        };
        bus.set(self.vtx_temp, t);
        Ok(())
    }
}
