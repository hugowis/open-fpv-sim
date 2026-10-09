//! Radio propagation as pure functions: free-space path loss, antenna patterns, polarization, knife-edge
//! diffraction around obstacles and the two-ray ground bounce. No bus and no state, so the video link uses them
//! today and the ELRS link can adopt them later. Positions and directions are NED metres (down is +z, the ground
//! is the plane z = 0).
use glam::DVec3;

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

/// A solid in the world, by its signed distance (negative inside).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Axis-aligned box: `half` is half the size along north, east and down.
    Box { center: DVec3, half: DVec3 },
    /// Vertical cylinder.
    Cylinder { center: DVec3, radius: f64, half_height: f64 },
}

/// How far below the ground a rooted shape reaches (any depth works: nothing travels under the ground).
const ROOT_DEPTH_M: f64 = 1000.0;
/// A shape whose bottom is within this of the ground stands on it.
const ON_GROUND_M: f64 = 0.05;

impl Shape {
    /// The shape as an obstacle: one that stands on the ground continues below it, so a signal diffracts over its
    /// top and around its sides, never underneath (its nearest face is never the bottom one).
    pub fn rooted(self) -> Shape {
        match self {
            Shape::Box { center, half } if center.z + half.z >= -ON_GROUND_M => {
                let top = center.z - half.z;
                let half_z = (ROOT_DEPTH_M - top) * 0.5;
                Shape::Box { center: DVec3::new(center.x, center.y, top + half_z), half: DVec3::new(half.x, half.y, half_z) }
            }
            Shape::Cylinder { center, radius, half_height } if center.z + half_height >= -ON_GROUND_M => {
                let top = center.z - half_height;
                let half_z = (ROOT_DEPTH_M - top) * 0.5;
                Shape::Cylinder { center: DVec3::new(center.x, center.y, top + half_z), radius, half_height: half_z }
            }
            other => other,
        }
    }

    pub fn signed_distance(&self, p: DVec3) -> f64 {
        match *self {
            Shape::Box { center, half } => {
                let q = (p - center).abs() - half;
                q.max(DVec3::ZERO).length() + q.max_element().min(0.0)
            }
            Shape::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let radial = (d.x * d.x + d.y * d.y).sqrt() - radius;
                let vertical = d.z.abs() - half_height;
                let outside = (radial.max(0.0).powi(2) + vertical.max(0.0).powi(2)).sqrt();
                outside + radial.max(vertical).min(0.0)
            }
        }
    }
}

/// An object that weakens a signal passing through or near it, by up to `rf_loss_db`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub shape: Shape,
    pub rf_loss_db: f64,
}

/// Golden-section search steps: the bracket shrinks to 0.618^60 of the path, far below a millimetre.
const SEARCH_STEPS: usize = 60;

/// The point of the segment `a`..`b` that goes deepest into (or passes closest to) `shape`, as its fraction t of
/// the way from `a`. A convex shape's signed distance is convex along a line, so a golden-section search finds the
/// minimum exactly, however thin the object and however long the path.
pub fn deepest_point(shape: &Shape, a: DVec3, b: DVec3) -> f64 {
    let f = |t: f64| shape.signed_distance(a.lerp(b, t));
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (0.0, 1.0);
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
/// diffraction losses of the obstacles on the direct ray.
pub fn path_gain(tx: &Endpoint, rx: &Endpoint, freq_mhz: f64, obstacles: &[Obstacle], ground_bounce: bool) -> PathGain {
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
    let obstruction_db: f64 = obstacles.iter().map(|o| obstruction_loss_db(o, tx.position, rx.position, lambda)).sum();
    PathGain { gain_db: gain_db - obstruction_db, obstruction_db }
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
