//! Frame conversion between the simulator (world NED, body FRD) and Godot (right-handed, +Y up, the camera
//! looks along -Z, +X to the right).
//!
//! Godot x = East (NED y) / body Right, Godot y = Up (-NED z) / body Up (-FRD z), Godot z = -North / body Back.
//! The same signed permutation `M` converts positions, world vectors and body vectors, and `det M = +1`, so an
//! attitude (a rotation from the body frame to the world frame) converts by conjugation with `M`: its angle is
//! kept and its axis is mapped with `M`.
use glam::{DQuat, DVec3};

/// A position or vector, in NED (world) or FRD (body), as Godot coordinates.
pub fn vec_to_godot(v: DVec3) -> DVec3 {
    DVec3::new(v.y, -v.z, -v.x)
}

/// An attitude (body FRD to world NED) as the rotation of the Godot body frame in the Godot world.
pub fn quat_to_godot(q: DQuat) -> DQuat {
    DQuat::from_xyzw(q.y, -q.z, -q.x, q.w)
}
