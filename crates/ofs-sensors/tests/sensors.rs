use std::f64::consts::FRAC_PI_2;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Scheduler};
use ofs_sensors::baro::{isa_pressure_pa, Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};

fn quiet_imu() -> ImuParams {
    ImuParams {
        gyro_noise_std_radps: 0.0,
        gyro_bias_radps: DVec3::ZERO,
        accel_noise_std_mps2: 0.0,
        accel_bias_mps2: DVec3::ZERO,
    }
}

fn imu_sim(p: ImuParams, seed: u64, att: DQuat, rate: DVec3) -> Scheduler {
    let mut bus = Bus::new();
    let imu = Imu::new(p, seed, &mut bus);
    let a = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(a, att);
    let r = bus.signal::<DVec3>(names::BODY_RATE_FRD);
    bus.set(r, rate);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(imu));
    s
}

fn vec3(s: &Scheduler, name: &str) -> DVec3 {
    s.bus().get(s.bus().lookup::<DVec3>(name).unwrap())
}

#[test]
fn level_at_rest_measures_one_g_up() {
    let mut s = imu_sim(quiet_imu(), 0, DQuat::IDENTITY, DVec3::ZERO);
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_ACCEL) - DVec3::new(0.0, 0.0, -9.80665)).length() < 1e-12);
}

#[test]
fn rolled_right_ninety_degrees_feels_gravity_toward_left_wing() {
    let mut s = imu_sim(quiet_imu(), 0, DQuat::from_rotation_x(FRAC_PI_2), DVec3::ZERO);
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_ACCEL) - DVec3::new(0.0, -9.80665, 0.0)).length() < 1e-9);
}

#[test]
fn gyro_reports_rate_plus_bias() {
    let mut p = quiet_imu();
    p.gyro_bias_radps = DVec3::new(0.01, 0.0, -0.02);
    let mut s = imu_sim(p, 0, DQuat::IDENTITY, DVec3::new(1.0, 2.0, 3.0));
    s.step().unwrap();
    assert!((vec3(&s, names::IMU_GYRO) - DVec3::new(1.01, 2.0, 2.98)).length() < 1e-12);
}

fn gyro_x_samples(seed: u64, n: usize) -> Vec<f64> {
    let mut p = quiet_imu();
    p.gyro_noise_std_radps = 0.01;
    let mut s = imu_sim(p, seed, DQuat::IDENTITY, DVec3::ZERO);
    (0..n)
        .map(|_| {
            s.step().unwrap();
            vec3(&s, names::IMU_GYRO).x
        })
        .collect()
}

#[test]
fn gyro_noise_has_configured_std_and_is_seeded() {
    let xs = gyro_x_samples(7, 20_000);
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    assert!((std - 0.01).abs() < 0.0005, "std {std}");
    assert_eq!(gyro_x_samples(7, 10), xs[..10].to_vec());
    assert_ne!(gyro_x_samples(8, 10), xs[..10].to_vec());
}

#[test]
fn barometer_follows_standard_atmosphere() {
    assert!((isa_pressure_pa(0.0) - 101_325.0).abs() < 1e-9);
    assert!((isa_pressure_pa(1000.0) - 89_874.6).abs() < 0.5);
    let mut bus = Bus::new();
    let baro = Baro::new(BaroParams { noise_std_pa: 0.0, home_alt_m: 1000.0 }, 0, &mut bus);
    let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(pos, DVec3::new(0.0, 0.0, -10.0));
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(baro));
    s.step().unwrap();
    let p = s.bus().get(s.bus().lookup::<f64>(names::BARO_PRESSURE).unwrap());
    assert!((p - isa_pressure_pa(1010.0)).abs() < 1e-9);
}

#[test]
fn standard_atmosphere_is_zero_above_its_top_not_nan() {
    // The troposphere formula's base goes negative above ~44.3 km; powf of a negative base is NaN.
    assert_eq!(isa_pressure_pa(50_000.0), 0.0);
    assert!(isa_pressure_pa(44_000.0) > 0.0);
}

fn baro_samples(seed: u64, n: usize) -> Vec<f64> {
    let mut bus = Bus::new();
    let baro = Baro::new(BaroParams { noise_std_pa: 2.0, home_alt_m: 0.0 }, seed, &mut bus);
    let mut s = Scheduler::new(1000, bus);
    s.add(Box::new(baro));
    let out = s.bus().lookup::<f64>(names::BARO_PRESSURE).unwrap();
    (0..n)
        .map(|_| {
            s.step().unwrap();
            s.bus().get(out) - isa_pressure_pa(0.0)
        })
        .collect()
}

#[test]
fn barometer_noise_has_configured_std_and_is_seeded() {
    let xs = baro_samples(5, 20_000);
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    assert!(mean.abs() < 0.1, "mean {mean}");
    assert!((std - 2.0).abs() < 0.1, "std {std}");
    assert_eq!(baro_samples(5, 10), xs[..10].to_vec());
    assert_ne!(baro_samples(6, 10), xs[..10].to_vec());
}
