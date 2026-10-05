use std::fs;
use std::path::Path;

use ofs_config::{load, ConfigError, FcKind};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap()
}

fn write_quad(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.join("opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    path
}

#[test]
fn reference_quad_loads_and_validates() {
    let cfg = load(Path::new(QUAD)).unwrap();
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.frame.motor_positions_frd_m.len(), 4);
    assert_eq!(cfg.fc.kind, FcKind::Sitl);
    assert!(cfg.validate().is_empty());
}

#[test]
fn every_problem_is_reported_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("mass_kg = 0.65", "mass_kg = -1.0")
        .replace("initial_soc = 1.0", "initial_soc = 2.0")
        .replace("motor_spin = [1, -1, -1, 1]", "motor_spin = [1, -1, -1, 2]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"frame.mass_kg"), "{fields:?}");
    assert!(fields.contains(&"battery.initial_soc"), "{fields:?}");
    assert!(fields.contains(&"frame.motor_spin"), "{fields:?}");
    let shown = err.to_string();
    assert!(shown.contains("frame.mass_kg") && shown.contains("battery.initial_soc"));
}

#[test]
fn unknown_field_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("mass_kg = 0.65", "mass_kg = 0.65\nmas_kg = 1.0");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }));
    assert!(err.to_string().contains("mas_kg"), "{err}");
}

#[test]
fn unsupported_schema_version_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 1", "schema_version = 2");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 2"), "{err}");
}

#[test]
fn diff_path_resolves_relative_to_quad_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.resolve(&cfg.fc.betaflight_diff), dir.path().join("opendrone-5f-freestyle.betaflight.diff"));
}

#[test]
fn missing_diff_file_is_reported_for_sitl_quads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::remove_file(dir.path().join("opendrone-5f-freestyle.betaflight.diff")).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("fc.betaflight_diff"), "{err}");
}

#[test]
fn zero_fc_timeouts_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("first_reply_timeout_ms = 5000", "first_reply_timeout_ms = 0")
        .replace("\nreply_timeout_ms = 500", "\nreply_timeout_ms = 0")
        .replace("startup_timeout_ms = 15000", "startup_timeout_ms = 0");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["fc.reply_timeout_ms", "fc.first_reply_timeout_ms", "fc.startup_timeout_ms"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
}
