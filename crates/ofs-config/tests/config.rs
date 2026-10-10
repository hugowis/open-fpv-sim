use std::fs;
use std::path::Path;

use ofs_config::{load, ConfigError, FcKind, RadioKind};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap().replace("\r\n", "\n")
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
    assert_eq!(cfg.schema_version, 3);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart), (500, 2));
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
    let text = quad_text().replace("schema_version = 3", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 1"), "{err}");
    assert!(err.to_string().contains("[radio]") && err.to_string().contains("betaflight.diff"), "{err}");
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

#[test]
fn radio_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("packet_rate_hz = 500", "packet_rate_hz = 333")
        .replace("uart = 2", "uart = 1")
        .replace("rssi_dbm = -50.0", "rssi_dbm = -50.0\nloss_good = 1.5");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["radio.packet_rate_hz", "radio.uart", "radio.loss_good"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
}

#[test]
fn a_quad_without_a_radio_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text();
    let without = &text[..text.find("[radio]").unwrap()];
    let err = load(&write_quad(dir.path(), without)).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("radio"), "{err}");
}

fn schema_2_text() -> String {
    let text = quad_text().replace("schema_version = 3", "schema_version = 2");
    text.split("\n[esc_telemetry]").next().unwrap().to_string() + "\n"
}

#[test]
fn the_reference_quad_has_the_video_sections() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &quad_text())).unwrap();
    assert_eq!(cfg.schema_version, 3);
    let esc = cfg.esc_telemetry.as_ref().expect("[esc_telemetry]");
    assert_eq!((esc.uart, esc.rate_hz), (3, 100));
    let osd = cfg.osd.as_ref().expect("[osd]");
    assert_eq!((osd.uart, osd.cols, osd.rows), (4, 30, 16));
    let vtx = cfg.vtx.as_ref().expect("[vtx]");
    assert_eq!((vtx.uart, vtx.default_band.as_str(), vtx.default_channel), (5, "R", 1));
    assert_eq!(vtx.power_levels_mw, vec![25, 200, 600, 1000]);
    assert_eq!(vtx.power_levels_dbm, vec![14, 23, 28, 30]);
    assert_eq!(vtx.default_power_index, 1);
}

#[test]
fn schema_2_files_still_load_without_the_video_sections() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &schema_2_text())).unwrap();
    assert_eq!(cfg.schema_version, 2);
    assert!(cfg.esc_telemetry.is_none() && cfg.osd.is_none() && cfg.vtx.is_none());
}

#[test]
fn video_sections_need_schema_3() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 3", "schema_version = 2");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version = 3"), "{err}");
}

#[test]
fn video_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("[osd]\nuart = 4", "[osd]\nuart = 3")           // collides with [esc_telemetry]
        .replace("default_band = \"R\"", "default_band = \"Z\"")
        .replace("power_levels_dbm = [14, 23, 28, 30]", "power_levels_dbm = [14, 23, 28]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"osd.uart"), "{fields:?}");
    assert!(fields.contains(&"vtx.default_band"), "{fields:?}");
    assert!(fields.contains(&"vtx.power_levels_dbm"), "{fields:?}");
}

/// `write_quad` writes a stub diff; this one enables SmartAudio on UART5 (serial index 4, function 2048).
const SMARTAUDIO_DIFF: &str = "feature -GPS\nserial 4 2048 115200 57600 0 115200\n";

#[test]
fn a_diff_that_enables_smartaudio_needs_a_vtx_section() {
    let dir = tempfile::tempdir().unwrap();
    let without_vtx = quad_text().split("\n[vtx]").next().unwrap().to_string() + "\n";
    let path = write_quad(dir.path(), &without_vtx);
    fs::write(dir.path().join("opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("SmartAudio") && err.to_string().contains("[vtx]"), "{err}");
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    assert!(problems.iter().any(|p| p.field == "fc.betaflight_diff"), "{problems:?}");
}

#[test]
fn a_smartaudio_diff_with_a_matching_vtx_section_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::write(dir.path().join("opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    assert!(load(&path).is_ok(), "the shipped quad has [vtx] uart = 5");
    let elsewhere = quad_text().replace("[vtx]\nkind = \"smartaudio\"\nuart = 5", "[vtx]\nkind = \"smartaudio\"\nuart = 6");
    let path = write_quad(dir.path(), &elsewhere);
    fs::write(dir.path().join("opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("UART5"), "a VTX on another UART than the diff's SmartAudio port: {err}");
}

#[test]
fn a_video_uart_outside_the_sitl_range_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("[vtx]\nkind = \"smartaudio\"\nuart = 5", "[vtx]\nkind = \"smartaudio\"\nuart = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("vtx.uart"), "{err}");
}

fn invalid_fields(text: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    match load(&write_quad(dir.path(), text)) {
        Err(ConfigError::Invalid { problems, .. }) => problems.into_iter().map(|p| p.field).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn non_finite_vectors_are_rejected() {
    let text = quad_text()
        .replace("motor_positions_frd_m = [[-0.08, 0.08, 0.0]", "motor_positions_frd_m = [[nan, 0.08, 0.0]")
        .replace("contact_points_frd_m = [[-0.08, 0.08, 0.03]", "contact_points_frd_m = [[-0.08, inf, 0.03]")
        .replace("gyro_bias_radps = [0.0, 0.0, 0.0]", "gyro_bias_radps = [0.0, nan, 0.0]")
        .replace("accel_bias_mps2 = [0.0, 0.0, 0.0]", "accel_bias_mps2 = [-inf, 0.0, 0.0]")
        .replace("position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [0.0, nan, -0.03]")
        .replace("yaw_deg = 0.0", "yaw_deg = inf")
        .replace("alt_m = 30.0", "alt_m = nan");
    let fields = invalid_fields(&text);
    for field in [
        "frame.motor_positions_frd_m",
        "frame.contact_points_frd_m",
        "imu.gyro_bias_radps",
        "imu.accel_bias_mps2",
        "initial.position_ned_m",
        "initial.yaw_deg",
        "home.alt_m",
    ] {
        assert!(fields.iter().any(|f| f == field), "{field} missing from {fields:?}");
    }
}

/// The shipped diff's video lines, without the SmartAudio port.
const ESC_OSD_DIFF: &str = "feature ESC_SENSOR\nserial 2 1024 115200 57600 0 115200\nset force_battery_cell_count = 6\n\
                            feature OSD\nserial 3 131073 115200 57600 0 115200\nset osd_displayport_device = MSP\n";

fn with_diff(text: &str, diff: &str) -> Result<ofs_config::QuadConfig, ConfigError> {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), text);
    fs::write(dir.path().join("opendrone-5f-freestyle.betaflight.diff"), diff).unwrap();
    load(&path)
}

#[test]
fn a_diff_that_enables_the_esc_sensor_needs_an_esc_telemetry_section_on_that_uart() {
    assert!(with_diff(&quad_text(), ESC_OSD_DIFF).is_ok(), "the shipped quad has [esc_telemetry] uart = 3 and [osd] uart = 4");
    let without = quad_text().replace("[esc_telemetry]\nuart = 3\nrate_hz = 100\n", "");
    assert!(without.len() < quad_text().len(), "the test edit must remove the section");
    let err = with_diff(&without, ESC_OSD_DIFF).unwrap_err();
    assert!(err.to_string().contains("ESC sensor") && err.to_string().contains("[esc_telemetry]"), "{err}");
}

#[test]
fn a_diff_that_sends_the_osd_over_msp_needs_an_osd_section_on_that_uart() {
    let elsewhere = quad_text().replace("[osd]\nuart = 4", "[osd]\nuart = 6");
    let err = with_diff(&elsewhere, ESC_OSD_DIFF).unwrap_err();
    assert!(err.to_string().contains("OSD") && err.to_string().contains("UART4"), "{err}");
}

#[test]
fn an_esc_sensor_battery_needs_the_forced_cell_count() {
    let missing = ESC_OSD_DIFF.replace("set force_battery_cell_count = 6\n", "");
    let err = with_diff(&quad_text(), &missing).unwrap_err();
    assert!(err.to_string().contains("force_battery_cell_count = 6"), "{err}");
    let wrong = ESC_OSD_DIFF.replace("= 6", "= 4");
    let err = with_diff(&quad_text(), &wrong).unwrap_err();
    assert!(err.to_string().contains("battery.cells"), "{err}");
}

#[test]
fn the_schema_1_hint_reads_as_one_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 3", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err().to_string();
    assert!(!err.contains("  "), "run-on spaces in: {err}");
}
