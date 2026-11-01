//! Reading a `.glb` into a [`TriMesh`] in NED metres. The file is in Godot's frame (x east, y up, -z north), so
//! NED = (-z, x, -y); the mesh is then turned by a heading (clockwise from north) and moved to a position, all in
//! f64. Geometry only: no images are decoded, and a `.glb` that points at external buffers is refused.
//!
//! Collision follows the Godot side's convention: a node whose name starts with `col_` is collision-only, and if
//! the file has any such node only those contribute; otherwise every node does.
use std::fs;
use std::path::Path;

use glam::{DMat4, DVec3};
use ofs_core::mesh::{TriMesh, Triangle};
use ofs_core::rng::fnv1a64;

/// At most this many `[[meshes]]` in one world.
pub const MAX_MESHES: usize = 8;
/// At most this many triangles in one file (counted as stored, before degenerate ones are dropped).
pub const MAX_MESH_TRIANGLES: usize = 400_000;
/// At most this many bytes in one file; the size is checked before the bytes are read.
pub const MAX_MESH_FILE_BYTES: u64 = 64 * 1024 * 1024;

const COLLISION_PREFIX: &str = "col_";

/// A loaded `.glb`: the collision mesh in NED, its material names (a triangle's `material` indexes them) and the
/// FNV-1a digest of the file's bytes.
#[derive(Debug)]
pub struct LoadedGlb {
    pub mesh: TriMesh,
    pub material_names: Vec<String>,
    pub digest: u64,
}

/// One primitive's triangles in the file frame, before the NED map.
struct Prim {
    collision: bool,
    material: String,
    triangles: Vec<[DVec3; 3]>,
}

/// [`load_glb_with`] at the default triangle cap.
pub fn load_glb(path: &Path, position_ned_m: [f64; 3], heading_deg: f64) -> Result<LoadedGlb, String> {
    load_glb_with(path, position_ned_m, heading_deg, MAX_MESH_TRIANGLES)
}

/// Reads `path`, turns it by `heading_deg` and moves it to `position_ned_m`. Every error names the path.
pub fn load_glb_with(path: &Path, position_ned_m: [f64; 3], heading_deg: f64, max_triangles: usize) -> Result<LoadedGlb, String> {
    load(path, position_ned_m, heading_deg, max_triangles).map_err(|e| format!("{}: {e}", path.display()))
}

fn load(path: &Path, position_ned_m: [f64; 3], heading_deg: f64, max_triangles: usize) -> Result<LoadedGlb, String> {
    if !position_ned_m.iter().chain([&heading_deg]).all(|v| v.is_finite()) {
        return Err("position and heading must be finite".into());
    }
    let len = fs::metadata(path).map_err(|e| format!("cannot read: {e}"))?.len();
    if len > MAX_MESH_FILE_BYTES {
        return Err(format!("{len} bytes is over the {MAX_MESH_FILE_BYTES}-byte file limit"));
    }
    let bytes = fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("not a readable .glb: {e}"))?;
    let scene = gltf.default_scene().or_else(|| gltf.scenes().next()).ok_or("the file has no scene")?;

    let mut prims = Vec::new();
    let mut triangles = 0usize;
    let mut stack: Vec<(gltf::Node, DMat4)> = scene.nodes().map(|n| (n, DMat4::IDENTITY)).collect();
    stack.reverse();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * DMat4::from_cols_array_2d(&node.transform().matrix().map(|c| c.map(f64::from)));
        if let Some(mesh) = node.mesh() {
            let collision = node.name().is_some_and(|n| n.starts_with(COLLISION_PREFIX));
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = prim.reader(|b| match b.source() {
                    gltf::buffer::Source::Bin => gltf.blob.as_deref(),
                    gltf::buffer::Source::Uri(_) => None,
                });
                let positions: Vec<[f32; 3]> = reader.read_positions().ok_or("a primitive has no readable positions (external buffers are not supported)")?.collect();
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..positions.len() as u32).collect(),
                };
                if indices.len() % 3 != 0 {
                    return Err(format!("a primitive has {} indices, not a multiple of 3", indices.len()));
                }
                triangles += indices.len() / 3;
                if triangles > max_triangles {
                    return Err(format!("more than {max_triangles} triangles"));
                }
                let vertex = |i: u32| -> Result<DVec3, String> {
                    let p = positions.get(i as usize).ok_or_else(|| format!("index {i} is past the {} vertices", positions.len()))?;
                    let v = world.transform_point3(DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
                    if v.is_finite() { Ok(v) } else { Err("a vertex is not finite".into()) }
                };
                let mut tris = Vec::with_capacity(indices.len() / 3);
                for t in indices.chunks_exact(3) {
                    tris.push([vertex(t[0])?, vertex(t[1])?, vertex(t[2])?]);
                }
                prims.push(Prim { collision, material: prim.material().name().unwrap_or("default").to_string(), triangles: tris });
            }
        }
        // Reversed so the first child is visited first: the triangle order is the file's order.
        let at = stack.len();
        stack.extend(node.children().map(|c| (c, world)));
        stack[at..].reverse();
    }

    let any_collision = prims.iter().any(|p| p.collision);
    let (sin, cos) = if heading_deg == 0.0 { (0.0, 1.0) } else { heading_deg.to_radians().sin_cos() };
    let to_ned = |p: DVec3| {
        let (n0, e0, d) = (-p.z, p.x, -p.y);
        DVec3::new(n0 * cos - e0 * sin, n0 * sin + e0 * cos, d) + DVec3::from(position_ned_m)
    };
    let mut material_names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for prim in prims.iter().filter(|p| p.collision || !any_collision) {
        let mut index = None;
        for [a, b, c] in &prim.triangles {
            let (a, b, c) = (to_ned(*a), to_ned(*b), to_ned(*c));
            if (b - a).cross(c - a).length_squared() == 0.0 {
                continue;
            }
            // Registered on the first kept triangle, so a primitive of only degenerate ones names nothing.
            let material = *index.get_or_insert_with(|| match material_names.iter().position(|m| *m == prim.material) {
                Some(i) => i as u32,
                None => {
                    material_names.push(prim.material.clone());
                    (material_names.len() - 1) as u32
                }
            });
            out.push(Triangle { a, b, c, material });
        }
    }
    let mesh = TriMesh::new(out).map_err(|e| format!("no usable triangles ({e})"))?;
    Ok(LoadedGlb { mesh, material_names, digest: fnv1a64(&bytes) })
}
