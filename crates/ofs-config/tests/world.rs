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
    assert_eq!(w.gates.len(), 3);
    for (gate, north) in w.gates.iter().zip([30.0, 60.0, 90.0]) {
        assert_eq!(gate.center_ned_m, [north, 0.0, -1.22]);
        assert_eq!((gate.heading_deg, gate.width_m, gate.height_m), (0.0, 3.08, 2.44));
    }
    assert_eq!(w.gates[0].name, "Gate0");
}

#[test]
fn the_circuit_world_loads() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/circuit.toml"))).unwrap();
    let gates: Vec<(&str, [f64; 3], f64)> = w.gates.iter().map(|g| (g.name.as_str(), g.center_ned_m, g.heading_deg)).collect();
    assert_eq!(
        gates,
        [
            ("Gate0", [20.0, 0.0, -2.0], 90.0),
            ("Gate1", [40.0, 20.0, -2.0], 0.0),
            ("Gate2", [60.0, 0.0, -2.0], 270.0),
            ("Gate3", [40.0, -20.0, -2.0], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 6.0 && g.height_m == 4.0));
}

#[test]
fn the_park_world_loads() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/park.toml"))).unwrap();
    assert_eq!(w.name, "Proximity park");
    assert_eq!(w.race.laps, Some(5));
    let gates: Vec<(&str, [f64; 3], f64)> = w.gates.iter().map(|g| (g.name.as_str(), g.center_ned_m, g.heading_deg)).collect();
    assert_eq!(
        gates,
        [
            ("Gate0", [12.0, 0.0, -1.1], 90.0),
            ("Gate1", [20.0, 9.0, -1.1], 0.0),
            ("Gate2", [28.0, 0.0, -1.5], 270.0),
            ("Gate3", [20.0, -9.0, -1.1], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 2.4 && g.height_m == 2.2));
    assert_eq!(w.gates[2].roll_deg, 15.0, "the west gate leans with the turn");
    let names: Vec<&str> = w.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 14, "gate frames (9), walls (2), pillars (2), hedge");
    let post = w.objects.iter().find(|o| o.name == "Gate0PostL").unwrap();
    assert_eq!(post.center_ned_m, [10.75, 0.0, -1.1], "Gate0 flies east: its posts separate along x");
    let hedge = w.objects.iter().find(|o| o.name == "Hedge").unwrap();
    assert_eq!((hedge.shape, hedge.rf_loss_db), (Shape::Box, 6.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!(w.emitters[0].freq_mhz, Some(5806.0));
}

#[test]
fn the_meadow_world_loads() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/meadow.toml"))).unwrap();
    assert_eq!(w.name, "Open meadow");
    assert_eq!(w.race.laps, Some(3));
    let gates: Vec<(&str, [f64; 3], f64)> = w.gates.iter().map(|g| (g.name.as_str(), g.center_ned_m, g.heading_deg)).collect();
    assert_eq!(
        gates,
        [
            ("Gate0", [30.0, 0.0, -2.5], 90.0),
            ("Gate1", [60.0, 45.0, -2.5], 45.0),
            ("Gate2", [90.0, 0.0, -3.8], 270.0),
            ("Gate3", [60.0, -45.0, -2.5], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 8.0 && g.height_m == 5.0));
    assert_eq!(w.gates[2].pitch_deg, 20.0, "the west gate leans up with the climb");
    let names: Vec<&str> = w.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 17, "gate frames (8), trees (6), barn, hay bales (2)");
    let post = w.objects.iter().find(|o| o.name == "Gate1PostL").unwrap();
    assert_eq!(post.center_ned_m, [57.12, 47.88, -2.5], "Gate1's posts sit across the 45-degree travel");
    let barn = w.objects.iter().find(|o| o.name == "Barn").unwrap();
    assert_eq!((barn.shape, barn.size_m, barn.rf_loss_db), (Shape::Box, Some([10.0, 6.0, 6.0]), 25.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!(w.emitters[0].freq_mhz, Some(5732.0));
}

#[test]
fn the_open_field_has_no_gates() {
    assert!(WorldConfig::open_field().gates.is_empty());
}

fn gate_block(name: &str, center: &str, heading: &str, width: &str, height: &str) -> String {
    format!("\n[[gates]]\nname = \"{name}\"\ncenter_ned_m = {center}\nheading_deg = {heading}\nwidth_m = {width}\nheight_m = {height}\n")
}

#[test]
fn every_gate_problem_is_reported_at_once() {
    // The shipped world has gates 0 to 2: these are 3 to 7.
    let text = world_text()
        + &gate_block("Wide", "[10.0, 0.0, -1.0]", "0.0", "0.0", "2.0")
        + &gate_block("Tall", "[10.0, 0.0, -1.0]", "0.0", "3.0", "nan")
        + &gate_block("Turn", "[10.0, 0.0, -1.0]", "inf", "3.0", "2.0")
        + &gate_block("Buried", "[10.0, 0.0, -0.5]", "0.0", "3.0", "2.0")
        + &gate_block("Gate0", "[10.0, 0.0, -1.0]", "0.0", "3.0", "2.0");
    let found = problems(&text);
    for expected in [
        "gates[3].width_m",
        "gates[4].height_m",
        "gates[5].heading_deg",
        "gates[6].center_ned_m: the gate's opening must not reach below the ground",
        "gates[7].name: \"Gate0\" is used twice",
    ] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
}

#[test]
fn a_gate_tilt_is_optional_and_checked() {
    let tilted = |center: &str, pitch: &str, roll: &str| {
        world_text() + &gate_block("Tilt", center, "0.0", "3.0", "2.0") + &format!("pitch_deg = {pitch}
roll_deg = {roll}
")
    };
    let loads = |text: &str| world::load(&write(tempfile::tempdir().unwrap().path(), text)).is_ok();
    assert!(loads(&tilted("[10.0, 0.0, -2.0]", "30.0", "-45.0")));
    let found = problems(&tilted("[10.0, 0.0, -2.0]", "90.0", "181.0"));
    for expected in ["gates[3].pitch_deg", "gates[3].roll_deg"] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
    // Rolled a quarter turn the 3 m width is the vertical extent: centred 1.2 m up it reaches 0.3 m below the ground.
    assert!(problems(&tilted("[10.0, 0.0, -1.2]", "0.0", "90.0")).iter().any(|p| p.starts_with("gates[3].center_ned_m")));
    assert!(loads(&tilted("[10.0, 0.0, -1.5]", "0.0", "90.0")), "its lowest edge just touches the ground");
    // The shipped gates are upright, with their axes exactly where they were before tilt existed.
    let (forward, across, down) = world::load(Path::new(WORLD)).unwrap().gates[0].axes();
    assert_eq!((forward, across, down), ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]));
}

#[test]
fn a_lap_limit_is_optional_and_checked() {
    let loads = |text: &str| world::load(&write(tempfile::tempdir().unwrap().path(), text)).is_ok();
    assert_eq!(world::load(Path::new(WORLD)).unwrap().race.laps, None, "the shipped world races forever");
    let mut limited = world_text();
    limited.push_str("
[race]
laps = 3
");
    assert_eq!(world::load(&write(tempfile::tempdir().unwrap().path(), &limited)).unwrap().race.laps, Some(3));
    assert!(loads(&limited));
    let found = problems(&limited.replace("laps = 3", "laps = 0"));
    assert!(found.iter().any(|p| p.starts_with("race.laps: must be >= 1")), "{found:#?}");
    // A limit with no gates has nothing to count.
    let text = world_text();
    let bare = text[..text.find("
[[gates]]").unwrap()].to_string() + "
[race]
laps = 2
";
    assert!(problems(&bare).iter().any(|p| p.starts_with("race.laps: a lap limit needs")), "{:#?}", problems(&bare));
}

#[test]
fn a_world_has_at_most_64_gates() {
    let mut text = world_text();
    for i in 0..61 {
        text += &gate_block(&format!("Extra{i}"), "[10.0, 0.0, -1.0]", "0.0", "3.0", "2.0");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(world::load(&write(dir.path(), &text)).is_ok(), "3 + 61 = 64 gates is allowed");
    let text = text + &gate_block("OneTooMany", "[10.0, 0.0, -1.0]", "0.0", "3.0", "2.0");
    assert!(problems(&text).iter().any(|p| p.starts_with("gates: at most 64 gates")), "65 gates");
}

#[test]
fn a_gate_missing_a_field_is_refused() {
    let text = world_text() + "\n[[gates]]\nname = \"Half\"\ncenter_ned_m = [1.0, 0.0, -1.0]\n";
    let dir = tempfile::tempdir().unwrap();
    let err = world::load(&write(dir.path(), &text)).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }) && err.to_string().contains("heading_deg"), "{err}");
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

const LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../library");

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
/// Mutate parts with `write_part` only AFTER this, never before.
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
    let path = write_quad(dir.path(), &quad_text());
    // A vtx part without [vtx.antenna] and pit_power_mw (and their provenance, the section's last entries).
    let part = part_text("vtx", "opendrone-smartaudio-1w")
        .replace(
            "pit_power_mw = 0.1\n\n[vtx.antenna]\nkind = \"omni\"\ngain_dbi = 2.0\npolarization = \"rhcp\"\n\
             # Up and back, on the back of the frame.\nmount_frd = [-0.5, 0.0, -1.0]\n",
            "",
        )
        .split("[provenance.\"vtx.pit_power_mw\"]").next().unwrap().to_string();
    write_part(dir.path(), "vtx", "opendrone-smartaudio-1w", &part);
    let cfg = load(&path).unwrap();
    let vtx = cfg.vtx.unwrap();
    assert_eq!(vtx.pit_power_mw, 0.1);
    assert_eq!(vtx.antenna, ofs_config::VtxAntennaSection::default());
    assert_eq!(vtx.antenna.mount_frd, [-0.5, 0.0, -1.0]);
}

#[test]
fn video_link_problems_in_the_quad_file_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_quad(dir.path(), &quad_text().replace("base_hz = 8000", "base_hz = 8010"));
    let part = part_text("vtx", "opendrone-smartaudio-1w")
        .replace("pit_power_mw = 0.1", "pit_power_mw = 0.0")
        .replace("kind = \"omni\"\ngain_dbi = 2.0", "kind = \"patch\"\ngain_dbi = 2.0")
        .replace("mount_frd = [-0.5, 0.0, -1.0]", "mount_frd = [0.0, 0.0, 0.0]");
    write_part(dir.path(), "vtx", "opendrone-smartaudio-1w", &part);
    let err = load(&path).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("{err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["sim.base_hz", "vtx.pit_power_mw", "vtx.antenna.beamwidth_deg", "vtx.antenna.mount_frd"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
    assert!(err.to_string().contains("multiple of 50"), "{err}");
}

#[test]
fn a_nan_pilot_height_reports_the_pilot_not_the_derived_handset() {
    let found = problems(&world_text().replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-3.0, 2.0, nan]", 1));
    assert!(found.iter().any(|p| p.starts_with("pilot.position_ned_m") && p.contains("finite")), "{found:#?}");
    assert!(!found.iter().any(|p| p.starts_with("handset.position_ned_m")), "the derived position is not the fault: {found:#?}");
}

#[test]
fn a_nan_pilot_height_is_reported_as_not_finite_only() {
    let found = problems(&world_text().replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [-3.0, 2.0, nan]", 1));
    assert!(found.iter().any(|p| p.starts_with("pilot.position_ned_m") && p.contains("finite")), "{found:#?}");
    assert!(!found.iter().any(|p| p.contains("below the ground")), "a NaN height is not below the ground: {found:#?}");
}

fn written_world(text: &str) -> Result<WorldConfig, ConfigError> {
    let dir = tempfile::tempdir().unwrap();
    world::load(&write(dir.path(), text))
}

#[test]
fn the_default_handset_sits_half_a_metre_below_the_goggles() {
    let world = WorldConfig::open_field();
    assert_eq!(world.handset.position(&world.pilot), [0.0, 0.0, -1.2]);
    let a = &world.handset.antennas[0];
    assert_eq!((a.name.as_str(), a.kind, a.polarization, a.gain_dbi), ("handset", AntennaKind::Omni, Polarization::Linear, 2.0));
    assert_eq!(a.aim_el(), 90.0, "the default dipole stands upright");
}

#[test]
fn a_world_file_may_place_and_aim_the_handset() {
    let world = written_world(
        r#"schema_version = 1
name = "handset field"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
facing_deg = 90.0
[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[handset]
position_ned_m = [-3.0, 2.0, -1.2]
[[handset.antennas]]
name = "left"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
aim_az_deg = -30.0
"#,
    )
    .unwrap();
    assert_eq!(world.handset.position(&world.pilot), [-3.0, 2.0, -1.2]);
    assert_eq!(world.handset.antennas.len(), 1);
    assert_eq!(world.handset.antennas[0].name, "left");
}

#[test]
fn handset_validation_problems_are_collected() {
    let err = written_world(
        r#"schema_version = 1
name = "bad handset"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[handset]
position_ned_m = [-3.0, 2.0, 0.5]
[[handset.antennas]]
name = "same"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
[[handset.antennas]]
name = "same"
kind = "omni"
gain_dbi = 2.0
polarization = "linear"
"#,
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("handset.position_ned_m"), "{message}");
    assert!(message.contains("used twice"), "{message}");
    let bare = written_world(
        r#"schema_version = 1
name = "bare handset"
[pilot]
position_ned_m = [-3.0, 2.0, -1.7]
[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"
[handset]
position_ned_m = [-3.0, 2.0, -1.2]
"#,
    )
    .unwrap_err();
    assert!(bare.to_string().contains("handset.antennas"), "{bare}");
}

#[test]
fn a_world_name_resolves_beside_the_quads_directory_and_a_path_stays_a_path() {
    assert_eq!(world::resolve("flat", Path::new("quads/x.toml")), Path::new("worlds").join("flat.toml"));
    assert_eq!(world::resolve("flat", Path::new("/data/sim/quads/x.toml")), Path::new("/data/sim/worlds").join("flat.toml"));
    assert_eq!(world::resolve("flat", Path::new("x.toml")), Path::new(".").join("worlds").join("flat.toml"));
    assert_eq!(world::resolve("worlds/flat.toml", Path::new("quads/x.toml")), Path::new("worlds/flat.toml"));
    assert_eq!(world::resolve("flat.toml", Path::new("quads/x.toml")), Path::new("flat.toml"), "a file name is a path");
    assert_eq!(world::resolve(r"C:\sim\flat", Path::new("quads/x.toml")), Path::new(r"C:\sim\flat"));
    let shipped = world::resolve("flat", Path::new(QUAD));
    assert_eq!(world::load(&shipped).unwrap().name, "Flat field");
}

#[test]
fn a_world_without_a_video_table_gets_the_default_thresholds() {
    assert_eq!(world::load(Path::new(WORLD)).unwrap().video, world::VideoSection::default());
    assert_eq!(WorldConfig::open_field().video, world::VideoSection::default());
}

#[test]
fn a_video_table_overrides_only_the_keys_it_names() {
    let w = written_world(&(world_text() + "\n[video]\nunstable_snr_db = 8.0\n")).unwrap();
    assert_eq!(w.video.unstable_snr_db, 8.0);
    assert_eq!(w.video, world::VideoSection { unstable_snr_db: 8.0, ..world::VideoSection::default() });
}

#[test]
fn the_ambient_defaults_to_25_and_is_range_checked() {
    assert_eq!(world::load(Path::new(WORLD)).unwrap().environment.ambient_c, 25.0);
    assert_eq!(WorldConfig::open_field().environment.ambient_c, 25.0);
    let hot = written_world(&(world_text() + "\n[environment]\nambient_c = 35.0\n")).unwrap();
    assert_eq!(hot.environment.ambient_c, 35.0);
    for bad in ["90.0", "-50.0", "nan"] {
        let shown = problems(&(world_text() + &format!("\n[environment]\nambient_c = {bad}\n"))).join("\n");
        assert!(shown.contains("environment.ambient_c"), "{bad}: {shown}");
    }
}

#[test]
fn the_magnetic_field_defaults_to_the_one_sitl_synthesises_and_is_range_checked() {
    let env = WorldConfig::open_field().environment;
    assert_eq!((env.magnetic_field_ut, env.magnetic_inclination_deg, env.magnetic_declination_deg), (50.0, 60.0, 2.0));
    let w = written_world(&(world_text() + "
[environment]
magnetic_field_ut = 30.0
magnetic_inclination_deg = -40.0
magnetic_declination_deg = -7.5
")).unwrap();
    assert_eq!((w.environment.magnetic_field_ut, w.environment.magnetic_inclination_deg, w.environment.magnetic_declination_deg), (30.0, -40.0, -7.5));
    for (key, bad) in [
        ("magnetic_field_ut", "0.0"),
        ("magnetic_field_ut", "150.0"),
        ("magnetic_inclination_deg", "95.0"),
        ("magnetic_declination_deg", "nan"),
        ("magnetic_declination_deg", "200.0"),
    ] {
        let shown = problems(&(world_text() + &format!("
[environment]
{key} = {bad}
"))).join("
");
        assert!(shown.contains(&format!("environment.{key}")), "{key} = {bad}: {shown}");
    }
}

#[test]
fn a_non_finite_video_threshold_names_its_field() {
    let shown = problems(&(world_text() + "\n[video]\nsparkle_snr_db = nan\nlost_snr_db = inf\n")).join("\n");
    assert!(shown.contains("video.sparkle_snr_db: ") && shown.contains("video.lost_snr_db: "), "{shown}");
    assert!(!shown.contains("video: "), "the group name is not a field:\n{shown}");
}

#[test]
fn video_threshold_problems_are_reported_together() {
    let text = world_text()
        + "\n[video]\nlost_snr_db = 7.0\nsparkle_snr_db = 0.0\ncolor_lost_snr_db = 9.0\nfield_drop_p_max = 1.5\nlost_after_fields = 0\n";
    let shown = problems(&text).join("\n");
    for field in ["video.unstable_snr_db", "video.sparkle_snr_db", "video.color_fade_snr_db", "video.field_drop_p_max", "video.lost_after_fields"] {
        assert!(shown.contains(field), "{field} missing from:\n{shown}");
    }
}
