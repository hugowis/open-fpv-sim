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
