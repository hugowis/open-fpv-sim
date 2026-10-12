use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model};
use ofs_sensors::imu::{Imu, ImuParams, VibrationParams};

const DT: f64 = 1.0 / 8000.0;
const OMEGA: f64 = 300.0; // ~2862 rpm

fn vib() -> VibrationParams {
    VibrationParams {
        positions_frd: vec![DVec3::new(0.08, 0.08, 0.0)],
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
    }
}

fn params(vibration: Option<VibrationParams>) -> ImuParams {
    ImuParams {
        gyro_noise_std_radps: 0.0, // deterministic runs: the disturbance must stand alone
        gyro_bias_radps: DVec3::ZERO,
        accel_noise_std_mps2: 0.0,
        accel_bias_mps2: DVec3::ZERO,
        vibration,
    }
}

/// Runs 400 IMU steps (0.05 s) with one motor at `omega` and `imbalance`; returns the last (gyro, accel).
fn run(vibration: Option<VibrationParams>, omega: f64, imbalance: f64) -> (DVec3, DVec3) {
    let mut bus = Bus::new();
    let omega_sig = bus.signal::<f64>("motor.0.omega_radps");
    bus.set(omega_sig, omega);
    let imbalance_sig = bus.signal::<f64>("prop.0.imbalance_kgm");
    bus.set(imbalance_sig, imbalance);
    let mut imu = Imu::new(params(vibration), 7, &mut bus);
    for _ in 0..400 {
        imu.step(&ofs_core::StepCtx { tick: 0, time_s: 0.0, dt_s: DT }, &mut bus).unwrap();
    }
    let gyro = bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap());
    let accel = bus.get(bus.lookup::<DVec3>(names::IMU_ACCEL).unwrap());
    (gyro, accel)
}

#[test]
fn imbalance_zero_is_exactly_zero() {
    let (gyro, accel) = run(Some(vib()), OMEGA, 0.0);
    assert_eq!(gyro, DVec3::ZERO, "no noise, no bias, no imbalance");
    // A level body: the specific force is the reaction to gravity, one g off the body z — FRD's z is
    // down, so level rest reads -g on z (the reading sensors.rs pins for the same quiet parameters).
    assert!((accel.z + ofs_core::consts::GRAVITY_MPS2).abs() < 1e-9, "{accel:?}");
    assert_eq!(accel.x, 0.0, "{accel:?}");
    assert_eq!(accel.y, 0.0, "{accel:?}");
}

#[test]
fn a_stopped_motor_contributes_exactly_zero() {
    let (gyro, _) = run(Some(vib()), 0.0, 1.0e-3);
    assert_eq!(gyro, DVec3::ZERO, "omega 0: frozen phase, zero force, no 0/0");
}

#[test]
fn the_rotating_force_has_the_analytic_magnitude() {
    let (_, accel) = run(Some(vib()), OMEGA, 1.0e-3);
    let f = 1.0e-3 * OMEGA * OMEGA; // (m·r)·ω² in N
    let specific = f / 0.65; // m/s², the body-frame xy magnitude
    let magnitude = accel.x.hypot(accel.y);
    assert!((magnitude - specific).abs() < specific * 0.05, "xy specific force {magnitude} vs analytic {specific}");
}

#[test]
fn the_rate_disturbance_is_bounded_by_the_analytic_amplitude() {
    let (gyro, _) = run(Some(vib()), OMEGA, 1.0e-3);
    let f = 1.0e-3 * OMEGA * OMEGA;
    let moment = 0.08_f64.hypot(0.08) * f; // r ⊥ F in the worst case
    let rate_amp = moment / (0.0025 * OMEGA);
    let gyro_xy = gyro.x.hypot(gyro.y);
    assert!(gyro_xy <= rate_amp * 1.05, "rate disturbance {gyro_xy} vs analytic amplitude {rate_amp}");
    assert!(gyro_xy > 0.0, "the disturbance is present");
}

#[test]
fn a_motor_without_vibration_params_stays_clean() {
    let (gyro, accel) = run(None, OMEGA, 1.0e-3);
    assert_eq!(gyro, DVec3::ZERO);
    assert_eq!(accel.x, 0.0);
    assert_eq!(accel.y, 0.0);
}

#[test]
fn the_gyro_fault_adds_its_bias_exactly() {
    let mut bus = Bus::new();
    let bias = bus.signal::<DVec3>(names::FAULT_IMU_GYRO_BIAS);
    bus.set(bias, DVec3::new(0.1, 0.0, 0.0));
    let rate = bus.signal::<DVec3>(names::BODY_RATE_FRD);
    bus.set(rate, DVec3::ZERO);
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let accel_ned = bus.signal::<DVec3>(names::BODY_ACCEL_NED);
    bus.set(accel_ned, DVec3::new(0.0, 0.0, -ofs_core::consts::GRAVITY_MPS2));
    let mut imu = Imu::new(params(None), 7, &mut bus);
    imu.step(&ofs_core::StepCtx { tick: 0, time_s: 0.0, dt_s: DT }, &mut bus).unwrap();
    let gyro = bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap());
    assert!((gyro.x - 0.1).abs() < 1e-12, "{gyro:?}");
    assert_eq!(gyro.y, 0.0);
    assert_eq!(gyro.z, 0.0);
}

#[test]
fn the_gyro_fault_scales_the_noise_std_exactly() {
    // Same seed, std 0.002: scale 3.0 triples every sample, so the sample std is exactly 3x.
    fn sampled_std(noise_scale: f64) -> f64 {
        let mut bus = Bus::new();
        let p = ImuParams {
            gyro_noise_std_radps: 0.002,
            gyro_bias_radps: DVec3::ZERO,
            accel_noise_std_mps2: 0.0,
            accel_bias_mps2: DVec3::ZERO,
            vibration: None,
        };
        let mut imu = Imu::new(p, 7, &mut bus);
        // The model builds the multiplicative seam at its identity, so the scale goes in after construction.
        let scale = bus.signal::<f64>(names::FAULT_IMU_NOISE_SCALE);
        bus.set(scale, noise_scale);
        let mut samples = Vec::with_capacity(1000);
        for k in 0..1000 {
            imu.step(&ofs_core::StepCtx { tick: k, time_s: k as f64 * DT, dt_s: DT }, &mut bus).unwrap();
            samples.push(bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap()).x);
        }
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        (samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }
    let ratio = sampled_std(3.0) / sampled_std(1.0);
    assert!((ratio - 3.0).abs() < 1e-9, "the same samples scaled: {ratio}");
}
