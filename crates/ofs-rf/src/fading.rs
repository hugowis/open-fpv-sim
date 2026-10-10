//! Rician fading, receiver diversity and the quad's body shadow: the receiver-side effects both links share.
use glam::{DQuat, DVec3};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, StandardNormal};

/// The most the frame, battery and stack take from a signal when they sit between the quad's antenna and the
/// other end of the link.
pub const BODY_SHADOW_DB: f64 = 8.0;
/// The body direction (FRD, unit) the frame shadows most: forward and down, through the stack and the battery.
const SHADOW_DIRECTION: DVec3 = DVec3::new(0.6, 0.0, 0.8);
/// The receiver changes antenna only when another one is better by this much.
pub const DIVERSITY_HYSTERESIS_DB: f64 = 2.0;
/// Rician K-factor with a clear line of sight; obstruction lowers it dB for dB (towards Rayleigh fading).
pub const LOS_K_DB: f64 = 10.0;
/// Lowest K-factor (deep behind an obstacle: practically Rayleigh).
pub const MIN_K_DB: f64 = -20.0;

/// Rician fading of one antenna: an AR(1) complex Gaussian scatter (unit mean power) added to the line of sight.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fader {
    re: f64,
    im: f64,
}

impl Fader {
    /// The fade in dB for this packet or field. `rho` is the correlation with the previous one (1 = stood still).
    pub fn next(&mut self, rng: &mut ChaCha8Rng, rho: f64, k_db: f64) -> f64 {
        let fresh = ((1.0 - rho * rho).max(0.0) * 0.5).sqrt();
        let (n1, n2): (f64, f64) = (StandardNormal.sample(rng), StandardNormal.sample(rng));
        self.re = rho * self.re + fresh * n1;
        self.im = rho * self.im + fresh * n2;
        let k = 10f64.powf(k_db.max(MIN_K_DB) / 10.0);
        let (los, scatter) = ((k / (k + 1.0)).sqrt(), (1.0 / (k + 1.0)).sqrt());
        let (re, im) = (los + scatter * self.re, scatter * self.im);
        10.0 * (re * re + im * im).max(1e-6).log10()
    }
}

/// Diversity: the antenna with the best signal, changed only for a [`DIVERSITY_HYSTERESIS_DB`] better one.
#[derive(Debug, Clone, Default)]
pub struct Diversity {
    active: usize,
}

impl Diversity {
    /// The antenna to use, given each antenna's level in dB (at least one).
    pub fn choose(&mut self, db: &[f64]) -> usize {
        self.active = self.active.min(db.len().saturating_sub(1));
        let best = (0..db.len()).max_by(|a, b| db[*a].total_cmp(&db[*b])).unwrap_or(0);
        if db[best] > db[self.active] + DIVERSITY_HYSTERESIS_DB {
            self.active = best;
        }
        self.active
    }

    /// The antenna in use (the handset transmits on the antenna it receives best).
    pub fn active(&self) -> usize {
        self.active
    }
}

/// How much of the frame, battery and stack lies between the quad's antenna and `other_pos`, as a loss in dB:
/// up to [`BODY_SHADOW_DB`] when the other end is forward and below the quad, nothing when it is behind or above.
pub fn body_shadow_db(att: DQuat, quad_pos: DVec3, other_pos: DVec3) -> f64 {
    let Some(dir_world) = (other_pos - quad_pos).try_normalize() else { return 0.0 };
    let dir_body = att.inverse() * dir_world;
    let x = (dir_body.dot(SHADOW_DIRECTION.normalize()) / 0.8).clamp(0.0, 1.0);
    BODY_SHADOW_DB * x * x * (3.0 - 2.0 * x)
}
