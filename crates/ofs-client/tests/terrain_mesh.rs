use ofs_client::{terrain_mesh, Terrain, TerrainMesh};

const CELL: f64 = 0.75;
const ORIGIN: [f64; 2] = [-1.5, 2.0];

/// 3 x 5 nodes, uneven on purpose: the cells are not planar, so the diagonal matters.
fn uneven() -> Terrain {
    #[rustfmt::skip]
    let heights = vec![
        0.0, 1.5, -0.5, 2.25, 0.75,
        1.0, -1.25, 3.0, 0.5, -2.0,
        -0.75, 2.0, 0.25, -1.5, 1.75,
    ];
    Terrain::from_grid(ORIGIN, CELL, 3, 5, heights).unwrap()
}

/// The mesh's height at Godot (x, z): the triangle of the cell holding the point, read from the index buffer.
fn mesh_height(m: &TerrainMesh, t: &Terrain, x: f64, z: f64) -> f64 {
    let (nodes_n, nodes_e) = t.nodes();
    let (fi, fj) = ((-z - ORIGIN[0]) / CELL, (x - ORIGIN[1]) / CELL);
    let i = (fi.floor() as usize).min(nodes_n - 2);
    let j = (fj.floor() as usize).min(nodes_e - 2);
    let c = i * (nodes_e - 1) + j;
    for tri in m.indices[6 * c..6 * c + 6].chunks(3) {
        let p: Vec<[f32; 3]> = tri.iter().map(|&k| m.positions[k as usize]).collect();
        let (x0, z0) = (f64::from(p[0][0]), f64::from(p[0][2]));
        let (ax, az) = (f64::from(p[1][0]) - x0, f64::from(p[1][2]) - z0);
        let (bx, bz) = (f64::from(p[2][0]) - x0, f64::from(p[2][2]) - z0);
        let det = ax * bz - bx * az;
        let (px, pz) = (x - x0, z - z0);
        let s = (px * bz - bx * pz) / det;
        let u = (ax * pz - px * az) / det;
        if s >= -1e-6 && u >= -1e-6 && s + u <= 1.0 + 1e-6 {
            let y0 = f64::from(p[0][1]);
            return y0 + s * (f64::from(p[1][1]) - y0) + u * (f64::from(p[2][1]) - y0);
        }
    }
    panic!("no triangle of cell {c} holds ({x}, {z})");
}

#[test]
fn mesh_height_equals_the_terrain_everywhere() {
    let t = uneven();
    let m = terrain_mesh(&t);
    let (nodes_n, nodes_e) = t.nodes();
    let at = |n: f64, e: f64| (ORIGIN[0] + n * CELL, ORIGIN[1] + e * CELL);
    let mut points = Vec::new(); // (north, east) in cell units from the origin
    for i in 0..nodes_n {
        for j in 0..nodes_e {
            points.push((i as f64, j as f64));
        }
    }
    for i in 0..nodes_n - 1 {
        for j in 0..nodes_e - 1 {
            let (i, j) = (i as f64, j as f64);
            points.push((i + 0.5, j + 0.5));
            for k in 1..=5 {
                let u = f64::from(k) / 6.0;
                points.push((i + u, j + u)); // on the diagonal
                points.push((i + u, j + u - 0.1)); // lower triangle side (u >= v)
                points.push((i + u - 0.1, j + u)); // upper triangle side
            }
        }
    }
    let (last_n, last_e) = ((nodes_n - 1) as f64, (nodes_e - 1) as f64);
    for k in 0..=12 {
        let f = f64::from(k) / 12.0;
        points.push((0.0, f * last_e));
        points.push((last_n, f * last_e));
        points.push((f * last_n, 0.0));
        points.push((f * last_n, last_e));
    }
    for (n, e) in points {
        let (north, east) = at(n, e);
        let want = t.height_up(north, east);
        let got = mesh_height(&m, &t, east, -north);
        assert!((got - want).abs() < 1e-4, "at cell units ({n}, {e}): mesh {got}, terrain {want}");
    }
    for (north, east) in [(-5.0, 0.0), (ORIGIN[0] - 0.01, 3.0), (10.0, 10.0), (0.0, ORIGIN[1] - 1.0), (0.0, 100.0)] {
        assert_eq!(t.height_up(north, east), 0.0, "outside the grid the ground is the plane at the ring's height");
    }
}

fn cross_y(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f64 {
    let (u, v) = ([b[0] - a[0], b[2] - a[2]], [c[0] - a[0], c[2] - a[2]]);
    f64::from(u[1]) * f64::from(v[0]) - f64::from(u[0]) * f64::from(v[1])
}

#[test]
fn every_triangle_faces_up_and_normals_are_unit() {
    let t = uneven();
    let m = terrain_mesh(&t);
    let (nodes_n, nodes_e) = t.nodes();
    assert_eq!(m.positions.len(), nodes_n * nodes_e);
    assert_eq!(m.normals.len(), nodes_n * nodes_e);
    assert_eq!(m.indices.len(), 6 * (nodes_n - 1) * (nodes_e - 1));
    assert!(m.indices.iter().all(|&k| (k as usize) < m.positions.len()));
    for tri in m.indices.chunks(3) {
        let (a, b, c) = (m.positions[tri[0] as usize], m.positions[tri[1] as usize], m.positions[tri[2] as usize]);
        // Godot's front faces are clockwise from above: the y of the edges' cross product is negative.
        assert!(cross_y(a, b, c) < 0.0, "triangle {a:?} {b:?} {c:?} faces down");
    }
    for n in &m.normals {
        let len = f64::from(n[0]).powi(2) + f64::from(n[1]).powi(2) + f64::from(n[2]).powi(2);
        assert!((len.sqrt() - 1.0).abs() < 1e-6, "{n:?}");
        assert!(n[1] > 0.0, "{n:?}");
    }
    let flat = Terrain::from_grid([0.0, 0.0], 1.0, 4, 3, vec![2.0; 12]).unwrap();
    assert!(terrain_mesh(&flat).normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
}

#[test]
fn normals_lean_away_from_the_slope_in_godots_axes() {
    // Height rises with the north index and with the east index: the normal leans south (+Z) and west (-X).
    let heights: Vec<f32> = (0..3).flat_map(|i| (0..3).map(move |j| (2 * i + j) as f32)).collect();
    let t = Terrain::from_grid([0.0, 0.0], 1.0, 3, 3, heights).unwrap();
    let n = terrain_mesh(&t).normals[4]; // the middle node: dh/dnorth = 2, dh/deast = 1
    let want = [-1.0 / 6f32.sqrt(), 1.0 / 6f32.sqrt(), 2.0 / 6f32.sqrt()];
    assert!(n.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "{n:?} vs {want:?}");
}

#[test]
fn vertex_positions_are_the_nodes() {
    let t = uneven();
    let m = terrain_mesh(&t);
    let (nodes_n, nodes_e) = t.nodes();
    for i in 0..nodes_n {
        for j in 0..nodes_e {
            let p = m.positions[i * nodes_e + j];
            let want = [(ORIGIN[1] + j as f64 * CELL) as f32, t.heights()[i * nodes_e + j], (-(ORIGIN[0] + i as f64 * CELL)) as f32];
            assert_eq!(p[1].to_bits(), want[1].to_bits(), "heights are bit-exact");
            assert_eq!((p[0], p[2]), (want[0], want[2]), "node ({i}, {j})");
        }
    }
}
