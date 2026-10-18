//! The submission format (docs/validation.md): what loads, and every way it can be wrong, reported together.
use std::path::Path;

use ofs_validate::submission::{load, Kind, Source};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

/// A `thrust_stand` submission.toml with each key replaceable: `lines` are TOML lines that replace the default of the
/// same key (a key left out of `lines` keeps its default; a line `key =` with nothing after it removes the key).
fn thrust_toml(lines: &[&str]) -> String {
    let defaults = [
        ("schema_version", "1".to_string()),
        ("kind", "\"thrust_stand\"".to_string()),
        ("title", "\"A test stand\"".to_string()),
        ("quad", format!("'{QUAD}'")),
        ("motor", "0".to_string()),
        ("data", "\"thrust.csv\"".to_string()),
    ];
    let origin = [
        ("source", "\"measured\"".to_string()),
        ("note", "\"a note\"".to_string()),
        ("contributor", "\"somebody\"".to_string()),
        ("license", "\"CC-BY-SA-4.0\"".to_string()),
    ];
    let pick = |defaults: &[(&str, String)]| -> String {
        defaults
            .iter()
            .filter_map(|(k, v)| {
                match lines.iter().find(|l| l.split('=').next().map(str::trim) == Some(*k)) {
                    Some(l) if l.trim_end().ends_with('=') => None,
                    Some(l) => Some(l.to_string()),
                    None => Some(format!("{k} = {v}")),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{}\n[origin]\n{}\n", pick(&defaults), pick(&origin))
}

const GOOD_CSV: &str = "command,voltage_v,rpm,thrust_n,current_a\n0.5,24,20000,5.0,10\n1.0,23,30000,10.0,40\n";

fn fixture(toml: &str, csv: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "submission.toml", toml);
    write(dir.path(), "thrust.csv", csv);
    dir
}

#[test]
fn a_valid_thrust_stand_loads() {
    let dir = fixture(&thrust_toml(&[]), GOOD_CSV);
    let s = load(dir.path()).unwrap();
    assert_eq!(s.kind, Kind::ThrustStand);
    assert_eq!(s.title, "A test stand");
    assert_eq!(s.motor, 0);
    assert_eq!(s.origin.source, Source::Measured);
    assert_eq!(s.origin.license, "CC-BY-SA-4.0");
    assert_eq!(s.rows.len(), 2);
    assert_eq!((s.rows[1].command, s.rows[1].voltage_v), (1.0, 23.0));
    assert_eq!((s.rows[1].rpm, s.rows[1].thrust_n, s.rows[1].current_a), (Some(30000.0), Some(10.0), Some(40.0)));
}

#[test]
fn problems_in_the_toml_are_reported_together() {
    let dir = fixture(
        &thrust_toml(&["schema_version = 2", "motor = 9", "source = \"guess\"", "license =", "title ="]),
        GOOD_CSV,
    );
    let problems = load(dir.path()).unwrap_err();
    let all = problems.join("\n");
    for needle in ["schema_version 2", "motor 9", "source", "license", "title"] {
        assert!(all.contains(needle), "{needle} missing from:\n{all}");
    }
    assert!(problems.iter().all(|p| p.starts_with("submission.toml")), "{problems:?}");
    assert!(problems.len() >= 5, "{problems:?}");
}

#[test]
fn an_unknown_kind_and_an_unknown_key_are_refused() {
    let dir = fixture(&thrust_toml(&["kind = \"treadmill\""]), GOOD_CSV);
    assert!(load(dir.path()).unwrap_err().join("\n").contains("kind 'treadmill'"));
    let dir = fixture(&format!("{}wobble = 1\n", thrust_toml(&[])), GOOD_CSV);
    assert!(load(dir.path()).unwrap_err().join("\n").contains("wobble"));
}

#[test]
fn a_toml_type_error_is_its_own_problem() {
    // Schema problems lost their line number when they started coming back all together (B5); a file that does not
    // PARSE still names its line.
    let dir = fixture("schema_version = 1\nkind = \"thrust_stand\"\nmotor = \"zero\"\n", GOOD_CSV);
    let problems = load(dir.path()).unwrap_err();
    assert!(problems.iter().any(|p| p.contains("motor must be a whole number")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("missing title")), "{problems:?}");
}

#[test]
fn an_unknown_key_is_its_own_problem() {
    let dir = fixture("schema_version = 1\nkind = \"thrust_stand\"\n\ncolour = 1\n", GOOD_CSV);
    let problems = load(dir.path()).unwrap_err();
    assert!(problems.iter().any(|p| p.contains("unknown key 'colour'")), "{problems:?}");
}

#[test]
fn problems_in_the_csv_are_reported_with_file_and_line() {
    let csv = "command,voltage_v,rpm,torque\n1.5,24,20000,1\n0.5,0,20000,1\n0.5,24,fast,1\n0.5,24,,\n0.5,24,100\n";
    let dir = fixture(&thrust_toml(&[]), csv);
    let problems = load(dir.path()).unwrap_err();
    let all = problems.join("\n");
    for needle in [
        "thrust.csv line 1: unknown column 'torque'",
        "thrust.csv line 2: command 1.5 is outside [0, 1]",
        "thrust.csv line 3: voltage_v 0 must be > 0",
        "thrust.csv line 4: 'fast' is not a finite number",
        "thrust.csv line 5: no measured value",
        "thrust.csv line 6: 3 values for 4 columns",
    ] {
        assert!(all.contains(needle), "{needle} missing from:\n{all}");
    }
}

#[test]
fn a_csv_needs_its_columns() {
    let dir = fixture(&thrust_toml(&[]), "command,rpm\n0.5,100\n");
    assert!(load(dir.path()).unwrap_err().join("\n").contains("thrust.csv line 1: the header needs a voltage_v column"));
    let dir = fixture(&thrust_toml(&[]), "command,voltage_v\n0.5,24\n");
    assert!(load(dir.path()).unwrap_err().join("\n").contains("at least one of rpm, thrust_n, current_a"));
    let dir = fixture(&thrust_toml(&[]), "command,voltage_v,rpm\n");
    assert!(load(dir.path()).unwrap_err().join("\n").contains("no rows"));
}

#[test]
fn a_bom_and_crlf_are_accepted() {
    let csv = format!("\u{feff}{}", GOOD_CSV.replace('\n', "\r\n"));
    let dir = fixture(&thrust_toml(&[]), &csv);
    assert_eq!(load(dir.path()).unwrap().rows.len(), 2);
}

#[test]
fn comments_and_blank_lines_are_skipped_but_line_numbers_stay_physical() {
    let dir = fixture(&thrust_toml(&[]), "# a stand log\n\ncommand,voltage_v,rpm\n0.5,24,abc\n");
    assert!(load(dir.path()).unwrap_err().join("\n").contains("thrust.csv line 4: 'abc'"));
}

#[test]
fn a_blank_cell_is_not_measured() {
    let dir = fixture(&thrust_toml(&[]), "command,voltage_v,rpm,thrust_n,current_a\n0.5,24,20000,,10\n");
    let s = load(dir.path()).unwrap();
    assert_eq!((s.rows[0].rpm, s.rows[0].thrust_n, s.rows[0].current_a), (Some(20000.0), None, Some(10.0)));
}

#[test]
fn a_missing_data_file_and_a_missing_quad_are_named() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "submission.toml", &thrust_toml(&["quad = \"nowhere/quad.toml\""]));
    let all = load(dir.path()).unwrap_err().join("\n");
    assert!(all.contains("thrust.csv"), "{all}");
    assert!(all.contains("nowhere/quad.toml"), "{all}");
    assert!(load(&dir.path().join("nothing")).unwrap_err().join("\n").contains("submission.toml"));
}

fn blackbox_toml(diff: &str) -> String {
    thrust_toml(&["kind = \"blackbox_step\"", "data = \"flight.csv\"", "motor ="]).replacen("[origin]", &format!("diff = \"{diff}\"\n[origin]"), 1)
}

const BLACKBOX_HEADER: &str = "loopIteration, time (us), gyroADC[0], gyroADC[1], gyroADC[2], setpoint[0], setpoint[1], setpoint[2], motor[0]";

#[test]
fn a_blackbox_submission_is_checked_for_files_and_columns() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "submission.toml", &blackbox_toml("tune.diff"));
    write(dir.path(), "tune.diff", "set motor_poles = 14\n");
    write(dir.path(), "flight.csv", &format!("{BLACKBOX_HEADER}\n0, 0, 1, 2, 3, 4, 5, 6, 7\n"));
    let s = load(dir.path()).unwrap();
    assert_eq!(s.kind, Kind::BlackboxStep);
    assert!(s.rows.is_empty());
    // Take a column away and the diff away: both are named, and the header found is shown.
    write(dir.path(), "flight.csv", "loopIteration, time (us), gyroADC[0], gyroADC[1], gyroADC[2], setpoint[0], setpoint[1]\n0,0,1,2,3,4,5\n");
    std::fs::remove_file(dir.path().join("tune.diff")).unwrap();
    let all = load(dir.path()).unwrap_err().join("\n");
    assert!(all.contains("flight.csv line 1: missing column 'setpoint[2]'"), "{all}");
    assert!(all.contains("found: loopIteration, time (us)"), "{all}");
    assert!(all.contains("tune.diff"), "{all}");
}

#[test]
fn a_blackbox_submission_needs_a_diff() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "submission.toml", &thrust_toml(&["kind = \"blackbox_step\"", "data = \"flight.csv\"", "motor ="]));
    write(dir.path(), "flight.csv", &format!("{BLACKBOX_HEADER}\n0, 0, 1, 2, 3, 4, 5, 6, 7\n"));
    assert!(load(dir.path()).unwrap_err().join("\n").contains("diff"));
}

#[test]
fn several_bad_keys_are_all_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    // One unknown key and one wrong-typed key, next to a perfectly good kind: both problems come back in one pass.
    // (thrust_toml only emits its own keys, so the unknown one is spliced in here.)
    write(dir.path(), "submission.toml", &thrust_toml(&["title = 5"]).replace("[origin]", "colour = 1
[origin]"));
    let problems = load(dir.path()).unwrap_err();
    assert!(problems.iter().any(|p| p.contains("'colour'")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("title") && p.contains("text")), "{problems:?}");
    assert!(!problems.iter().any(|p| p.contains("kind")), "{problems:?}");
}
