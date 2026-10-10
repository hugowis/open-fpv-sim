use std::fs;
use std::path::Path;

use ofs_config::world::{self, AntennaKind, Shape, WorldConfig};
use ofs_config::{load, ConfigError, Polarization};

const WORLD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/flat.toml");
const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn world_text() -> String {
    fs::read_to_string(WORLD).unwrap().replace("\r\n", "\n")
}

fn write(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("world.toml");
    fs::write(&path, text).unwrap();
    path
}

fn problems(text: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    match world::load(&write(dir.path(), text)) {
        Err(ConfigError::Invalid { problems, .. }) => problems.into_iter().map(|p| format!("{}: {}", p.field, p.message)).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn the_shipped_world_loads() {
    let w = world::load(Path::new(WORLD)).unwrap();
    assert_eq!(w.schema_version, 1);
    assert_eq!(w.pilot.position_ned_m, [-3.0, 2.0, -1.7]);
    let names: Vec<&str> = w.receiver.antennas.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["omni", "patch"]);
    assert!(w.receiver.diversity);
    assert_eq!(w.receiver.antennas[1].kind, AntennaKind::Patch);
    assert_eq!(w.receiver.antennas[1].beamwidth_deg, Some(60.0));
    assert_eq!(w.receiver.antennas[0].aim_el(), 90.0, "an omni stands upright by default");
    assert_eq!(w.receiver.antennas[1].aim_el(), 10.0);
    assert_eq!(w.objects.len(), 1 + 16 + 9 + 4, "pad, pylons, gates, buildings");
    let b = w.objects.iter().find(|o| o.name == "BuildingB").unwrap();
    assert_eq!((b.shape, b.size_m, b.rf_loss_db), (Shape::Box, Some([10.0, 10.0, 22.0]), 30.0));
    let pylon = w.objects.iter().find(|o| o.name == "Pylon1L").unwrap();
    assert_eq!((pylon.shape, pylon.radius_m, pylon.height_m, pylon.rf_loss_db), (Shape::Cylinder, Some(0.15), Some(3.0), 0.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!((w.emitters[0].band.as_deref(), w.emitters[0].channel), (Some("R"), Some(2)));
    assert_eq!(w.emitters[0].polarization, Polarization::Rhcp);
}

#[test]
fn the_open_field_is_valid_and_bare() {
    let w = WorldConfig::open_field();
    assert!(w.validate().is_empty(), "{:?}", w.validate());
    assert_eq!(w.receiver.antennas.len(), 1);
    assert!(w.objects.is_empty() && w.emitters.is_empty());
    assert_eq!(w.pilot.position_ned_m, [0.0, 0.0, -1.7]);
}

#[test]
fn every_world_problem_is_reported_at_once() {
    let text = world_text()
        .replace("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-3.0, 2.0, 1.0]")
        .replace("name = \"patch\"", "name = \"omni\"")
        .replace("beamwidth_deg = 60.0\n", "")
        .replace("size_m = [10.0, 10.0, 22.0]", "size_m = [10.0, 0.0, 22.0]")
        .replace("rf_loss_db = 20.0", "rf_loss_db = -1.0")
        .replace("color = [0.62, 0.64, 0.66]", "color = [0.62, 1.5, 0.66]")
        .replace("band = \"R\"\nchannel = 2", "band = \"Z\"\nchannel = 9");
    let found = problems(&text);
    for expected in [
        "pilot.position_ned_m: the pilot must not be below the ground",
        "receiver.antennas[1].name: \"omni\" is used twice",
        "receiver.antennas[1].beamwidth_deg: a patch needs beamwidth_deg",
        "objects[27].size_m",
        "objects[28].rf_loss_db",
        "objects[26].color",
        "emitters[0].band",
        "emitters[0].channel",
    ] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
}

#[test]
fn shapes_need_their_own_sizes() {
    let text = world_text()
        .replace("radius_m = 0.15\nheight_m = 3.0\ncolor = [0.92, 0.92, 0.9]\n\n[[objects]]\nname = \"Pylon1R\"", "size_m = [1.0, 1.0, 1.0]\ncolor = [0.92, 0.92, 0.9]\n\n[[objects]]\nname = \"Pylon1R\"")
        .replace("shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nsize_m = [3.0, 3.0, 0.02]", "shape = \"box\"\ncenter_ned_m = [0.0, 0.0, -0.01]\nradius_m = 1.0");
    let found = problems(&text);
    for expected in ["objects[1].radius_m", "objects[1].height_m", "objects[1].shape: size_m is for a box", "objects[0].size_m", "objects[0].shape: radius_m"] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
}

#[test]
fn an_emitter_needs_one_frequency_in_range() {
    let both = world_text().replace("band = \"R\"\nchannel = 2", "band = \"R\"\nchannel = 2\nfreq_mhz = 5800.0");
    assert!(problems(&both).iter().any(|p| p.starts_with("emitters[0].freq_mhz: give either")), "both");
    let neither = world_text().replace("band = \"R\"\nchannel = 2\n", "");
    assert!(problems(&neither).iter().any(|p| p.starts_with("emitters[0].freq_mhz: give either")), "neither");
    let low = world_text().replace("band = \"R\"\nchannel = 2", "freq_mhz = 2400.0");
    assert!(problems(&low).iter().any(|p| p.starts_with("emitters[0].freq_mhz: must be in 5300..=6000")), "2.4 GHz");
    let ok = world_text().replace("band = \"R\"\nchannel = 2", "freq_mhz = 5695.0");
    let dir = tempfile::tempdir().unwrap();
    assert!(world::load(&write(dir.path(), &ok)).is_ok());
}

#[test]
fn names_must_be_signal_friendly() {
    let text = world_text().replace("name = \"patch\"", "name = \"the patch\"");
    assert!(problems(&text).iter().any(|p| p.starts_with("receiver.antennas[1].name: must be letters")), "a space");
}

#[test]
fn non_finite_numbers_are_rejected() {
    let text = world_text().replace("center_ned_m = [110.0, -45.0, -7.0]", "center_ned_m = [nan, -45.0, -7.0]").replace("facing_deg = 0.0", "facing_deg = inf");
    let found = problems(&text);
    assert!(found.iter().any(|p| p.starts_with("objects[") && p.contains("center_ned_m")), "{found:#?}");
    assert!(found.iter().any(|p| p.starts_with("pilot.facing_deg")), "{found:#?}");
}

#[test]
fn a_world_needs_an_antenna_and_a_known_schema() {
    let none = world_text().split("\n[[receiver.antennas]]").next().unwrap().to_string() + "\nantennas = []\n";
    assert!(problems(&none).iter().any(|p| p.starts_with("receiver.antennas: needs at least one antenna")));
    let dir = tempfile::tempdir().unwrap();
    let err = world::load(&write(dir.path(), &world_text().replace("schema_version = 1", "schema_version = 2"))).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }) && err.to_string().contains("world schema_version 2"), "{err}");
    let err = world::load(&write(dir.path(), &world_text().replace("kind = \"omni\"", "kind = \"yagi\""))).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }) && err.to_string().contains("yagi"), "{err}");
    let err = world::load(Path::new("does/not/exist.toml")).unwrap_err();
    assert!(matches!(err, ConfigError::Io { .. }), "{err}");
}

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
fn the_shipped_quad_describes_its_vtx_antenna() {
    let cfg = load(Path::new(QUAD)).unwrap();
    let vtx = cfg.vtx.as_ref().unwrap();
    assert_eq!(vtx.pit_power_mw, 0.1);
    assert_eq!((vtx.antenna.kind, vtx.antenna.gain_dbi, vtx.antenna.polarization), (AntennaKind::Omni, 2.0, Polarization::Rhcp));
    assert_eq!(vtx.antenna.mount_frd, [-0.5, 0.0, -1.0]);
}

#[test]
fn the_vtx_antenna_and_pit_power_have_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text();
    let bare = text.split("# Pit mode").next().unwrap().to_string();
    let cfg = load(&write_quad(dir.path(), &bare)).unwrap();
    let vtx = cfg.vtx.unwrap();
    assert_eq!(vtx.pit_power_mw, 0.1);
    assert_eq!(vtx.antenna, ofs_config::VtxAntennaSection::default());
    assert_eq!(vtx.antenna.mount_frd, [-0.5, 0.0, -1.0]);
}

#[test]
fn video_link_problems_in_the_quad_file_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("base_hz = 8000", "base_hz = 8010")
        .replace("pit_power_mw = 0.1", "pit_power_mw = 0.0")
        .replace("kind = \"omni\"\ngain_dbi = 2.0", "kind = \"patch\"\ngain_dbi = 2.0")
        .replace("mount_frd = [-0.5, 0.0, -1.0]", "mount_frd = [0.0, 0.0, 0.0]");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["sim.base_hz", "vtx.pit_power_mw", "vtx.antenna.beamwidth_deg", "vtx.antenna.mount_frd"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
    assert!(err.to_string().contains("multiple of 50"), "{err}");
}

#[test]
fn a_nan_pilot_height_is_reported_as_not_finite_only() {
    let found = problems(&world_text().replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-3.0, 2.0, nan]", 1));
    assert!(found.iter().any(|p| p.starts_with("pilot.position_ned_m") && p.contains("finite")), "{found:#?}");
    assert!(!found.iter().any(|p| p.contains("below the ground")), "a NaN height is not below the ground: {found:#?}");
}
