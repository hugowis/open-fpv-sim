//! The thermal model inside a whole vehicle (docs/research/battery-and-thermal.md).
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn vehicle_in(world: WorldConfig) -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world };
    build(&cfg, &opts).unwrap()
}

fn throttle(t: f64) -> Sticks {
    Sticks { throttle: t, ..Sticks::default() }
}

fn temp(v: &Vehicle, name: &str) -> f64 {
    v.signal(name).unwrap_or_else(|| panic!("{name} is not on the bus"))
}

#[test]
fn a_quad_at_rest_heats_only_its_vtx() {
    let mut v = vehicle_in(WorldConfig::open_field());
    v.run_for(30.0).unwrap();
    assert_eq!(temp(&v, "esc.0.temp_c"), 25.0, "no current, no loss: exactly ambient");
    assert_eq!(temp(&v, "motor.0.temp_c"), 25.0);
    // The VTX transmits from power-up: its idle draw plus the RF it radiates, at 25 % efficiency, into 15 K/W and
    // 2 J/K (a 30 s time constant): after 30 s it has covered 1 − 1/e of the way to its steady temperature.
    let rf_w = temp(&v, "vtx.power_mw") / 1000.0;
    let steady = 25.0 + (rf_w * (1.0 / 0.25 - 1.0) + 0.4) * 15.0;
    let want = 25.0 + (steady - 25.0) * (1.0 - (-1.0f64).exp());
    assert!((temp(&v, "vtx.temp_c") - want).abs() < 0.05, "vtx {} vs analytic {want}", temp(&v, "vtx.temp_c"));
}

#[test]
fn full_throttle_heats_the_esc_and_the_motor_and_they_cool_again() {
    let mut v = vehicle_in(WorldConfig::open_field());
    v.set_sticks(&throttle(1.0)).unwrap();
    // The pack is nearly flat after twenty seconds of this, so look at the peak on the way.
    let (mut esc, mut motor) = (0.0f64, 0.0f64);
    for _ in 0..40 {
        v.run_for(0.5).unwrap();
        esc = esc.max(temp(&v, "esc.0.temp_c"));
        motor = motor.max(temp(&v, "motor.0.temp_c"));
    }
    // With the shipped (estimated) constants and this model's phase currents the winding gains about 20 K and the
    // ESC a couple: the assertions are the direction and a sane bound, not the constants.
    assert!(esc > 26.0 && esc < 255.0, "esc {esc}");
    assert!(motor > 35.0 && motor < 255.0, "motor {motor}");
    v.set_sticks(&throttle(0.0)).unwrap();
    v.run_for(60.0).unwrap();
    assert!(temp(&v, "esc.0.temp_c") < esc - 1.0, "the ESC cools once the load is gone");
    assert!(temp(&v, "motor.0.temp_c") < motor - 1.0, "so does the motor");
}

#[test]
fn a_warmer_world_starts_warmer() {
    let mut world = WorldConfig::open_field();
    world.environment.ambient_c = 35.0;
    let mut v = vehicle_in(world);
    v.run_for(5.0).unwrap();
    assert_eq!(temp(&v, "esc.0.temp_c"), 35.0);
    assert_eq!(temp(&v, "motor.0.temp_c"), 35.0);
}
