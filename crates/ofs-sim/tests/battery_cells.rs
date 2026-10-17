//! The per-cell battery inside a whole vehicle (docs/research/battery-and-thermal.md).
use std::path::Path;

use ofs_config::part;
use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const AGED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library/batteries/opendrone-6s-1300-aged.toml");

fn vehicle(cfg: &QuadConfig) -> Vehicle {
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(cfg, &opts).unwrap()
}

fn full_throttle() -> Sticks {
    Sticks { throttle: 1.0, ..Sticks::default() }
}

#[test]
fn a_clean_pack_has_six_equal_cells() {
    let mut v = vehicle(&load(Path::new(QUAD)).unwrap());
    v.set_sticks(&full_throttle()).unwrap();
    v.run_for(0.5).unwrap();
    let s = v.state();
    assert_eq!(s.battery_cell_voltages.len(), 6);
    for c in &s.battery_cell_voltages {
        assert!((c - s.battery_voltage_v / 6.0).abs() < 1e-9, "{c} vs {}", s.battery_voltage_v / 6.0);
    }
}

#[test]
fn an_aged_pack_sags_more_in_flight() {
    let fresh_cfg = load(Path::new(QUAD)).unwrap();
    let mut aged_cfg = fresh_cfg.clone();
    aged_cfg.battery.cycles = 200;
    aged_cfg.battery.capacity_fade_per_cycle = 0.001;
    aged_cfg.battery.resistance_growth_per_cycle = 0.004;
    let (mut fresh, mut aged) = (vehicle(&fresh_cfg), vehicle(&aged_cfg));
    for v in [&mut fresh, &mut aged] {
        v.set_sticks(&full_throttle()).unwrap();
        v.run_for(1.0).unwrap();
    }
    let (f, a) = (fresh.state(), aged.state());
    assert!(a.battery_voltage_v < f.battery_voltage_v, "aged {} fresh {}", a.battery_voltage_v, f.battery_voltage_v);
    // 200 cycles at 0.1 % leave 80 % of the 1300 mAh: whatever charge the (sagging) pack delivered, the state of charge
    // fell by that over the capacity that is left. (The two packs draw different currents, so compare per pack.)
    let drained_mah = |v: &Vehicle, capacity_mah: f64| (1.0 - v.signal("battery.soc").unwrap()) * capacity_mah;
    let consumed = |v: &Vehicle| v.signal("battery.consumed_mah").unwrap();
    assert!((drained_mah(&fresh, 1300.0) - consumed(&fresh)).abs() < 1e-3 * consumed(&fresh), "fresh pack");
    assert!((drained_mah(&aged, 1300.0 * 0.8) - consumed(&aged)).abs() < 1e-3 * consumed(&aged), "aged pack");
}

#[test]
fn a_weak_cell_fault_lowers_exactly_that_cell() {
    let mut v = vehicle(&load(Path::new(QUAD)).unwrap());
    v.set_sticks(&full_throttle()).unwrap();
    v.set_fault(Fault::WeakCell { cell: 3, r_offset_ohm: 0.02, v_offset_v: 0.1 }, true).unwrap();
    v.run_for(0.2).unwrap();
    let s = v.state();
    let healthy = s.battery_cell_voltages[0];
    assert!(s.battery_cell_voltages[3] < healthy - 0.1, "cell 3 carries the fault: {:?}", s.battery_cell_voltages);
    for k in [0, 1, 2, 4, 5] {
        assert!((s.battery_cell_voltages[k] - healthy).abs() < 1e-9, "cell {k} is healthy");
    }
    v.set_fault(Fault::WeakCell { cell: 3, r_offset_ohm: 0.02, v_offset_v: 0.1 }, false).unwrap();
    v.run_for(0.01).unwrap();
    let s = v.state();
    for c in &s.battery_cell_voltages {
        assert!((c - s.battery_cell_voltages[0]).abs() < 1e-9, "equal again once the fault clears");
    }
}

#[test]
fn vehicle_signal_reads_the_bus() {
    let mut v = vehicle(&load(Path::new(QUAD)).unwrap());
    v.run_for(0.05).unwrap();
    assert_eq!(v.signal("battery.voltage_v"), Some(v.state().battery_voltage_v));
    assert_eq!(v.signal("no.such.signal"), None);
}

#[test]
fn the_aged_library_pack_loads() {
    let pack = part::load_battery(Path::new(AGED)).unwrap().battery;
    assert_eq!(pack.cycles, 150);
    assert_eq!(pack.cell_r0_scale.len(), pack.cells as usize);
}
