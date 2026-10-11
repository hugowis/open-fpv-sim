//! 6-axis IMU: gyro = body rate + bias + noise; accel = specific force R^T(a − g) + bias + noise.
use glam::{DQuat, DVec3};
use ofs_core::consts::GRAVITY_MPS2;
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

/// Per-motor rotating imbalance, rigid-body transmitted (Ruling 8): the motor positions give the force's
/// azimuth and lever arm, the airframe mass and inertia give the specific force and rate amplitudes.
#[derive(Debug, Clone)]
pub struct VibrationParams {
    pub positions_frd: Vec<DVec3>,
    pub mass_kg: f64,
    pub inertia_kgm2: DVec3,
}

#[derive(Debug, Clone)]
pub struct ImuParams {
    pub gyro_noise_std_radps: f64,
    pub gyro_bias_radps: DVec3,
    pub accel_noise_std_mps2: f64,
    pub accel_bias_mps2: DVec3,
    pub vibration: Option<VibrationParams>,
}

pub struct Imu {
    p: ImuParams,
    rng: ChaCha8Rng,
    gyro_noise: Normal<f64>,
    accel_noise: Normal<f64>,
    rate: Signal<DVec3>,
    att: Signal<DQuat>,
    accel_ned: Signal<DVec3>,
    gyro_out: Signal<DVec3>,
    accel_out: Signal<DVec3>,
    fault_bias: Signal<DVec3>,
    fault_noise_scale: Signal<f64>,
    imbalance: Vec<Signal<f64>>,
    theta: Vec<Signal<f64>>,
    omega: Vec<Signal<f64>>,
}

impl Imu {
    pub fn new(p: ImuParams, seed: u64, bus: &mut Bus) -> Self {
        let fault_noise_scale = bus.signal(names::FAULT_IMU_NOISE_SCALE);
        bus.set(fault_noise_scale, 1.0); // the multiplicative fault seam starts at its identity (M4b)
        // The vibration model reads its per-motor inputs by name like every other signal, one set per
        // motor position; with no `vibration` params it registers nothing and stays exactly as before.
        let motors = p.vibration.as_ref().map_or(0, |v| v.positions_frd.len());
        let imbalance = (0..motors).map(|i| bus.signal(&names::prop_imbalance(i))).collect();
        let theta = (0..motors).map(|i| bus.signal(&names::motor_theta(i))).collect();
        let omega = (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect();
        Self {
            rng: model_rng(seed, "imu"),
            gyro_noise: Normal::new(0.0, p.gyro_noise_std_radps).expect("gyro noise std must be finite and >= 0"),
            accel_noise: Normal::new(0.0, p.accel_noise_std_mps2).expect("accel noise std must be finite and >= 0"),
            rate: bus.signal(names::BODY_RATE_FRD),
            att: bus.signal(names::BODY_ATT),
            accel_ned: bus.signal(names::BODY_ACCEL_NED),
            gyro_out: bus.signal(names::IMU_GYRO),
            accel_out: bus.signal(names::IMU_ACCEL),
            fault_bias: bus.signal(names::FAULT_IMU_GYRO_BIAS),
            fault_noise_scale,
            imbalance,
            theta,
            omega,
            p,
        }
    }

    fn noise3(&mut self, d: Normal<f64>) -> DVec3 {
        DVec3::new(d.sample(&mut self.rng), d.sample(&mut self.rng), d.sample(&mut self.rng))
    }
}

impl Model for Imu {
    fn name(&self) -> &str {
        "imu"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let gravity = DVec3::new(0.0, 0.0, GRAVITY_MPS2);
        let specific_force = bus.get(self.att).inverse() * (bus.get(self.accel_ned) - gravity);
        let mut gyro = bus.get(self.rate) + self.p.gyro_bias_radps + bus.get(self.fault_bias)
            + self.noise3(self.gyro_noise) * bus.get(self.fault_noise_scale);
        let mut accel = specific_force + self.p.accel_bias_mps2 + self.noise3(self.accel_noise);
        if let Some(v) = &self.p.vibration {
            // Per-motor imbalance (Ruling 8): a rotating force in the prop plane, F = imbalance·ω² at phase
            // theta + motor azimuth, deterministic in the integrated rotor phase (no RNG). The imbalance
            // force pushes the airframe; the IMU rides the airframe, so the specific force gains +F/mass.
            let mut force_frd = DVec3::ZERO;
            let mut rate_disturb = DVec3::ZERO;
            let mut disturbed = false;
            for (i, pos) in v.positions_frd.iter().enumerate() {
                let imbalance = bus.get(self.imbalance[i]);
                if imbalance <= 0.0 {
                    continue;
                }
                let omega = bus.get(self.omega[i]);
                if omega <= 0.0 {
                    continue; // a stopped motor contributes exactly zero (no 0/0 in the rate term)
                }
                disturbed = true;
                let phase = bus.get(self.theta[i]) + pos.y.atan2(pos.x);
                let f = imbalance * omega * omega;
                let (sin, cos) = phase.sin_cos();
                force_frd += DVec3::new(f * cos, f * sin, 0.0);
                // The lever arm turns the rotating force into a moment that leads it by a quarter turn
                // (M ⊥ F, the lever relation): magnitude |r|·f, the |r × F| amplitude, on the roll/pitch
                // inertias, rotating at the motor's frequency.
                let lever = pos.x.hypot(pos.y);
                rate_disturb += DVec3::new(
                    -lever * f * sin / v.inertia_kgm2.x / omega,
                    lever * f * cos / v.inertia_kgm2.y / omega,
                    0.0, // the prop plane's force has no z lever in v1
                );
            }
            // Gated on a motor actually contributing so an all-quiet IMU stays bit-identical to the
            // no-vibration path (adding a zero vector could still flip -0.0 sign bits).
            if disturbed {
                gyro += rate_disturb;
                accel += force_frd * (1.0 / v.mass_kg);
            }
        }
        bus.set(self.gyro_out, gyro);
        bus.set(self.accel_out, accel);
        Ok(())
    }
}
