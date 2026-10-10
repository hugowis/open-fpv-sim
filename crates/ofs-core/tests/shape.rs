use glam::DVec3;
use ofs_core::shape::Shape;

fn close_dir(v: DVec3, expected: DVec3, what: &str) {
    assert!((v - expected).length() < 1e-12, "{what}: expected {expected}, got {v}");
}

const BOX: Shape = Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(1.0, 2.0, 3.0) };
const CYL: Shape = Shape::Cylinder { center: DVec3::new(0.0, 0.0, -2.0), radius: 1.0, half_height: 2.0 };

#[test]
fn a_box_face_normal_points_straight_out_of_that_face() {
    close_dir(BOX.normal(DVec3::new(11.5, 20.0, -5.0)), DVec3::X, "+x face");
    close_dir(BOX.normal(DVec3::new(8.0, 20.0, -5.0)), -DVec3::X, "-x face");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -1.0)), DVec3::Z, "bottom face (down is +z)");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -9.0)), -DVec3::Z, "top face");
    close_dir(BOX.normal(DVec3::new(10.0, 23.0, -5.0)), DVec3::Y, "+y face");
}

#[test]
fn a_box_edge_normal_is_the_diagonal_of_its_two_faces() {
    let n = BOX.normal(DVec3::new(11.5, 22.5, -5.0));
    close_dir(n, DVec3::new(1.0, 1.0, 0.0).normalize(), "+x+y edge");
}

#[test]
fn an_inside_box_point_normals_towards_the_closest_face() {
    close_dir(BOX.normal(DVec3::new(10.9, 20.0, -5.0)), DVec3::X, "closest face is +x");
    close_dir(BOX.normal(DVec3::new(10.0, 20.0, -2.5)), DVec3::Z, "closest face is the bottom");
    // On a tie the lower axis (x, then y, then z) and the positive side win, so the result is stable.
    close_dir(BOX.normal(BOX.center()), DVec3::X, "dead centre: +x wins");
}

#[test]
fn a_cylinder_normals_out_of_its_side_caps_and_rim() {
    close_dir(CYL.normal(DVec3::new(2.0, 0.0, -2.0)), DVec3::X, "side");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, -5.0)), -DVec3::Z, "top cap (up is -z)");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, 1.0)), DVec3::Z, "bottom cap");
    // On the rim one metre beyond the radius and one beyond the cap the gradient is the diagonal of side and cap.
    close_dir(CYL.normal(DVec3::new(2.0, 0.0, -5.0)), DVec3::new(1.0, 0.0, -1.0).normalize(), "top rim");
    close_dir(CYL.normal(DVec3::new(-2.0, 0.0, 1.0)), DVec3::new(-1.0, 0.0, 1.0).normalize(), "bottom rim");
    close_dir(CYL.normal(DVec3::new(0.5, 0.0, -2.0)), DVec3::X, "inside, side is closest");
    close_dir(CYL.normal(DVec3::new(0.0, 0.0, -0.5)), DVec3::Z, "inside, bottom is closest");
}

#[test]
fn normals_are_unit_length_everywhere_around_a_shape() {
    for shape in [BOX, CYL] {
        let (lo, hi) = (shape.bounds().min, shape.bounds().max);
        for i in 0..20 {
            for j in 0..20 {
                let p = DVec3::new(
                    lo.x + (hi.x - lo.x) * i as f64 / 19.0,
                    lo.y + (hi.y - lo.y) * j as f64 / 19.0,
                    lo.z - 1.0,
                );
                let n = shape.normal(p);
                assert!((n.length() - 1.0).abs() < 1e-9, "normal at {p} is {n}");
                // The gradient points towards increasing signed distance: stepping along it never goes deeper in.
                let step = shape.signed_distance(p + n * 1e-6) - shape.signed_distance(p);
                assert!(step >= -1e-9, "gradient points inward at {p}");
            }
        }
    }
}

#[test]
fn bounds_contain_the_shape_and_their_surface() {
    for shape in [BOX, CYL] {
        let b = shape.bounds();
        assert!(b.contains(shape.bounds().min + DVec3::ONE * 1e-6));
        // Every axis sample of the shape's surface is inside the box.
        for t in 0..=10 {
            let f = t as f64 / 10.0;
            match shape {
                Shape::Box { center, half } => {
                    assert!(b.contains(center + DVec3::new(half.x, half.y * (f - 0.5), half.z * (f - 0.5))));
                }
                Shape::Cylinder { center, radius, half_height } => {
                    let a = f * 2.0 * std::f64::consts::PI;
                    assert!(b.contains(center + DVec3::new(radius * a.cos(), radius * a.sin(), half_height)));
                }
            }
        }
    }
    let b = BOX.bounds();
    assert!(!b.contains(DVec3::new(0.0, 0.0, 0.0)), "a far point is outside");
}

#[test]
fn grown_boxes_and_contains_behave() {
    let b = BOX.bounds();
    let big = b.grown(2.0);
    assert!((big.min.x - (b.min.x - 2.0)).abs() < 1e-12 && (big.max.z - (b.max.z + 2.0)).abs() < 1e-12);
    assert!(big.contains(BOX.center() + DVec3::new(2.5, 0.0, 0.0)));
    assert!(!b.contains(BOX.center() + DVec3::new(1.5, 0.0, 0.0)));
}

// An inherent `impl Shape` here would break the orphan rules (Shape is foreign), so the test-local trait keeps
// `BOX.center()` working.
trait Center {
    fn center(&self) -> DVec3;
}

impl Center for Shape {
    fn center(&self) -> DVec3 {
        match *self {
            Shape::Box { center, .. } | Shape::Cylinder { center, .. } => center,
        }
    }
}
