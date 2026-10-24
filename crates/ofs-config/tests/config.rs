use std::fs;
use std::path::{Path, PathBuf};

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

fn search_path(roots: &[&Path]) -> String {
    roots.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join(if cfg!(windows) { ";" } else { ":" })
}

fn write_root_part(root: &Path, kind: &str, id: &str, text: &str) {
    fs::write(root.join(kind).join(format!("{id}.toml")), text).unwrap();
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
    assert_eq!(cfg.radio.telemetry_slot_divisor, 4);
    assert_eq!(cfg.frame.motor_positions_frd_m.len(), 4);
    assert_eq!(cfg.motors.len(), 4, "the single motor part broadcasts to every motor");
    assert_eq!(cfg.motors[0].pole_pairs, 7);
    assert_eq!(cfg.parts.battery, "opendrone-6s-1300");
    assert_eq!(cfg.fc.kind, FcKind::Sitl);
    assert_eq!(cfg.vtx.as_ref().unwrap().kind, VtxKind::Smartaudio);
    assert!(cfg.validate().is_empty());
}

#[test]
fn a_loaded_quad_keeps_each_parts_provenance_once_per_referenced_id() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let kinds: Vec<_> = cfg.part_provenance.iter().map(|p| (p.kind, p.id.as_str())).collect();
    assert_eq!(
        kinds,
        [
            ("frame", "opendrone-5f"),
            ("motor", "opendrone-openmotor-2207"),
            ("prop", "opendrone-5in-prop"),
            ("esc", "opendrone-4in1-esc"),
            ("battery", "opendrone-6s-1300"),
            ("vtx", "opendrone-smartaudio-1w"),
        ],
        "the single motor, prop and esc ids are listed once, not once per position"
    );
    assert!(cfg.part_provenance.iter().all(|p| !p.name.is_empty() && !p.parameters.is_empty()));
    let motor = &cfg.part_provenance[1];
    assert_eq!(motor.parameters["motor.kv_rpm_per_v"].source.as_str(), "estimated");
}

#[test]
fn the_bounding_radius_is_the_farthest_collision_sphere_edge() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let r = cfg.bounding_radius_m();
    assert!((0.1..0.4).contains(&r), "a 5 inch quad is about 0.2 m from its centre to a prop tip: {r}");
    let reach = cfg.collision_spheres().iter().map(|s| (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt() + s[3]).fold(0.0, f64::max);
    assert_eq!(r, reach);
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
    let message = err.to_string();
    assert!(message.contains("frames"), "{message}");
    assert!(message.contains("kind directory not found"), "{message}");
    assert!(message.contains("OFS_LIBRARY"), "{message}");
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
    assert_eq!(ofs_config::library_root(Some("my-library"), quad_dir), Path::new("my-library"));
}

#[test]
fn library_roots_defaults_beside_the_quads_and_ignores_a_blank_override() {
    let quad_dir = Path::new("C:/anywhere/quads");
    assert_eq!(ofs_config::library_roots(None, quad_dir), vec![Path::new("C:/anywhere/library")]);
    assert_eq!(ofs_config::library_roots(Some("  "), quad_dir), vec![Path::new("C:/anywhere/library")]);
}

#[test]
fn library_roots_drops_empty_entries() {
    let quad_dir = Path::new("C:/anywhere/quads");
    #[cfg(windows)]
    {
        let roots = ofs_config::library_roots(Some("C:/one;;C:/two;"), quad_dir);
        assert_eq!(roots, vec![PathBuf::from("C:/one"), PathBuf::from("C:/two")]);
    }
    #[cfg(not(windows))]
    {
        let roots = ofs_config::library_roots(Some("/one::/two:"), quad_dir);
        assert_eq!(roots, vec![PathBuf::from("/one"), PathBuf::from("/two")]);
    }
}

#[test]
fn parts_layer_across_roots_and_the_first_root_wins() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let root_a = dir.path().join("library");
    let root_b = dir.path().join("library-b");
    copy_dir(&root_a, &root_b);

    // Root A has no frames or motors; root B supplies both. The prop exists in both: root A must win.
    fs::remove_dir_all(root_a.join("frames")).unwrap();
    fs::remove_dir_all(root_a.join("motors")).unwrap();
    write_root_part(
        &root_b,
        "frames",
        "opendrone-5f",
        &part_text("frames", "opendrone-5f").replace("mass_kg = 0.65", "mass_kg = 0.75"),
    );
    write_root_part(
        &root_a,
        "props",
        "opendrone-5in-prop",
        &part_text("props", "opendrone-5in-prop").replace("diameter_m = 0.127", "diameter_m = 0.2"),
    );
    write_root_part(
        &root_b,
        "props",
        "opendrone-5in-prop",
        &part_text("props", "opendrone-5in-prop").replace("diameter_m = 0.127", "diameter_m = 0.3"),
    );

    let env = search_path(&[&root_a, &root_b]);
    let cfg = ofs_config::load_with_library_env(&path, Some(&env)).unwrap();
    assert_eq!(cfg.frame.mass_kg, 0.75, "the frame comes from the second root");
    assert_eq!(cfg.props[0].diameter_m, 0.2, "the first root's copy of a part wins");
    assert_eq!(cfg.motors[0].pole_pairs, 7, "the motor comes from the second root");
}

#[test]
fn a_missing_part_names_every_root_and_the_ids_each_has() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text().replace("motors = [\"opendrone-openmotor-2207\"]", "motors = [\"no-such-motor\"]");
    let path = write_quad(dir.path(), &text);
    let root_a = dir.path().join("library");
    let root_b = dir.path().join("library-b");
    copy_dir(&root_a, &root_b);

    let motor = part_text("motors", "opendrone-openmotor-2207");
    fs::remove_file(root_a.join("motors/opendrone-openmotor-2207.toml")).unwrap();
    fs::remove_file(root_b.join("motors/opendrone-openmotor-2207.toml")).unwrap();
    write_root_part(&root_a, "motors", "root-a-only", &motor);
    write_root_part(&root_b, "motors", "root-b-only", &motor);

    let env = search_path(&[&root_a, &root_b]);
    let err = ofs_config::load_with_library_env(&path, Some(&env)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("no-such-motor"), "{message}");
    assert!(message.contains(&root_a.join("motors").display().to_string()), "names root A: {message}");
    assert!(message.contains(&root_b.join("motors").display().to_string()), "names root B: {message}");
    assert!(message.contains("root-a-only"), "lists root A's ids: {message}");
    assert!(message.contains("root-b-only"), "lists root B's ids: {message}");
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
fn windmill_j0_must_not_be_negative() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = part_text("props", "opendrone-5in-prop").replace("windmill_j0 = 1.0", "windmill_j0 = -1.0");
    write_part(dir.path(), "props", "opendrone-5in-prop", &part);
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    assert!(fields.iter().any(|f| f.ends_with("windmill_j0")), "{fields:?}");
}

/// The shipped frame part with a `[frame.fc_mount]` table (and its provenance) appended.
#[test]
fn windmill_j0_must_not_be_tiny() {
    let fields_for = |value: &str| -> Option<Vec<String>> {
        let dir = tempfile::tempdir().unwrap();
        let path = write_quad(dir.path(), &quad_text());
        write_part(dir.path(), "props", "opendrone-5in-prop", &part_text("props", "opendrone-5in-prop").replace("windmill_j0 = 1.0", &format!("windmill_j0 = {value}")));
        match load(&path) {
            Ok(_) => None,
            Err(ConfigError::Invalid { problems, .. }) => Some(problems.into_iter().map(|p| p.field).collect()),
            Err(e) => panic!("{e}"),
        }
    };
    // A windmilling rotor settles at n = inflow / (j0 D): a small j0 is a runaway rotor.
    for tiny in ["0.01", "0.15"] {
        let fields = fields_for(tiny).unwrap_or_else(|| panic!("{tiny} is a runaway rotor and must be refused"));
        assert!(fields.iter().any(|f| f.ends_with("windmill_j0")), "{fields:?}");
    }
    assert!(fields_for("0.2").is_none(), "about half the pitch/diameter of the flattest real prop is the floor");
    assert!(fields_for("0.0").is_none(), "0 turns windmilling off");
    assert!(fields_for("1.0").is_none(), "the shipped value");
}

#[test]
fn a_non_finite_resistance_growth_is_one_problem_not_two() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    write_part(dir.path(), "batteries", "opendrone-6s-1300", &battery_part_with(&[("cycles", "100"), ("resistance_growth_per_cycle", "nan")]));
    let ConfigError::Invalid { problems, .. } = load(&path).unwrap_err() else { panic!("expected Invalid") };
    let growth: Vec<_> = problems.iter().filter(|p| p.field == "battery.resistance_growth_per_cycle").collect();
    assert_eq!(growth.len(), 1, "{growth:?}");
    assert!(growth[0].message.contains(">= 0"), "the finite check speaks, not the cap: {growth:?}");
}

#[test]
fn an_aged_pack_may_not_exceed_ten_times_the_resistance() {
    let load_aged = |growth: &str| {
        let dir = tempfile::tempdir().unwrap();
        let path = write_quad(dir.path(), &quad_text());
        write_part(dir.path(), "batteries", "opendrone-6s-1300", &battery_part_with(&[("cycles", "100"), ("resistance_growth_per_cycle", growth)]));
        load(&path)
    };
    let ConfigError::Invalid { problems, .. } = load_aged("0.1").unwrap_err() else { panic!("expected Invalid") };
    assert!(problems.iter().any(|p| p.field == "battery.resistance_growth_per_cycle"), "{problems:?}");
    assert!(load_aged("0.05").is_ok(), "a factor of 6 is a very tired pack, not an error");
}

fn frame_with_mount(natural_hz: f64, damping_ratio: f64) -> String {
    format!(
        "{}\n[frame.fc_mount]\nnatural_hz = {natural_hz:?}\ndamping_ratio = {damping_ratio:?}\n\
         [provenance.\"frame.fc_mount.natural_hz\"]\nsource = \"estimated\"\n\
         [provenance.\"frame.fc_mount.damping_ratio\"]\nsource = \"estimated\"\n",
        part_text("frames", "opendrone-5f")
    )
}

fn load_with_mount(natural_hz: f64, damping_ratio: f64) -> Result<ofs_config::QuadConfig, ConfigError> {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    write_part(dir.path(), "frames", "opendrone-5f", &frame_with_mount(natural_hz, damping_ratio));
    load(&path)
}

#[test]
fn fc_mount_values_are_validated() {
    for (hz, zeta, field) in [
        (0.0, 0.15, "natural_hz"),
        (900.0, 0.15, "natural_hz"), // above sim.base_hz / 10 = 800 at 8 kHz
        (120.0, 0.0, "damping_ratio"),
        (120.0, 6.0, "damping_ratio"),
    ] {
        let err = load_with_mount(hz, zeta).unwrap_err();
        let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
        assert!(problems.iter().any(|p| p.field == format!("frame.fc_mount.{field}")), "{hz} {zeta}: {problems:?}");
    }
    let mount = load_with_mount(120.0, 0.15).unwrap().frame.fc_mount.expect("a valid mount loads");
    assert_eq!((mount.natural_hz, mount.damping_ratio), (120.0, 0.15));
}

#[test]
fn the_shipped_frame_is_rigid() {
    assert!(load(Path::new(QUAD)).unwrap().frame.fc_mount.is_none());
}

/// The shipped battery part with extra `[battery]` keys and `estimated` provenance for each.
fn battery_part_with(keys: &[(&str, &str)]) -> String {
    let mut text = part_text("batteries", "opendrone-6s-1300");
    let inserted: String = keys.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    text = text.replacen("c1_f = 2000.0\n", &format!("c1_f = 2000.0\n{inserted}"), 1);
    for (k, _) in keys {
        text.push_str(&format!("[provenance.\"battery.{k}\"]\nsource = \"estimated\"\n"));
    }
    text
}

#[test]
fn battery_spread_and_fade_problems_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = battery_part_with(&[
        ("cell_r0_scale", "[1.0, 1.0]"),
        ("cell_capacity_scale", "[1.0, 1.0, 1.0, 1.0, 1.0, 0.0]"),
        ("cycles", "120"),
        ("capacity_fade_per_cycle", "0.01"),
    ]);
    write_part(dir.path(), "batteries", "opendrone-6s-1300", &part);
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for want in ["battery.cell_r0_scale", "battery.cell_capacity_scale", "battery.cycles"] {
        assert!(fields.contains(&want), "{want} missing from {fields:?}");
    }
}

#[test]
fn a_battery_part_with_spread_and_fade_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    let part = battery_part_with(&[
        ("cell_r0_scale", "[1.0, 1.0, 1.0, 1.2, 1.0, 1.0]"),
        ("cycles", "150"),
        ("capacity_fade_per_cycle", "0.001"),
        ("resistance_growth_per_cycle", "0.004"),
    ]);
    write_part(dir.path(), "batteries", "opendrone-6s-1300", &part);
    let b = load(&path).unwrap().battery;
    assert_eq!((b.cycles, b.cell_r0_scale[3]), (150, 1.2));
    assert!(b.cell_capacity_scale.is_empty());
}

#[test]
fn the_shipped_battery_has_no_spread_or_fade() {
    let b = load(Path::new(QUAD)).unwrap().battery;
    assert!(b.cell_r0_scale.is_empty() && b.cell_capacity_scale.is_empty());
    assert_eq!((b.cycles, b.capacity_fade_per_cycle, b.resistance_growth_per_cycle), (0, 0.0, 0.0));
}

#[test]
fn the_shipped_parts_carry_thermal_data() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let motor = cfg.motors[0].thermal.expect("the motor part has a thermal node");
    assert_eq!((motor.heat_capacity_jpk, motor.resistance_kpw), (15.0, 1.5));
    let esc = cfg.escs[0].thermal.expect("the ESC part has a thermal node");
    assert_eq!((esc.heat_capacity_jpk, esc.resistance_kpw, esc.r_on_ohm, esc.switching_loss_coeff), (3.0, 8.0, 0.004, 0.0024));
    let vtx = cfg.vtx.as_ref().unwrap().thermal.expect("the VTX part has a thermal node");
    assert_eq!((vtx.heat_capacity_jpk, vtx.resistance_kpw, vtx.efficiency, vtx.idle_w), (2.0, 15.0, 0.25, 0.4));
}

#[test]
fn thermal_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text());
    write_part(dir.path(), "motors", "opendrone-openmotor-2207", &part_text("motors", "opendrone-openmotor-2207").replace("resistance_kpw = 1.5", "resistance_kpw = 0.0"));
    write_part(dir.path(), "escs", "opendrone-4in1-esc", &part_text("escs", "opendrone-4in1-esc").replace("r_on_ohm = 0.004", "r_on_ohm = -0.1"));
    write_part(dir.path(), "vtx", "opendrone-smartaudio-1w", &part_text("vtx", "opendrone-smartaudio-1w").replace("efficiency = 0.25", "efficiency = 1.5"));
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for want in ["motors[0].thermal.resistance_kpw", "escs[0].thermal.r_on_ohm", "vtx.thermal.efficiency"] {
        assert!(fields.contains(&want), "{want} missing from {fields:?}");
    }
}

#[test]
fn the_shipped_prop_turns_windmilling_on() {
    assert_eq!(load(Path::new(QUAD)).unwrap().props[0].windmill_j0, 1.0);
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
fn telemetry_slot_divisor_defaults_to_four_and_accepts_the_listed_ratios() {
    assert_eq!(shipped_with(|_| {}).unwrap().radio.telemetry_slot_divisor, 4);
    for kind in ["elrs", "fixed"] {
        let cfg = shipped_with(|t| {
            *t = t.replace("kind = \"elrs\"", &format!("kind = \"{kind}\"\ntelemetry_slot_divisor = 8"));
        })
        .unwrap();
        assert_eq!(cfg.radio.telemetry_slot_divisor, 8, "{kind}");
    }
}

#[test]
fn telemetry_slot_divisor_must_be_a_listed_ratio() {
    for value in [3, 0, 256, 129] {
        let err = shipped_with(|t| {
            *t = t.replace("tx_power_mw = 250", &format!("tx_power_mw = 250\ntelemetry_slot_divisor = {value}"));
        })
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("radio.telemetry_slot_divisor"), "{value}: {message}");
        assert!(message.contains("1, 2, 4, 8, 16, 32, 64 or 128"), "{value}: {message}");
        assert!(message.contains(&format!("(got {value})")), "{value}: {message}");
    }
    let err = shipped_with(|t| {
        *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"\ntelemetry_slot_divisor = 3");
    })
    .unwrap_err();
    assert!(err.to_string().contains("radio.telemetry_slot_divisor"), "the fixed link validates it too: {err}");
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

#[test]
fn the_handset_and_receiver_default_to_the_joystick_and_crsf_and_can_be_named() {
    let cfg = shipped_with(|_| {}).unwrap();
    assert_eq!((cfg.radio.handset, cfg.radio.receiver), (ofs_config::HandsetKind::Joystick, ofs_config::ReceiverKind::Crsf));
    let named = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nhandset = \"joystick\"\nreceiver = \"crsf\"")).unwrap();
    assert_eq!(named.radio.handset, ofs_config::HandsetKind::Joystick);
}

#[test]
fn a_quad_can_ask_for_the_open_loop_stabilizer() {
    let cfg = shipped_with(|t| *t = t.replace("kind = \"sitl\"", "kind = \"open_loop_stabilized\"")).unwrap();
    assert_eq!(cfg.fc.kind, ofs_config::FcKind::OpenLoopStabilized);
}

#[test]
fn an_unknown_part_name_is_an_error_that_lists_the_valid_names() {
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nhandset = \"mixer\"")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `mixer`") && err.to_string().contains("joystick"), "{err}");
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nreceiver = \"sbus\"")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `sbus`") && err.to_string().contains("crsf"), "{err}");
}
