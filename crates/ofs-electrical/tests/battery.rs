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
        cell_r0_scale: vec![],
        cell_capacity_scale: vec![],
        capacity_factor: 1.0,
        resistance_factor: 1.0,
    }
}

fn cell(s: &Scheduler, k: usize) -> f64 {
    get(s, &names::battery_cell_voltage(k))
}

fn cell_soc(s: &Scheduler, k: usize) -> f64 {
    get(s, &names::battery_cell_soc(k))
}

fn set_fault(s: &mut Scheduler, weak_cell: f64, r_offset: f64, v_offset: f64) {
    let bus = s.bus_mut();
    for (name, value) in [
        (names::FAULT_BATTERY_WEAK_CELL, weak_cell),
        (names::FAULT_BATTERY_R_OFFSET_OHM, r_offset),
        (names::FAULT_BATTERY_V_OFFSET_V, v_offset),
    ] {
        let sig = bus.signal::<f64>(name);
        bus.set(sig, value);
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
fn a_weak_cell_deepens_the_sag_exactly_by_its_offsets() {
    let mut clean = sim(params());
    load(&mut clean, 10.0);
    clean.step().unwrap();
    let mut weak = sim(params());
    {
        let bus = weak.bus_mut();
        for (name, value) in [
            (names::FAULT_BATTERY_R_OFFSET_OHM, 0.02),
            (names::FAULT_BATTERY_V_OFFSET_V, 0.1),
            (names::FAULT_BATTERY_WEAK_CELL, 1.0),
        ] {
            let sig = bus.signal::<f64>(name);
            bus.set(sig, value);
        }
    }
    load(&mut weak, 10.0);
    weak.step().unwrap();
    let drop = get(&clean, names::BATTERY_VOLTAGE) - get(&weak, names::BATTERY_VOLTAGE);
    assert!((drop - (10.0 * 0.02 + 0.1)).abs() < 1e-9, "sag deepens by I·Δr + Δv, got {drop}");
    // The Thevenin state is untouched: the same SoC and consumed charge.
    assert_eq!(get(&weak, names::BATTERY_SOC), get(&clean, names::BATTERY_SOC));
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

#[test]
fn uniform_cells_reproduce_the_pack_model() {
    // The pack Thevenin in closed form: 6·OCV(soc) − I·R0 − I·R1·(1 − e^(−t/τ)), τ = R1·C1 = 20 s.
    let mut s = sim(params());
    load(&mut s, 10.0);
    s.run_for(20.0).unwrap();
    let soc = 1.0 - 10.0 * 20.0 / (1300.0 * 3.6);
    let expected = 6.0 * interp::linear(&lipo(), soc) - 10.0 * 0.024 - 10.0 * 0.010 * (1.0 - (-1.0f64).exp());
    let pack = get(&s, names::BATTERY_VOLTAGE);
    assert!((pack - expected).abs() < 1e-9, "pack {pack} expected {expected}");
    for k in 0..6 {
        assert!((cell(&s, k) - pack / 6.0).abs() < 1e-9, "cell {k}: {} vs {}", cell(&s, k), pack / 6.0);
    }
}

#[test]
fn the_pack_is_the_series_sum_of_its_cells() {
    let mut p = params();
    p.cell_r0_scale = vec![1.0, 1.0, 1.0, 1.0, 1.0, 2.0];
    let mut s = sim(p);
    load(&mut s, 10.0);
    s.step().unwrap();
    // Each cell's R0 is a sixth of the pack's (0.004 ohm) times its scale; the polarisation drop is tiny after one tick.
    for k in 0..6 {
        let scale = if k == 5 { 2.0 } else { 1.0 };
        let expected = interp::linear(&lipo(), 1.0) - 10.0 * 0.004 * scale;
        assert!((cell(&s, k) - expected).abs() < 1e-5, "cell {k}: {} vs {expected}", cell(&s, k));
    }
    assert!((cell(&s, 0) - cell(&s, 5) - 10.0 * 0.004).abs() < 1e-12, "the doubled cell sags by one more cell resistance");
    let sum: f64 = (0..6).map(|k| cell(&s, k)).sum();
    assert!((get(&s, names::BATTERY_VOLTAGE) - sum).abs() < 1e-12);
}

#[test]
fn a_smaller_cell_drains_faster() {
    let mut p = params();
    p.cell_capacity_scale = vec![0.5, 1.0, 1.0, 1.0, 1.0, 1.0];
    let mut s = sim(p);
    load(&mut s, 10.0);
    s.run_for(100.0).unwrap();
    let small = 1.0 - 10.0 * 100.0 / (1300.0 * 0.5 * 3.6);
    let normal = 1.0 - 10.0 * 100.0 / (1300.0 * 3.6);
    assert!((cell_soc(&s, 0) - small).abs() < 1e-9, "{} vs {small}", cell_soc(&s, 0));
    for k in 1..6 {
        assert!((cell_soc(&s, k) - normal).abs() < 1e-9, "cell {k}");
    }
    let mean = (0..6).map(|k| cell_soc(&s, k)).sum::<f64>() / 6.0;
    assert!((get(&s, names::BATTERY_SOC) - mean).abs() < 1e-12, "the pack soc is the mean of the cells");
    assert!((get(&s, names::BATTERY_SOC) - (small + 5.0 * normal) / 6.0).abs() < 1e-9);
    assert!((get(&s, names::BATTERY_CONSUMED) - 10.0 * 100.0 / 3.6).abs() < 1e-6, "the pack's consumption ignores the spread");
}

#[test]
fn fade_and_growth_scale_the_pack() {
    let mut fresh = sim(params());
    let mut grown = sim(BatteryParams { resistance_factor: 1.5, ..params() });
    load(&mut fresh, 10.0);
    load(&mut grown, 10.0);
    fresh.step().unwrap();
    grown.step().unwrap();
    assert!((cell(&fresh, 0) - cell(&grown, 0) - 10.0 * 0.004 * 0.5).abs() < 1e-9, "R0 grows by 50 %");
    let mut aged = sim(BatteryParams { capacity_factor: 0.8, ..params() });
    load(&mut aged, 10.0);
    aged.step().unwrap(); // the same one tick `fresh` has already run
    fresh.run_for(100.0).unwrap();
    aged.run_for(100.0).unwrap();
    let drop_fresh = 1.0 - cell_soc(&fresh, 0);
    let drop_aged = 1.0 - cell_soc(&aged, 0);
    assert!((drop_aged - drop_fresh / 0.8).abs() < 1e-9, "a pack at 80 % capacity drains a quarter faster");
}

#[test]
fn the_weak_cell_fault_lands_on_the_named_cell() {
    let mut clean = sim(params());
    load(&mut clean, 10.0);
    clean.step().unwrap();
    let mut weak = sim(params());
    set_fault(&mut weak, 3.0, 0.02, 0.1); // cell index 2
    load(&mut weak, 10.0);
    weak.step().unwrap();
    assert!((cell(&clean, 2) - cell(&weak, 2) - (10.0 * 0.02 + 0.1)).abs() < 1e-12);
    for k in [0, 1, 3, 4, 5] {
        assert_eq!(cell(&clean, k), cell(&weak, k), "cell {k} is untouched");
    }
    // No cell named: the offsets do nothing.
    let mut nobody = sim(params());
    set_fault(&mut nobody, 0.0, 0.02, 0.1);
    load(&mut nobody, 10.0);
    nobody.step().unwrap();
    assert_eq!(get(&nobody, names::BATTERY_VOLTAGE), get(&clean, names::BATTERY_VOLTAGE));
    // Cleared: back to the clean numbers exactly.
    set_fault(&mut weak, 0.0, 0.0, 0.0);
    clean.run_for(0.002).unwrap(); // the battery runs every 8th tick: two periods
    weak.run_for(0.002).unwrap();
    for k in 0..6 {
        assert_eq!(cell(&clean, k), cell(&weak, k), "cell {k} after the fault clears");
    }
}

#[test]
fn an_empty_cell_stays_finite() {
    let mut p = params();
    p.cell_capacity_scale = vec![0.001, 1.0, 1.0, 1.0, 1.0, 1.0];
    let mut s = sim(p);
    load(&mut s, 10.0);
    s.run_for(5.0).unwrap();
    assert_eq!(cell_soc(&s, 0), 0.0);
    for k in 0..6 {
        assert!(cell(&s, k).is_finite() && cell(&s, k) > 0.0, "cell {k}: {}", cell(&s, k));
    }
    assert!((1..6).all(|k| cell(&s, 0) < cell(&s, k)), "the empty cell is the lowest");
    assert!(get(&s, names::BATTERY_VOLTAGE).is_finite());
}
