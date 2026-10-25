//! The flight character of the pre-assembled quads: thrust-to-weight and hover throttle come out of the part files, so a
//! quad that is meant to be gentle (the toothpick), sharp (the race quad) or efficient (the long-range quad) is, and the
//! numbers in each quad file's header are the ones the running vehicle shows.
use std::path::Path;

use ofs_config::{load, FcKind, QuadConfig, WorldConfig};
use ofs_physics::propeller::{thrust_torque, PropParams};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const G: f64 = 9.81;

fn quad(stem: &str) -> QuadConfig {
    load(Path::new(&format!("{}/../../quads/{stem}.toml", env!("CARGO_MANIFEST_DIR")))).unwrap()
}

fn vehicle(cfg: &QuadConfig) -> Vehicle {
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(cfg, &opts, None).unwrap()
}

/// The total rotor thrust (N) once the motors have settled at this throttle, from the rpm the vehicle reports.
fn thrust_at(cfg: &QuadConfig, throttle: f64) -> f64 {
    let mut v = vehicle(cfg);
    v.set_sticks(&Sticks { throttle, ..Sticks::default() }).unwrap();
    v.run_for(0.6).unwrap();
    v.state()
        .motor_rpm
        .iter()
        .zip(&cfg.props)
        .map(|(rpm, p)| {
            let params = PropParams {
                diameter_m: p.diameter_m,
                ct_table: p.ct_table.iter().map(|r| (r[0], r[1])).collect(),
                cp_table: p.cp_table.iter().map(|r| (r[0], r[1])).collect(),
                windmill_j0: p.windmill_j0,
            };
            thrust_torque(&params, rpm * std::f64::consts::TAU / 60.0).0
        })
        .sum()
}

struct Character {
    twr: f64,
    hover: f64,
}

fn character(cfg: &QuadConfig) -> Character {
    let weight = cfg.frame.mass_kg * G;
    let twr = thrust_at(cfg, 1.0) / weight;
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if thrust_at(cfg, mid) < weight {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Character { twr, hover: (lo + hi) / 2.0 }
}

/// (quad, thrust-to-weight, hover throttle) with a full pack, sag included: what each quad file's header says.
const EXPECTED: [(&str, f64, f64); 3] = [("opendrone-3t-toothpick", 4.2, 0.44), ("opendrone-5r-race", 14.8, 0.19), ("opendrone-7lr-longrange", 6.3, 0.24)];

#[test]
fn each_new_quad_has_the_character_its_parts_explain() {
    for (stem, twr, hover) in EXPECTED {
        let c = character(&quad(stem));
        assert!((c.twr / twr - 1.0).abs() < 0.08, "{stem}: thrust-to-weight {:.1}, expected about {twr}", c.twr);
        assert!((c.hover - hover).abs() < 0.03, "{stem}: hover throttle {:.2}, expected about {hover}", c.hover);
    }
}

#[test]
fn the_quads_order_by_thrust_and_by_hover_throttle() {
    let [t, r, l] = EXPECTED.map(|(stem, ..)| character(&quad(stem)));
    let f = character(&quad("opendrone-5f-freestyle"));
    assert!(r.twr > f.twr && f.twr > l.twr && l.twr > t.twr, "race {} > freestyle {} > long-range {} > toothpick {}", r.twr, f.twr, l.twr, t.twr);
    assert!(t.hover > l.hover && l.hover > r.hover, "the toothpick works hardest to hover, the race quad least: {} {} {}", t.hover, l.hover, r.hover);
}

#[test]
fn a_new_quad_lifts_off_above_its_hover_throttle_and_stays_down_below_it() {
    for (stem, _, hover) in EXPECTED {
        let cfg = quad(stem);
        for (throttle, climbs) in [(hover + 0.12, true), (hover - 0.12, false)] {
            let mut v = vehicle(&cfg);
            v.set_sticks(&Sticks { throttle, ..Sticks::default() }).unwrap();
            v.run_for(1.0).unwrap();
            let height = -v.state().pos_ned_m.z;
            assert_eq!(height > 0.3, climbs, "{stem} at throttle {throttle:.2} is {height:.2} m up");
        }
    }
}
