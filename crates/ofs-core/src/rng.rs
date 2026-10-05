//! Stable hashing and per-model seeded random streams.
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// 64-bit FNV-1a. Stable across platforms and Rust versions (unlike `DefaultHasher`).
#[derive(Debug, Clone, Copy)]
pub struct Fnv1a(u64);

impl Fnv1a {
    pub fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a::new();
    h.write(bytes);
    h.finish()
}

/// Independent random stream per model, so adding a model never perturbs another's noise.
pub fn model_rng(seed: u64, model_name: &str) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed ^ fnv1a64(model_name.as_bytes()))
}
