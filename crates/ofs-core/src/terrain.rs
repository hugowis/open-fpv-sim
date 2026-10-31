//! The ground as a regular grid of heights, sampled as a triangle mesh. Positions are NED metres; the grid stores
//! heights up (a hill is positive), the ground's NED depth is their negative. Beyond the grid the ground is the
//! plane d = 0.
use glam::DVec3;

use crate::rng::Fnv1a;

/// A regular grid of heights (f32, metres up), row-major with north as the slow index, node (0, 0) at `origin_ne`
/// and `cell_m` apart. Each cell is split along its (south-west, north-east) diagonal, so the surface is piecewise
/// planar and the height and normal are exactly those of the rendered mesh. Beyond the grid the ground is the plane
/// d = 0 with the normal straight up; a grid with a non-zero border has a cliff there.
#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    origin_ne: [f64; 2],
    cell_m: f64,
    nodes_n: usize,
    nodes_e: usize,
    heights: Vec<f32>,
    max_slope: f64,
    max_height: f64,
}

/// The plane of one triangle of a cell: height up `h0 + gn * u + ge * v` for the cell fractions (u north, v east),
/// gradients in height per cell.
struct Plane {
    h0: f64,
    gn: f64,
    ge: f64,
}

impl Terrain {
    pub fn from_grid(origin_ne: [f64; 2], cell_m: f64, nodes_n: usize, nodes_e: usize, heights: Vec<f32>) -> Result<Terrain, String> {
        if nodes_n < 2 || nodes_e < 2 {
            return Err(format!("a terrain needs at least 2 nodes a side, got {nodes_n} x {nodes_e}"));
        }
        if !cell_m.is_finite() || cell_m <= 0.0 {
            return Err(format!("the terrain cell size must be finite and positive, got {cell_m}"));
        }
        if !origin_ne.iter().all(|v| v.is_finite()) {
            return Err("the terrain origin must be finite".to_string());
        }
        if nodes_n.checked_mul(nodes_e) != Some(heights.len()) {
            return Err(format!("a {nodes_n} x {nodes_e} terrain needs {nodes_n} * {nodes_e} heights, got {}", heights.len()));
        }
        if heights.iter().any(|h| !h.is_finite()) {
            return Err("the terrain heights must be finite".to_string());
        }
        let mut t = Terrain { origin_ne, cell_m, nodes_n, nodes_e, heights, max_slope: 0.0, max_height: f64::NEG_INFINITY };
        t.max_height = (0..nodes_n * nodes_e).map(|k| t.node(k / nodes_e, k % nodes_e)).fold(f64::NEG_INFINITY, f64::max);
        let mut steepest: f64 = 0.0;
        for i in 0..nodes_n - 1 {
            for j in 0..nodes_e - 1 {
                for lower in [true, false] {
                    let p = t.plane(i, j, lower);
                    steepest = steepest.max((p.gn / cell_m).hypot(p.ge / cell_m));
                }
            }
        }
        t.max_slope = steepest;
        Ok(t)
    }

    fn node(&self, i: usize, j: usize) -> f64 {
        f64::from(self.heights[i * self.nodes_e + j])
    }

    /// The lower-right triangle (u >= v: nodes 00, 10, 11) or the upper-left one (00, 01, 11) of cell (i, j).
    fn plane(&self, i: usize, j: usize, lower: bool) -> Plane {
        let (h00, h10, h01, h11) = (self.node(i, j), self.node(i + 1, j), self.node(i, j + 1), self.node(i + 1, j + 1));
        if lower {
            Plane { h0: h00, gn: h10 - h00, ge: h11 - h10 }
        } else {
            Plane { h0: h00, gn: h11 - h01, ge: h01 - h00 }
        }
    }

    /// The ground's depth `d` (NED, down positive; a hill is negative) at (north, east).
    pub fn ground_d(&self, north: f64, east: f64) -> f64 {
        self.surface(north, east).0
    }

    /// Height up at (north, east): `-ground_d`.
    pub fn height_up(&self, north: f64, east: f64) -> f64 {
        -self.ground_d(north, east)
    }

    /// `ground_d` and the surface's outward (up) unit normal, in NED, there.
    pub fn surface(&self, north: f64, east: f64) -> (f64, DVec3) {
        let fn_ = (north - self.origin_ne[0]) / self.cell_m;
        let fe = (east - self.origin_ne[1]) / self.cell_m;
        let (last_n, last_e) = ((self.nodes_n - 1) as f64, (self.nodes_e - 1) as f64);
        // Negated so that a NaN falls through to the plane.
        if !(fn_ >= 0.0 && fn_ <= last_n && fe >= 0.0 && fe <= last_e) {
            return (0.0, DVec3::NEG_Z);
        }
        let i = (fn_.floor() as usize).min(self.nodes_n - 2);
        let j = (fe.floor() as usize).min(self.nodes_e - 2);
        let (u, v) = (fn_ - i as f64, fe - j as f64);
        let p = self.plane(i, j, u >= v);
        let h = p.h0 + u * p.gn + v * p.ge;
        (-h, DVec3::new(-p.gn / self.cell_m, -p.ge / self.cell_m, -1.0).normalize())
    }

    /// The tangent of the steepest triangle.
    pub fn max_slope(&self) -> f64 {
        self.max_slope
    }

    /// The highest node, metres up (negative for a grid sunk below the plane).
    pub fn max_height(&self) -> f64 {
        self.max_height
    }

    /// FNV-1a of the exact grid: its size, cell size, origin and every height's bits.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write(&(self.nodes_n as u64).to_le_bytes());
        h.write(&(self.nodes_e as u64).to_le_bytes());
        h.write(&self.cell_m.to_bits().to_le_bytes());
        h.write(&self.origin_ne[0].to_bits().to_le_bytes());
        h.write(&self.origin_ne[1].to_bits().to_le_bytes());
        for v in &self.heights {
            h.write(&v.to_bits().to_le_bytes());
        }
        h.finish()
    }

    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    pub fn origin_ne(&self) -> [f64; 2] {
        self.origin_ne
    }

    pub fn cell_m(&self) -> f64 {
        self.cell_m
    }

    /// The node counts, north then east.
    pub fn nodes(&self) -> (usize, usize) {
        (self.nodes_n, self.nodes_e)
    }
}
