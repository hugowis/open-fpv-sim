//! A minimal GLB writer for tests and examples: flat nodes of non-indexed triangles, one mesh per node,
//! one named material per distinct name. Positions only; the reader needs nothing else.
#![allow(dead_code)]

/// One node: a name, its triangles (file frame: x east, y up, -z north) and an optional material name.
pub struct GlbNode<'a> {
    pub name: &'a str,
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub material: Option<&'a str>,
}

/// Wraps a JSON chunk and a binary chunk into a GLB container (each padded to 4 bytes).
pub fn pack_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut j = json.as_bytes().to_vec();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let mut b = bin.to_vec();
    while b.len() % 4 != 0 {
        b.push(0);
    }
    let total = 12 + 8 + j.len() + if b.is_empty() { 0 } else { 8 + b.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&j);
    if !b.is_empty() {
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}

pub fn write_glb(nodes: &[GlbNode]) -> Vec<u8> {
    let mut bin = Vec::new();
    let mut materials: Vec<&str> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut node_json) = (vec![], vec![], vec![], vec![]);
    for n in nodes {
        let mut mesh_field = String::new();
        if !n.triangles.is_empty() {
            let verts: Vec<[f32; 3]> = n.triangles.iter().flatten().copied().collect();
            let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for v in &verts {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            let offset = bin.len();
            for v in &verts {
                for c in v {
                    bin.extend_from_slice(&c.to_le_bytes());
                }
            }
            views.push(format!(r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{}}}"#, verts.len() * 12));
            accessors.push(format!(
                r#"{{"bufferView":{},"componentType":5126,"count":{},"type":"VEC3","min":[{},{},{}],"max":[{},{},{}]}}"#,
                views.len() - 1,
                verts.len(),
                lo[0],
                lo[1],
                lo[2],
                hi[0],
                hi[1],
                hi[2]
            ));
            let material = n.material.map(|m| {
                let i = materials.iter().position(|x| *x == m).unwrap_or_else(|| {
                    materials.push(m);
                    materials.len() - 1
                });
                format!(r#","material":{i}"#)
            });
            meshes.push(format!(r#"{{"primitives":[{{"attributes":{{"POSITION":{}}}{}}}]}}"#, accessors.len() - 1, material.unwrap_or_default()));
            mesh_field = format!(r#","mesh":{}"#, meshes.len() - 1);
        }
        node_json.push(format!(r#"{{"name":"{}"{mesh_field}}}"#, n.name));
    }
    let ids: Vec<String> = (0..nodes.len()).map(|i| i.to_string()).collect();
    let mats: Vec<String> = materials.iter().map(|m| format!(r#"{{"name":"{m}"}}"#)).collect();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[{}]}}],"nodes":[{}],"meshes":[{}],"materials":[{}],"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
        ids.join(","),
        node_json.join(","),
        meshes.join(","),
        mats.join(","),
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    pack_glb(&json, &bin)
}
