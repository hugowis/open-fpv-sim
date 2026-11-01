mod common;

use std::fs;
use std::path::Path;

use ofs_config::world::{self, AntennaKind, Shape, WorldConfig};
use ofs_config::{load, ConfigError, Polarization, TerrainSection, MAX_TERRAIN_SLOPE_DEG};

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
fn yaw_deg_parses_and_defaults() {
    // LaunchPad, the flat world's first object, is a box; Pylon1L right after it is a cylinder.
    let yawed = world_text().replace("size_m = [3.0, 3.0, 0.02]", "size_m = [3.0, 3.0, 0.02]\nyaw_deg = 135.0");
    let dir = tempfile::tempdir().unwrap();
    let w = world::load(&write(dir.path(), &yawed)).unwrap();
    assert_eq!(w.objects[0].yaw_deg, Some(135.0));
    assert_eq!(w.objects[1].yaw_deg, None, "an object without the key carries no yaw");
    let bare = world::load(Path::new(WORLD)).unwrap();
    assert!(bare.objects.iter().all(|o| o.yaw_deg.is_none()), "the shipped world's objects are unchanged");
}

#[test]
fn every_yaw_problem_is_reported_at_once() {
    let text = world_text()
        .replace("size_m = [3.0, 3.0, 0.02]", "size_m = [3.0, 3.0, 0.02]\nyaw_deg = nan")
        .replace("shape = \"cylinder\"\ncenter_ned_m = [15.0, -6.0, -1.5]", "shape = \"cylinder\"\nyaw_deg = 10.0\ncenter_ned_m = [15.0, -6.0, -1.5]");
    let found = problems(&text);
    for expected in ["objects[0].yaw_deg: must be finite", "objects[1].yaw_deg: yaw_deg is for a box"] {
        assert!(found.contains(&expected.to_string()), "{expected} in {found:?}");
    }
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
            ("Gate1", [20.0, 9.0, -1.168], 0.0),
            ("Gate2", [28.0, 0.0, -1.781], 270.0),
            ("Gate3", [20.0, -9.0, -1.125], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 2.4 && g.height_m == 2.2));
    assert_eq!(w.gates[2].roll_deg, 15.0, "the west gate leans with the turn");
    let courses = w.resolved_courses();
    assert_eq!(courses.iter().map(|c| (c.name.as_str(), c.mode, c.laps, c.time_limit_s)).collect::<Vec<_>>(), [("Diamond", world::RaceMode::Laps, Some(5), None), ("Hunt", world::RaceMode::TimeAttack, None, Some(60.0))]);
    assert_eq!(w.default_course(), 0, "the default course is the whole loop, as before");
    let names: Vec<&str> = w.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 14, "gate frames (9), walls (2), pillars (2), hedge");
    let post = w.objects.iter().find(|o| o.name == "Gate0PostL").unwrap();
    assert_eq!(&post.center_ned_m[..2], &[10.75, 0.0], "Gate0 flies east: its posts separate along x");
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
    assert_eq!(names.len(), 18, "gate frames (9: Gate1 has its bar), trees (6), barn, hay bales (2)");
    let post = w.objects.iter().find(|o| o.name == "Gate1PostL").unwrap();
    assert_eq!(post.center_ned_m, [57.12, 47.88, -2.5], "Gate1's posts sit across the 45-degree travel");
    let bar = w.objects.iter().find(|o| o.name == "Gate1Bar").unwrap();
    assert_eq!((bar.shape, bar.size_m, bar.yaw_deg), (Shape::Box, Some([8.3, 0.15, 0.15]), Some(135.0)), "the bar Gate1 was missing, yawed across the travel");
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

/// The shipped world's three gates, plus two courses (`extra` is appended after them, so a `[race]` table in it works).
fn with_courses(extra: &str) -> String {
    world_text() + extra
}

const TWO_COURSES: &str = r#"
[[courses]]
name = "Line"
gates = ["Gate0", "Gate1", "Gate2"]

[[courses]]
name = "Hunt"
mode = "time_attack"
time_limit_s = 45.0
gates = ["Gate2", "Gate0"]
"#;

#[test]
fn a_world_without_courses_has_one_implicit_course_of_all_its_gates() {
    let w = world::load(Path::new(WORLD)).unwrap();
    assert!(w.courses.is_empty());
    let courses = w.resolved_courses();
    assert_eq!(courses.len(), 1);
    assert_eq!((courses[0].name.as_str(), courses[0].gates.clone(), courses[0].mode, courses[0].laps), ("", vec![0, 1, 2], world::RaceMode::Laps, None));
    assert_eq!(w.default_course(), 0);
    assert!(WorldConfig::open_field().resolved_courses().is_empty(), "no gates, no course");
}

#[test]
fn courses_pick_and_order_the_declared_gates() {
    let text = with_courses(&format!("{TWO_COURSES}\n[race]\nlaps = 3\ncourse = \"Hunt\"\n"));
    let w = world::load(&write(tempfile::tempdir().unwrap().path(), &text)).unwrap();
    let c = w.resolved_courses();
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].name.as_str(), c[0].gates.clone(), c[0].mode, c[0].laps, c[0].time_limit_s), ("Line", vec![0, 1, 2], world::RaceMode::Laps, Some(3), None));
    assert_eq!((c[1].name.as_str(), c[1].gates.clone(), c[1].mode, c[1].laps, c[1].time_limit_s), ("Hunt", vec![2, 0], world::RaceMode::TimeAttack, None, Some(45.0)));
    assert_eq!(w.default_course(), 1, "[race] course picks the one a session starts in");
    // A course's own laps beat the world's.
    let own = with_courses(&TWO_COURSES.replace("name = \"Line\"", "name = \"Line\"\nlaps = 2")) + "\n[race]\nlaps = 3\n";
    let w = world::load(&write(tempfile::tempdir().unwrap().path(), &own)).unwrap();
    assert_eq!(w.resolved_courses()[0].laps, Some(2));
    assert_eq!(w.default_course(), 0, "the first course is the default");
}

#[test]
fn every_course_problem_is_reported_at_once() {
    let text = with_courses(
        r#"
[[courses]]
name = "Line"
gates = []

[[courses]]
name = "Line"
gates = ["Gate0", "Nope", "Gate0"]

[[courses]]
name = "Timed"
mode = "time_attack"
gates = ["Gate0"]

[[courses]]
name = "Laps"
laps = 0
gates = ["Gate0"]

[[courses]]
name = "Hunt2"
mode = "time_attack"
time_limit_s = 30.0
laps = 2
gates = ["Gate0"]

[race]
course = "Missing"
"#,
    );
    let found = problems(&text);
    for expected in [
        "courses[0].gates: a course needs at least one gate",
        "courses[1].name: \"Line\" is used twice",
        "courses[1].gates[1]: no gate named \"Nope\"",
        "courses[1].gates[2]: \"Gate0\" is already in this course",
        "courses[2].time_limit_s: a time_attack course needs time_limit_s > 0",
        "courses[3].laps: must be >= 1",
        "courses[4].laps: a time_attack course ends on the clock, not on laps",
        "race.course: no course named \"Missing\"",
    ] {
        assert!(found.iter().any(|p| p.starts_with(expected)), "{expected} missing from {found:#?}");
    }
    let lapped = with_courses("\n[[courses]]\nname = \"Laps\"\ntime_limit_s = 30.0\ngates = [\"Gate0\"]\n");
    assert!(problems(&lapped).iter().any(|p| p.starts_with("courses[0].time_limit_s: only a time_attack course has")), "{:#?}", problems(&lapped));
}

#[test]
fn courses_need_gates_and_a_course_pick_needs_courses() {
    let bare = world_text();
    let bare = bare[..bare.find("\n[[gates]]").unwrap()].to_string();
    let found = problems(&format!("{bare}\n[[courses]]\nname = \"X\"\ngates = [\"A\"]\n"));
    assert!(found.iter().any(|p| p.starts_with("courses: a course needs [[gates]]")), "{found:#?}");
    let found = problems(&with_courses("\n[race]\ncourse = \"Line\"\n"));
    assert!(found.iter().any(|p| p.starts_with("race.course: no course named")), "{found:#?}");
}

fn with_terrain(body: &str) -> String {
    format!("{}
[terrain]
{body}
", world_text())
}

fn terrain_of(body: &str) -> ofs_core::terrain::Terrain {
    written_world(&with_terrain(body)).unwrap().terrain.expect("a [terrain] section").build()
}

#[test]
fn a_bare_terrain_section_is_rolling_hills_and_absence_is_none() {
    assert_eq!(written_world(&with_terrain("")).unwrap().terrain, Some(TerrainSection::default()));
    assert_eq!(written_world(&world_text()).unwrap().terrain, None);
    assert_eq!(WorldConfig::open_field().terrain, None);
    let w = written_world(&with_terrain("seed = 7
size_m = 200.0
cell_m = 2.0
amplitude_m = 3.0
wavelength_m = 50.0
octaves = 2
persistence = 0.4
flat_radius_m = 10.0")).unwrap();
    let want = TerrainSection { seed: 7, size_m: 200.0, cell_m: 2.0, amplitude_m: 3.0, wavelength_m: 50.0, octaves: 2, persistence: 0.4, flat_radius_m: 10.0 };
    assert_eq!(w.terrain, Some(want));
    assert!(written_world(&with_terrain("sede = 1")).is_err(), "unknown keys are refused");
}

#[test]
fn the_default_terrain_is_a_flat_pad_in_rolling_hills() {
    let section = TerrainSection::default();
    let t = section.build();
    assert_eq!(t.heights().len(), 401 * 401);
    assert_eq!(t.nodes(), (401, 401));
    assert_eq!(t.origin_ne(), [-200.0, -200.0]);
    assert_eq!(t.ground_d(0.0, 0.0), 0.0);
    for (i, h) in t.heights().iter().enumerate() {
        let (row, col) = (i / 401, i % 401);
        let (n, e) = (row as f64 - 200.0, col as f64 - 200.0);
        if (n * n + e * e).sqrt() <= section.flat_radius_m {
            assert_eq!(*h, 0.0, "pad node ({row}, {col})");
        }
        if row == 0 || row == 400 || col == 0 || col == 400 {
            assert_eq!(*h, 0.0, "border node ({row}, {col})");
        }
    }
    // A point between nodes is flat once every node of its triangle is (a cell is a metre across).
    for k in 0..360 {
        let a = f64::from(k).to_radians();
        let r = section.flat_radius_m - 2.0;
        assert_eq!(t.ground_d(r * a.cos(), r * a.sin()), 0.0, "the pad at {k} degrees");
    }
    let tallest = t.heights().iter().fold(0.0f32, |m, h| m.max(h.abs()));
    assert!(tallest > 1.0, "relief exists (tallest {tallest})");
    assert!(t.max_slope() < MAX_TERRAIN_SLOPE_DEG.to_radians().tan(), "default slope {} deg", t.max_slope().atan().to_degrees());
}

#[test]
fn a_seed_gives_one_grid_and_other_seeds_other_grids() {
    let at = |seed: u64| TerrainSection { seed, ..Default::default() }.build().digest();
    assert_eq!(at(1), at(1));
    assert_ne!(at(1), at(2));
}

/// The cross-OS claim: IEEE basic operations and `sqrt` only, so this one constant holds on Windows, Linux and macOS.
/// If it fails on one OS, the generator calls a libm function (`sin`, `exp`, `pow`, ...) or the compiler fused a
/// multiply-add: fix the generator, do not add a per-OS constant.
#[test]
fn the_default_grid_digest_is_pinned_on_every_os() {
    assert_eq!(TerrainSection::default().build().digest(), 0x9d1c_89d7_d6f5_fa1d, "the default section's baked grid changed");
}

#[test]
fn every_terrain_rule_names_its_field() {
    let cases: &[(&str, &str)] = &[
        ("size_m = nan", "terrain.size_m: must be finite"),
        ("cell_m = inf", "terrain.cell_m: must be finite"),
        ("amplitude_m = nan", "terrain.amplitude_m: must be finite"),
        ("wavelength_m = nan", "terrain.wavelength_m: must be finite"),
        ("persistence = nan", "terrain.persistence: must be finite"),
        ("flat_radius_m = nan", "terrain.flat_radius_m: must be finite"),
        ("size_m = 0.0", "terrain.size_m: must be in (0, 2000]"),
        ("size_m = 2001.0", "terrain.size_m: must be in (0, 2000]"),
        ("cell_m = 0.1", "terrain.cell_m: must be in [0.25, size_m]"),
        ("size_m = 100.0
cell_m = 3.0", "terrain.size_m: must be a whole multiple of cell_m"),
        ("size_m = 500.0
cell_m = 0.25", "terrain.size_m: at most 1025 nodes a side"),
        ("amplitude_m = -1.0", "terrain.amplitude_m: must be in [0, 100]"),
        ("amplitude_m = 101.0", "terrain.amplitude_m: must be in [0, 100]"),
        ("wavelength_m = 3.9", "terrain.wavelength_m: must be at least 4 cells"),
        ("octaves = 0", "terrain.octaves: must be 1 to 6"),
        ("octaves = 7", "terrain.octaves: must be 1 to 6"),
        ("persistence = 0.0", "terrain.persistence: must be in (0, 1]"),
        ("persistence = 1.5", "terrain.persistence: must be in (0, 1]"),
        ("flat_radius_m = -1.0", "terrain.flat_radius_m: must be >= 0"),
        ("amplitude_m = 100.0
wavelength_m = 8.0", "terrain.amplitude_m: the steepest slope is"),
    ];
    for (body, expected) in cases {
        let found = problems(&with_terrain(body));
        assert!(found.iter().any(|p| p.starts_with(expected)), "[terrain] {body:?}: {expected} missing from {found:#?}");
    }
    // The boundaries themselves are fine.
    for body in ["size_m = 2000.0
cell_m = 2.0", "cell_m = 0.25
size_m = 200.0", "wavelength_m = 4.0
amplitude_m = 0.5", "octaves = 6", "persistence = 1.0", "amplitude_m = 0.0", "flat_radius_m = 0.0"] {
        assert!(written_world(&with_terrain(body)).is_ok(), "[terrain] {body:?}: {:?}", written_world(&with_terrain(body)).err());
    }
}

/// With no flat disc, seed 37's hills stand about 9.6 m high at (60, 40).
const HILL: &str = "seed = 37
amplitude_m = 50.0
flat_radius_m = 0.0";

#[test]
fn a_pilot_inside_a_hill_is_refused() {
    assert!(terrain_of(HILL).height_up(60.0, 40.0) > 1.7);
    let text = with_terrain(HILL).replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [60.0, 40.0, -1.7]", 1);
    let found = problems(&text);
    assert!(found.iter().any(|p| p.starts_with("pilot.position_ned_m: the pilot must not be below the terrain")), "{found:#?}");
    assert!(!found.iter().any(|p| p.starts_with("handset.position_ned_m")), "the derived handset repeats the pilot's fault: {found:#?}");
}

#[test]
fn a_handset_inside_a_hill_is_refused() {
    let text = with_terrain(HILL).replacen("[pilot]", "[handset]
position_ned_m = [60.0, 40.0, -1.7]
[[handset.antennas]]
name = \"h\"
kind = \"omni\"
gain_dbi = 2.0
polarization = \"linear\"

[pilot]", 1);
    let text = text.replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [0.0, 0.0, -1.7]", 1);
    let found = problems(&text);
    assert!(found.iter().any(|p| p.starts_with("handset.position_ned_m: the handset must not be below the terrain")), "{found:#?}");
    assert!(!found.iter().any(|p| p.starts_with("pilot.position_ned_m")), "{found:#?}");
}

#[test]
fn a_pilot_above_the_hills_is_fine() {
    let text = with_terrain(HILL).replacen("position_ned_m = [-3.0, 2.0, -1.7]", "position_ned_m = [60.0, 40.0, -30.0]", 1);
    assert!(written_world(&text).is_ok(), "{:?}", written_world(&text).err());
}

/// The lowest and highest ground height (up) under a footprint: a 9 x 9 sample of its rectangle, or of the circle.
fn ground_under(t: &ofs_core::terrain::Terrain, o: &world::ObjectSection) -> (f64, f64) {
    let (half_n, half_e) = match o.shape {
        Shape::Box => (o.size_m.unwrap()[0] / 2.0, o.size_m.unwrap()[1] / 2.0),
        Shape::Cylinder => (o.radius_m.unwrap(), o.radius_m.unwrap()),
    };
    let heights: Vec<f64> = (0..81)
        .map(|k| (half_n * (k / 9) as f64 / 4.0 - half_n, half_e * (k % 9) as f64 / 4.0 - half_e))
        .filter(|&(n, e)| o.shape == Shape::Box || n * n + e * e <= half_n * half_n * 1.0001)
        .map(|(n, e)| t.height_up(o.center_ned_m[0] + n, o.center_ned_m[1] + e))
        .collect();
    (heights.iter().copied().fold(f64::INFINITY, f64::min), heights.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

/// The park's course was designed on flat ground; with relief it stands on the ground. Gates hang 0.9 to 1.6 m above
/// the ground at their centre, every object is buried slightly (never floating) and keeps its design height above
/// the ground around it, and nothing is steeper than a gentle hill.
#[test]
fn the_park_course_stands_on_its_relief() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/park.toml"))).unwrap();
    let t = w.terrain.expect("the park has relief").build();
    assert!(t.max_slope() < 30f64.to_radians().tan(), "max slope tan = {}", t.max_slope());
    assert_eq!(t.height_up(0.0, 0.0), 0.0, "home is on the flat pad");
    let handset = w.handset.position(&w.pilot);
    for (what, at) in [("pilot", w.pilot.position_ned_m), ("handset", handset)] {
        assert!(-at[2] > t.height_up(at[0], at[1]), "{what} is above the ground");
    }

    for g in &w.gates {
        let ground = t.height_up(g.center_ned_m[0], g.center_ned_m[1]);
        let above = -g.center_ned_m[2] - ground;
        assert!((0.9..=1.6).contains(&above), "{} centre is {above} m above the ground", g.name);
        // The opening's lowest point (half the height and half the width, rolled) clears the lowest ground across its width,
        // not the highest: a gate's bottom may sit a centimetre or so under the ground at its high end.
        let (sr, cr) = g.roll_deg.to_radians().sin_cos();
        let drop = g.height_m / 2.0 * cr + g.width_m / 2.0 * sr.abs();
        let (_, across, _) = g.axes();
        let lowest_ground = (0..9)
            .map(|k| {
                let s = g.width_m * (k as f64 / 8.0 - 0.5);
                t.height_up(g.center_ned_m[0] + across[0] * s, g.center_ned_m[1] + across[1] * s)
            })
            .fold(f64::INFINITY, f64::min);
        let opening_bottom = -g.center_ned_m[2] - drop;
        assert!(opening_bottom >= lowest_ground, "{}: the opening's lowest point {opening_bottom} is below the lowest ground across its width {lowest_ground}", g.name);
        // Gate frame: the bar crowns the opening and the posts stand on the ground.
        if let Some(bar) = w.objects.iter().find(|o| o.name == format!("{}Bar", g.name)) {
            assert!((-bar.center_ned_m[2] - (-g.center_ned_m[2] + 1.15)).abs() < 1e-9, "{} bar is 1.15 m over the centre", g.name);
        }
    }

    // The heights the objects were designed with on flat ground: how far each top stands above the ground around it.
    let design = |name: &str| match name {
        n if n.ends_with("Wall") => 3.0,
        n if n.starts_with("Pillar") => 4.0,
        "Hedge" => 0.8,
        n if n.contains("Post") => 2.2,
        n => panic!("{n}: no design height"),
    };
    for o in w.objects.iter().filter(|o| !o.name.ends_with("Bar")) {
        let (lowest, highest) = ground_under(&t, o);
        let h = match o.shape {
            Shape::Box => o.size_m.unwrap()[2],
            Shape::Cylinder => o.height_m.unwrap(),
        };
        let (base, top) = (-o.center_ned_m[2] - h / 2.0, -o.center_ned_m[2] + h / 2.0);
        assert!(base <= lowest, "{}: base {base} floats above the ground {lowest}", o.name);
        assert!(base >= lowest - 0.6, "{}: base {base} is buried too deep under {lowest}", o.name);
        assert!(top >= highest + design(&o.name) - 0.35, "{}: top {top} is less than {} over the highest ground {highest}", o.name, design(&o.name));
    }

    for e in &w.emitters {
        let above = -e.position_ned_m[2] - t.height_up(e.position_ned_m[0], e.position_ned_m[1]);
        assert!((0.5..=2.0).contains(&above), "{} is {above} m above the ground", e.name);
    }
}

/// The baked slopes really do straddle tan 60 = sqrt(3) by a whisker (the amplitude is the only difference), and the
/// sections are built directly because `terrain_of` loads a world file and the over-cap one must not load.
#[test]
fn the_slope_cap_takes_a_world_just_under_tan_60_and_refuses_one_just_over() {
    // The amplitude pair was tuned so the steepest slope is the closest this generator gets to the cap from below
    // (2.4e-9 relative) and the first past it (4.7e-8 relative).
    let body = |a: &str| format!("seed = 0
size_m = 60.0
cell_m = 1.0
amplitude_m = {a}
wavelength_m = 15.0
octaves = 3
persistence = 0.5
flat_radius_m = 5.0");
    let section = |a: f64| TerrainSection { amplitude_m: a, size_m: 60.0, wavelength_m: 15.0, flat_radius_m: 5.0, ..Default::default() };
    let tan60 = 3.0f64.sqrt();
    for (a, side) in [("10.666648164268156", -1.0f64), ("10.666648723199209", 1.0f64)] {
        let gap = section(a.parse().unwrap()).build().max_slope() / tan60 - 1.0;
        assert!(gap * side > 0.0 && gap.abs() < 1.0e-7, "amplitude {a}: the steepest slope is {gap:e} from tan 60");
    }
    let under = body("10.666648164268156");
    let over = body("10.666648723199209");
    assert!(written_world(&with_terrain(&under)).is_ok(), "{:?}", written_world(&with_terrain(&under)).err());
    let found = problems(&with_terrain(&over));
    assert!(
        found.iter().any(|p| p.starts_with("terrain.amplitude_m: the steepest slope is 60.0 degrees, over the 60 degree limit")),
        "{found:#?}"
    );
}

// --- [[meshes]] and [[materials]] ---

const MESH_WORLD: &str = "schema_version = 1\nname = \"m\"\n[pilot]\nposition_ned_m = [0.0, 0.0, -1.7]\n[receiver]\n\
    [[receiver.antennas]]\nname = \"omni\"\nkind = \"omni\"\ngain_dbi = 2.0\npolarization = \"rhcp\"\n";

/// A `.glb` of two triangles per named material, one node each.
fn glb(materials: &[&str]) -> Vec<u8> {
    let t = [[0.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 1.0, -1.0]];
    let nodes: Vec<_> = materials.iter().map(|m| common::glb::GlbNode { name: m, triangles: vec![t, t], material: Some(m) }).collect();
    common::glb::write_glb(&nodes)
}

/// Writes `MESH_WORLD + extra` and, beside it, `a.glb` (materials `wall`, `glass`: 4 triangles) and `b.glb` (`wall`: 2).
fn mesh_world(dir: &Path, extra: &str) -> std::path::PathBuf {
    fs::write(dir.join("a.glb"), glb(&["wall", "glass"])).unwrap();
    fs::write(dir.join("b.glb"), glb(&["wall"])).unwrap();
    write(dir, &format!("{MESH_WORLD}{extra}"))
}

fn mesh_problems(extra: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    match world::load(&mesh_world(dir.path(), extra)) {
        Err(ConfigError::Invalid { problems, .. }) => problems.into_iter().map(|p| format!("{}: {}", p.field, p.message)).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn assert_field(problems: &[String], field: &str) {
    assert!(problems.iter().any(|p| p.starts_with(&format!("{field}: "))), "no problem on {field}: {problems:?}");
}

#[test]
fn meshes_and_materials_parse() {
    let dir = tempfile::tempdir().unwrap();
    let path = mesh_world(
        dir.path(),
        "[[meshes]]\nname = \"hall\"\nfile = \"a.glb\"\nposition_ned_m = [10.0, 0.0, 0.0]\nheading_deg = 90.0\n\
         [[meshes]]\nname = \"shed\"\nfile = \"b.glb\"\n\
         [[materials]]\nname = \"glass\"\nrf_loss_db = 2.0\n[[materials]]\nname = \"wall\"\nrf_loss_db = 18.0\n",
    );
    let w = world::load(&path).unwrap();
    assert_eq!((w.meshes[0].name.as_str(), w.meshes[0].position_ned_m, w.meshes[0].heading_deg), ("hall", [10.0, 0.0, 0.0], 90.0));
    assert_eq!((w.meshes[1].position_ned_m, w.meshes[1].heading_deg), ([0.0; 3], 0.0));
    let data = w.load_meshes().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0].name, "hall");
    assert!(data[0].path.is_absolute() && data[0].path.ends_with("a.glb"), "{:?}", data[0].path);
    assert!(!data[0].path.to_string_lossy().starts_with(r"\\?\"), "no verbatim prefix: {:?}", data[0].path);
    assert_eq!(data[0].digest, ofs_core::rng::fnv1a64(&fs::read(dir.path().join("a.glb")).unwrap()));
    assert_eq!(data[0].mesh.len(), 4);
    // The loss follows the .glb's material order (wall, glass), not the world file's [[materials]] order.
    assert_eq!(data[0].loss_db, [18.0, 2.0]);
    assert_eq!(data[1].loss_db, [18.0]);
}

#[test]
fn a_material_name_may_be_free_form() {
    let dir = tempfile::tempdir().unwrap();
    let path = mesh_world(dir.path(), "[[meshes]]\nname = \"hall\"\nfile = \"c.glb\"\n[[materials]]\nname = \"Wall Concrete.001\"\nrf_loss_db = 17.5\n");
    fs::write(dir.path().join("c.glb"), glb(&["Paint", "Wall Concrete.001"])).unwrap();
    assert_eq!(world::load(&path).unwrap().load_meshes().unwrap()[0].loss_db, [0.0, 17.5]);
}

#[test]
fn an_unlisted_material_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = mesh_world(dir.path(), "[[meshes]]\nname = \"hall\"\nfile = \"a.glb\"\n[[materials]]\nname = \"glass\"\nrf_loss_db = 2.0\n");
    assert_eq!(world::load(&path).unwrap().load_meshes().unwrap()[0].loss_db, [0.0, 2.0]);
}

#[test]
fn every_mesh_and_material_rule_names_its_field() {
    let mesh = |body: &str| format!("[[meshes]]\nname = \"x\"\nfile = \"a.glb\"\n{body}");
    let twice = "[[meshes]]\nname = \"x\"\nfile = \"a.glb\"\n[[meshes]]\nname = \"x\"\nfile = \"b.glb\"\n";
    assert_field(&mesh_problems(twice), "meshes[1].name");
    assert_field(&mesh_problems("[[meshes]]\nname = \"a b\"\nfile = \"a.glb\"\n"), "meshes[0].name");
    for file in ["/etc/a.glb", "\\a.glb", "C:/a.glb", "../a.glb", "sub/../a.glb", "sub\\..\\a.glb", "sub\\a.glb", "a.gltf", "a"] {
        let p = mesh_problems(&format!("[[meshes]]\nname = \"x\"\nfile = {file:?}\n"));
        assert_field(&p, "meshes[0].file");
        assert_eq!(p.len(), 1, "{file}: {p:?}");
        assert!(!file.contains('\\') || p[0].contains("backslash"), "{file}: {p:?}");
    }
    for (body, field) in [
        ("position_ned_m = [nan, 0.0, 0.0]", "meshes[0].position_ned_m"),
        ("position_ned_m = [0.0, inf, 0.0]", "meshes[0].position_ned_m"),
        ("heading_deg = nan", "meshes[0].heading_deg"),
        ("heading_deg = -inf", "meshes[0].heading_deg"),
    ] {
        assert_field(&mesh_problems(&mesh(body)), field);
    }
    let many: String = (0..=ofs_config::mesh::MAX_MESHES).map(|i| format!("[[meshes]]\nname = \"m{i}\"\nfile = \"a.glb\"\n")).collect();
    assert_field(&mesh_problems(&many), "meshes");
    assert_field(&mesh_problems("[[materials]]\nname = \"x\"\n[[materials]]\nname = \"x\"\n"), "materials[1].name");
    assert_field(&mesh_problems("[[materials]]\nname = \"\"\n"), "materials[0].name");
    for loss in ["-1.0", "nan", "inf"] {
        assert_field(&mesh_problems(&format!("[[materials]]\nname = \"x\"\nrf_loss_db = {loss}\n")), "materials[0].rf_loss_db");
    }
}

#[test]
fn a_material_no_mesh_uses_and_an_unreadable_file_are_refused_at_load() {
    let p = mesh_problems("[[meshes]]\nname = \"hall\"\nfile = \"b.glb\"\n[[materials]]\nname = \"wall\"\n[[materials]]\nname = \"wal\"\n");
    assert_eq!(p.len(), 1);
    assert_field(&p, "materials[1].name");
    let p = mesh_problems("[[meshes]]\nname = \"hall\"\nfile = \"b.glb\"\n[[meshes]]\nname = \"gone\"\nfile = \"gone.glb\"\n");
    assert_field(&p, "meshes[1].file");
    assert!(p[0].contains("gone.glb"), "{p:?}");
    let dir = tempfile::tempdir().unwrap();
    let path = mesh_world(dir.path(), "[[meshes]]\nname = \"hall\"\nfile = \"bad.glb\"\n");
    fs::write(dir.path().join("bad.glb"), b"nope").unwrap();
    match world::load(&path) {
        Err(ConfigError::Invalid { problems, .. }) => {
            assert_eq!(problems[0].field, "meshes[0].file");
            assert!(problems[0].message.contains("bad.glb"), "{:?}", problems[0].message);
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn the_triangle_cap_counts_every_mesh_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = mesh_world(dir.path(), "[[meshes]]\nname = \"x\"\nfile = \"a.glb\"\n[[meshes]]\nname = \"y\"\nfile = \"b.glb\"\n");
    let w = world::load(&path).unwrap();
    assert_eq!(w.load_meshes_with(6).unwrap().len(), 2);
    let e = w.load_meshes_with(5).unwrap_err();
    assert!(e.starts_with("meshes[1].file: ") && e.contains("b.glb"), "{e}");
    let e = w.load_meshes_with(3).unwrap_err();
    assert!(e.starts_with("meshes[0].file: ") && e.contains("a.glb"), "{e}");
}

#[test]
fn a_world_not_loaded_from_disk_has_no_meshes_to_load() {
    let mut w = WorldConfig::open_field();
    assert!(w.meshes.is_empty() && w.materials.is_empty());
    assert!(w.load_meshes().unwrap().is_empty());
    w.meshes.push(ofs_config::MeshSection { name: "x".into(), file: "x.glb".into(), position_ned_m: [0.0; 3], heading_deg: 0.0 });
    assert!(w.load_meshes().unwrap().is_empty());
}

fn shipped_world(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../worlds").join(format!("{name}.toml"))
}

#[test]
fn a_world_without_meshes_loads_exactly_as_before() {
    let mut seen = 0;
    for entry in fs::read_dir(shipped_world("x").parent().unwrap()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") || ["hall", "chernobyl"].contains(&path.file_stem().unwrap().to_str().unwrap()) {
            continue;
        }
        let w = world::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(w.meshes.is_empty() && w.materials.is_empty(), "{}", path.display());
        assert!(w.load_meshes().unwrap().is_empty());
        seen += 1;
    }
    assert!(seen >= 5, "found only {seen} worlds");
}

#[test]
fn the_hall_loads_with_its_doorway_open_and_window_glazed() {
    use glam::DVec3;
    let w = world::load(&shipped_world("hall")).unwrap();
    assert_eq!(w.pilot.position_ned_m, [5.0, 0.0, -1.7]);
    let hall = &w.load_meshes().unwrap()[0];
    assert_eq!(hall.loss_db, [18.0, 2.0], "concrete, glass");
    let materials_crossed = |a: [f64; 3], b: [f64; 3]| -> Vec<u32> {
        let mut m = Vec::new();
        hall.mesh.crossings(DVec3::from(a), DVec3::from(b), &mut m);
        m
    };
    assert_eq!(materials_crossed([5.0, 0.0, -1.7], [40.0, 0.0, -1.7]), Vec::<u32>::new(), "through the doorway");
    assert_eq!(materials_crossed([5.0, -4.0, -1.7], [40.0, -4.0, -1.7]), [0], "through the south wall: concrete");
    assert_eq!(materials_crossed([45.0, 0.0, -1.7], [45.0, 10.0, -1.7]), [0], "through the east wall: concrete");
    assert_eq!(materials_crossed([40.0, 0.0, -3.5], [40.0, 10.0, -3.5]), [1], "through the window: glass only");
    assert_eq!(materials_crossed([40.0, 0.0, -1.7], [40.0, 0.0, -9.0]), [0], "through the roof");
}

#[test]
fn chernobyl_loads_and_the_pilot_sees_the_nsc_door() {
    use glam::DVec3;
    let w = world::load(&shipped_world("chernobyl")).unwrap();
    assert_eq!((w.name.as_str(), w.pilot.position_ned_m), ("Chernobyl", [0.0, -231.0, -1.7]));
    let nsc = &w.load_meshes().unwrap()[0];
    assert_eq!(nsc.loss_db, [40.0]);
    let mut m = Vec::new();
    nsc.mesh.crossings(DVec3::from(w.pilot.position_ned_m), DVec3::new(0.0, -81.0, -20.0), &mut m);
    assert!(m.is_empty(), "the door is open");
    // A mesh that loaded empty would pass the line above: this one crosses the closed end wall beside the door.
    nsc.mesh.crossings(DVec3::from(w.pilot.position_ned_m), DVec3::new(25.0, -76.0, -10.0), &mut m);
    assert!(!m.is_empty(), "the closed end wall beside the door is crossed");
}

/// The shipped hall copied to a temp dir with its relative layout; returns the world path and the `.glb` path.
fn hall_copy(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    fs::create_dir_all(dir.join("assets")).unwrap();
    let glb = dir.join("assets/hall.glb");
    fs::copy(shipped_world("hall").parent().unwrap().join("assets/hall.glb"), &glb).unwrap();
    fs::copy(shipped_world("hall"), dir.join("hall.toml")).unwrap();
    (dir.join("hall.toml"), glb)
}

#[test]
fn load_meshes_after_load_reads_the_file_once() {
    let dir = tempfile::tempdir().unwrap();
    let (path, glb) = hall_copy(dir.path());
    let w = world::load(&path).unwrap();
    fs::remove_file(&glb).unwrap();
    let (a, b) = (w.load_meshes().unwrap(), w.load_meshes().unwrap());
    assert!(std::sync::Arc::ptr_eq(&a[0].mesh, &b[0].mesh));
}

#[test]
fn a_changed_mesh_section_is_reloaded_not_stale() {
    let dir = tempfile::tempdir().unwrap();
    let (path, glb) = hall_copy(dir.path());
    let mut w = world::load(&path).unwrap();
    let before = w.load_meshes().unwrap()[0].mesh.bounds();
    let bytes = fs::read(&glb).unwrap();
    fs::remove_file(&glb).unwrap();
    fs::write(&glb, bytes).unwrap();
    w.meshes[0].position_ned_m[0] += 10.0;
    let after = w.load_meshes().unwrap()[0].mesh.bounds();
    assert!((after.min.x - before.min.x - 10.0).abs() < 1e-9 && (after.max.x - before.max.x - 10.0).abs() < 1e-9, "{before:?} -> {after:?}");
}

#[test]
fn load_meshes_with_a_small_cap_still_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = hall_copy(dir.path());
    let w = world::load(&path).unwrap();
    let e = w.load_meshes_with(3).unwrap_err();
    assert!(e.starts_with("meshes[0].file: "), "{e}");
}

#[test]
fn a_cloned_world_shares_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let (path, glb) = hall_copy(dir.path());
    let w = world::load(&path).unwrap();
    let c = w.clone();
    fs::remove_file(&glb).unwrap();
    assert!(std::sync::Arc::ptr_eq(&w.load_meshes().unwrap()[0].mesh, &c.load_meshes().unwrap()[0].mesh));
}

#[test]
fn a_changed_material_loss_is_not_stale() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = hall_copy(dir.path());
    let mut w = world::load(&path).unwrap();
    let before = w.load_meshes().unwrap()[0].loss_db.clone();
    assert!(!before.is_empty());
    w.materials[0].rf_loss_db += 7.0;
    let after = w.load_meshes().unwrap()[0].loss_db.clone();
    assert_ne!(after, before, "the loss of a material changed after load");
    let named = &w.materials[0].name;
    assert!(after.iter().zip(&before).any(|(a, b)| (a - b - 7.0).abs() < 1e-9), "{named}: {before:?} -> {after:?}");
}
