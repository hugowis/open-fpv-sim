//! The shipped quads and library as a whole: every quad loads, every part is in the ledger.
use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn files(dir: impl AsRef<Path>, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(Path::new(ROOT).join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    out.sort();
    out
}

#[test]
fn every_shipped_quad_loads_and_validates() {
    let quads = files("quads", "toml");
    assert!(quads.len() >= 8, "five freestyle variants plus the toothpick, the race quad and the long-range quad: {quads:?}");
    for path in quads {
        let cfg = ofs_config::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(cfg.validate().is_empty(), "{}: {:?}", path.display(), cfg.validate());
    }
}

#[test]
fn every_library_part_is_listed_in_the_ledger() {
    let ledger = fs::read_to_string(Path::new(ROOT).join("docs/research/component-library.md")).unwrap();
    for kind in fs::read_dir(Path::new(ROOT).join("library")).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n != "README.md") {
        for part in files(Path::new("library").join(&kind), "toml") {
            let heading = format!("## `{}/{}`", kind.to_str().unwrap(), part.file_name().unwrap().to_str().unwrap());
            assert!(ledger.contains(&heading), "the ledger has no section {heading}");
        }
    }
}

#[test]
fn the_new_quads_differ_in_the_ways_their_parts_explain() {
    let load = |stem: &str| ofs_config::load(&Path::new(ROOT).join(format!("quads/{stem}.toml"))).unwrap();
    let (t, r, l, f) = (load("opendrone-3t-toothpick"), load("opendrone-5r-race"), load("opendrone-7lr-longrange"), load("opendrone-5f-freestyle"));
    assert_eq!([t.battery.cells, r.battery.cells, l.battery.cells], [3, 6, 6]);
    assert!(t.frame.mass_kg < 0.15 && r.frame.mass_kg < f.frame.mass_kg && l.frame.mass_kg > f.frame.mass_kg);
    assert!(l.battery.capacity_mah > 3.0 * f.battery.capacity_mah, "the long-range pack is the big one");
    assert!(l.props[0].diameter_m > f.props[0].diameter_m && f.props[0].diameter_m > t.props[0].diameter_m);
}
