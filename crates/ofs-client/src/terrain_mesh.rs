//! The terrain as a triangle mesh in Godot's frame (+Y up, x east, z south), the same triangles the sim's
//! `Terrain::surface` interpolates, so what is drawn is what the physics touches.
use ofs_core::terrain::Terrain;

/// Vertex `i * nodes_e + j` is node (i, j); cell (i, j) owns indices `6c..6c + 6` with `c = i * (nodes_e - 1) + j`.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainMesh {
    pub positions: Vec<[f32; 3]>,
    /// Unit vectors from central differences of the grid (one-sided at its edge), smooth across the facets.
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

pub fn terrain_mesh(t: &Terrain) -> TerrainMesh {
    let (nodes_n, nodes_e) = t.nodes();
    let (cell, [origin_n, origin_e]) = (t.cell_m(), t.origin_ne());
    let h = |i: usize, j: usize| f64::from(t.heights()[i * nodes_e + j]);
    // Height slope along an axis of `count` nodes at node `k`: central, or one-sided at the ends.
    let slope = |k: usize, count: usize, at: &dyn Fn(usize) -> f64| {
        let (lo, hi) = (k.saturating_sub(1), (k + 1).min(count - 1));
        (at(hi) - at(lo)) / ((hi - lo) as f64 * cell)
    };
    let mut positions = Vec::with_capacity(nodes_n * nodes_e);
    let mut normals = Vec::with_capacity(nodes_n * nodes_e);
    for i in 0..nodes_n {
        for j in 0..nodes_e {
            positions.push([(origin_e + j as f64 * cell) as f32, t.heights()[i * nodes_e + j], (-(origin_n + i as f64 * cell)) as f32]);
            // Godot's z is -north, so dh/dz = -dh/dnorth.
            let (dx, dz) = (slope(j, nodes_e, &|j| h(i, j)), -slope(i, nodes_n, &|i| h(i, j)));
            let len = (dx * dx + 1.0 + dz * dz).sqrt();
            normals.push([(-dx / len) as f32, (1.0 / len) as f32, (-dz / len) as f32]);
        }
    }
    let mut indices = Vec::with_capacity(6 * (nodes_n - 1) * (nodes_e - 1));
    let node = |i: usize, j: usize| (i * nodes_e + j) as u32;
    for i in 0..nodes_n - 1 {
        for j in 0..nodes_e - 1 {
            let (n00, n10, n01, n11) = (node(i, j), node(i + 1, j), node(i, j + 1), node(i + 1, j + 1));
            // The sim's lower (00, 10, 11) and upper (00, 01, 11) triangles, each clockwise seen from above.
            indices.extend([n00, n10, n11, n00, n11, n01]);
        }
    }
    TerrainMesh { positions, normals, indices }
}
