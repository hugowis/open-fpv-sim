//! Solids in the world, by their signed distance (negative inside): the video link's obstacles, the physics'
//! collidable objects and the ELRS link's obstructions all use them. Positions and directions are NED metres
//! (down is +z, the ground is the plane z = 0).
use glam::DVec3;

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

    /// The unit gradient of the signed distance at `p`: it points out of the solid. On a face it is the face's
    /// normal, on an edge or a rim the diagonal of its two faces, inside it points at the closest face (on a tie
    /// the lower axis wins, so the result is stable). Deep inside a degenerate point gives up.
    pub fn normal(&self, p: DVec3) -> DVec3 {
        match *self {
            Shape::Box { center, half } => {
                let d = p - center;
                let q = d.abs() - half;
                let outside = q.max(DVec3::ZERO);
                if outside != DVec3::ZERO {
                    return (d.signum() * outside).normalize();
                }
                let axis = if q.x >= q.y && q.x >= q.z { 0 } else if q.y >= q.z { 1 } else { 2 };
                let mut n = DVec3::ZERO;
                n[axis] = if d[axis] >= 0.0 { 1.0 } else { -1.0 };
                n.try_normalize().unwrap_or(DVec3::NEG_Z)
            }
            Shape::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let radial = DVec3::new(d.x, d.y, 0.0);
                let radial_len = radial.length();
                let r = radial_len - radius;
                let v = d.z.abs() - half_height;
                let cap = DVec3::new(0.0, 0.0, d.z.signum());
                if r > 0.0 && v > 0.0 {
                    return (radial / radial_len * r + cap * v).normalize();
                }
                if r > 0.0 {
                    return radial / radial_len;
                }
                if v > 0.0 {
                    return cap;
                }
                if r >= v && radial_len > 0.0 {
                    radial / radial_len
                } else {
                    cap.try_normalize().unwrap_or(DVec3::NEG_Z)
                }
            }
        }
    }

    /// The axis-aligned bounding box of the shape.
    pub fn bounds(&self) -> Aabb {
        match *self {
            Shape::Box { center, half } => Aabb { min: center - half, max: center + half },
            Shape::Cylinder { center, radius, half_height } => Aabb {
                min: DVec3::new(center.x - radius, center.y - radius, center.z - half_height),
                max: DVec3::new(center.x + radius, center.y + radius, center.z + half_height),
            },
        }
    }
}

/// An axis-aligned bounding box: the collision broad phase's unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    /// The box grown by `margin` on every side.
    pub fn grown(self, margin: f64) -> Aabb {
        Aabb { min: self.min - DVec3::splat(margin), max: self.max + DVec3::splat(margin) }
    }

    pub fn contains(self, p: DVec3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z <= self.max.z
    }
}
