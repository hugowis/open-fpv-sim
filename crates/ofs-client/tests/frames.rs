use glam::{DQuat, DVec3};
use ofs_client::frames::{quat_to_godot, vec_to_godot};
use proptest::prelude::*;

const FORWARD_GODOT: DVec3 = DVec3::new(0.0, 0.0, -1.0);
const UP_GODOT: DVec3 = DVec3::new(0.0, 1.0, 0.0);
const RIGHT_GODOT: DVec3 = DVec3::new(1.0, 0.0, 0.0);

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-12
}

#[test]
fn north_east_down_become_forward_right_down() {
    assert!(close(vec_to_godot(DVec3::new(1.0, 0.0, 0.0)), FORWARD_GODOT), "north is -Z");
    assert!(close(vec_to_godot(DVec3::new(0.0, 1.0, 0.0)), RIGHT_GODOT), "east is +X");
    assert!(close(vec_to_godot(DVec3::new(0.0, 0.0, 1.0)), -UP_GODOT), "down is -Y");
}

#[test]
fn a_level_body_facing_north_has_the_godot_camera_looking_north() {
    let q = quat_to_godot(DQuat::IDENTITY);
    assert!(close(q * FORWARD_GODOT, FORWARD_GODOT));
    assert!(close(q * UP_GODOT, UP_GODOT));
    assert!(close(q * RIGHT_GODOT, RIGHT_GODOT));
}

#[test]
fn yawing_90_degrees_to_the_right_faces_east() {
    // NED yaw is a rotation about +down; positive is clockwise seen from above.
    let q = quat_to_godot(DQuat::from_axis_angle(DVec3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2));
    assert!(close(q * FORWARD_GODOT, RIGHT_GODOT), "forward now points east (+X): {:?}", q * FORWARD_GODOT);
    assert!(close(q * UP_GODOT, UP_GODOT));
}

#[test]
fn pitching_up_raises_the_nose() {
    // FRD pitch is a rotation about +right; positive pitches the nose up.
    let q = quat_to_godot(DQuat::from_axis_angle(DVec3::new(0.0, 1.0, 0.0), 0.5));
    let nose = q * FORWARD_GODOT;
    assert!(nose.y > 0.4 && nose.z < 0.0, "{nose:?}");
}

proptest! {
    /// Rotating a body vector and then converting equals converting both and rotating in Godot's frame.
    #[test]
    fn conversion_commutes_with_rotation(
        axis in (-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0).prop_filter("non-zero", |a| a.0 * a.0 + a.1 * a.1 + a.2 * a.2 > 0.01),
        angle in -6.3f64..6.3,
        v in (-10.0f64..10.0, -10.0f64..10.0, -10.0f64..10.0),
    ) {
        let q = DQuat::from_axis_angle(DVec3::new(axis.0, axis.1, axis.2).normalize(), angle);
        let v = DVec3::new(v.0, v.1, v.2);
        let world_then_convert = vec_to_godot(q * v);
        let convert_then_rotate = quat_to_godot(q) * vec_to_godot(v);
        prop_assert!((world_then_convert - convert_then_rotate).length() < 1e-9);
        prop_assert!((quat_to_godot(q).length() - 1.0).abs() < 1e-12);
    }
}
