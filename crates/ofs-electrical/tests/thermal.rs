//! The thermal model: exact RC nodes fed by the losses already on the bus (docs/research/battery-and-thermal.md).
use ofs_core::{names, Bus, Scheduler};
use ofs_electrical::thermal::{
    EscThermalParams, MotorThermalParams, NodeParams, Thermal, ThermalNode, ThermalParams, VtxThermalParams,
};

fn node(c: f64, r: f64) -> NodeParams {
    NodeParams { heat_capacity_jpk: c, resistance_kpw: r }
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus().lookup::<f64>(name).unwrap();
    s.bus_mut().set(sig, v);
}

/// A thermal model at 100 Hz (divisor 80 at 8 kHz) on its own bus.
fn rig(p: ThermalParams) -> Scheduler {
    let mut bus = Bus::new();
    let model = Thermal::new(p, 80, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(model));
    s
}

fn params(ambient_c: f64) -> ThermalParams {
    ThermalParams { ambient_c, esc: vec![None], motor: vec![None], vtx: None }
}

fn esc() -> EscThermalParams {
    EscThermalParams { node: node(3.0, 8.0), r_on_ohm: 0.004, switching_loss_coeff: 0.0024 }
}

fn motor() -> MotorThermalParams {
    MotorThermalParams { node: node(15.0, 1.5), winding_resistance_ohm: 0.07, friction_torque_nm: 0.005 }
}

fn vtx() -> VtxThermalParams {
    VtxThermalParams { node: node(2.0, 15.0), efficiency: 0.25, idle_w: 0.4 }
}

#[test]
fn a_node_follows_the_rc_step_response() {
    // C = 2 J/K, R = 10 K/W: tau = 20 s; 5 W into 25 degC ambient settles at 75.
    let mut n = ThermalNode::new(node(2.0, 10.0), 25.0);
    let mut t = 25.0;
    for _ in 0..2000 {
        t = n.step(5.0, 25.0, 0.01);
    }
    assert!((t - (25.0 + 50.0 * (1.0 - (-1.0f64).exp()))).abs() < 1e-9, "after one time constant: {t}");
    for _ in 0..38_000 {
        t = n.step(5.0, 25.0, 0.01);
    }
    assert!((t - 75.0).abs() < 1e-6, "after twenty: {t}");
}

#[test]
fn the_step_size_does_not_change_the_answer() {
    let mut one = ThermalNode::new(node(2.0, 10.0), 25.0);
    let mut many = ThermalNode::new(node(2.0, 10.0), 25.0);
    let a = one.step(5.0, 25.0, 20.0);
    let mut b = 0.0;
    for _ in 0..2000 {
        b = many.step(5.0, 25.0, 0.01);
    }
    assert!((a - b).abs() < 1e-9, "{a} vs {b}");
}

#[test]
fn an_esc_settles_at_the_analytic_temperature() {
    let mut s = rig(ThermalParams { esc: vec![Some(esc())], ..params(25.0) });
    set(&mut s, &names::motor_current(0), 20.0);
    set(&mut s, names::BATTERY_VOLTAGE, 24.0);
    s.run_for(300.0).unwrap();
    let power = 20.0 * 20.0 * 0.004 + 0.0024 * 24.0 * 20.0; // conduction + switching
    let want = 25.0 + power * 8.0;
    assert!((get(&s, &names::esc_temp(0)) - want).abs() < 1e-3, "{} vs {want}", get(&s, &names::esc_temp(0)));
}

#[test]
fn a_motor_settles_at_the_analytic_temperature() {
    let mut s = rig(ThermalParams { motor: vec![Some(motor())], ..params(25.0) });
    set(&mut s, &names::motor_current(0), 20.0);
    set(&mut s, &names::motor_omega(0), 1000.0);
    s.run_for(300.0).unwrap();
    let power = 20.0 * 20.0 * 0.07 + 0.005 * 1000.0; // winding + friction
    let want = 25.0 + power * 1.5;
    assert!((get(&s, &names::motor_temp(0)) - want).abs() < 1e-3, "{} vs {want}", get(&s, &names::motor_temp(0)));
}

#[test]
fn the_vtx_heats_by_its_dissipation() {
    let mut s = rig(ThermalParams { vtx: Some(vtx()), ..params(25.0) });
    set(&mut s, names::VTX_PRESENT, 1.0);
    set(&mut s, names::VTX_POWER_MW, 1000.0);
    s.run_for(400.0).unwrap();
    let power = 1.0 * (1.0 / 0.25 - 1.0) + 0.4;
    assert!((get(&s, names::VTX_TEMP_C) - (25.0 + power * 15.0)).abs() < 1e-3, "{}", get(&s, names::VTX_TEMP_C));
}

#[test]
fn a_vtx_with_no_output_power_heats_only_by_its_idle_draw() {
    let mut s = rig(ThermalParams { vtx: Some(vtx()), ..params(25.0) });
    set(&mut s, names::VTX_PRESENT, 1.0);
    set(&mut s, names::VTX_POWER_MW, 0.0);
    s.run_for(400.0).unwrap();
    assert!((get(&s, names::VTX_TEMP_C) - (25.0 + 0.4 * 15.0)).abs() < 1e-3);
}

#[test]
fn a_vtx_in_pit_mode_heats_only_by_its_idle_draw() {
    // `vtx.power_mw` keeps the selected level's power in pit mode (the link model applies the pit power), so the
    // thermal model must look at the pit flag.
    let mut s = rig(ThermalParams { vtx: Some(vtx()), ..params(25.0) });
    set(&mut s, names::VTX_PRESENT, 1.0);
    set(&mut s, names::VTX_POWER_MW, 1000.0);
    set(&mut s, names::VTX_PIT, 1.0);
    s.run_for(400.0).unwrap();
    assert!((get(&s, names::VTX_TEMP_C) - (25.0 + 0.4 * 15.0)).abs() < 1e-3);
}

#[test]
fn an_absent_vtx_stays_at_ambient() {
    let mut s = rig(ThermalParams { vtx: Some(vtx()), ..params(25.0) });
    set(&mut s, names::VTX_PRESENT, 0.0);
    set(&mut s, names::VTX_POWER_MW, 1000.0);
    s.run_for(60.0).unwrap();
    assert_eq!(get(&s, names::VTX_TEMP_C), 25.0);
}

#[test]
fn idle_stays_exactly_at_ambient() {
    for ambient in [25.0, 40.0] {
        let mut s = rig(ThermalParams { esc: vec![Some(esc())], motor: vec![Some(motor())], vtx: Some(vtx()), ambient_c: ambient });
        s.run_for(60.0).unwrap();
        assert_eq!(get(&s, &names::esc_temp(0)), ambient);
        assert_eq!(get(&s, &names::motor_temp(0)), ambient);
        assert_eq!(get(&s, names::VTX_TEMP_C), ambient);
    }
}

#[test]
fn a_part_without_thermal_data_never_leaves_ambient() {
    let mut s = rig(params(25.0));
    set(&mut s, &names::motor_current(0), 40.0);
    set(&mut s, &names::motor_omega(0), 2000.0);
    set(&mut s, names::BATTERY_VOLTAGE, 24.0);
    s.run_for(60.0).unwrap();
    assert_eq!(get(&s, &names::esc_temp(0)), 25.0);
    assert_eq!(get(&s, &names::motor_temp(0)), 25.0);
}
