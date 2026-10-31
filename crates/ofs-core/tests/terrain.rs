use glam::DVec3;
use ofs_core::terrain::Terrain;

fn grid(heights: Vec<f32>, n: usize, e: usize) -> Terrain {
    Terrain::from_grid([-2.0, -2.0], 1.0, n, e, heights).unwrap()
}

#[test]
fn every_node_reads_back_exactly() {
    let heights: Vec<f32> = (0..20).map(|k| (k as f32) * 0.37 - 1.1).collect();
    let t = grid(heights.clone(), 4, 5);
    for i in 0..4 {
        for j in 0..5 {
            let h = t.height_up(-2.0 + i as f64, -2.0 + j as f64);
            assert_eq!(h.to_bits(), f64::from(heights[i * 5 + j]).to_bits(), "node {i},{j}");
        }
    }
}

#[test]
fn a_ramp_has_its_slope_and_normal() {
    let t = Terrain::from_grid([0.0, 0.0], 1.0, 3, 3, vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0]).unwrap();
    let (d, n) = t.surface(1.3, 0.2);
    assert!((d + 0.65).abs() < 1e-12);
    assert!((t.ground_d(1.3, 0.2) + 0.65).abs() < 1e-12);
    assert!((n - DVec3::new(-0.5, 0.0, -1.0).normalize()).abs().max_element() < 1e-12);
    assert!((t.max_slope() - 0.5).abs() < 1e-12);
}

#[test]
fn the_two_triangles_of_a_cell_differ() {
    let t = grid(vec![0.0, 2.0, 1.0, 4.0], 2, 2);
    let (d_low, n_low) = t.surface(-2.0 + 0.75, -2.0 + 0.25);
    let (d_up, n_up) = t.surface(-2.0 + 0.25, -2.0 + 0.75);
    assert!((d_low + 1.5).abs() < 1e-12);
    assert!((d_up + 2.0).abs() < 1e-12);
    assert!((n_low - n_up).length() > 0.1);
}

#[test]
fn outside_the_grid_is_the_plane() {
    let t = grid(vec![1.0, 2.0, 3.0, 4.0], 2, 2);
    assert_eq!(t.surface(100.0, 0.0), (0.0, DVec3::NEG_Z));
    assert_eq!(t.surface(f64::NAN, 0.0), (0.0, DVec3::NEG_Z));
    assert_eq!(t.surface(-2.5, -2.0), (0.0, DVec3::NEG_Z));
    assert_eq!(t.height_up(-1.0, -1.0), 4.0);
    assert_eq!(t.height_up(-1.0, -1.5), 3.5);
    assert_eq!(t.height_up(-2.0, -1.0), 2.0);
}

#[test]
fn bad_grids_are_rejected() {
    let ok = vec![0.0; 4];
    assert!(Terrain::from_grid([0.0, 0.0], 1.0, 2, 3, ok.clone()).is_err());
    assert!(Terrain::from_grid([0.0, 0.0], 1.0, 2, 2, vec![0.0, f32::NAN, 0.0, 0.0]).is_err());
    assert!(Terrain::from_grid([0.0, 0.0], 0.0, 2, 2, ok.clone()).is_err());
    assert!(Terrain::from_grid([0.0, 0.0], f64::INFINITY, 2, 2, ok.clone()).is_err());
    assert!(Terrain::from_grid([0.0, f64::NAN], 1.0, 2, 2, ok.clone()).is_err());
    assert!(Terrain::from_grid([0.0, 0.0], 1.0, 1, 4, ok).is_err());
}

#[test]
fn the_digest_follows_the_exact_grid() {
    let a = grid(vec![0.0, 1.0, 2.0, 3.0], 2, 2);
    let b = grid(vec![0.0, 1.0, 2.0, 3.0], 2, 2);
    let c = grid(vec![0.0, 1.0, 2.0, f32::from_bits(3.0f32.to_bits() + 1)], 2, 2);
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), c.digest());
    assert_eq!(a, b);
}
