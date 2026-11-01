//! TriMesh: closest-point sphere queries, segment crossings, and the BVH's determinism.
use glam::DVec3;
use ofs_core::mesh::{closest_point_on_triangle, Triangle, TriMesh};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn tri(a: DVec3, b: DVec3, c: DVec3, material: u32) -> Triangle {
    Triangle { a, b, c, material }
}

/// The triangle (0,0,0) (10,0,0) (0,10,0) in the plane d = 0.
fn floor() -> TriMesh {
    TriMesh::new(vec![tri(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(0.0, 10.0, 0.0), 1)]).unwrap()
}

/// A two-triangle wall quad at north = `n`, spanning east 0..10 and down -5..5, split on the diagonal
/// (n,0,-5)-(n,10,5).
fn wall(n: f64, material: u32) -> Vec<Triangle> {
    let (p00, p10, p01, p11) = (v(n, 0.0, -5.0), v(n, 10.0, -5.0), v(n, 0.0, 5.0), v(n, 10.0, 5.0));
    vec![tri(p00, p10, p11, material), tri(p00, p11, p01, material)]
}

fn within(m: &TriMesh, p: DVec3, r: f64) -> Vec<ofs_core::mesh::MeshHit> {
    let mut out = Vec::new();
    m.within(p, r, &mut out);
    out
}

fn crossings(m: &TriMesh, a: DVec3, b: DVec3) -> Vec<u32> {
    let mut out = Vec::new();
    m.crossings(a, b, &mut out);
    out
}

#[test]
fn within_finds_the_triangle_a_sphere_touches() {
    let hits = within(&floor(), v(2.0, 2.0, -0.3), 0.5);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].distance - 0.3).abs() < 1e-12);
    assert!((hits[0].point - v(2.0, 2.0, 0.0)).length() < 1e-12);
    assert!((hits[0].normal - v(0.0, 0.0, -1.0)).length() < 1e-12);
    assert_eq!(hits[0].triangle, 0);
    assert!(within(&floor(), v(2.0, 2.0, -0.6), 0.5).is_empty());
}

#[test]
fn within_is_two_sided() {
    let hits = within(&floor(), v(2.0, 2.0, 0.3), 0.5);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].normal - v(0.0, 0.0, 1.0)).length() < 1e-12);
}

#[test]
fn an_edge_contact_points_away_from_the_edge() {
    let hits = within(&floor(), v(5.0, -0.3, 0.0), 0.5);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].distance - 0.3).abs() < 1e-12);
    assert!((hits[0].normal - v(0.0, -1.0, 0.0)).length() < 1e-12);
}

#[test]
fn a_sphere_centred_on_the_surface_gets_a_unit_face_normal() {
    let hits = within(&floor(), v(2.0, 2.0, 0.0), 0.5);
    assert_eq!(hits.len(), 1);
    let n = hits[0].normal;
    assert!(n.is_finite());
    assert!((n.length() - 1.0).abs() < 1e-12);
    assert!((n.x.abs() + n.y.abs()) < 1e-12 && n.z.abs() > 0.999);
}

#[test]
fn within_matches_a_brute_force_scan() {
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    let pt = |rng: &mut ChaCha8Rng, s: f64| v(rng.gen_range(-s..s), rng.gen_range(-s..s), rng.gen_range(-s..s));
    let tris: Vec<Triangle> = (0..500)
        .map(|i| {
            let o = pt(&mut rng, 50.0);
            tri(o, o + pt(&mut rng, 6.0), o + pt(&mut rng, 6.0), i)
        })
        .collect();
    let mesh = TriMesh::new(tris.clone()).unwrap();
    for _ in 0..200 {
        let p = pt(&mut rng, 55.0);
        let r = rng.gen_range(0.5..8.0);
        let mut got: Vec<u32> = within(&mesh, p, r).iter().map(|h| h.triangle).collect();
        got.sort_unstable();
        let want: Vec<u32> = (0..tris.len() as u32)
            .filter(|&i| {
                let t = &tris[i as usize];
                (p - closest_point_on_triangle(p, t.a, t.b, t.c)).length() < r
            })
            .collect();
        assert_eq!(got, want);
    }
}

#[test]
fn crossings_report_each_surface_once() {
    let one = TriMesh::new(wall(10.0, 7)).unwrap();
    assert_eq!(crossings(&one, v(0.0, 3.0, -1.0), v(20.0, 3.0, -1.0)), vec![7]);
    let mut two = wall(10.0, 7);
    two.extend(wall(15.0, 9));
    let two = TriMesh::new(two).unwrap();
    assert_eq!(crossings(&two, v(0.0, 3.0, -1.0), v(20.0, 3.0, -1.0)), vec![7, 9]);
    // The same walls crossed the other way come back in segment order.
    assert_eq!(crossings(&two, v(20.0, 3.0, -1.0), v(0.0, 3.0, -1.0)), vec![9, 7]);
}

#[test]
fn a_crossing_on_a_shared_edge_counts_once() {
    let m = TriMesh::new(wall(10.0, 7)).unwrap();
    // (n,5,0) is on the diagonal (n,0,-5)-(n,10,5).
    assert_eq!(crossings(&m, v(0.0, 5.0, 0.0), v(20.0, 5.0, 0.0)), vec![7]);
}

#[test]
fn a_segment_in_a_walls_plane_or_ending_short_crosses_nothing() {
    let m = TriMesh::new(wall(10.0, 7)).unwrap();
    assert!(crossings(&m, v(10.0, 1.0, -1.0), v(10.0, 8.0, 1.0)).is_empty());
    assert!(crossings(&m, v(0.0, 3.0, -1.0), v(9.0, 3.0, -1.0)).is_empty());
    assert!(crossings(&m, v(0.0, 3.0, -1.0), v(10.0, 3.0, -1.0)).is_empty()); // ends on it: open segment
    assert!(crossings(&m, v(0.0, 30.0, -1.0), v(20.0, 30.0, -1.0)).is_empty()); // passes beside it
}

#[test]
fn new_rejects_empty_non_finite_and_zero_area() {
    assert!(TriMesh::new(vec![]).is_err());
    assert!(TriMesh::new(vec![tri(v(0.0, 0.0, 0.0), v(f64::NAN, 0.0, 0.0), v(0.0, 1.0, 0.0), 0)]).is_err());
    assert!(TriMesh::new(vec![tri(v(0.0, 0.0, 0.0), v(f64::INFINITY, 0.0, 0.0), v(0.0, 1.0, 0.0), 0)]).is_err());
    assert!(TriMesh::new(vec![tri(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0), v(2.0, 2.0, 2.0), 0)]).is_err());
    assert!(TriMesh::new(vec![tri(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0), v(0.0, 2.0, 0.0), 0)]).is_err());
}

#[test]
fn bounds_cover_every_vertex() {
    let mut tris = wall(10.0, 7);
    tris.extend(wall(-3.0, 9));
    tris.push(tri(v(0.0, 0.0, 0.0), v(0.0, 40.0, 0.0), v(0.0, 0.0, -20.0), 1));
    let m = TriMesh::new(tris.clone()).unwrap();
    let b = m.bounds();
    for t in &tris {
        for p in [t.a, t.b, t.c] {
            assert!(b.contains(p));
        }
    }
    assert_eq!(b.min, v(-3.0, 0.0, -20.0));
    assert_eq!(b.max, v(10.0, 40.0, 5.0));
    assert_eq!(m.len(), 5);
    assert_eq!(m.triangle(4).material, 1);
}

/// FNV-1a of the BVH over an 8 x 8 grid of integer-coordinate triangles; identical on every OS.
const PIN: u64 = 0x39c1_ea32_0006_e925;

#[test]
fn bvh_digest_is_pinned() {
    let mut tris = Vec::new();
    for i in 0..8 {
        for j in 0..8 {
            let (x, y) = (f64::from(i) * 3.0, f64::from(j) * 3.0);
            let z = f64::from((i * j) % 4);
            tris.push(tri(v(x, y, z), v(x + 3.0, y, z), v(x, y + 3.0, z + 1.0), 1));
            tris.push(tri(v(x + 3.0, y, z), v(x + 3.0, y + 3.0, z), v(x, y + 3.0, z + 1.0), 2));
        }
    }
    let m = TriMesh::new(tris).unwrap();
    assert_eq!(m.len(), 128);
    assert_eq!(m.digest(), PIN);
}
