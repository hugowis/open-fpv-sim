//! Simulated KISS ESC telemetry: the battery model as Betaflight's ESC sensor sees it (UART frames, 10 bytes).
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};

pub const FRAME_SIZE: usize = 10;

/// CRC-8, polynomial 0x07, initial value 0 (Betaflight's `calculateCrc8` in `esc_sensor.c`).
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

fn field(value: f64) -> u16 {
    value.round().clamp(0.0, 65_535.0) as u16
}

/// One telemetry frame: temperature, voltage (0.01 V), current (0.01 A), consumed charge (mAh) and eRPM / 100.
pub fn kiss_frame(temperature_c: u8, voltage_v: f64, current_a: f64, consumed_mah: f64, erpm: f64) -> [u8; FRAME_SIZE] {
    let mut f = [0u8; FRAME_SIZE];
    f[0] = temperature_c;
    f[1..3].copy_from_slice(&field(voltage_v * 100.0).to_be_bytes());
    f[3..5].copy_from_slice(&field(current_a * 100.0).to_be_bytes());
    f[5..7].copy_from_slice(&field(consumed_mah).to_be_bytes());
    f[7..9].copy_from_slice(&field(erpm / 100.0).to_be_bytes());
    f[9] = crc8(&f[..9]);
    f
}

/// Writes `motors` identical frames every period to the UART wired to Betaflight's ESC-sensor port. Betaflight sums
/// current and consumption over its motor slots and averages voltage and RPM, so each frame carries a share of
/// the pack current and charge and the pack voltage; frame `k` carries ESC `k`'s temperature (the thermal model's
/// `esc.{k}.temp_c`, rounded and clamped to a byte).
///
/// eRPM = mechanical rpm × pole pairs (uniform on SITL quads; enforced by ofs-config).
pub struct EscTelemetry {
    uart: Wire,
    motors: usize,
    divisor: u32,
    pole_pairs: f64,
    voltage: Signal<f64>,
    current: Signal<f64>,
    consumed: Signal<f64>,
    omega: Vec<Signal<f64>>,
    temperature: Vec<Signal<f64>>,
}

impl EscTelemetry {
    pub fn new(uart: Wire, motors: usize, divisor: u32, pole_pairs: f64, bus: &mut Bus) -> Self {
        assert!(motors > 0, "an ESC telemetry model needs at least one motor");
        Self {
            uart,
            motors,
            divisor,
            pole_pairs,
            voltage: bus.signal(names::BATTERY_VOLTAGE),
            current: bus.signal(names::BATTERY_CURRENT),
            consumed: bus.signal(names::BATTERY_CONSUMED),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            temperature: (0..motors).map(|i| bus.signal(&names::esc_temp(i))).collect(),
        }
    }
}

impl Model for EscTelemetry {
    fn name(&self) -> &str {
        "fc.esc_telemetry"
    }

    fn rate_divisor(&self) -> u32 {
        self.divisor
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let share = self.motors as f64;
        let mean_rpm = self.omega.iter().map(|s| bus.get(*s)).sum::<f64>() / share * 60.0 / (2.0 * std::f64::consts::PI);
        for k in 0..self.motors {
            let temperature = bus.get(self.temperature[k]).round().clamp(0.0, 255.0) as u8;
            let frame = kiss_frame(
                temperature,
                bus.get(self.voltage),
                bus.get(self.current) / share,
                bus.get(self.consumed) / share,
                mean_rpm * self.pole_pairs,
            );
            self.uart.write(&frame);
        }
        Ok(())
    }
}
