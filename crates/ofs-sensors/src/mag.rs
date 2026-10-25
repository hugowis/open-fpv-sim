//! Magnetometer: the earth's field in the body frame, distorted by soft iron, offset by hard iron, plus noise; and the
//! tilt-compensated magnetic heading a compass computes from it.
use std::f64::consts::TAU;

use glam::{DMat3, DQuat, DVec3};
use ofs_core::{names, rng::model_rng, Bus, Model, Signal, SimError, StepCtx};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};

/// The earth's field in NED (north, east, down), µT, from its total intensity, its inclination (degrees below the
/// horizontal, negative = up) and the declination (degrees of magnetic north east of true north).
pub fn earth_field_ned(total_ut: f64, inclination_deg: f64, declination_deg: f64) -> DVec3 {
    let (sin_i, cos_i) = inclination_deg.to_radians().sin_cos();
    let (sin_d, cos_d) = declination_deg.to_radians().sin_cos();
    let horizontal = total_ut * cos_i;
    DVec3::new(horizontal * cos_d, horizontal * sin_d, total_ut * sin_i)
}

/// Magnetic heading in [0, 2π): the angle from magnetic north to the body x axis, clockwise from above. The field is
/// levelled with the roll and pitch of `att` (FRD -> NED), so the reading does not depend on the tilt; the yaw of `att`
/// is not used. A zero horizontal field reads 0.
pub fn magnetic_heading(field_frd: DVec3, att: DQuat) -> f64 {
    let r = DMat3::from_quat(att);
    // Row 2 of the body-to-NED matrix is (-sin pitch, cos pitch sin roll, cos pitch cos roll).
    let (r20, r21, r22) = (r.col(0).z, r.col(1).z, r.col(2).z);
    let pitch = (-r20).clamp(-1.0, 1.0).asin();
    let roll = r21.atan2(r22);
    let (sin_r, cos_r) = roll.sin_cos();
    let (sin_p, cos_p) = pitch.sin_cos();
    let xh = field_frd.x * cos_p + field_frd.y * sin_r * sin_p + field_frd.z * cos_r * sin_p;
    let yh = field_frd.y * cos_r - field_frd.z * sin_r;
    (-yh).atan2(xh).rem_euclid(TAU)
}

/// True heading from a magnetic one: add the declination (east positive), radians.
pub fn true_heading(magnetic_rad: f64, declination_rad: f64) -> f64 {
    (magnetic_rad + declination_rad).rem_euclid(TAU)
}

/// The soft-iron matrix from the rows a part file lists (glam's matrices are written by column).
pub fn soft_iron_from_rows(rows: [[f64; 3]; 3]) -> DMat3 {
    DMat3::from_cols_array_2d(&rows).transpose()
}

#[derive(Debug, Clone)]
pub struct MagParams {
    /// White noise per output sample, µT.
    pub noise_std_ut: f64,
    /// Constant offset in the body frame (magnetised parts that turn with the sensor), µT.
    pub hard_iron_ut: DVec3,
    /// Linear distortion of the body-frame field (soft-iron materials and the sensor's axis gains); identity = none.
    pub soft_iron: DMat3,
    /// The world's field, NED, µT.
    pub earth_ned_ut: DVec3,
    /// The model runs every `rate_divisor` base ticks (the part's output data rate).
    pub rate_divisor: u32,
}

pub struct Mag {
    p: MagParams,
    rng: ChaCha8Rng,
    noise: Normal<f64>,
    att: Signal<DQuat>,
    field_out: Signal<DVec3>,
    heading_out: Signal<f64>,
}

impl Mag {
    pub fn new(p: MagParams, seed: u64, bus: &mut Bus) -> Self {
        Self {
            rng: model_rng(seed, "mag"),
            noise: Normal::new(0.0, p.noise_std_ut).expect("mag noise std must be finite and >= 0"),
            att: bus.signal(names::BODY_ATT),
            field_out: bus.signal(names::MAG_FIELD),
            heading_out: bus.signal(names::MAG_HEADING),
            p,
        }
    }
}

impl Model for Mag {
    fn name(&self) -> &str {
        "mag"
    }

    fn rate_divisor(&self) -> u32 {
        self.p.rate_divisor
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let att = bus.get(self.att);
        let noise = DVec3::new(
            self.noise.sample(&mut self.rng),
            self.noise.sample(&mut self.rng),
            self.noise.sample(&mut self.rng),
        );
        let field = self.p.soft_iron * (att.inverse() * self.p.earth_ned_ut) + self.p.hard_iron_ut + noise;
        bus.set(self.field_out, field);
        bus.set(self.heading_out, magnetic_heading(field, att));
        Ok(())
    }
}
