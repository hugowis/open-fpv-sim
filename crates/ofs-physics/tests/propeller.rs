use std::f64::consts::PI;

use ofs_core::{names, Bus, Scheduler};
use ofs_physics::propeller::{thrust_torque, PropParams, Propeller};

fn prop(ct: Vec<(f64, f64)>, cp: Vec<(f64, f64)>) -> PropParams {
    PropParams { diameter_m: 0.127, ct_table: ct, cp_table: cp }
}

#[test]
fn thrust_and_torque_follow_coefficient_formula() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    let omega = 24_000.0 * 2.0 * PI / 60.0;
    let n: f64 = 400.0;
    let (t, q) = thrust_torque(&p, omega);
    let t_expected = 0.11 * 1.225 * n * n * 0.127_f64.powi(4);
    let q_expected = 0.045 * 1.225 * n * n * 0.127_f64.powi(5) / (2.0 * PI);
    assert!((t - t_expected).abs() < 1e-9);
    assert!((q - q_expected).abs() < 1e-12);
    assert!(t > 5.0 && t < 6.0, "5-inch prop at 24k rpm makes ~5.6 N, got {t}");
}

#[test]
fn stopped_or_reversed_rotor_makes_nothing() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    assert_eq!(thrust_torque(&p, 0.0), (0.0, 0.0));
    assert_eq!(thrust_torque(&p, -100.0), (0.0, 0.0));
}

#[test]
fn coefficients_are_interpolated_by_rpm() {
    let p = prop(vec![(0.0, 0.10), (20_000.0, 0.20)], vec![(0.0, 0.05)]);
    let omega = 10_000.0 * 2.0 * PI / 60.0;
    let n = 10_000.0 / 60.0;
    let (t, _) = thrust_torque(&p, omega);
    assert!((t - 0.15 * 1.225 * n * n * 0.127_f64.powi(4)).abs() < 1e-9);
}

#[test]
fn model_reads_rotor_speed_and_writes_thrust_and_torque() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    let mut bus = Bus::new();
    let model = Propeller::new(2, p.clone(), &mut bus);
    let omega = bus.signal::<f64>(&names::motor_omega(2));
    bus.set(omega, 1500.0);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(model));
    s.step().unwrap();
    let (t, q) = thrust_torque(&p, 1500.0);
    let b = s.bus();
    assert_eq!(b.get(b.lookup::<f64>(&names::prop_thrust(2)).unwrap()), t);
    assert_eq!(b.get(b.lookup::<f64>(&names::prop_torque(2)).unwrap()), q);
}

#[test]
#[should_panic(expected = "prop.ct_table")]
fn an_unsorted_coefficient_table_is_refused_when_the_model_is_built() {
    let mut bus = Bus::new();
    Propeller::new(0, prop(vec![(10_000.0, 0.1), (5_000.0, 0.12)], vec![(0.0, 0.05)]), &mut bus);
}

#[test]
fn each_rotor_has_its_own_model_name_and_runs_every_tick() {
    use ofs_core::Model;
    let mut bus = Bus::new();
    let p = Propeller::new(2, prop(vec![(0.0, 0.1)], vec![(0.0, 0.05)]), &mut bus);
    assert_eq!((p.name(), p.rate_divisor()), ("prop.2", 1));
}
