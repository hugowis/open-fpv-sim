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
fn the_hd_quad_uses_the_hd_canvas_and_the_others_stay_sd() {
    let load = |stem: &str| ofs_config::load(&Path::new(ROOT).join(format!("quads/{stem}.toml"))).unwrap();
    let hd = load("opendrone-5f-hd");
    let hd_osd = hd.osd.as_ref().expect("the HD quad has an OSD");
    assert_eq!((hd_osd.cols, hd_osd.rows), (53, 20), "the pinned SITL's HD canvas");
    let diff = fs::read_to_string(Path::new(ROOT).join("quads/opendrone-5f-hd.betaflight.diff")).unwrap();
    assert!(diff.contains("set vcd_video_system = HD"), "the HD quad selects the HD video system");
    const SYS_NAMES: [&str; 11] = [
        "goggle_voltage", "vtx_voltage", "bitrate", "delay", "distance", "lq", "goggle_dvr", "vtx_dvr", "warnings",
        "vtx_temp", "fan_speed",
    ];
    for name in SYS_NAMES {
        assert!(diff.contains(&format!("set osd_sys_{name}_pos")), "the diff places {name}");
    }
    // Profile 1 (2048) + col 40 (the XHD bit 1024 + 8): a column past 31 needs the extra bit, on every marker.
    for name in SYS_NAMES {
        let prefix = format!("set osd_sys_{name}_pos = ");
        let line = diff.lines().find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("the diff places {name}"));
        let value: i64 = line[prefix.len()..].trim().parse().unwrap_or_else(|_| panic!("{name}'s position is a number: {line}"));
        assert_ne!(value & 1024, 0, "{name} sits at column 40 and carries the XHD bit");
        assert_eq!(value >> 11, 1, "{name} is placed on profile 1");
    }

    for stem in ["opendrone-5f-freestyle", "opendrone-5r-race", "opendrone-3t-toothpick"] {
        let sd = load(stem);
        let sd_osd = sd.osd.as_ref().expect("an SD quad has an OSD");
        assert_eq!((sd_osd.cols, sd_osd.rows), (30, 16), "{stem} keeps the 30 x 16 PAL grid");
        let sd_diff = fs::read_to_string(Path::new(ROOT).join(format!("quads/{stem}.betaflight.diff"))).unwrap();
        assert!(sd_diff.contains("set vcd_video_system = PAL"), "{stem} keeps PAL");
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
