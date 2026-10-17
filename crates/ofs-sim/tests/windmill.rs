//! Windmilling end to end: a propeller and an ESC/motor on one bus, the motor killed in a descent. The steady
//! rotor speed is the analytic root of "aero drive balances friction" (docs/research/rotor-dynamics.md).
use std::f64::consts::PI;

use ofs_core::consts::AIR_DENSITY_KGPM3;
use ofs_core::{names, Bus, Scheduler};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_physics::propeller::{PropParams, Propeller};

const D: f64 = 0.127;
const CP: f64 = 0.045;
const J0: f64 = 1.0;
const KV: f64 = 2000.0;
const NO_LOAD_A: f64 = 0.05;

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus().lookup::<f64>(name).unwrap();
    s.bus_mut().set(sig, v);
}

/// A prop + motor spun up at 60 % throttle for half a second, then killed (fault scale 0) with `inflow` m/s of air
/// coming up through the disc.
fn killed_in_a_descent(inflow: f64) -> Scheduler {
    killed_in_a_descent_with(inflow, J0)
}

/// `killed_in_a_descent` with the propeller's `windmill_j0` given (0 turns windmilling off).
fn killed_in_a_descent_with(inflow: f64, windmill_j0: f64) -> Scheduler {
    let mut bus = Bus::new();
    let prop = PropParams { diameter_m: D, ct_table: vec![(0.0, 0.11)], cp_table: vec![(0.0, CP)], windmill_j0 };
    let prop = Propeller::new(0, prop, &mut bus);
    let motor = MotorParams { kv_rpm_per_v: KV, resistance_ohm: 0.1, no_load_current_a: NO_LOAD_A, rotor_inertia_kgm2: 5e-6 };
    let esc = EscMotor::new(0, EscParams { response_tau_s: 0.002, current_limit_a: 40.0 }, motor, 5e-6, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(prop));
    s.add(Box::new(esc));
    set(&mut s, names::BATTERY_VOLTAGE, 16.0);
    set(&mut s, &names::motor_cmd(0), 0.6);
    s.run_for(0.5).unwrap();
    set(&mut s, &names::fault_motor_scale(0), 0.0);
    set(&mut s, &names::prop_inflow(0), inflow);
    s
}

/// The larger root of `D n² − (V/J0) n + κ = 0`, in rev/s: where the windmill drive `Cp ρ D⁴ n (V/J0 − D n) / 2π`
/// equals the friction torque `Ke · I0`, with `κ = 2π F / (ρ D⁴ Cp)`. None when the discriminant is negative.
fn windmill_speed_rev_s(inflow: f64) -> Option<f64> {
    let ke = 60.0 / (2.0 * PI * KV);
    let friction = ke * NO_LOAD_A;
    let kappa = 2.0 * PI * friction / (AIR_DENSITY_KGPM3 * D.powi(4) * CP);
    let v = inflow / J0;
    let disc = v * v - 4.0 * D * kappa;
    (disc >= 0.0).then(|| (v + disc.sqrt()) / (2.0 * D))
}

#[test]
fn an_unpowered_rotor_settles_at_the_analytic_windmill_speed() {
    let expected = 2.0 * PI * windmill_speed_rev_s(20.0).expect("20 m/s is above the windmilling threshold");
    let mut s = killed_in_a_descent(20.0);
    s.run_for(15.0).unwrap(); // the approach has a time constant of about 1.5 s (J / dQ/dω at the root)
    let omega = get(&s, &names::motor_omega(0));
    assert!(((omega - expected) / expected).abs() < 0.01, "omega {omega} expected {expected}");
}

#[test]
fn a_slow_descent_does_not_keep_the_rotor_turning() {
    assert!(windmill_speed_rev_s(5.0).is_none(), "5 m/s is below the threshold: no root");
    let mut s = killed_in_a_descent(5.0);
    s.run_for(40.0).unwrap(); // the drive nearly cancels friction near n = V/(2 D j0), so the last turns are slow
    assert_eq!(get(&s, &names::motor_omega(0)), 0.0);
}

#[test]
fn a_dying_motor_in_a_descent_changes_speed_without_a_step() {
    // The eRPM and ESC telemetry Betaflight reads follow the rotor speed: it must move smoothly through the kill.
    let mut s = killed_in_a_descent(20.0);
    let mut prev = get(&s, &names::motor_omega(0));
    let mut worst = 0.0f64;
    for _ in 0..8000 {
        s.step().unwrap();
        let now = get(&s, &names::motor_omega(0));
        worst = worst.max((now - prev).abs());
        prev = now;
    }
    // The real worst case is 0.16 rad/s per tick; a step on the kill would be the whole rotor speed (~2000 rad/s).
    assert!(worst < 0.4, "largest single-tick change {worst} rad/s");
    // And the air is what keeps it turning: with windmilling off, the same kill lets the rotor spin down faster.
    let windmilling = get(&s, &names::motor_omega(0));
    let mut off = killed_in_a_descent_with(20.0, 0.0);
    off.run_for(1.0).unwrap();
    let dead = get(&off, &names::motor_omega(0));
    assert!(windmilling > dead + 200.0, "windmilling {windmilling} rad/s vs without {dead} rad/s");
}
