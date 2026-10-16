use std::fs;
use std::path::Path;

use ofs_config::{load, ConfigError, FcKind, RadioKind, VtxKind};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library");

fn quad_text() -> String {
    fs::read_to_string(QUAD).unwrap().replace("\r\n", "\n")
}

fn part_text(kind: &str, id: &str) -> String {
    fs::read_to_string(Path::new(LIBRARY).join(kind).join(format!("{id}.toml"))).unwrap().replace("\r\n", "\n")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// The quad at {dir}/quads/quad.toml, its diff beside it, the shipped library at {dir}/library/.
/// Re-copies the library every call — mutate parts with `write_part` only AFTER this, never before.
fn write_quad(dir: &Path, text: &str) -> std::path::PathBuf {
    fs::create_dir_all(dir.join("quads")).unwrap();
    let path = dir.join("quads/quad.toml");
    fs::write(&path, text).unwrap();
    fs::write(dir.join("quads/opendrone-5f-freestyle.betaflight.diff"), "feature -GPS\n").unwrap();
    copy_dir(Path::new(LIBRARY), &dir.join("library"));
    path
}

/// Replaces one part file's text (after `write_quad`).
fn write_part(dir: &Path, kind: &str, id: &str, text: &str) {
    fs::write(dir.join("library").join(kind).join(format!("{id}.toml")), text).unwrap();
}

#[test]
fn reference_quad_loads_and_validates() {
    let cfg = load(Path::new(QUAD)).unwrap();
    assert_eq!(cfg.schema_version, 5);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart), (500, 2));
    assert_eq!(cfg.frame.motor_positions_frd_m.len(), 4);
    assert_eq!(cfg.motors.len(), 4, "the single motor part broadcasts to every motor");
    assert_eq!(cfg.motors[0].pole_pairs, 7);
    assert_eq!(cfg.parts.battery, "opendrone-6s-1300");
    assert_eq!(cfg.fc.kind, FcKind::Sitl);
    assert_eq!(cfg.vtx.as_ref().unwrap().kind, VtxKind::Smartaudio);
    assert!(cfg.validate().is_empty());
}

#[test]
fn a_schema_4_quad_file_is_refused_with_the_migration_message() {
    let dir = tempfile::tempdir().unwrap();
    let old = quad_text()
        .replace("schema_version = 5", "schema_version = 4")
        .replace("frame = \"opendrone-5f\"", "[frame]\nmass_kg = 0.65");
    let err = load(&write_quad(dir.path(), &old)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("schema_version 4"), "{message}");
    assert!(message.contains("library"), "{message}");
}

#[test]
fn an_unknown_part_id_lists_what_the_directory_holds() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]", "motors = [\"no-such-motor\"]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("no-such-motor"), "{err}");
    assert!(err.to_string().contains("opendrone-openmotor-2207"), "lists the ids it holds: {err}");
}

#[test]
fn a_missing_kind_directory_names_the_library_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::remove_dir_all(dir.path().join("library/frames")).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("frames"), "{err}");
    assert!(err.to_string().contains("OFS_LIBRARY"), "{err}");
}

#[test]
fn a_wrong_length_part_list_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]",
                                   "motors = [\"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\"]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("motors"), "{err}");
    assert!(err.to_string().contains("1 or 4"), "{err}");
}

#[test]
fn the_library_root_honours_the_override() {
    let quad_dir = Path::new("C:/anywhere/quads");
    assert_eq!(ofs_config::library_root(None, quad_dir), Path::new("C:/anywhere/library"));
    assert_eq!(ofs_config::library_root(Some("C:/my-library"), quad_dir), Path::new("C:/my-library"));
}

#[test]
fn a_missing_vtx_part_is_reported_on_the_vtx_section() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("part = \"opendrone-smartaudio-1w\"", "part = \"no-such-vtx\"");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("vtx.part"), "{err}");
}

#[test]
fn frame_part_problems_are_reported_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = part_text("frames", "opendrone-5f")
        .replace("mass_kg = 0.65", "mass_kg = -1.0")
        .replace("motor_spin = [1, -1, -1, 1]", "motor_spin = [1, -1, -1, 2]");
    write_part(dir.path(), "frames", "opendrone-5f", &part);
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"frame.mass_kg"), "{fields:?}");
    assert!(fields.contains(&"frame.motor_spin"), "{fields:?}");
    let shown = err.to_string();
    assert!(shown.contains("frame.mass_kg") && shown.contains("frame.motor_spin"));
}

#[test]
fn every_problem_is_reported_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("initial_soc = 1.0", "initial_soc = 2.0")
        .replace("packet_rate_hz = 500", "packet_rate_hz = 333");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"battery.initial_soc"), "{fields:?}");
    assert!(fields.contains(&"radio.packet_rate_hz"), "{fields:?}");
    let shown = err.to_string();
    assert!(shown.contains("battery.initial_soc") && shown.contains("radio.packet_rate_hz"));
}

#[test]
fn unknown_field_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = part_text("frames", "opendrone-5f").replace("mass_kg = 0.65", "mass_kg = 0.65\nmas_kg = 1.0");
    write_part(dir.path(), "frames", "opendrone-5f", &part);
    let err = load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }));
    assert!(err.to_string().contains("mas_kg"), "{err}");
}

#[test]
fn unsupported_schema_version_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("schema_version = 5", "schema_version = 4");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 4"), "{err}");
    assert!(err.to_string().contains("library"), "{err}");
    let text = quad_text().replace("schema_version = 5", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 1"), "{err}");
    assert!(err.to_string().contains("radio.rssi_dbm") && err.to_string().contains("[collision]"), "{err}");
}

#[test]
fn diff_path_resolves_relative_to_quad_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.resolve(&cfg.fc.betaflight_diff), dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff"));
}

#[test]
fn missing_diff_file_is_reported_for_sitl_quads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::remove_file(dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff")).unwrap();
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
        .replace("tx_power_mw = 250", "tx_power_mw = 300");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["radio.packet_rate_hz", "radio.uart", "radio.tx_power_mw"] {
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

#[test]
fn the_reference_quad_has_the_video_sections() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = load(&write_quad(dir.path(), &quad_text())).unwrap();
    assert_eq!(cfg.schema_version, 5);
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
fn video_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("[osd]\nuart = 4", "[osd]\nuart = 3")           // collides with [esc_telemetry]
        .replace("default_band = \"R\"", "default_band = \"Z\"");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"osd.uart"), "{fields:?}");
    assert!(fields.contains(&"vtx.default_band"), "{fields:?}");
}

#[test]
fn vtx_part_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = part_text("vtx", "opendrone-smartaudio-1w").replace("power_levels_dbm = [14, 23, 28, 30]", "power_levels_dbm = [14, 23, 28]");
    write_part(dir.path(), "vtx", "opendrone-smartaudio-1w", &part);
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.contains(&"vtx.power_levels_dbm"), "{fields:?}");
}

/// `write_quad` writes a stub diff; this one enables SmartAudio on UART5 (serial index 4, function 2048).
const SMARTAUDIO_DIFF: &str = "feature -GPS\nserial 4 2048 115200 57600 0 115200\n";

#[test]
fn a_diff_that_enables_smartaudio_needs_a_vtx_section() {
    let dir = tempfile::tempdir().unwrap();
    let without_vtx = quad_text().split("\n[vtx]").next().unwrap().to_string() + "\n";
    let path = write_quad(dir.path(), &without_vtx);
    fs::write(dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("SmartAudio") && err.to_string().contains("[vtx]"), "{err}");
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    assert!(problems.iter().any(|p| p.field == "fc.betaflight_diff"), "{problems:?}");
}

#[test]
fn a_smartaudio_diff_with_a_matching_vtx_section_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    fs::write(dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    assert!(load(&path).is_ok(), "the shipped quad has [vtx] uart = 5");
    let elsewhere = quad_text().replace("[vtx]\npart = \"opendrone-smartaudio-1w\"\nuart = 5", "[vtx]\npart = \"opendrone-smartaudio-1w\"\nuart = 6");
    let path = write_quad(dir.path(), &elsewhere);
    fs::write(dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff"), SMARTAUDIO_DIFF).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("UART5"), "a VTX on another UART than the diff's SmartAudio port: {err}");
}

#[test]
fn a_video_uart_outside_the_sitl_range_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("[vtx]\npart = \"opendrone-smartaudio-1w\"\nuart = 5", "[vtx]\npart = \"opendrone-smartaudio-1w\"\nuart = 1");
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
        .replace("gyro_bias_radps = [0.0, 0.0, 0.0]", "gyro_bias_radps = [0.0, nan, 0.0]")
        .replace("accel_bias_mps2 = [0.0, 0.0, 0.0]", "accel_bias_mps2 = [-inf, 0.0, 0.0]")
        .replace("position_ned_m = [0.0, 0.0, -0.03]", "position_ned_m = [0.0, nan, -0.03]")
        .replace("yaw_deg = 0.0", "yaw_deg = inf")
        .replace("alt_m = 30.0", "alt_m = nan");
    let fields = invalid_fields(&text);
    for field in ["imu.gyro_bias_radps", "imu.accel_bias_mps2", "initial.position_ned_m", "initial.yaw_deg", "home.alt_m"] {
        assert!(fields.iter().any(|f| f == field), "{field} missing from {fields:?}");
    }
}

#[test]
fn non_finite_part_vectors_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = part_text("frames", "opendrone-5f")
        .replace("motor_positions_frd_m = [[-0.08, 0.08, 0.0]", "motor_positions_frd_m = [[nan, 0.08, 0.0]")
        .replace("contact_points_frd_m = [[-0.08, 0.08, 0.03]", "contact_points_frd_m = [[-0.08, inf, 0.03]");
    write_part(dir.path(), "frames", "opendrone-5f", &part);
    let fields: Vec<String> = match load(&path) {
        Err(ConfigError::Invalid { problems, .. }) => problems.into_iter().map(|p| p.field).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    };
    for field in ["frame.motor_positions_frd_m", "frame.contact_points_frd_m"] {
        assert!(fields.iter().any(|f| f == field), "{field} missing from {fields:?}");
    }
}

#[test]
fn an_empty_motor_list_on_a_frame_part_is_a_collected_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let emptied = part_text("frames", "opendrone-5f")
        .replace("motor_positions_frd_m = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]]",
                 "motor_positions_frd_m = []");
    write_part(dir.path(), "frames", "opendrone-5f", &emptied);
    let err = load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Invalid { .. }), "{err}");
    assert!(err.to_string().contains("motor_positions_frd_m"), "{err}");
}

/// The shipped diff's video lines, without the SmartAudio port.
const ESC_OSD_DIFF: &str = "feature ESC_SENSOR\nserial 2 1024 115200 57600 0 115200\nset force_battery_cell_count = 6\n\
                            feature OSD\nserial 3 131073 115200 57600 0 115200\nset osd_displayport_device = MSP\n";

fn with_diff(text: &str, diff: &str) -> Result<ofs_config::QuadConfig, ConfigError> {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), text);
    fs::write(dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff"), diff).unwrap();
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
    let text = quad_text().replace("schema_version = 5", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err().to_string();
    assert!(!err.contains("  "), "run-on spaces in: {err}");
}

const SHIPPED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

/// The shipped quad with `edit` applied, loaded from a temporary file.
fn shipped_with(edit: impl FnOnce(&mut String)) -> Result<ofs_config::QuadConfig, ofs_config::ConfigError> {
    let mut text = std::fs::read_to_string(Path::new(SHIPPED)).unwrap();
    edit(&mut text);
    let dir = tempfile::tempdir().unwrap();
    ofs_config::load(&write_quad(dir.path(), &text))
}

#[test]
fn a_schema_3_quad_file_is_refused_with_the_field_changes() {
    let err = shipped_with(|t| *t = t.replace("schema_version = 5", "schema_version = 3")).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("schema_version 3"), "{message}");
    assert!(message.contains("radio.rssi_dbm"), "{message}");
    assert!(message.contains("radio.snr_db"), "{message}");
    assert!(message.contains("radio.loss_good"), "{message}");
    assert!(message.contains("radio.tx_power_mw"), "{message}");
    assert!(message.contains("radio.antennas"), "{message}");
    assert!(message.contains("[collision]"), "{message}");
}

#[test]
fn the_old_radio_fields_are_unknown_in_schema_5() {
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 250\nrssi_dbm = -50.0")).unwrap_err();
    assert!(err.to_string().contains("unknown field `rssi_dbm`"), "{}", err);
}

#[test]
fn packet_rate_must_be_an_elrs_lora_mode() {
    for rate in [10, 75, 100, 1000] {
        let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", &format!("packet_rate_hz = {rate}"))).unwrap_err();
        assert!(err.to_string().contains("radio.packet_rate_hz"), "{rate}: {err}");
    }
    assert!(shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "packet_rate_hz = 250")).is_ok());
}

#[test]
fn tx_power_must_be_a_real_handset_power() {
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 300")).unwrap_err();
    assert!(err.to_string().contains("radio.tx_power_mw"), "{err}");
    assert!(shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 10")).is_ok());
}

#[test]
fn radio_antennas_are_validated() {
    let three = "[[radio.antennas]]\nname = \"b\"\n\n[[radio.antennas]]\nname = \"c\"\n\n[[radio.antennas]]\nname = \"d\"\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{three}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas"), "{err}");
    let patch = "[[radio.antennas]]\nname = \"patchy\"\nkind = \"patch\"\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{patch}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas[0].kind"), "{err}");
    let flat = "[[radio.antennas]]\nname = \"flat\"\nmount_frd = [0.0, 0.0, 0.0]\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{flat}"))).unwrap_err();
    assert!(err.to_string().contains("radio.antennas[0].mount_frd"), "{err}");
    let twin = "[[radio.antennas]]\nname = \"a\"\n\n[[radio.antennas]]\nname = \"a\"\n";
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n{twin}"))).unwrap_err();
    assert!(err.to_string().contains("used twice"), "{err}");
    // Two named dipoles are fine: receiver diversity.
    assert!(shipped_with(|t| {
        *t = t.replace("tx_power_mw = 250", "tx_power_mw = 250\n\n[[radio.antennas]]\nname = \"left\"\n\n[[radio.antennas]]\nname = \"right\"");
    })
    .is_ok());
}

#[test]
fn the_shipped_quad_gets_25_collision_spheres_with_no_gap_for_a_post() {
    let cfg = ofs_config::load(Path::new(SHIPPED)).unwrap();
    let spheres = cfg.collision_spheres();
    assert_eq!(spheres.len(), 25, "6 spheres per prop tip circle + one body sphere");
    let (restitution, friction) = cfg.collision_contact();
    assert_eq!((restitution, friction), (0.3, 0.5), "the spec's defaults");
    let tip = cfg.props[0].diameter_m / 2.0;
    for m in &cfg.frame.motor_positions_frd_m {
        let mut ring: Vec<[f64; 4]> = spheres
            .iter()
            .filter(|s| (s[2] - m[2]).abs() < 1e-9 && (s[0] - m[0]).hypot(s[1] - m[1]) <= tip + 1e-9)
            .copied()
            .collect();
        ring.sort_by(|a, b| (a[0] - m[0]).atan2(a[1] - m[1]).total_cmp(&(b[0] - m[0]).atan2(b[1] - m[1])));
        assert_eq!(ring.len(), 6, "six spheres around each motor");
        for w in ring.windows(2) {
            let d = (w[0][0] - w[1][0]).hypot(w[0][1] - w[1][1]);
            assert!((d - tip).abs() < 1e-6, "adjacent spheres are {d} apart, the 60 degree chord is {tip}");
            assert!(d - w[0][3] - w[1][3] < 0.12, "no 12 cm post passes between two prop spheres");
        }
    }
    let body = spheres.last().unwrap();
    assert_eq!((body[0], body[1], body[2], body[3]), (0.0, 0.0, 0.0, 0.04), "the body sphere sits at the centre of mass");
    for s in &spheres[..spheres.len() - 1] {
        let d = ((s[0] - body[0]).powi(2) + (s[1] - body[1]).powi(2) + (s[2] - body[2]).powi(2)).sqrt();
        assert!(d - s[3] - body[3] < 0.12, "no 12 cm post passes between a prop sphere and the body sphere");
    }
}

/// The frame part's `[collision]` lines plus the provenance its parameters need.
fn frame_part_with_collision(collision: &str) -> String {
    part_text("frames", "opendrone-5f").replace(
        "[provenance.\"frame.mass_kg\"]",
        &format!("[collision]\n{collision}\n\n[provenance.\"collision.restitution\"]\nsource = \"estimated\"\n\
                  [provenance.\"collision.friction_coeff\"]\nsource = \"estimated\"\n\
                  [provenance.\"collision.spheres_frd_m\"]\nsource = \"estimated\"\n\
                  \n[provenance.\"frame.mass_kg\"]"),
    )
}

#[test]
fn a_collision_section_is_honoured() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    write_part(dir.path(), "frames", "opendrone-5f", &frame_part_with_collision("restitution = 0.7\nfriction_coeff = 0.2\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.05]]"));
    let cfg = load(&path).unwrap();
    assert_eq!(cfg.collision_spheres(), vec![[0.0, 0.0, 0.0, 0.05]]);
    assert_eq!(cfg.collision_contact(), (0.7, 0.2));
}

#[test]
fn collision_validation_problems_are_collected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let collision = "restitution = 1.5\nfriction_coeff = -1.0\nspheres_frd_m = [[0.0, 0.0, 0.0, 0.0], [nan, 0.0, 0.0, 0.02]]";
    write_part(dir.path(), "frames", "opendrone-5f", &frame_part_with_collision(collision));
    let err = load(&path).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("collision.restitution"), "{message}");
    assert!(message.contains("collision.friction_coeff"), "{message}");
    assert!(message.contains("collision.spheres_frd_m"), "{message}");
}

#[test]
fn a_diffs_motor_poles_must_match_the_motors() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let diff = dir.path().join("quads/opendrone-5f-freestyle.betaflight.diff");
    let mismatched = "feature -GPS\nset motor_poles = 12\n";
    fs::write(&diff, mismatched).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("motor_poles"), "{err}");
    assert!(err.to_string().contains("14"), "names the motors' pole count: {err}");
    fs::write(&diff, "feature -GPS\nset motor_poles = 14\n").unwrap();
    assert!(load(&path).is_ok());
}

#[test]
fn pole_pairs_without_a_diff_line_must_match_the_default_14() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text()); // the library lands first; mutate parts after it
    let six = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7", "pole_pairs = 6");
    write_part(dir.path(), "motors", "opendrone-openmotor-2207", &six);
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("motor_poles"), "{err}");
    assert!(err.to_string().contains("12 poles"), "{err}");
}

#[test]
fn mixed_pole_pairs_on_a_sitl_quad_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]",
        "motors = [\"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\", \"opendrone-openmotor-2207\", \"opendrone-openmotor-6pp\"]");
    let path = write_quad(dir.path(), &text);
    let six = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7", "pole_pairs = 6");
    write_part(dir.path(), "motors", "opendrone-openmotor-6pp", &six);
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("uniform"), "{err}");
}

#[test]
fn a_motor_part_without_pole_pairs_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let missing = part_text("motors", "opendrone-openmotor-2207").replace("pole_pairs = 7\n", "\n");
    write_part(dir.path(), "motors", "opendrone-openmotor-2207", &missing);
    let err = load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("pole_pairs"), "{err}");
}

#[test]
fn the_900_mhz_band_takes_its_own_packet_rates() {
    let ok = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "band = \"900\"\npacket_rate_hz = 200")).unwrap();
    assert_eq!((ok.radio.band_or_default(), ok.radio.packet_rate_hz), (ofs_config::BandKind::Mhz900, 200));
    for rate in [500, 150, 30] {
        let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", &format!("band = \"900\"\npacket_rate_hz = {rate}"))).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("radio.packet_rate_hz") && message.contains("25, 50, 100, 200 or 250"), "{rate}: {message}");
    }
    let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "packet_rate_hz = 200")).unwrap_err();
    assert!(err.to_string().contains("50, 150, 250 or 500"), "the default band is 2.4 GHz: {err}");
}

#[test]
fn an_unknown_band_is_a_parse_error_listing_the_names() {
    let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "band = \"5g8\"\npacket_rate_hz = 500")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `5g8`") && err.to_string().contains("900"), "{err}");
}

#[test]
fn a_fixed_link_takes_its_own_table_and_any_dividing_rate() {
    let cfg = shipped_with(|t| {
        *t = t
            .replace("kind = \"elrs\"", "kind = \"fixed\"")
            .replace("packet_rate_hz = 500", "packet_rate_hz = 400")
            .replace("tx_power_mw = 250", "tx_power_mw = 250\n\n[radio.fixed]\nloss_fraction = 0.1\nburst_on_s = 0.05\nburst_off_s = 0.2");
    })
    .unwrap();
    assert_eq!(cfg.radio.kind, RadioKind::Fixed);
    let f = cfg.radio.fixed.unwrap();
    assert_eq!((f.loss_fraction, f.burst_on_s, f.burst_off_s), (0.1, 0.05, 0.2));
}

#[test]
fn keys_that_belong_to_the_other_link_are_errors_not_ignored() {
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"\nband = \"900\"")).unwrap_err();
    assert!(err.to_string().contains("radio.band"), "{err}");
    let err = shipped_with(|t| *t = t.replace("tx_power_mw = 250", "tx_power_mw = 250\n\n[radio.fixed]\nloss_fraction = 0.1")).unwrap_err();
    assert!(err.to_string().contains("radio.fixed"), "{err}");
}

#[test]
fn fixed_link_values_are_validated() {
    let with = |fixed: &str| shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"").replace("tx_power_mw = 250", &format!("tx_power_mw = 250\n\n[radio.fixed]\n{fixed}")));
    assert!(with("loss_fraction = 1.5").unwrap_err().to_string().contains("radio.fixed.loss_fraction"));
    assert!(with("loss_fraction = -0.1").unwrap_err().to_string().contains("radio.fixed.loss_fraction"));
    assert!(with("burst_on_s = 0.1").unwrap_err().to_string().contains("radio.fixed.burst_off_s"));
    assert!(with("burst_on_s = -1.0").unwrap_err().to_string().contains("radio.fixed.burst_on_s"));
    assert!(with("loss_fraction = 0.0").is_ok());
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"").replace("packet_rate_hz = 500", "packet_rate_hz = 300")).unwrap_err();
    assert!(err.to_string().contains("radio.packet_rate_hz"), "300 does not divide 8000: {err}");
}
