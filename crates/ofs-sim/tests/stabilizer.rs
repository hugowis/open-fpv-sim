//! The open-loop stabilizer on the whole vehicle: sticks in, attitude and motion out.
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_fc::stabilizer::{MAX_ANGLE_DEG, MAX_YAW_RATE_DPS};
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const DT: f64 = 0.02;

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions {
        seed: 1,
        data_dir: std::env::temp_dir().join("ofs-test-data"),
        fc_override: Some(FcKind::OpenLoopStabilized),
        world: WorldConfig::open_field(),
    };
    build(&cfg, &opts).unwrap()
}

/// A pilot's altitude loop (the numbers of `python/examples/hover.py`), so the tests can ask for attitudes without the
/// quad climbing away or sinking.
struct Altitude {
    integral: f64,
}

impl Altitude {
    fn new() -> Self {
        Altitude { integral: 0.0 }
    }

    fn throttle(&mut self, v: &Vehicle, target_m: f64) -> f64 {
        let s = v.state();
        let err = target_m - (-s.pos_ned_m.z);
        self.integral = (self.integral + err * DT).clamp(-2.0, 2.0);
        (0.35 + 0.15 * err + 0.10 * self.integral + 0.10 * s.vel_ned_mps.z).clamp(0.0, 1.0)
    }
}

/// Steps `seconds` of simulated time holding 1 m, with these roll, pitch and yaw sticks; calls `each` after every step.
fn fly(v: &mut Vehicle, alt: &mut Altitude, roll: f64, pitch: f64, yaw: f64, seconds: f64, mut each: impl FnMut(&Vehicle)) {
    for _ in 0..(seconds / DT).round() as u32 {
        let throttle = alt.throttle(v, 1.0);
        v.set_sticks(&Sticks { roll, pitch, yaw, throttle, aux: [0.0; 4] }).unwrap();
        v.run_for(DT).unwrap();
        each(v);
    }
}

fn euler_deg(v: &Vehicle) -> (f64, f64, f64) {
    let (r, p, y) = ofs_fc::stabilizer::euler_zyx(v.state().att);
    (r.to_degrees(), p.to_degrees(), y.to_degrees())
}

/// A vehicle that has climbed to 1 m and settled.
fn hovering() -> (Vehicle, Altitude) {
    let mut v = vehicle();
    let mut alt = Altitude::new();
    fly(&mut v, &mut alt, 0.0, 0.0, 0.0, 6.0, |_| {});
    (v, alt)
}

#[test]
fn level_sticks_hold_the_attitude() {
    let (mut v, mut alt) = hovering();
    let yaw0 = euler_deg(&v).2;
    let (mut max_tilt, mut max_yaw) = (0.0f64, 0.0f64);
    fly(&mut v, &mut alt, 0.0, 0.0, 0.0, 5.0, |v| {
        let (r, p, y) = euler_deg(v);
        max_tilt = max_tilt.max(r.abs()).max(p.abs());
        max_yaw = max_yaw.max((y - yaw0).abs());
    });
    assert!(max_tilt < 1.0, "tilt {max_tilt} degrees");
    assert!(max_yaw < 2.0, "yaw drift {max_yaw} degrees");
}

#[test]
fn a_roll_stick_leans_the_quad_to_the_commanded_angle_and_stays() {
    let (mut v, mut alt) = hovering();
    let target = 0.3 * MAX_ANGLE_DEG;
    let (mut at_half_second, mut peak, mut late) = (0.0, 0.0f64, Vec::new());
    let mut t = 0.0;
    fly(&mut v, &mut alt, 0.3, 0.0, 0.0, 2.5, |v| {
        t += DT;
        let roll = euler_deg(v).0;
        peak = peak.max(roll);
        if (t - 0.5).abs() < DT / 2.0 {
            at_half_second = roll;
        }
        if t > 1.5 {
            late.push(roll);
        }
    });
    assert!((at_half_second - target).abs() < 0.1 * target, "roll {at_half_second} at 0.5 s, wanted {target}");
    assert!(peak < 1.25 * target, "overshoot to {peak} for {target}");
    assert!(late.iter().all(|r| (r - target).abs() < 2.0), "settled within 2 degrees: {late:?}");
}

#[test]
fn the_stick_signs_move_the_quad_the_way_a_pilot_expects() {
    let (mut east, mut alt) = hovering();
    fly(&mut east, &mut alt, 0.2, 0.0, 0.0, 2.0, |_| {});
    let v = east.state().vel_ned_mps;
    assert!(v.y > 0.5 && v.x.abs() < 0.2 * v.y, "roll + goes east at heading north: {v}");

    let (mut north, mut alt) = hovering();
    fly(&mut north, &mut alt, 0.0, 0.2, 0.0, 2.0, |_| {});
    let v = north.state().vel_ned_mps;
    assert!(v.x > 0.5 && v.y.abs() < 0.2 * v.x, "pitch + goes north: {v}");
    assert!(euler_deg(&north).1 < -3.0, "with the nose down: {}", euler_deg(&north).1);

    let (mut turn, mut alt) = hovering();
    let y0 = euler_deg(&turn).2;
    let mut y_half = y0;
    let mut t = 0.0;
    fly(&mut turn, &mut alt, 0.0, 0.0, 0.3, 1.0, |v| {
        t += DT;
        if (t - 0.5).abs() < DT / 2.0 {
            y_half = euler_deg(v).2;
        }
    });
    let turned = euler_deg(&turn).2 - y_half;
    let wanted = 0.3 * MAX_YAW_RATE_DPS * 0.5;
    assert!((turned - wanted).abs() < 0.15 * wanted, "yaw + turns clockwise at the stick's rate: {turned} degrees in 0.5 s, wanted {wanted}");
}

#[test]
fn a_quad_on_the_ground_with_no_throttle_stays_dark() {
    let mut v = vehicle();
    v.run_for(0.5).unwrap(); // settle onto the collision spheres, as resting_quad_stays_put does
    let start = v.state().pos_ned_m;
    v.set_sticks(&Sticks { roll: 1.0, pitch: 1.0, yaw: 1.0, throttle: 0.0, aux: [0.0; 4] }).unwrap();
    v.run_for(1.0).unwrap();
    let s = v.state();
    assert_eq!(s.motor_cmd, vec![0.0; 4]);
    assert!((s.pos_ned_m - start).length() < 0.01, "moved to {}", s.pos_ned_m);
}

#[test]
fn full_sticks_and_full_throttle_stay_finite_and_bounded() {
    let mut v = vehicle();
    v.set_sticks(&Sticks { roll: 1.0, pitch: 1.0, yaw: 1.0, throttle: 1.0, aux: [0.0; 4] }).unwrap();
    for _ in 0..200 {
        v.run_for(0.01).unwrap();
        let s = v.state();
        assert!(s.motor_cmd.iter().all(|c| (0.0..=1.0).contains(c)), "{:?}", s.motor_cmd);
        assert!(s.pos_ned_m.is_finite() && s.vel_ned_mps.is_finite());
    }
}

#[test]
fn the_same_seed_and_sticks_give_the_same_run() {
    let run = || {
        let mut v = vehicle();
        let mut alt = Altitude::new();
        fly(&mut v, &mut alt, 0.0, 0.0, 0.0, 3.0, |_| {});
        fly(&mut v, &mut alt, 0.25, -0.1, 0.2, 2.0, |_| {});
        (v.state(), v.digest())
    };
    assert_eq!(run(), run());
}
