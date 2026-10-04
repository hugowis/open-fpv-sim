//! Barometer: ISA pressure at home altitude + height above home, plus noise.
use glam::DVec3;
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub struct BaroParams {
    pub noise_std_pa: f64,
    pub home_alt_m: f64,
}

/// International Standard Atmosphere pressure (troposphere).
pub fn isa_pressure_pa(alt_m: f64) -> f64 {
    101_325.0 * (1.0 - 2.25577e-5 * alt_m).powf(5.25588)
}

pub struct Baro {
    p: BaroParams,
    rng: ChaCha8Rng,
    noise: Normal<f64>,
    pos: Signal<DVec3>,
    out: Signal<f64>,
}

impl Baro {
    pub fn new(p: BaroParams, seed: u64, bus: &mut Bus) -> Self {
        Self {
            rng: model_rng(seed, "baro"),
            noise: Normal::new(0.0, p.noise_std_pa).expect("baro noise std must be finite and >= 0"),
            pos: bus.signal(names::BODY_POS_NED),
            out: bus.signal(names::BARO_PRESSURE),
            p,
        }
    }
}

impl Model for Baro {
    fn name(&self) -> &str {
        "baro"
    }

    fn rate_divisor(&self) -> u32 {
        1
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let alt = self.p.home_alt_m - bus.get(self.pos).z;
        bus.set(self.out, isa_pressure_pa(alt) + self.noise.sample(&mut self.rng));
        Ok(())
    }
}
