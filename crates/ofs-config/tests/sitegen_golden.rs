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
