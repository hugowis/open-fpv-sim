//! LiPo pack as a Thevenin equivalent circuit: OCV(SoC) − I·R0 − V1, with dV1/dt = I/C1 − V1/(R1·C1).
use ofs_core::{interp, names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct BatteryParams {
    pub cells: u32,
    pub capacity_mah: f64,
    /// Pack ohmic resistance.
    pub r0_ohm: f64,
    /// Pack polarization resistance and capacitance.
    pub r1_ohm: f64,
    pub c1_f: f64,
    /// (state of charge 0..1, cell open-circuit volts), sorted by state of charge.
    pub ocv_table: Vec<(f64, f64)>,
    pub initial_soc: f64,
}

pub struct Battery {
    p: BatteryParams,
    div: u32,
    soc: f64,
    v1: f64,
    consumed_mah: f64,
    bus_currents: Vec<Signal<f64>>,
    voltage: Signal<f64>,
    current: Signal<f64>,
    soc_sig: Signal<f64>,
    consumed: Signal<f64>,
}

impl Battery {
    pub fn new(p: BatteryParams, motor_count: usize, rate_divisor: u32, bus: &mut Bus) -> Self {
        let b = Self {
            bus_currents: (0..motor_count).map(|i| bus.signal(&names::esc_bus_current(i))).collect(),
            voltage: bus.signal(names::BATTERY_VOLTAGE),
            current: bus.signal(names::BATTERY_CURRENT),
            soc_sig: bus.signal(names::BATTERY_SOC),
            consumed: bus.signal(names::BATTERY_CONSUMED),
            div: rate_divisor,
            soc: p.initial_soc,
            v1: 0.0,
            consumed_mah: 0.0,
            p,
        };
        b.publish(bus, 0.0);
        b
    }

    fn publish(&self, bus: &mut Bus, current: f64) {
        let ocv = f64::from(self.p.cells) * interp::linear(&self.p.ocv_table, self.soc);
        bus.set(self.voltage, ocv - current * self.p.r0_ohm - self.v1);
        bus.set(self.current, current);
        bus.set(self.soc_sig, self.soc);
        bus.set(self.consumed, self.consumed_mah);
    }
}

impl Model for Battery {
    fn name(&self) -> &str {
        "battery"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let current: f64 = self.bus_currents.iter().map(|s| bus.get(*s)).sum();
        let decay = (-dt / (self.p.r1_ohm * self.p.c1_f)).exp();
        self.v1 = self.v1 * decay + current * self.p.r1_ohm * (1.0 - decay);
        let used_mah = current * dt / 3.6;
        self.consumed_mah += used_mah;
        self.soc = (self.soc - used_mah / self.p.capacity_mah).clamp(0.0, 1.0);
        self.publish(bus, current);
        Ok(())
    }
}
