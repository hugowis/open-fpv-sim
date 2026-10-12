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
fn a_half_scale_motor_makes_half_the_torque_while_current_limited() {
    // During the 40 A clamp the torque is ke·I_scaled, so omega integrates at (about) half the rate
    // and the published current is half the clamp. (Steady state with no prop load would hide the
    // scale — the no-load current is scale-independent — so the transient is the pin. The shared
    // no-load drag leaves the half-scale rotor at 19/39 of full, inside the 2% tolerance.)
    let mut full = sim(16.0, 1.0, 1.0);
    for _ in 0..400 {
        full.step().unwrap(); // 50 ms, still on the clamp
    }
    let mut half = sim(16.0, 1.0, 1.0);
    let scale = half.bus().lookup::<f64>(&names::fault_motor_scale(0)).unwrap();
    half.bus_mut().set(scale, 0.5);
    for _ in 0..400 {
        half.step().unwrap();
    }
    assert_eq!(get(&half, &names::motor_current(0)), 20.0, "the 40 A clamp scales to 20 A");
    let wf = get(&full, &names::motor_omega(0));
    let wh = get(&half, &names::motor_omega(0));
    assert!(((wh - wf * 0.5) / wf).abs() < 0.02, "torque halves from standstill: {wh} vs {wf}/2");
}

#[test]
fn a_dead_motor_draws_no_current_and_coasts_down() {
    let mut s = sim(10.0, 1.0, 1.0);
    s.run_for(0.5).unwrap(); // spinning
    let scale = s.bus().lookup::<f64>(&names::fault_motor_scale(0)).unwrap();
    s.bus_mut().set(scale, 0.0);
    s.run_for(0.05).unwrap();
    assert_eq!(get(&s, &names::motor_current(0)), 0.0, "scale 0: no current");
    let omega_now = get(&s, &names::motor_omega(0));
    assert!(omega_now > 0.0, "the rotor coasts down through drag, it does not stop dead");
    // The no-load drag alone decelerates at ke·I0/J ≈ 477 rad/s², so ~4.35 s to stop 2073 rad/s.
    s.run_for(4.5).unwrap();
    assert_eq!(get(&s, &names::motor_omega(0)), 0.0, "drag stops it");
    let theta = get(&s, &names::motor_theta(0));
    s.run_for(0.1).unwrap();
    assert_eq!(get(&s, &names::motor_theta(0)), theta, "a stopped motor's phase is frozen");
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
