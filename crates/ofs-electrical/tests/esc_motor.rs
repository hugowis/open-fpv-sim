use std::f64::consts::PI;

use ofs_core::{names, Bus, Scheduler};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};

fn sim(volts: f64, cmd: f64, no_load_a: f64) -> Scheduler {
    let mut bus = Bus::new();
    let m = EscMotor::new(
        0,
        EscParams { response_tau_s: 0.002, current_limit_a: 40.0 },
        MotorParams { kv_rpm_per_v: 2000.0, resistance_ohm: 0.1, no_load_current_a: no_load_a, rotor_inertia_kgm2: 5e-6 },
        5e-6,
        &mut bus,
    );
    let v = bus.signal::<f64>(names::BATTERY_VOLTAGE);
    bus.set(v, volts);
    let c = bus.signal::<f64>(&names::motor_cmd(0));
    bus.set(c, cmd);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(m));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

#[test]
fn unloaded_motor_reaches_kv_times_voltage_minus_resistive_drop() {
    let mut s = sim(10.0, 1.0, 1.0);
    s.run_for(2.0).unwrap();
    let expected = (10.0 - 1.0 * 0.1) * 2000.0 * 2.0 * PI / 60.0;
    let omega = get(&s, &names::motor_omega(0));
    assert!(((omega - expected) / expected).abs() < 1e-3, "omega {omega} expected {expected}");
    assert!((get(&s, &names::motor_current(0)) - 1.0).abs() < 1e-2);
}

#[test]
fn startup_current_is_limited_by_the_esc() {
    let mut s = sim(16.0, 1.0, 0.0);
    for _ in 0..16 {
        s.step().unwrap();
    }
    assert_eq!(get(&s, &names::motor_current(0)), 40.0);
}

#[test]
fn duty_follows_command_with_first_order_lag() {
    let mut s = sim(10.0, 1.0, 0.0);
    for _ in 0..16 {
        s.step().unwrap(); // 16 ticks = 2 ms = one time constant
    }
    let duty = get(&s, &names::esc_duty(0));
    assert!((duty - (1.0 - (-1.0f64).exp())).abs() < 1e-9, "duty {duty}");
}

#[test]
fn zero_command_brakes_without_reversing() {
    let mut s = sim(10.0, 1.0, 0.5);
    s.run_for(1.0).unwrap();
    let cmd = s.bus().lookup::<f64>(&names::motor_cmd(0)).unwrap();
    s.bus_mut().set(cmd, 0.0);
    s.run_for(0.01).unwrap();
    assert!(get(&s, &names::motor_current(0)) < 0.0, "active braking draws negative current");
    s.run_for(2.0).unwrap();
    assert!(get(&s, &names::motor_omega(0)) >= 0.0);
    assert!(get(&s, &names::motor_omega(0)) < 1.0);
}
