use std::f64::consts::{PI, TAU};

use glam::{DMat3, DQuat, DVec3};
use ofs_core::consts::{DEFAULT_MAG_DECLINATION_DEG, DEFAULT_MAG_FIELD_UT, DEFAULT_MAG_INCLINATION_DEG};
use ofs_core::{names, Bus, Scheduler};
use ofs_sensors::mag::{earth_field_ned, magnetic_heading, soft_iron_from_rows, true_heading, Mag, MagParams};

/// Smallest signed difference of two angles.
fn angle_diff(a: f64, b: f64) -> f64 {
    (a - b + PI).rem_euclid(TAU) - PI
}

fn earth() -> DVec3 {
    earth_field_ned(DEFAULT_MAG_FIELD_UT, DEFAULT_MAG_INCLINATION_DEG, DEFAULT_MAG_DECLINATION_DEG)
}

/// FRD -> NED attitude from aerospace yaw, pitch, roll (Z-Y-X).
fn att(yaw: f64, pitch: f64, roll: f64) -> DQuat {
    DQuat::from_rotation_z(yaw) * DQuat::from_rotation_y(pitch) * DQuat::from_rotation_x(roll)
}

#[test]
fn the_earth_field_follows_inclination_and_declination() {
    // Horizontal field pointing north, no dip.
    assert!((earth_field_ned(40.0, 0.0, 0.0) - DVec3::new(40.0, 0.0, 0.0)).length() < 1e-12);
    // Straight down at the magnetic pole.
    assert!((earth_field_ned(60.0, 90.0, 0.0) - DVec3::new(0.0, 0.0, 60.0)).length() < 1e-12);
    // 90 degrees east declination: the horizontal field points east; 45 degrees dip splits it with the vertical.
    let b = earth_field_ned(10.0, 45.0, 90.0);
    let h = 10.0 * 45f64.to_radians().cos();
    assert!((b - DVec3::new(0.0, h, h)).length() < 1e-12, "{b}");
    // Negative inclination points up (southern hemisphere), and the magnitude is the total.
    let b = earth_field_ned(50.0, -30.0, 12.0);
    assert!(b.z < 0.0);
    assert!((b.length() - 50.0).abs() < 1e-12);
}

#[test]
fn the_default_field_has_the_direction_sitl_synthesises() {
    // third_party/betaflight: sitl.c's virtual mag, NWU (north, west, up) counts of 4096; here as NED (north, east, down).
    let sitl = DVec3::new(2046.8, 71.5, 3547.2).normalize();
    assert!((earth().normalize() - sitl).length() < 1e-4, "{} vs {sitl}", earth().normalize());
}

#[test]
fn a_level_compass_reads_yaw_minus_declination() {
    let decl = DEFAULT_MAG_DECLINATION_DEG.to_radians();
    for yaw_deg in [0.0, 30.0, 90.0, 179.0, 270.0, 350.0] {
        let yaw = f64::to_radians(yaw_deg);
        let a = att(yaw, 0.0, 0.0);
        let field_frd = a.inverse() * earth();
        let heading = magnetic_heading(field_frd, a);
        assert!(angle_diff(heading, yaw - decl).abs() < 1e-12, "yaw {yaw_deg}: {heading}");
        assert!((0.0..TAU).contains(&heading), "{heading} is in [0, 2 pi)");
        assert!(angle_diff(true_heading(heading, decl), yaw).abs() < 1e-12, "true heading is yaw");
    }
}

#[test]
fn the_heading_does_not_depend_on_roll_and_pitch() {
    let decl = DEFAULT_MAG_DECLINATION_DEG.to_radians();
    for (yaw, pitch, roll) in [(40.0, 25.0, 0.0), (40.0, 0.0, -35.0), (200.0, -30.0, 50.0), (310.0, 60.0, -70.0)] {
        let a = att(f64::to_radians(yaw), f64::to_radians(pitch), f64::to_radians(roll));
        let heading = magnetic_heading(a.inverse() * earth(), a);
        assert!(angle_diff(heading, f64::to_radians(yaw) - decl).abs() < 1e-9, "yaw {yaw} pitch {pitch} roll {roll}: {heading}");
    }
}

#[test]
fn a_zero_field_has_a_defined_heading() {
    assert_eq!(magnetic_heading(DVec3::ZERO, DQuat::IDENTITY), 0.0);
}

fn mag_params() -> MagParams {
    MagParams {
        noise_std_ut: 0.0,
        hard_iron_ut: DVec3::ZERO,
        soft_iron: DMat3::IDENTITY,
        earth_ned_ut: earth(),
        rate_divisor: 1,
    }
}

fn mag_sim(p: MagParams, seed: u64, a: DQuat) -> Scheduler {
    let mut bus = Bus::new();
    let mag = Mag::new(p, seed, &mut bus);
    let s = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(s, a);
    let mut sched = Scheduler::new(8000, bus);
    sched.add(Box::new(mag));
    sched
}

fn field(s: &Scheduler) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(names::MAG_FIELD).unwrap())
}

fn heading(s: &Scheduler) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(names::MAG_HEADING).unwrap())
}

#[test]
fn the_field_is_the_earth_field_in_the_body_frame() {
    let a = att(1.0, 0.3, -0.4);
    let mut s = mag_sim(mag_params(), 0, a);
    s.step().unwrap();
    assert!((field(&s) - a.inverse() * earth()).length() < 1e-12);
    assert!(angle_diff(heading(&s), 1.0 - DEFAULT_MAG_DECLINATION_DEG.to_radians()).abs() < 1e-9);
}

#[test]
fn hard_iron_adds_after_soft_iron_scales() {
    let a = att(0.5, 0.0, 0.0);
    let mut p = mag_params();
    p.hard_iron_ut = DVec3::new(3.0, -2.0, 1.0);
    p.soft_iron = DMat3::from_diagonal(DVec3::new(1.1, 0.9, 1.0));
    let mut s = mag_sim(p, 0, a);
    s.step().unwrap();
    let b = a.inverse() * earth();
    let expect = DVec3::new(1.1 * b.x, 0.9 * b.y, b.z) + DVec3::new(3.0, -2.0, 1.0);
    assert!((field(&s) - expect).length() < 1e-12);
}

#[test]
fn a_hard_iron_offset_shows_as_a_heading_error() {
    let a = att(0.0, 0.0, 0.0);
    let mut p = mag_params();
    p.hard_iron_ut = DVec3::new(0.0, 5.0, 0.0);
    let mut s = mag_sim(p, 0, a);
    s.step().unwrap();
    let clean = -DEFAULT_MAG_DECLINATION_DEG.to_radians();
    assert!(angle_diff(heading(&s), clean).abs() > 0.1, "5 uT east of a ~25 uT horizontal field is several degrees");
}

fn x_samples(seed: u64, n: usize) -> Vec<f64> {
    let mut p = mag_params();
    p.noise_std_ut = 0.4;
    let mut s = mag_sim(p, seed, DQuat::IDENTITY);
    let clean = earth().x;
    (0..n)
        .map(|_| {
            s.step().unwrap();
            field(&s).x - clean
        })
        .collect()
}

#[test]
fn noise_has_the_configured_std_and_is_seeded() {
    let xs = x_samples(3, 20_000);
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    assert!(mean.abs() < 0.02, "mean {mean}");
    assert!((std - 0.4).abs() < 0.02, "std {std}");
    assert_eq!(x_samples(3, 10), xs[..10].to_vec());
    assert_ne!(x_samples(4, 10), xs[..10].to_vec());
}

#[test]
fn the_model_samples_at_its_rate_and_holds_between_samples() {
    let mut p = mag_params();
    p.rate_divisor = 80; // 100 Hz at 8 kHz
    let mut s = mag_sim(p, 0, DQuat::IDENTITY);
    s.step().unwrap();
    let first = field(&s);
    let att_sig = s.bus().lookup::<DQuat>(names::BODY_ATT).unwrap();
    s.bus_mut().set(att_sig, att(1.0, 0.0, 0.0));
    for _ in 1..80 {
        s.step().unwrap();
        assert_eq!(field(&s), first, "held until the next sample");
    }
    s.step().unwrap();
    assert!((field(&s) - first).length() > 1.0, "the 81st tick samples the new attitude");
}

#[test]
fn the_soft_iron_rows_of_a_part_file_are_the_matrix_rows() {
    let m = soft_iron_from_rows([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
    assert_eq!(m * DVec3::new(1.0, 1.0, 1.0), DVec3::new(6.0, 15.0, 24.0), "row sums");
    assert_eq!(m * DVec3::X, DVec3::new(1.0, 4.0, 7.0), "the first column");
}
