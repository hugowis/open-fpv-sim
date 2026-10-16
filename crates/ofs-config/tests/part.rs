use std::fs;
use std::path::Path;

use ofs_config::part::{
    load_battery, load_esc, load_frame, load_motor, load_prop, load_vtx, ProvenanceSource,
};

const FRAME: &str = r#"
schema_version = 1
name = "Test 5F frame"

[frame]
mass_kg = 0.65
inertia_kgm2 = [0.0025, 0.0025, 0.0045]
cda_m2 = [0.010, 0.010, 0.020]
angular_damping_nms = 0.0001
motor_positions_frd_m = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]]
motor_spin = [1, -1, -1, 1]
contact_points_frd_m = [[-0.08, 0.08, 0.03], [0.08, 0.08, 0.03], [-0.08, -0.08, 0.03], [0.08, -0.08, 0.03]]

[ground]
stiffness_npm = 3000.0
damping_nspm = 40.0
friction_coeff = 0.6

[provenance."frame.mass_kg"]
source = "estimated"
[provenance."frame.inertia_kgm2"]
source = "estimated"
[provenance."frame.cda_m2"]
source = "estimated"
[provenance."frame.angular_damping_nms"]
source = "estimated"
[provenance."frame.motor_positions_frd_m"]
source = "measured"
[provenance."frame.motor_spin"]
source = "estimated"
[provenance."frame.contact_points_frd_m"]
source = "estimated"
[provenance."ground.stiffness_npm"]
source = "estimated"
[provenance."ground.damping_nspm"]
source = "estimated"
[provenance."ground.friction_coeff"]
source = "estimated"
"#;

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

const PROP: &str = r#"
schema_version = 1
name = "Test prop"

[prop]
diameter_m = 0.127
inertia_kgm2 = 6.0e-6
ct_table = [[0.0, 0.11], [30000.0, 0.11]]
cp_table = [[0.0, 0.045], [30000.0, 0.045]]
imbalance_kgm = 0.0

[provenance."prop.diameter_m"]
source = "estimated"
[provenance."prop.inertia_kgm2"]
source = "estimated"
[provenance."prop.ct_table"]
source = "estimated"
[provenance."prop.cp_table"]
source = "estimated"
[provenance."prop.imbalance_kgm"]
source = "estimated"
"#;

#[test]
fn a_frame_part_loads_with_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let part = load_frame(&write(dir.path(), "f.toml", FRAME)).unwrap();
    assert_eq!(part.name, "Test 5F frame");
    assert_eq!(part.frame.mass_kg, 0.65);
    assert_eq!(part.ground.friction_coeff, 0.6);
    assert!(part.collision.is_none());
    assert_eq!(part.provenance["frame.motor_positions_frd_m"].source, ProvenanceSource::Measured);
}

#[test]
fn missing_provenance_names_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let missing = FRAME.replace("\n[provenance.\"ground.friction_coeff\"]\nsource = \"estimated\"\n", "\n");
    let err = load_frame(&write(dir.path(), "f.toml", &missing)).unwrap_err();
    assert!(err.to_string().contains("ground.friction_coeff"), "{err}");
}

#[test]
fn unknown_provenance_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let extra = format!("{FRAME}\n[provenance.\"frame.does_not_exist\"]\nsource = \"estimated\"\n");
    let err = load_frame(&write(dir.path(), "f.toml", &extra)).unwrap_err();
    assert!(err.to_string().contains("frame.does_not_exist"), "{err}");
}

const MOUNT: &str = "\n[frame.fc_mount]\nnatural_hz = 120.0\ndamping_ratio = 0.15\n";
const MOUNT_PROVENANCE: &str = "[provenance.\"frame.fc_mount.natural_hz\"]\nsource = \"estimated\"\n\
                                [provenance.\"frame.fc_mount.damping_ratio\"]\nsource = \"estimated\"\n";

#[test]
fn a_frame_part_may_carry_an_fc_mount() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!("{FRAME}{MOUNT}{MOUNT_PROVENANCE}");
    let part = load_frame(&write(dir.path(), "f.toml", &text)).unwrap();
    let mount = part.frame.fc_mount.expect("the mount is read");
    assert_eq!((mount.natural_hz, mount.damping_ratio), (120.0, 0.15));
    assert_eq!(part.provenance["frame.fc_mount.natural_hz"].source, ProvenanceSource::Estimated);
}

#[test]
fn an_fc_mount_without_provenance_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = load_frame(&write(dir.path(), "f.toml", &format!("{FRAME}{MOUNT}"))).unwrap_err();
    assert!(err.to_string().contains("frame.fc_mount.natural_hz"), "{err}");
}

#[test]
fn a_battery_part_may_carry_spread_and_fade() {
    let dir = tempfile::tempdir().unwrap();
    let shipped = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library/batteries/opendrone-6s-1300.toml"))
        .unwrap()
        .replace("\r\n", "\n");
    let with_fade = format!(
        "{}[provenance.\"battery.cycles\"]\nsource = \"estimated\"\n",
        shipped.replacen("c1_f = 2000.0\n", "c1_f = 2000.0\ncycles = 10\n", 1)
    );
    let part = load_battery(&write(dir.path(), "b.toml", &with_fade)).unwrap();
    assert_eq!(part.battery.cycles, 10);
    assert!(part.battery.cell_r0_scale.is_empty());
    let missing = shipped.replacen("c1_f = 2000.0\n", "c1_f = 2000.0\ncycles = 10\n", 1);
    let err = load_battery(&write(dir.path(), "b2.toml", &missing)).unwrap_err();
    assert!(err.to_string().contains("battery.cycles"), "{err}");
}

#[test]
fn a_bad_source_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = FRAME.replace("source = \"estimated\"", "source = \"guessed\"").replace("source = \"measured\"", "source = \"guessed\"");
    let err = load_frame(&write(dir.path(), "f.toml", &bad)).unwrap_err();
    assert!(err.to_string().contains("guessed"), "{err}");
}

#[test]
fn an_unknown_part_schema_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = load_frame(&write(dir.path(), "f.toml", &FRAME.replace("schema_version = 1", "schema_version = 2"))).unwrap_err();
    assert!(err.to_string().contains("schema_version 2"), "{err}");
}

#[test]
fn an_unknown_data_field_is_a_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = load_frame(&write(dir.path(), "f.toml", &FRAME.replace("mass_kg = 0.65", "mass_kg = 0.65\nmas_kg = 1.0"))).unwrap_err();
    assert!(err.to_string().contains("mas_kg"), "{err}");
}

const MOTOR: &str = r#"
schema_version = 1
name = "Test motor"

[motor]
kv_rpm_per_v = 1950.0
resistance_ohm = 0.07
no_load_current_a = 1.2
rotor_inertia_kgm2 = 6.0e-6
pole_pairs = 7

[provenance."motor.kv_rpm_per_v"]
source = "estimated"
[provenance."motor.resistance_ohm"]
source = "estimated"
[provenance."motor.no_load_current_a"]
source = "estimated"
[provenance."motor.rotor_inertia_kgm2"]
source = "estimated"
[provenance."motor.pole_pairs"]
source = "estimated"
"#;

#[test]
fn a_motor_part_carries_pole_pairs_and_the_sections_default_the_new_fields() {
    let dir = tempfile::tempdir().unwrap();
    let motor = load_motor(&write(dir.path(), "m.toml", MOTOR)).unwrap();
    assert_eq!(motor.motor.pole_pairs, 7);
    let prop = load_prop(&write(dir.path(), "p.toml", PROP)).unwrap();
    assert_eq!(prop.prop.imbalance_kgm, 0.0);
    let cfg = ofs_config::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"))).unwrap();
    assert_eq!(cfg.motors[0].pole_pairs, 7);
    assert_eq!(cfg.props[0].imbalance_kgm, 0.0);
}

#[test]
fn table_valued_parameters_are_one_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = PROP.replace("[provenance.\"prop.ct_table\"]\nsource = \"estimated\"\n", "\n");
    let err = load_prop(&write(dir.path(), "p.toml", &missing)).unwrap_err();
    let shown = err.to_string();
    assert!(shown.contains("prop.ct_table"), "{shown}");
    assert_eq!(shown.matches("prop.ct_table").count(), 1, "the table is one parameter, not one problem per row: {shown}");
}

#[test]
fn every_shipped_part_loads_and_names_its_id() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../library"));
    let frame = load_frame(&root.join("frames/opendrone-5f.toml")).unwrap();
    assert_eq!(frame.frame.mass_kg, 0.65);
    assert_eq!(frame.frame.motor_positions_frd_m.len(), 4);
    assert!(frame.collision.is_none());
    let motor = load_motor(&root.join("motors/opendrone-openmotor-2207.toml")).unwrap();
    assert_eq!(motor.motor.pole_pairs, 7);
    let prop = load_prop(&root.join("props/opendrone-5in-prop.toml")).unwrap();
    assert_eq!(prop.prop.imbalance_kgm, 0.0);
    load_esc(&root.join("escs/opendrone-4in1-esc.toml")).unwrap();
    load_battery(&root.join("batteries/opendrone-6s-1300.toml")).unwrap();
    let sa = load_vtx(&root.join("vtx/opendrone-smartaudio-1w.toml")).unwrap();
    assert_eq!(sa.vtx.power_levels_mw, vec![25, 200, 600, 1000]);
    let tramp = load_vtx(&root.join("vtx/opendrone-tramp-600.toml")).unwrap();
    assert_eq!(tramp.vtx.power_levels_mw, vec![25, 100, 200, 400, 600]);
    assert_eq!(tramp.vtx.kind, ofs_config::VtxKind::Tramp);
}
