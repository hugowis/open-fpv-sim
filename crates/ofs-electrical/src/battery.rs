//! LiPo pack as a series string of cells, each a Thevenin equivalent circuit: OCV(SoC) − I·R0 − V1, with
//! dV1/dt = I/C1 − V1/(R1·C1). The pack parameters are the string's totals, shared out evenly (R0 and R1 divided by
//! the cell count, C1 multiplied by it, so the polarisation time constant and the summed V1 are the old single-pack
//! model's); cells differ only through their own resistance and capacity scales. The pack voltage is the sum.
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
    /// Per-cell multipliers of the cell's share of `r0_ohm` and of `capacity_mah`; empty = every cell 1.0,
    /// otherwise one entry per cell.
    pub cell_r0_scale: Vec<f64>,
    pub cell_capacity_scale: Vec<f64>,
    /// Ageing (from the part's cycle count): a multiplier on every cell's capacity and on every cell's R0.
    pub capacity_factor: f64,
    pub resistance_factor: f64,
}

pub struct Battery {
    p: BatteryParams,
    div: u32,
    soc: Vec<f64>,
    v1: Vec<f64>,
    /// Each cell's ohmic resistance and capacity, after the scales and the ageing factors.
    r0: Vec<f64>,
    capacity_mah: Vec<f64>,
    consumed_mah: f64,
    bus_currents: Vec<Signal<f64>>,
    voltage: Signal<f64>,
    current: Signal<f64>,
    soc_sig: Signal<f64>,
    consumed: Signal<f64>,
    cell_voltage: Vec<Signal<f64>>,
    cell_soc: Vec<Signal<f64>>,
    r_offset: Signal<f64>,
    v_offset: Signal<f64>,
    weak_cell: Signal<f64>,
}

impl Battery {
    pub fn new(p: BatteryParams, motor_count: usize, rate_divisor: u32, bus: &mut Bus) -> Self {
        if let Err(e) = interp::check_table(&p.ocv_table, "battery.ocv_table") {
            panic!("{e}");
        }
        let n = p.cells as usize;
        for (list, what) in [(&p.cell_r0_scale, "battery.cell_r0_scale"), (&p.cell_capacity_scale, "battery.cell_capacity_scale")] {
            assert!(list.is_empty() || list.len() == n, "{what} needs one entry per cell ({n}) or none, got {}", list.len());
        }
        let scale = |list: &[f64], k: usize| list.get(k).copied().unwrap_or(1.0);
        let b = Self {
            bus_currents: (0..motor_count).map(|i| bus.signal(&names::esc_bus_current(i))).collect(),
            voltage: bus.signal(names::BATTERY_VOLTAGE),
            current: bus.signal(names::BATTERY_CURRENT),
            soc_sig: bus.signal(names::BATTERY_SOC),
            consumed: bus.signal(names::BATTERY_CONSUMED),
            cell_voltage: (0..n).map(|k| bus.signal(&names::battery_cell_voltage(k))).collect(),
            cell_soc: (0..n).map(|k| bus.signal(&names::battery_cell_soc(k))).collect(),
            r_offset: bus.signal(names::FAULT_BATTERY_R_OFFSET_OHM),
            v_offset: bus.signal(names::FAULT_BATTERY_V_OFFSET_V),
            weak_cell: bus.signal(names::FAULT_BATTERY_WEAK_CELL),
            div: rate_divisor,
            soc: vec![p.initial_soc; n],
            v1: vec![0.0; n],
            r0: (0..n).map(|k| p.r0_ohm / n as f64 * scale(&p.cell_r0_scale, k) * p.resistance_factor).collect(),
            capacity_mah: (0..n).map(|k| p.capacity_mah * scale(&p.cell_capacity_scale, k) * p.capacity_factor).collect(),
            consumed_mah: 0.0,
            p,
        };
        b.publish(bus, 0.0);
        b
    }

    /// The cell the weak-cell fault names (`fault.battery.weak_cell` is its index + 1, 0 = none).
    fn weak_index(&self, bus: &Bus) -> Option<usize> {
        let weak = bus.get(self.weak_cell);
        (weak >= 0.5).then(|| weak.round() as usize - 1)
    }

    fn publish(&self, bus: &mut Bus, current: f64) {
        let weak = self.weak_index(bus);
        let extra = current * bus.get(self.r_offset) + bus.get(self.v_offset);
        let mut pack = 0.0;
        for k in 0..self.soc.len() {
            let fault = if weak == Some(k) { extra } else { 0.0 };
            let v = interp::linear(&self.p.ocv_table, self.soc[k]) - current * self.r0[k] - self.v1[k] - fault;
            pack += v;
            bus.set(self.cell_voltage[k], v);
            bus.set(self.cell_soc[k], self.soc[k]);
        }
        bus.set(self.voltage, pack);
        bus.set(self.current, current);
        bus.set(self.soc_sig, self.soc.iter().sum::<f64>() / self.soc.len().max(1) as f64);
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
        let n = self.soc.len() as f64;
        let (r1, c1) = (self.p.r1_ohm / n, self.p.c1_f * n);
        let decay = (-dt / (r1 * c1)).exp();
        let used_mah = current * dt / 3.6;
        self.consumed_mah += used_mah;
        for k in 0..self.soc.len() {
            self.v1[k] = self.v1[k] * decay + current * r1 * (1.0 - decay);
            self.soc[k] = (self.soc[k] - used_mah / self.capacity_mah[k]).clamp(0.0, 1.0);
        }
        self.publish(bus, current);
        Ok(())
    }
}
