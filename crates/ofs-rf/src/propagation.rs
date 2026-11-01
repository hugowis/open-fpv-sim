//! Radio propagation as pure functions: free-space path loss, antenna patterns, polarization, knife-edge
//! diffraction around obstacles and the two-ray ground bounce. Positions and directions are NED metres (down is
//! +z, the ground is the plane z = 0 or the terrain the caller passes).
use std::sync::Arc;

use glam::DVec3;
use ofs_core::mesh::TriMesh;
use ofs_core::shape::Shape;
use ofs_core::terrain::Terrain;

pub const SPEED_OF_LIGHT_MPS: f64 = 299_792_458.0;
/// Antenna patterns never fall more than this below their peak (real nulls are filled by reflections).
pub const PATTERN_FLOOR_DB: f64 = 20.0;
/// Loss between opposite-hand circular antennas, and the cap of a crossed linear pair (estimated).
pub const CROSS_POLARIZATION_DB: f64 = 20.0;
/// Loss between a circular and a linear antenna (half the power is in the other polarization).
pub const CIRCULAR_TO_LINEAR_DB: f64 = 3.0;
/// Shortest distance used by the path loss and the Fresnel geometry (the far-field formulas fail closer in).
pub const MIN_DISTANCE_M: f64 = 1.0;

pub fn wavelength_m(freq_mhz: f64) -> f64 {
    SPEED_OF_LIGHT_MPS / (freq_mhz * 1e6)
}

/// Free-space path loss in dB: `20 log10(d) + 20 log10(f) - 27.55` (d in metres, f in MHz), d at least 1 m.
pub fn fspl_db(distance_m: f64, freq_mhz: f64) -> f64 {
    20.0 * distance_m.max(MIN_DISTANCE_M).log10() + 20.0 * freq_mhz.log10() - 27.55
}

pub fn mw_to_dbm(mw: f64) -> f64 {
    10.0 * mw.log10()
}

pub fn dbm_to_mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

/// The sum of powers given in dBm, in dBm.
pub fn power_sum_dbm(dbm: impl IntoIterator<Item = f64>) -> f64 {
    mw_to_dbm(dbm.into_iter().map(dbm_to_mw).sum::<f64>())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarization {
    Rhcp,
    Lhcp,
    Linear,
}

impl Polarization {
    /// A reflection off the ground reverses the hand of a circular wave.
    pub fn reflected(self) -> Polarization {
        match self {
            Polarization::Rhcp => Polarization::Lhcp,
            Polarization::Lhcp => Polarization::Rhcp,
            Polarization::Linear => Polarization::Linear,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AntennaKind {
    /// A dipole-like doughnut around its axis.
    Omni,
    /// A directional antenna: a main lobe around its boresight, `beamwidth_deg` wide at -3 dB.
    Patch { beamwidth_deg: f64 },
}

/// An antenna placed in the world: `axis` is the omni's axis or the patch's boresight (a unit vector, NED).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Antenna {
    pub kind: AntennaKind,
    pub gain_dbi: f64,
    pub polarization: Polarization,
    pub axis: DVec3,
}

impl Antenna {
    /// Gain in dBi towards the unit vector `dir` (from this antenna to the other end).
    pub fn gain_towards(&self, dir: DVec3) -> f64 {
        let cos_theta = self.axis.dot(dir).clamp(-1.0, 1.0);
        let relative = match self.kind {
            AntennaKind::Omni => {
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                20.0 * sin_theta.max(1e-12).log10()
            }
            AntennaKind::Patch { beamwidth_deg } => {
                if cos_theta <= 0.0 {
                    -PATTERN_FLOOR_DB
                } else {
                    10.0 * patch_exponent(beamwidth_deg) * cos_theta.log10()
                }
            }
        };
        self.gain_dbi + relative.max(-PATTERN_FLOOR_DB)
    }

    /// The direction of a linear antenna's electric field: an omni's axis, or for a patch the world's up made
    /// square to its boresight (a vertically polarized patch).
    pub fn field_direction(&self) -> DVec3 {
        match self.kind {
            AntennaKind::Omni => self.axis,
            AntennaKind::Patch { .. } => {
                let up = DVec3::NEG_Z;
                let across = up - self.axis * up.dot(self.axis);
                across.try_normalize().unwrap_or(DVec3::X)
            }
        }
    }
}

/// The exponent n of a `cos^n` main lobe that is 3 dB down at half the beamwidth.
pub fn patch_exponent(beamwidth_deg: f64) -> f64 {
    let half = (beamwidth_deg * 0.5).to_radians();
    0.5f64.ln() / half.cos().ln()
}

/// Polarization mismatch loss in dB between a transmitting and a receiving antenna along the unit vector `path`.
pub fn polarization_loss_db(tx: &Antenna, tx_polarization: Polarization, rx: &Antenna, path: DVec3) -> f64 {
    use Polarization::*;
    match (tx_polarization, rx.polarization) {
        (Rhcp, Rhcp) | (Lhcp, Lhcp) => 0.0,
        (Rhcp, Lhcp) | (Lhcp, Rhcp) => CROSS_POLARIZATION_DB,
        (Linear, Linear) => {
            let across = |v: DVec3| (v - path * v.dot(path)).try_normalize();
            match (across(tx.field_direction()), across(rx.field_direction())) {
                (Some(a), Some(b)) => (-20.0 * a.dot(b).abs().max(1e-12).log10()).min(CROSS_POLARIZATION_DB),
                // A field along the path: the pattern's null already accounts for it.
                _ => 0.0,
            }
        }
        _ => CIRCULAR_TO_LINEAR_DB,
    }
}

/// ITU-R P.526 single knife-edge diffraction loss J(v) in dB (0 for a clear path, v <= -0.78).
pub fn knife_edge_loss_db(v: f64) -> f64 {
    if v <= -0.78 {
        0.0
    } else {
        6.9 + 20.0 * (((v - 0.1).powi(2) + 1.0).sqrt() + v - 0.1).log10()
    }
}

/// The Fresnel-Kirchhoff parameter v of an edge `h` metres into the path (negative: clear by `-h`), `d1` and
/// `d2` metres from the two ends.
pub fn fresnel_v(h: f64, d1: f64, d2: f64, wavelength_m: f64) -> f64 {
    let (d1, d2) = (d1.max(MIN_DISTANCE_M), d2.max(MIN_DISTANCE_M));
    h * (2.0 * (d1 + d2) / (wavelength_m * d1 * d2)).sqrt()
}

/// An object that weakens a signal passing through or near it, by up to `rf_loss_db`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub shape: Shape,
    pub rf_loss_db: f64,
}

/// Golden-section search steps: the bracket shrinks to 0.618^60 of the path, far below a millimetre.
const SEARCH_STEPS: usize = 60;

/// The argument in `lo..hi` that minimizes `f`, by golden-section search (exact for a convex or unimodal `f`).
fn golden_min(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..SEARCH_STEPS {
        let m1 = hi - ratio * (hi - lo);
        let m2 = lo + ratio * (hi - lo);
        if f(m1) < f(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    (lo + hi) * 0.5
}

/// The point of the segment `a`..`b` that goes deepest into (or passes closest to) `shape`, as its fraction t of
/// the way from `a`. A convex shape's signed distance is convex along a line, so a golden-section search finds the
/// minimum exactly, however thin the object and however long the path.
pub fn deepest_point(shape: &Shape, a: DVec3, b: DVec3) -> f64 {
    golden_min(|t| shape.signed_distance(a.lerp(b, t)), 0.0, 1.0)
}

/// Diffraction loss of one obstacle on the path `a`..`b`, capped at the obstacle's `rf_loss_db`.
pub fn obstruction_loss_db(obstacle: &Obstacle, a: DVec3, b: DVec3, wavelength_m: f64) -> f64 {
    if obstacle.rf_loss_db <= 0.0 {
        return 0.0;
    }
    let t = deepest_point(&obstacle.shape, a, b);
    let length = a.distance(b);
    let depth = -obstacle.shape.signed_distance(a.lerp(b, t));
    let v = fresnel_v(depth, t * length, (1.0 - t) * length, wavelength_m);
    knife_edge_loss_db(v).min(obstacle.rf_loss_db)
}

/// A triangle mesh in the way of a radio path: every surface the path crosses costs `loss_db[material]`.
#[derive(Debug, Clone)]
pub struct MeshObstacle {
    pub mesh: Arc<TriMesh>,
    /// The loss of one crossing per material index; a material past the end costs nothing.
    pub loss_db: Vec<f64>,
}

/// Two obstacles are equal when they share the same mesh (not a copy of it) and the same losses.
impl PartialEq for MeshObstacle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.mesh, &other.mesh) && self.loss_db == other.loss_db
    }
}

/// The loss of the mesh on the path `a`..`b`: the sum of `loss_db[material]` over every surface the path crosses.
/// Known limits: frequency-independent, no diffraction around the mesh's edges and no reflection off it.
pub fn mesh_loss_db(m: &MeshObstacle, a: DVec3, b: DVec3) -> f64 {
    let mut crossed = Vec::new();
    m.mesh.crossings(a, b, &mut crossed);
    crossed.iter().map(|&i| m.loss_db.get(i as usize).copied().unwrap_or(0.0)).sum()
}

/// The most the terrain takes off a path, in dB (a ridge in the way, like a solid wall).
pub const TERRAIN_RF_LOSS_DB: f64 = 60.0;

/// Diffraction loss of the terrain on the path `a`..`b`: the single knife edge that obstructs it most. The ray's
/// clearance over the ground, as a Fresnel parameter v, is sampled twice per cell along the part of the path over
/// the grid and the best sample refined by a golden-section search; the loss is capped at [`TERRAIN_RF_LOSS_DB`].
pub fn terrain_loss_db(terrain: &Terrain, a: DVec3, b: DVec3, wavelength_m: f64) -> f64 {
    let length = a.distance(b);
    if length.is_nan() || length < 1e-9 {
        return 0.0;
    }
    // The part [t0, t1] of the path over the grid (a slab test per axis).
    let (nodes_n, nodes_e) = terrain.nodes();
    let (origin, cell) = (terrain.origin_ne(), terrain.cell_m());
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (from, to, lo, hi) in [
        (a.x, b.x, origin[0], origin[0] + (nodes_n - 1) as f64 * cell),
        (a.y, b.y, origin[1], origin[1] + (nodes_e - 1) as f64 * cell),
    ] {
        if to == from {
            if !(from >= lo && from <= hi) {
                return 0.0;
            }
        } else {
            let (s0, s1) = ((lo - from) / (to - from), (hi - from) / (to - from));
            t0 = t0.max(s0.min(s1));
            t1 = t1.min(s0.max(s1));
        }
    }
    if t0 >= t1 {
        return 0.0;
    }
    // Clear when even the lowest point of the path is above the highest ground by the clear-path Fresnel margin
    // (v <= -0.78 wherever the edge is, since d1 * d2 <= (length / 2)^2). `fresnel_v` clamps d1 and d2 to 1 m, so
    // d1 * d2 can reach 1 at the ends of a path shorter than 3 m: the margin uses at least 3 m.
    let lowest = -a.lerp(b, t0).z.max(a.lerp(b, t1).z);
    if lowest > terrain.max_height() + 0.78 * (wavelength_m * length.max(3.0) / 8.0).sqrt() {
        return 0.0;
    }
    let v = |t: f64| {
        let p = a.lerp(b, t);
        fresnel_v(p.z - terrain.ground_d(p.x, p.y), t * length, (1.0 - t) * length, wavelength_m)
    };
    // shortcut: at most 2048 samples, so a span over 1024 cells is sampled coarser than half a cell; refine, or lower the cap, if a world needs it.
    let n = ((t1 - t0) * length / (cell / 2.0)).ceil().clamp(2.0, 2048.0) as usize;
    let at = |k: usize| t0 + (t1 - t0) * k as f64 / (n - 1) as f64;
    let (best, best_v) = (0..n).map(|k| (k, v(at(k)))).fold((0, f64::NEG_INFINITY), |m, c| if c.1 > m.1 { c } else { m });
    let refined = golden_min(|t| -v(t), at(best.saturating_sub(1)), at((best + 1).min(n - 1)));
    knife_edge_loss_db(best_v.max(v(refined))).min(TERRAIN_RF_LOSS_DB)
}

/// One end of a radio path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Endpoint {
    pub position: DVec3,
    pub antenna: Antenna,
}

/// What a path does to a signal, in dB (gains positive, losses negative in `gain_db`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathGain {
    /// Antenna gains, path loss, polarization, ground bounce and obstruction together.
    pub gain_db: f64,
    /// The obstruction part alone (>= 0), for the fading model.
    pub obstruction_db: f64,
}

fn unit(v: DVec3) -> DVec3 {
    v.try_normalize().unwrap_or(DVec3::X)
}

/// Gain of one ray from `tx` to `rx` that leaves `tx` along `out_dir` and arrives travelling along `in_dir`.
fn ray_gain_db(tx: &Endpoint, tx_polarization: Polarization, rx: &Endpoint, out_dir: DVec3, in_dir: DVec3, length_m: f64, freq_mhz: f64) -> f64 {
    tx.antenna.gain_towards(out_dir) + rx.antenna.gain_towards(-in_dir) - fspl_db(length_m, freq_mhz)
        - polarization_loss_db(&tx.antenna, tx_polarization, &rx.antenna, in_dir)
}

/// The path from `tx` to `rx` at `freq_mhz`: the direct ray, plus (with `ground_bounce`) the ray reflected off the
/// ground (coefficient -1, which reverses circular polarization), added with their phase difference; then the
/// diffraction losses of the obstacles and the terrain, and the crossing losses of the meshes, on the direct ray.
pub fn path_gain(
    tx: &Endpoint,
    rx: &Endpoint,
    freq_mhz: f64,
    obstacles: &[Obstacle],
    terrain: Option<&Terrain>,
    meshes: &[MeshObstacle],
    ground_bounce: bool,
) -> PathGain {
    let obstruction_db = obstruction_db(obstacles, terrain, meshes, tx.position, rx.position, freq_mhz);
    PathGain { gain_db: unobstructed_gain_db(tx, rx, freq_mhz, ground_bounce) - obstruction_db, obstruction_db }
}

/// The diffraction losses of the obstacles and the terrain, plus the crossing losses of the meshes, on the direct ray
/// from `a` to `b`, in dB. It depends on the two positions only, so receivers that share a position (the goggles'
/// antennas) share it.
pub fn obstruction_db(obstacles: &[Obstacle], terrain: Option<&Terrain>, meshes: &[MeshObstacle], a: DVec3, b: DVec3, freq_mhz: f64) -> f64 {
    let lambda = wavelength_m(freq_mhz);
    let objects: f64 = obstacles.iter().map(|o| obstruction_loss_db(o, a, b, lambda)).sum();
    let ground = match terrain {
        Some(t) => objects + terrain_loss_db(t, a, b, lambda),
        None => objects,
    };
    if meshes.is_empty() {
        ground
    } else {
        ground + meshes.iter().map(|m| mesh_loss_db(m, a, b)).sum::<f64>()
    }
}

/// [`path_gain`] without the obstacles: the direct ray, plus the ground bounce.
pub fn unobstructed_gain_db(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, ground_bounce: bool) -> f64 {
    let lambda = wavelength_m(freq_mhz);
    let direct_vec = rx.position - tx.position;
    let direct_len = direct_vec.length();
    let dir = unit(direct_vec);
    let direct_db = ray_gain_db(tx, tx.antenna.polarization, rx, dir, dir, direct_len, freq_mhz);
    let mut gain_db = direct_db;
    // Both ends above the ground (z < 0): the image of tx below the ground gives the reflected ray.
    if ground_bounce && tx.position.z < 0.0 && rx.position.z < 0.0 {
        let image = DVec3::new(tx.position.x, tx.position.y, -tx.position.z);
        let reflected_vec = rx.position - image;
        let reflected_len = reflected_vec.length();
        let t = -image.z / (rx.position.z - image.z); // where the image-to-rx line crosses z = 0
        let bounce = image.lerp(rx.position, t);
        let out_dir = unit(bounce - tx.position);
        let in_dir = unit(rx.position - bounce);
        let reflected_db = ray_gain_db(tx, tx.antenna.polarization.reflected(), rx, out_dir, in_dir, reflected_len, freq_mhz);
        let (a, b) = (10f64.powf(direct_db / 20.0), 10f64.powf(reflected_db / 20.0));
        let phase = 2.0 * std::f64::consts::PI * (reflected_len - direct_len) / lambda + std::f64::consts::PI;
        let power = a * a + b * b + 2.0 * a * b * phase.cos();
        gain_db = 10.0 * power.max(1e-30).log10();
    }
    gain_db
}

/// Receiver rejection of a transmitter `offset_mhz` away from the tuned channel (estimated for analog 5.8 GHz
/// receivers): 0 dB on channel, 10 dB at 20 MHz, 25 dB at 40 MHz, 40 dB at 60 MHz and beyond, linear in between.
pub fn adjacent_channel_rejection_db(offset_mhz: f64) -> f64 {
    const POINTS: [(f64, f64); 4] = [(0.0, 0.0), (20.0, 10.0), (40.0, 25.0), (60.0, 40.0)];
    let x = offset_mhz.abs();
    for pair in POINTS.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    POINTS[POINTS.len() - 1].1
}
