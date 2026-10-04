//! 6-axis IMU: gyro = body rate + bias + noise; accel = specific force R^T(a − g) + bias + noise.
use glam::{DQuat, DVec3};
use ofs_core::consts::GRAVITY_MPS2;
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub struct ImuParams {
    pub gyro_noise_std_radps: f64,
    pub gyro_bias_radps: DVec3,
    pub accel_noise_std_mps2: f64,
    pub accel_bias_mps2: DVec3,
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
}

impl Imu {
    pub fn new(p: ImuParams, seed: u64, bus: &mut Bus) -> Self {
        Self {
            rng: model_rng(seed, "imu"),
            gyro_noise: Normal::new(0.0, p.gyro_noise_std_radps).expect("gyro noise std must be finite and >= 0"),
            accel_noise: Normal::new(0.0, p.accel_noise_std_mps2).expect("accel noise std must be finite and >= 0"),
            rate: bus.signal(names::BODY_RATE_FRD),
            att: bus.signal(names::BODY_ATT),
            accel_ned: bus.signal(names::BODY_ACCEL_NED),
            gyro_out: bus.signal(names::IMU_GYRO),
            accel_out: bus.signal(names::IMU_ACCEL),
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
        let gyro = bus.get(self.rate) + self.p.gyro_bias_radps + self.noise3(self.gyro_noise);
        let accel = specific_force + self.p.accel_bias_mps2 + self.noise3(self.accel_noise);
        bus.set(self.gyro_out, gyro);
        bus.set(self.accel_out, accel);
        Ok(())
    }
}
