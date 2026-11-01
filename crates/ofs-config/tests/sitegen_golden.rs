//! The site generator's golden `.glb` (tools/sitegen/tests/make_golden.py) is read by the engine's own loader.
use std::path::Path;

use ofs_config::mesh::load_glb;

#[test]
fn the_generators_golden_glb_loads() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/sitegen/tests/golden/fixture.glb");
    let g = load_glb(&path, [0.0; 3], 0.0).unwrap();
    assert_eq!(g.mesh.len(), 3);
    let b = g.mesh.bounds();
    // The point written at NED (100, 0, -5) is the northmost, highest one.
    assert!((b.max.x - 100.0).abs() < 1e-3 && b.min.x.abs() < 1e-3, "{b:?}");
    assert!((b.max.y - 50.0).abs() < 1e-3 && b.min.y.abs() < 1e-3, "{b:?}");
    assert!((b.min.z + 5.0).abs() < 1e-3 && b.max.z.abs() < 1e-3, "{b:?}");
    assert_eq!(g.material_names, ["grass", "concrete"]);
}

#[test]
fn the_demo_world_loads() {
    // The world file names `assets/demo.glb` relative to itself; the goldens sit side by side, so rebuild that layout.
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/sitegen/tests/golden");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::copy(golden.join("demo.glb"), dir.path().join("assets/demo.glb")).unwrap();
    std::fs::copy(golden.join("demo.toml"), dir.path().join("demo.toml")).unwrap();

    let w = ofs_config::world::load(&dir.path().join("demo.toml")).unwrap();
    assert_eq!(w.name, "Чорнобиль Демо");
    let meshes = w.load_meshes().unwrap();
    assert_eq!(meshes.len(), 1);
    let b = meshes[0].mesh.bounds();
    // The recipe's tallest thing is the footprint override at 20 m (the chimney is 15 m, the hangar 8 m).
    assert!((b.min.z + 20.0).abs() < 1e-3 && b.max.z.abs() < 1e-3, "{b:?}");
    // Materials the mesh uses and the world lists carry their loss; the rest lose 0 dB.
    assert!(meshes[0].loss_db.contains(&12.0) && meshes[0].loss_db.contains(&18.0), "{:?}", meshes[0].loss_db);
}
