use std::fs;
use std::path::Path;

#[test]
fn no_simulation_crate_depends_on_audio() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for entry in fs::read_dir(crates).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
        if !name.starts_with("ofs-") || ["ofs-audio", "ofs-client", "ofs-godot"].contains(&name.as_str()) {
            continue;
        }
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(!manifest.contains("ofs-audio"), "{name} depends on ofs-audio");
    }

    let own = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps: Vec<&str> = own
        .lines()
        .skip_while(|l| l.trim() != "[dependencies]")
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert!(deps.is_empty(), "ofs-audio must have no dependencies: {deps:?}");
}
