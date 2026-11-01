use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::terrain::Terrain;

fn close_dir(v: DVec3, expected: DVec3, what: &str) {
    assert!((v - expected).length() < 1e-12, "{what}: expected {expected}, got {v}");
}

const BOX: Shape = Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(1.0, 2.0, 3.0), yaw_rad: 0.0 };
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
        // One plane above the top, one through the middle (beside the sides), one below the bottom.
        for z in [lo.z - 1.0, (lo.z + hi.z) * 0.5, hi.z + 1.0] {
            for i in 0..20 {
                for j in 0..20 {
                    let p = DVec3::new(
                        lo.x + (hi.x - lo.x) * i as f64 / 19.0,
                        lo.y + (hi.y - lo.y) * j as f64 / 19.0,
                        z,
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
                Shape::Box { center, half, .. } => {
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

fn hill() -> Terrain {
    Terrain::from_grid([-2.0, -2.0], 1.0, 5, 5, vec![8.0; 25]).unwrap()
}

fn box_with_bottom_at(z: f64) -> Shape {
    Shape::Box { center: DVec3::new(0.0, 0.0, z - 1.0), half: DVec3::new(1.0, 1.0, 1.0), yaw_rad: 0.0 }
}

fn yawed(yaw_rad: f64) -> Shape {
    Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(2.0, 1.0, 5.0), yaw_rad }
}

#[test]
fn a_yaw_of_90_degrees_is_the_swapped_extents_box() {
    // cos(pi/2) is 6.1e-17, not 0, so compare with a tolerance; every other value agrees to the last useful bit.
    let b = yawed(std::f64::consts::FRAC_PI_2);
    let swapped = Shape::Box { center: DVec3::new(10.0, 20.0, -5.0), half: DVec3::new(1.0, 2.0, 5.0), yaw_rad: 0.0 };
    for p in [DVec3::new(14.0, 25.0, -6.0), DVec3::new(9.0, 21.5, -2.0), DVec3::new(12.0, 18.0, -11.0)] {
        assert!((b.signed_distance(p) - swapped.signed_distance(p)).abs() < 1e-12);
    }
    // 4 m east of the centre: out past the +x face, whose outward normal at yaw 90 is east.
    close_dir(b.normal(DVec3::new(10.0, 24.0, -5.0)), DVec3::Y, "yaw 90: the local +x face now faces east");
}

#[test]
fn a_45_degree_face_normal_is_the_rotated_axis() {
    // A point 1.5 m along the box's own north axis (inside, nearest the +x face): the outward normal is that
    // face's axis rotated to its heading, the north-east diagonal.
    let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
    let n = yawed(std::f64::consts::FRAC_PI_4).normal(DVec3::new(10.0 + 1.5 * c, 20.0 + 1.5 * s, -5.0));
    close_dir(n, DVec3::new(std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2, 0.0).normalize(), "north-east");
}

#[test]
fn the_bounds_of_a_yawed_box_are_its_exact_corners() {
    let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let b = yawed(30f64.to_radians()).bounds();
    let want = DVec3::new(c.abs() * 2.0 + s.abs() * 1.0, s.abs() * 2.0 + c.abs() * 1.0, 5.0);
    assert!((b.max - DVec3::new(10.0, 20.0, -5.0) - want).length() < 1e-12);
    assert!((DVec3::new(10.0, 20.0, -5.0) - b.min - want).length() < 1e-12);
    // The corners are on the bounds at every awkward angle (the broad phase never misses).
    for yaw in [0.7, std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2, 3.665] {
        let shape = yawed(yaw);
        let (sn, cs) = (yaw.sin(), yaw.cos());
        for (u, v) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            // The local corner (u*2, v*1) rotated to its heading: (c*ln - s*le, s*ln + c*le).
            let corner = DVec3::new(10.0 + u * cs * 2.0 - v * sn, 20.0 + u * sn * 2.0 + v * cs, -5.0);
            assert!(shape.bounds().grown(1e-9).contains(corner), "yaw {yaw}: a corner outside the AABB");
        }
    }
}

#[test]
fn a_zero_yaw_gives_todays_values_exactly() {
    // Hand-computed on dyadic points (all exact in f64): the values the axis-aligned box of half (2, 1, 5)
    // has always returned, through the identity path, bit for bit.
    let b = yawed(0.0);
    assert_eq!(b.signed_distance(DVec3::new(13.5, 20.0, -5.0)), 1.5);   // 1.5 m out past the +x face
    assert_eq!(b.signed_distance(DVec3::new(10.0, 20.0, -5.0)), -1.0); // the centre: the nearest face is 1 m away
    assert_eq!(b.signed_distance(DVec3::new(11.0, 21.5, -2.0)), 0.5);  // an edge, diagonally out past the y face
    for p in [DVec3::new(13.5, 20.0, -5.0), DVec3::new(10.0, 20.0, -5.0)] {
        assert_eq!(yawed(-0.0).signed_distance(p), b.signed_distance(p), "-0.0 is identity too");
    }
}

#[test]
fn rooting_keeps_the_yaw() {
    let b = yawed(0.8).rooted_on(None);
    let Shape::Box { yaw_rad, .. } = b else { panic!("still a box") };
    assert_eq!(yaw_rad, 0.8);
    assert!(b.signed_distance(DVec3::new(10.0, 20.0, 500.0)) < 0.0, "rooted down");
}

#[test]
fn rooted_on_a_hill_roots_a_box_standing_on_it() {
    let b = box_with_bottom_at(-8.0).rooted_on(Some(&hill()));
    assert!(b.signed_distance(DVec3::new(0.0, 0.0, 92.0)) < 0.0);
}

#[test]
fn rooted_on_leaves_a_floating_box_alone() {
    let b = box_with_bottom_at(-12.0);
    assert_eq!(b.rooted_on(Some(&hill())), b);
}

#[test]
fn rooted_on_roots_a_buried_box() {
    let b = box_with_bottom_at(0.0).rooted_on(Some(&hill()));
    assert!(b.signed_distance(DVec3::new(0.0, 0.0, 100.0)) < 0.0);
}

#[test]
fn rooted_on_none_is_rooted() {
    for b in [box_with_bottom_at(0.0), box_with_bottom_at(-12.0)] {
        assert_eq!(b.rooted_on(None), b.rooted());
    }
}
