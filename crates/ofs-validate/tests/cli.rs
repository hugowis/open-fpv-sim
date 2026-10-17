//! The `ofs-validate` binary: its report, its exit codes and the CI job summary.
use std::path::Path;
use std::process::Command;

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn submission(dir: &Path, csv: &str) {
    let toml = format!(
        "schema_version = 1\nkind = \"thrust_stand\"\ntitle = \"cli test\"\nquad = '{QUAD}'\nmotor = 0\ndata = \"thrust.csv\"\n\
         [origin]\nsource = \"estimated\"\nnote = \"test\"\ncontributor = \"me\"\nlicense = \"CC-BY-SA-4.0\"\n"
    );
    std::fs::write(dir.join("submission.toml"), toml).unwrap();
    std::fs::write(dir.join("thrust.csv"), csv).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ofs-validate")).arg(dir).args(args).env_remove("GITHUB_STEP_SUMMARY").output().unwrap()
}

const CSV: &str = "command,voltage_v,rpm,thrust_n\n0.5,24,20000,5.0\n1.0,23,30000,10.0\n";

#[test]
fn a_valid_submission_prints_its_report_and_exits_zero_whatever_the_error() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    let out = run(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("# Thrust-stand comparison: cli test"), "{text}");
    assert!(text.contains("| rpm | 2 |"), "{text}");
}

#[test]
fn out_writes_the_same_report_to_a_file() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    let file = dir.path().join("REPORT.md");
    let out = run(dir.path(), &["--out", file.to_str().unwrap()]);
    assert_eq!(std::fs::read_to_string(file).unwrap(), String::from_utf8_lossy(&out.stdout));
}

#[test]
fn the_gate_is_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    assert_eq!(run(dir.path(), &["--max-error-pct", "0.0001"]).status.code(), Some(1), "an error above the gate");
    assert_eq!(run(dir.path(), &["--max-error-pct", "100000"]).status.code(), Some(0));
}

/// A copy of the shipped library whose propeller is a flywheel, so no row ever settles.
fn flywheel_library(dir: &Path) -> std::path::PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let library = dir.join("library");
    copy(&Path::new(QUAD).parent().unwrap().join("../library"), &library);
    let prop = library.join("props/opendrone-5in-prop.toml");
    let text = std::fs::read_to_string(&prop).unwrap();
    assert!(text.contains("inertia_kgm2 = 6.0e-6"));
    std::fs::write(&prop, text.replace("inertia_kgm2 = 6.0e-6", "inertia_kgm2 = 1.0e3")).unwrap();
    library
}

fn run_with_library(dir: &Path, library: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ofs-validate")).arg(dir).args(args).env("OFS_LIBRARY", library).env_remove("GITHUB_STEP_SUMMARY").output().unwrap()
}

#[test]
fn a_gate_fails_when_a_row_never_settles() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    let library = flywheel_library(dir.path());
    let out = run_with_library(dir.path(), &library, &["--max-error-pct", "100000"]);
    assert_eq!(out.status.code(), Some(1), "{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("2 row(s) did not settle"), "{stderr}");
}

#[test]
fn no_gate_means_exit_zero_even_with_an_unsettled_row() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    let library = flywheel_library(dir.path());
    let out = run_with_library(dir.path(), &library, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("no steady state within 10 s"));
}

#[test]
fn an_invalid_submission_exits_two_with_one_problem_per_line() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), "command,voltage_v,rpm\n1.5,0,abc\n");
    let out = run(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.lines().count() >= 3 && stderr.lines().all(|l| l.starts_with("ofs-validate: thrust.csv line 2")), "{stderr}");
    assert_eq!(run(&dir.path().join("nowhere"), &[]).status.code(), Some(2));
}

#[test]
fn summary_appends_the_report_to_the_job_summary_file() {
    let dir = tempfile::tempdir().unwrap();
    submission(dir.path(), CSV);
    let summary = dir.path().join("summary.md");
    std::fs::write(&summary, "# Earlier step\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ofs-validate")).arg(dir.path()).arg("--summary").env("GITHUB_STEP_SUMMARY", &summary).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = std::fs::read_to_string(summary).unwrap();
    assert!(text.starts_with("# Earlier step\n") && text.contains("# Thrust-stand comparison: cli test"), "{text}");
}
