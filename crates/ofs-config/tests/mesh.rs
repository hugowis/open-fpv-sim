mod common;

use std::fs;
use std::path::PathBuf;

use common::glb::{pack_glb, write_glb, GlbNode};
use ofs_config::mesh::{load_glb, load_glb_with, MAX_MESH_FILE_BYTES};

fn tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [[f32; 3]; 3] {
    [a, b, c]
}

/// A triangle at depth `z`, used where only the count matters.
fn flat(z: f32) -> [[f32; 3]; 3] {
    tri([0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z])
}

fn node<'a>(name: &'a str, triangles: Vec<[[f32; 3]; 3]>, material: Option<&'a str>) -> GlbNode<'a> {
    GlbNode { name, triangles, material }
}

fn write(dir: &tempfile::TempDir, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join("w.glb");
    fs::write(&path, bytes).unwrap();
    path
}

fn near(p: glam::DVec3, n: f64, e: f64, d: f64) {
    assert!((p.x - n).abs() < 1e-9 && (p.y - e).abs() < 1e-9 && (p.z - d).abs() < 1e-9, "{p:?} != ({n}, {e}, {d})");
}

#[test]
fn a_triangle_comes_out_in_ned() {
    let dir = tempfile::tempdir().unwrap();
    let t = tri([3.0, 5.0, -10.0], [4.0, 5.0, -10.0], [3.0, 6.0, -10.0]);
    let path = write(&dir, &write_glb(&[node("a", vec![t], None)]));
    let g = load_glb(&path, [0.0; 3], 0.0).unwrap();
    assert_eq!(g.mesh.len(), 1);
    near(g.mesh.triangle(0).a, 10.0, 3.0, -5.0);
    assert_eq!(g.digest, ofs_core::rng::fnv1a64(&fs::read(&path).unwrap()));
}

#[test]
fn heading_turns_a_mesh_clockwise_from_north() {
    let dir = tempfile::tempdir().unwrap();
    // 10 m north is -z in the file.
    let t = tri([0.0, 0.0, -10.0], [1.0, 0.0, -10.0], [0.0, 1.0, -10.0]);
    let path = write(&dir, &write_glb(&[node("a", vec![t], None)]));
    near(load_glb(&path, [0.0; 3], 90.0).unwrap().mesh.triangle(0).a, 0.0, 10.0, 0.0);
    near(load_glb(&path, [100.0, 20.0, -3.0], 90.0).unwrap().mesh.triangle(0).a, 100.0, 30.0, -3.0);
}

#[test]
fn col_nodes_replace_the_visible_ones_for_collision() {
    let dir = tempfile::tempdir().unwrap();
    let both = write_glb(&[node("visible", vec![flat(-1.0), flat(-2.0)], None), node("col_ground", vec![flat(-3.0)], None)]);
    let g = load_glb(&write(&dir, &both), [0.0; 3], 0.0).unwrap();
    assert_eq!(g.mesh.len(), 1);
    near(g.mesh.triangle(0).a, 3.0, 0.0, 0.0);
    let plain = write_glb(&[node("a", vec![flat(-1.0)], None), node("b", vec![flat(-2.0)], None)]);
    assert_eq!(load_glb(&write(&dir, &plain), [0.0; 3], 0.0).unwrap().mesh.len(), 2);
}

#[test]
fn material_names_follow_first_use() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = write_glb(&[
        node("a", vec![flat(-1.0)], Some("grass")),
        node("b", vec![flat(-2.0)], None),
        node("c", vec![flat(-3.0)], Some("concrete")),
        node("d", vec![flat(-4.0)], Some("grass")),
    ]);
    let g = load_glb(&write(&dir, &bytes), [0.0; 3], 0.0).unwrap();
    assert_eq!(g.material_names, ["grass", "default", "concrete"]);
    let m: Vec<u32> = (0..4).map(|i| g.mesh.triangle(i).material).collect();
    assert_eq!(m, [0, 1, 2, 0]);
}

#[test]
fn materials_only_the_collision_nodes_use_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = write_glb(&[node("visible", vec![flat(-1.0)], Some("paint")), node("col_x", vec![flat(-2.0)], Some("asphalt"))]);
    assert_eq!(load_glb(&write(&dir, &bytes), [0.0; 3], 0.0).unwrap().material_names, ["asphalt"]);
}

#[test]
fn degenerate_triangles_are_dropped_and_an_all_degenerate_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let line = tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
    let mixed = write_glb(&[node("a", vec![line, flat(-1.0), line], None)]);
    assert_eq!(load_glb(&write(&dir, &mixed), [0.0; 3], 0.0).unwrap().mesh.len(), 1);
    let only = write_glb(&[node("a", vec![line], None)]);
    let path = write(&dir, &only);
    let e = load_glb(&path, [0.0; 3], 0.0).unwrap_err();
    assert!(e.contains(&path.display().to_string()), "{e}");
}

#[test]
fn a_file_over_the_triangle_cap_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &write_glb(&[node("a", vec![flat(-1.0), flat(-2.0), flat(-3.0)], None)]));
    assert_eq!(load_glb_with(&path, [0.0; 3], 0.0, 3).unwrap().mesh.len(), 3);
    let e = load_glb_with(&path, [0.0; 3], 0.0, 2).unwrap_err();
    assert!(e.contains(&path.display().to_string()) && e.contains("triangles"), "{e}");
}

#[test]
fn bad_files_are_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let good = write_glb(&[node("a", vec![flat(-1.0), flat(-2.0)], None)]);
    let nan = write_glb(&[node("a", vec![tri([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])], None)]);
    let cases: Vec<(&str, Option<Vec<u8>>)> = vec![
        ("missing", None),
        ("empty", Some(vec![])),
        ("garbage", Some(vec![0x9e, 0x01, 0x77, 0x13, 0xff, 0x00, 0x42, 0x5a, 0xc3, 0x11, 0x08, 0x6d])),
        ("truncated", Some(good[..good.len() - 20].to_vec())),
        ("nan", Some(nan)),
    ];
    for (name, bytes) in cases {
        let path = dir.path().join(format!("{name}.glb"));
        if let Some(b) = bytes {
            fs::write(&path, b).unwrap();
        }
        let e = load_glb(&path, [0.0; 3], 0.0).unwrap_err();
        assert!(e.contains(&path.display().to_string()), "{name}: {e}");
        if name == "nan" {
            assert!(e.contains("finite"), "{e}");
        }
    }
}

#[test]
fn a_file_over_the_size_cap_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.glb");
    fs::File::create(&path).unwrap().set_len(MAX_MESH_FILE_BYTES + 1).unwrap();
    let e = load_glb(&path, [0.0; 3], 0.0).unwrap_err();
    assert!(e.contains(&path.display().to_string()) && e.contains("bytes"), "{e}");
}

#[test]
fn nested_nodes_apply_their_transforms() {
    // The parent moves (0, 0, -5) in the file frame; the child is scaled 2 and moved +x 1; its mesh is one triangle.
    let mut bin = Vec::new();
    for c in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
        "nodes":[{"name":"p","translation":[0,0,-5],"children":[1]},{"name":"c","scale":[2,2,2],"translation":[1,0,0],"mesh":0}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],
        "bufferViews":[{"buffer":0,"byteLength":36}],"buffers":[{"byteLength":36}]}"#;
    let dir = tempfile::tempdir().unwrap();
    let g = load_glb(&write(&dir, &pack_glb(json, &bin)), [0.0; 3], 0.0).unwrap();
    let t = g.mesh.triangle(0);
    // File (1,0,-5), (3,0,-5), (1,2,-5) -> NED (5,1,0), (5,3,0), (5,1,-2).
    near(t.a, 5.0, 1.0, 0.0);
    near(t.b, 5.0, 3.0, 0.0);
    near(t.c, 5.0, 1.0, -2.0);
}

/// A one-node GLB around `accessors`/`extra_nodes` JSON fragments over a 36-byte triangle buffer.
fn glb_with(nodes: &str, accessors: &str, primitive: &str) -> Vec<u8> {
    glb_with_views(nodes, accessors, primitive, r#"{"buffer":0,"byteLength":36}"#)
}

fn glb_with_views(nodes: &str, accessors: &str, primitive: &str, views: &str) -> Vec<u8> {
    let mut bin = Vec::new();
    for c in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{nodes}],
        "meshes":[{{"primitives":[{primitive}]}}],"accessors":[{accessors}],
        "bufferViews":[{views}],"buffers":[{{"byteLength":36}}]}}"#
    );
    pack_glb(&json, &bin)
}

const POSITIONS: &str = r#"{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}"#;

fn refused(bytes: &[u8], needle: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, bytes);
    let e = load_glb(&path, [0.0; 3], 0.0).unwrap_err();
    assert!(e.contains(&path.display().to_string()) && e.contains(needle), "{e}");
}

#[test]
fn hostile_accessor_counts_are_errors_not_aborts() {
    let prim = r#"{"attributes":{"POSITION":0}}"#;
    let node = r#"{"name":"n","mesh":0}"#;
    // Sparse, no bufferView, a count that would allocate tens of GB if iterated.
    let sparse = r#"{"componentType":5126,"count":4000000000,"type":"VEC3","min":[0,0,0],"max":[1,1,0],
        "sparse":{"count":1,"indices":{"bufferView":0,"componentType":5125},"values":{"bufferView":0}}}"#;
    refused(&glb_with(node, sparse, prim), "sparse");
    // A plain accessor over the cap, and one with count 0.
    let huge = POSITIONS.replace("\"count\":3", "\"count\":4000000000");
    refused(&glb_with(node, &huge, prim), "limit");
    let zero = POSITIONS.replace("\"count\":3", "\"count\":0");
    refused(&glb_with(node, &zero, prim), "empty");
}

#[test]
fn bad_indices_are_errors() {
    let node = r#"{"name":"n","mesh":0}"#;
    let prim = r#"{"attributes":{"POSITION":0},"indices":1}"#;
    let accessors = |count: u32| format!("{POSITIONS},{{\"bufferView\":0,\"byteOffset\":12,\"componentType\":5125,\"count\":{count},\"type\":\"SCALAR\"}}");
    // The indices read the buffer from byte 12 as u32: the first is 1.0f32 = 0x3f800000, far past 3 vertices.
    refused(&glb_with(node, &accessors(3), prim), "past");
    refused(&glb_with(node, &accessors(4), prim), "multiple of 3");
}

#[test]
fn accessor_types_the_reader_cannot_take_are_errors_not_panics() {
    let node = r#"{"name":"n","mesh":0}"#;
    // FLOAT indices reach an `unreachable!` in the gltf reader.
    let float_indices = format!("{POSITIONS},{{\"bufferView\":0,\"componentType\":5126,\"count\":3,\"type\":\"SCALAR\"}}");
    refused(&glb_with(node, &float_indices, r#"{"attributes":{"POSITION":0},"indices":1}"#), "indices");
    // u16 positions trip a debug assertion in the reader.
    let u16_positions = r#"{"bufferView":0,"componentType":5123,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}"#;
    refused(&glb_with(node, u16_positions, r#"{"attributes":{"POSITION":0}}"#), "POSITION");
    let vec2_positions = POSITIONS.replace("VEC3", "VEC2").replace(",0]", "]");
    refused(&glb_with(node, &vec2_positions, r#"{"attributes":{"POSITION":0}}"#), "POSITION");
}

const NODE: &str = r#"{"name":"n","mesh":0}"#;
const POSITION_PRIM: &str = r#"{"attributes":{"POSITION":0}}"#;

#[test]
fn a_short_byte_stride_is_an_error_not_a_panic() {
    // 4 < 12 bytes of a F32 VEC3: the reader would subtract past the slice.
    let view = r#"{"buffer":0,"byteLength":36,"byteStride":4}"#;
    refused(&glb_with_views(NODE, POSITIONS, POSITION_PRIM, view), "byteStride");
    // Still short at 8.
    let view = r#"{"buffer":0,"byteLength":36,"byteStride":8}"#;
    refused(&glb_with_views(NODE, POSITIONS, POSITION_PRIM, view), "byteStride");
}

#[test]
fn an_overflowing_buffer_view_offset_is_an_error() {
    let view = r#"{"buffer":0,"byteOffset":18446744073709551600,"byteLength":36}"#;
    refused(&glb_with_views(NODE, POSITIONS, POSITION_PRIM, view), "past the buffer");
}

#[test]
fn an_overflowing_accessor_offset_is_an_error() {
    let overflow = r#"{"bufferView":0,"byteOffset":18446744073709551600,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}"#;
    refused(&glb_with(NODE, overflow, POSITION_PRIM), "past its");
    // An indices accessor too.
    let accessors = format!("{POSITIONS},{{\"bufferView\":0,\"byteOffset\":18446744073709551600,\"componentType\":5125,\"count\":3,\"type\":\"SCALAR\"}}");
    refused(&glb_with(NODE, &accessors, r#"{"attributes":{"POSITION":0},"indices":1}"#), "past its");
    // And one whose count runs past its view without overflowing.
    let long = POSITIONS.replace("\"count\":3", "\"count\":4");
    refused(&glb_with(NODE, &long, POSITION_PRIM), "past its");
}

#[test]
fn a_buffer_view_past_the_buffer_is_an_error() {
    let view = r#"{"buffer":0,"byteOffset":12,"byteLength":36}"#;
    refused(&glb_with_views(NODE, POSITIONS, POSITION_PRIM, view), "past the buffer");
    let claims_more = r#"{"buffer":0,"byteLength":3600}"#;
    refused(&glb_with_views(NODE, POSITIONS, POSITION_PRIM, claims_more), "past the buffer");
}

#[test]
fn instancing_one_big_primitive_is_bounded_by_the_positions_read() {
    // 300 nodes share one primitive: 30 positions, 3 u16 indices. 300 triangles pass a 400-triangle cap, but the
    // walk would read 9000 positions, over the 1200 (3 x cap) budget.
    let mut bin = Vec::new();
    for i in 0..30 * 3 {
        let c = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].get(i).copied().unwrap_or(0.0);
        bin.extend_from_slice(&c.to_le_bytes());
    }
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let nodes: Vec<String> = (0..300).map(|i| format!(r#"{{"name":"n{i}","mesh":0}}"#)).collect();
    let ids: Vec<String> = (0..300).map(|i| i.to_string()).collect();
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[{}]}}],"nodes":[{}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":30,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
            {{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
        "bufferViews":[{{"buffer":0,"byteLength":360}},{{"buffer":0,"byteOffset":360,"byteLength":6}}],"buffers":[{{"byteLength":366}}]}}"#,
        ids.join(","),
        nodes.join(",")
    );
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, &pack_glb(&json, &bin));
    assert_eq!(load_glb_with(&path, [0.0; 3], 0.0, 3000).unwrap().mesh.len(), 300);
    let e = load_glb_with(&path, [0.0; 3], 0.0, 400).unwrap_err();
    assert!(e.contains(&path.display().to_string()) && e.contains("positions"), "{e}");
}

#[test]
fn node_cycles_and_fan_out_are_errors_not_hangs() {
    let prim = r#"{"attributes":{"POSITION":0}}"#;
    refused(&glb_with(r#"{"name":"a","children":[0]}"#, POSITIONS, prim), "not a tree");
    refused(&glb_with(r#"{"name":"a","children":[1]},{"name":"b","children":[0]}"#, POSITIONS, prim), "not a tree");
    // Each node lists the next one twice: 2^40 visits if walked as a DAG.
    let chain: Vec<String> = (0..40).map(|i| format!(r#"{{"name":"n{i}","children":[{},{}]}}"#, i + 1, i + 1)).collect();
    let nodes = format!("{},{{\"name\":\"end\",\"mesh\":0}}", chain.join(","));
    refused(&glb_with(&nodes, POSITIONS, prim), "not a tree");
}
