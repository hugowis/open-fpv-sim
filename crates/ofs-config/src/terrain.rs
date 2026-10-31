//! The world file's `[terrain]` section: a few numbers that bake into a height grid (`ofs_core::terrain::Terrain`).
//! The generator is value noise on an integer-hashed lattice and uses only `+ - * /` and `sqrt` (correctly rounded
//! everywhere), never `sin`, `exp` or `pow`, so the grid is bit-identical on every OS.
use ofs_core::rng::fnv1a64;
use ofs_core::terrain::Terrain;
use serde::Deserialize;

use crate::Checker;

/// The steepest triangle a terrain may have: hills, not cliffs (the contact maths wants a floor under the normal's
/// vertical part).
pub const MAX_TERRAIN_SLOPE_DEG: f64 = 60.0;

/// Most nodes along a side (4 MB of f32 heights).
const MAX_NODES_A_SIDE: usize = 1025;

/// `[terrain]`: every key optional, so a bare `[terrain]` is rolling hills. Opt-in: a world without it is flat.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerrainSection {
    pub seed: u64,
    /// Side of the square the relief covers, centred on home; flat (d = 0) beyond it. In (0, 2000].
    pub size_m: f64,
    /// Grid spacing in [0.25, size_m]; `size_m` must be a whole multiple of it.
    pub cell_m: f64,
    /// Height scale; the hills span about +-amplitude. In [0, 100].
    pub amplitude_m: f64,
    /// Size of the largest features; at least 4 cells.
    pub wavelength_m: f64,
    /// Layers of detail, 1 to 6, each half the wavelength and `persistence` times the height.
    pub octaves: u32,
    /// In (0, 1].
    pub persistence: f64,
    /// Within this of home the ground is exactly d = 0 (the pad, the pilot); relief eases in beyond it.
    pub flat_radius_m: f64,
}

impl Default for TerrainSection {
    fn default() -> Self {
        Self { seed: 0, size_m: 400.0, cell_m: 1.0, amplitude_m: 8.0, wavelength_m: 100.0, octaves: 3, persistence: 0.5, flat_radius_m: 25.0 }
    }
}

/// A lattice value in [-1, 1): the top 53 bits of FNV-1a over the little-endian bytes of the key.
fn lattice(seed: u64, octave: u32, ix: i64, iy: i64) -> f64 {
    let mut b = [0u8; 28];
    b[..8].copy_from_slice(&seed.to_le_bytes());
    b[8..12].copy_from_slice(&octave.to_le_bytes());
    b[12..20].copy_from_slice(&ix.to_le_bytes());
    b[20..].copy_from_slice(&iy.to_le_bytes());
    (fnv1a64(&b) >> 11) as f64 / 4_503_599_627_370_496.0 - 1.0
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise: the lattice values around (x, y), quintic-faded and mixed linearly.
fn noise(seed: u64, octave: u32, x: f64, y: f64) -> f64 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i64, fy as i64);
    let (u, v) = (fade(x - fx), fade(y - fy));
    let (a, b) = (lattice(seed, octave, ix, iy), lattice(seed, octave, ix + 1, iy));
    let (c, d) = (lattice(seed, octave, ix, iy + 1), lattice(seed, octave, ix + 1, iy + 1));
    let (lo, hi) = (a + (b - a) * u, c + (d - c) * u);
    lo + (hi - lo) * v
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl TerrainSection {
    /// Fractal value noise at (north, east), normalised to [-1, 1].
    fn fbm(&self, n: f64, e: f64) -> f64 {
        let (mut sum, mut weight_sum, mut weight) = (0.0, 0.0, 1.0);
        for o in 0..self.octaves {
            let scale = self.wavelength_m / f64::from(1u32 << o);
            sum += weight * noise(self.seed, o, n / scale, e / scale);
            weight_sum += weight;
            weight *= self.persistence;
        }
        sum / weight_sum
    }

    /// Bakes the grid. Call it on validated values only.
    pub fn build(&self) -> Terrain {
        let cells = (self.size_m / self.cell_m).round();
        let nodes = cells as usize + 1;
        let half = cells * self.cell_m / 2.0;
        let origin = -half;
        let mut heights = Vec::with_capacity(nodes * nodes);
        for i in 0..nodes {
            let n = origin + i as f64 * self.cell_m;
            for j in 0..nodes {
                let e = origin + j as f64 * self.cell_m;
                let home = smooth(((n * n + e * e).sqrt() - self.flat_radius_m) / self.wavelength_m);
                let edge = smooth((half - n.abs().max(e.abs())) / self.wavelength_m);
                heights.push((self.amplitude_m * self.fbm(n, e) * home * edge) as f32);
            }
        }
        Terrain::from_grid([origin, origin], self.cell_m, nodes, nodes, heights).expect("a validated section bakes a valid grid")
    }

    /// The section's own rules; the slope cap and the pilot's place need the baked grid and are checked by the caller.
    pub(crate) fn check(&self, c: &mut Checker) {
        let numbers = [
            ("size_m", self.size_m),
            ("cell_m", self.cell_m),
            ("amplitude_m", self.amplitude_m),
            ("wavelength_m", self.wavelength_m),
            ("persistence", self.persistence),
            ("flat_radius_m", self.flat_radius_m),
        ];
        for (name, value) in numbers {
            c.check(value.is_finite(), &format!("terrain.{name}"), format!("must be finite (got {value})"));
        }
        if numbers.iter().any(|(_, v)| !v.is_finite()) {
            return;
        }
        let (size, cell) = (self.size_m, self.cell_m);
        c.check(size > 0.0 && size <= 2000.0, "terrain.size_m", format!("must be in (0, 2000] (got {size})"));
        c.check(cell >= 0.25 && cell <= size, "terrain.cell_m", format!("must be in [0.25, size_m] (got {cell})"));
        if size > 0.0 && size <= 2000.0 && cell >= 0.25 && cell <= size {
            let cells = (size / cell).round();
            c.check((size / cell - cells).abs() < 1e-9, "terrain.size_m", format!("must be a whole multiple of cell_m (got {size} / {cell})"));
            c.check(
                (cells as usize) < MAX_NODES_A_SIDE,
                "terrain.size_m",
                format!("at most {MAX_NODES_A_SIDE} nodes a side, size_m / cell_m + 1 (got {})", cells as usize + 1),
            );
        }
        c.check(self.amplitude_m >= 0.0 && self.amplitude_m <= 100.0, "terrain.amplitude_m", format!("must be in [0, 100] (got {})", self.amplitude_m));
        c.check(
            self.wavelength_m >= 4.0 * cell,
            "terrain.wavelength_m",
            format!("must be at least 4 cells, 4 x cell_m = {} (got {})", 4.0 * cell, self.wavelength_m),
        );
        c.check((1..=6).contains(&self.octaves), "terrain.octaves", format!("must be 1 to 6 (got {})", self.octaves));
        c.check(self.persistence > 0.0 && self.persistence <= 1.0, "terrain.persistence", format!("must be in (0, 1] (got {})", self.persistence));
        c.check(self.flat_radius_m >= 0.0, "terrain.flat_radius_m", format!("must be >= 0 (got {})", self.flat_radius_m));
    }
}
