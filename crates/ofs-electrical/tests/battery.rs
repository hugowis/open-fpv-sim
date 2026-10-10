use ofs_core::{interp, names, Bus, Scheduler};
use ofs_electrical::battery::{Battery, BatteryParams};

fn lipo() -> Vec<(f64, f64)> {
    vec![
        (0.0, 3.27), (0.05, 3.61), (0.1, 3.69), (0.2, 3.73), (0.3, 3.77), (0.4, 3.79),
        (0.5, 3.82), (0.6, 3.87), (0.7, 3.92), (0.8, 3.97), (0.9, 4.06), (1.0, 4.20),
    ]
}

fn params() -> BatteryParams {
    BatteryParams {
        cells: 6,
        capacity_mah: 1300.0,
        r0_ohm: 0.024,
        r1_ohm: 0.010,
        c1_f: 2000.0,
        ocv_table: lipo(),
        initial_soc: 1.0,
    }
}

fn sim(p: BatteryParams) -> Scheduler {
    let mut bus = Bus::new();
    let battery = Battery::new(p, 4, 8, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(battery));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn load(s: &mut Scheduler, amps: f64) {
    let sig = s.bus().lookup::<f64>(&names::esc_bus_current(0)).unwrap();
    s.bus_mut().set(sig, amps);
}

#[test]
fn open_circuit_voltage_is_cells_times_ocv() {
    let mut s = sim(params());
    assert!((get(&s, names::BATTERY_VOLTAGE) - 25.2).abs() < 1e-12);
    s.run_for(0.01).unwrap();
    assert!((get(&s, names::BATTERY_VOLTAGE) - 25.2).abs() < 1e-12);
}

#[test]
fn load_step_sags_instantly_then_relaxes() {
    let mut s = sim(params());
    load(&mut s, 10.0);
    s.step().unwrap();
    assert!((get(&s, names::BATTERY_VOLTAGE) - (25.2 - 0.24)).abs() < 1e-3);
    s.run_for(100.0).unwrap();
    let soc = 1.0 - 10.0 * 100.0 / (1300.0 * 3.6);
    let expected = 6.0 * interp::linear(&lipo(), soc) - 0.24 - 0.1 * (1.0 - (-5.0f64).exp());
    assert!((get(&s, names::BATTERY_SOC) - soc).abs() < 1e-4);
    assert!((get(&s, names::BATTERY_VOLTAGE) - expected).abs() < 1e-3);
    assert_eq!(get(&s, names::BATTERY_CURRENT), 10.0);
}

#[test]
fn consumed_capacity_integrates_current() {
    let mut s = sim(params());
    load(&mut s, 10.0);
    s.run_for(360.0).unwrap();
    assert!((get(&s, names::BATTERY_CONSUMED) - 1000.0).abs() < 0.01);
}

#[test]
fn empty_battery_clamps_state_of_charge() {
    let mut p = params();
    p.capacity_mah = 1.0;
    let mut s = sim(p);
    load(&mut s, 10.0);
    s.run_for(1.0).unwrap();
    assert_eq!(get(&s, names::BATTERY_SOC), 0.0);
    assert!(get(&s, names::BATTERY_VOLTAGE).is_finite());
}

#[test]
#[should_panic(expected = "battery.ocv_table")]
fn an_empty_ocv_table_is_refused_when_the_model_is_built() {
    sim(BatteryParams { ocv_table: vec![], ..params() });
}

#[test]
#[should_panic(expected = "battery.ocv_table")]
fn an_unsorted_ocv_table_is_refused_when_the_model_is_built() {
    sim(BatteryParams { ocv_table: vec![(1.0, 4.2), (0.0, 3.3)], ..params() });
}
