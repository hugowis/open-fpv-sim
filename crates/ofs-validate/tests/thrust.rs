//! The thrust stand: the simulator's own ESC, motor and propeller held at a measured command and voltage until they
//! settle, compared here with the analytic steady state.
use std::f64::consts::PI;
use std::path::Path;

use ofs_core::consts::AIR_DENSITY_KGPM3 as RHO;
use ofs_validate::submission::ThrustRow;
use ofs_validate::thrust::simulate;

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn row(command: f64, voltage_v: f64) -> ThrustRow {
    ThrustRow { command, voltage_v, rpm: None, thrust_n: None, current_a: None }
}

fn quad() -> ofs_config::QuadConfig {
    ofs_config::load(Path::new(QUAD)).unwrap()
}

/// Steady state at duty = `command`, solved by bisection from the part values: the motor torque Ke(I − I0) equals
/// the propeller torque Cp ρ n² D⁵ / 2π, with I = (duty·V − Ke·ω)/R clamped to the ESC limit.
fn analytic(cfg: &ofs_config::QuadConfig, command: f64, voltage_v: f64) -> (f64, f64, f64) {
    let (m, p, e) = (&cfg.motors[0], &cfg.props[0], &cfg.escs[0]);
    let ke = 60.0 / (2.0 * PI * m.kv_rpm_per_v);
    let (ct, cp, d) = (p.ct_table[0][1], p.cp_table[0][1], p.diameter_m);
    let current = |w: f64| ((command * voltage_v - ke * w) / m.resistance_ohm).clamp(-e.current_limit_a, e.current_limit_a);
    let net = |w: f64| {
        let n = w / (2.0 * PI);
        ke * current(w) - ke * m.no_load_current_a - cp * RHO * n * n * d.powi(5) / (2.0 * PI)
    };
    let (mut lo, mut hi) = (0.0, command * voltage_v / ke);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if net(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let w = 0.5 * (lo + hi);
    let n = w / (2.0 * PI);
    (w * 60.0 / (2.0 * PI), ct * RHO * n * n * d.powi(4), command * current(w))
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() <= 1e-3 * b.abs().max(1e-9), "{what}: simulated {a} vs analytic {b}");
}

#[test]
fn the_stand_matches_the_analytic_steady_state() {
    let cfg = quad();
    for (command, volts) in [(1.0, 24.0), (0.6, 23.4), (0.25, 24.0)] {
        let sim = simulate(&cfg, 0, &row(command, volts)).unwrap();
        let (rpm, thrust, amps) = analytic(&cfg, command, volts);
        close(sim.rpm, rpm, "rpm");
        close(sim.thrust_n, thrust, "thrust");
        close(sim.current_a, amps, "battery current");
        assert!(sim.settle_s < 10.0);
    }
}

#[test]
fn a_zero_command_gives_zero_everything() {
    let sim = simulate(&quad(), 0, &row(0.0, 24.0)).unwrap();
    assert_eq!((sim.rpm, sim.thrust_n, sim.current_a), (0.0, 0.0, 0.0));
}

#[test]
fn a_row_at_the_current_limit_gets_a_prediction_inside_the_limit() {
    let mut cfg = quad();
    cfg.escs[0].current_limit_a = 5.0;
    let sim = simulate(&cfg, 0, &row(1.0, 24.0)).unwrap();
    assert!(sim.current_a <= 5.0 + 1e-9, "{sim:?}");
    let (rpm, _, amps) = analytic(&cfg, 1.0, 24.0);
    close(sim.rpm, rpm, "rpm");
    close(sim.current_a, amps, "battery current");
}

#[test]
fn a_rotor_that_never_settles_is_reported_instead_of_looped_on() {
    let mut cfg = quad();
    cfg.props[0].inertia_kgm2 = 1.0e3; // a flywheel: still spinning up after ten seconds
    let err = simulate(&cfg, 0, &row(1.0, 24.0)).unwrap_err();
    assert!(err.contains("no steady state within 10 s"), "{err}");
}

#[test]
fn a_motor_the_quad_lacks_is_an_error_not_a_panic() {
    let err = simulate(&quad(), 7, &row(1.0, 24.0)).unwrap_err();
    assert!(err.contains("motor 7"), "{err}");
}
