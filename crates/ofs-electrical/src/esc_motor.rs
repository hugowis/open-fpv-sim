//! Averaged ESC + brushless DC motor (winding inductance neglected).
//! duty lags the command; I = (duty·Vbus − Ke·ω)/R clamped to the ESC limit;
//! J·dω/dt = Ke·I − Ke·I0 − Q_prop; bus current = duty·I.
use std::f64::consts::PI;

use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

#[derive(Debug, Clone)]
pub struct MotorParams {
    pub kv_rpm_per_v: f64,
    pub resistance_ohm: f64,
    pub no_load_current_a: f64,
    pub rotor_inertia_kgm2: f64,
}

#[derive(Debug, Clone)]
pub struct EscParams {
    pub response_tau_s: f64,
    pub current_limit_a: f64,
}

pub struct EscMotor {
    name: String,
    esc: EscParams,
    motor: MotorParams,
    inertia_kgm2: f64,
    ke: f64,
    duty: f64,
    omega: f64,
    cmd: Signal<f64>,
    vbus: Signal<f64>,
    load: Signal<f64>,
    omega_sig: Signal<f64>,
    omega_dot_sig: Signal<f64>,
    current_sig: Signal<f64>,
    bus_current_sig: Signal<f64>,
    duty_sig: Signal<f64>,
}

impl EscMotor {
    pub fn new(index: usize, esc: EscParams, motor: MotorParams, prop_inertia_kgm2: f64, bus: &mut Bus) -> Self {
        Self {
            name: format!("esc_motor.{index}"),
            ke: 60.0 / (2.0 * PI * motor.kv_rpm_per_v),
            inertia_kgm2: motor.rotor_inertia_kgm2 + prop_inertia_kgm2,
            esc,
            motor,
            duty: 0.0,
            omega: 0.0,
            cmd: bus.signal(&names::motor_cmd(index)),
            vbus: bus.signal(names::BATTERY_VOLTAGE),
            load: bus.signal(&names::prop_torque(index)),
            omega_sig: bus.signal(&names::motor_omega(index)),
            omega_dot_sig: bus.signal(&names::motor_omega_dot(index)),
            current_sig: bus.signal(&names::motor_current(index)),
            bus_current_sig: bus.signal(&names::esc_bus_current(index)),
            duty_sig: bus.signal(&names::esc_duty(index)),
        }
    }
}

impl Model for EscMotor {
    fn name(&self) -> &str {
        &self.name
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let dt = ctx.dt_s;
        let target = bus.get(self.cmd).clamp(0.0, 1.0);
        self.duty += (target - self.duty) * (1.0 - (-dt / self.esc.response_tau_s).exp());

        let applied_v = self.duty * bus.get(self.vbus);
        let limit = self.esc.current_limit_a;
        let current = ((applied_v - self.ke * self.omega) / self.motor.resistance_ohm).clamp(-limit, limit);
        let friction = if self.omega > 0.0 { self.ke * self.motor.no_load_current_a } else { 0.0 };
        let net_torque = self.ke * current - friction - bus.get(self.load);
        let omega_dot = net_torque / self.inertia_kgm2;
        self.omega = (self.omega + omega_dot * dt).max(0.0); // no reversing in v1 (3D mode unsupported)

        bus.set(self.omega_sig, self.omega);
        bus.set(self.omega_dot_sig, omega_dot);
        bus.set(self.current_sig, current);
        bus.set(self.bus_current_sig, self.duty * current);
        bus.set(self.duty_sig, self.duty);
        Ok(())
    }
}
