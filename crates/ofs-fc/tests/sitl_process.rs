//! SITL supervision against fake programs that print what Betaflight SITL prints (no Betaflight needed).
use std::time::{Duration, Instant};

use ofs_fc::sitl::process::{LaunchConfig, SitlProcess, READY_LINE};
use ofs_fc::sitl::FcError;

/// A program that prints the ready line, waits a little, then runs `then` (shell lines) and exits.
fn fake(then: &[&str]) -> Vec<String> {
    if cfg!(windows) {
        let mut script = format!("Write-Output '{READY_LINE}'; Start-Sleep -Milliseconds 800");
        for line in then {
            script.push_str("; ");
            script.push_str(line);
        }
        vec!["powershell".into(), "-NoProfile".into(), "-Command".into(), script]
    } else {
        let mut script = format!("echo '{READY_LINE}'; sleep 0.8");
        for line in then {
            script.push_str("; ");
            script.push_str(&line.replace("Write-Output", "echo"));
        }
        vec!["sh".into(), "-c".into(), script]
    }
}

fn launch(dir: &std::path::Path, argv: Vec<String>) -> LaunchConfig {
    let workdir = dir.join("fc");
    std::fs::create_dir_all(&workdir).unwrap();
    // Already configured with this diff: no first-boot diff run.
    std::fs::write(workdir.join("eeprom.bin"), b"").unwrap();
    std::fs::write(workdir.join("betaflight.diff"), "feature -GPS\n").unwrap();
    std::fs::write(dir.join("quad.diff"), "feature -GPS\n").unwrap();
    LaunchConfig { launch: argv, cleanup: vec![], workdir, diff_file: dir.join("quad.diff"), startup_timeout: Duration::from_secs(20) }
}

#[test]
fn a_reset_line_then_a_clean_exit_is_a_reboot() {
    let dir = tempfile::tempdir().unwrap();
    let mut proc = SitlProcess::start(&launch(dir.path(), fake(&["Write-Output '[system]Reset'", "exit 0"]))).unwrap();
    assert!(proc.rebooted());
}

#[test]
fn a_crash_without_a_reset_line_is_not_a_reboot_and_is_seen_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let mut proc = SitlProcess::start(&launch(dir.path(), fake(&["exit 3"]))).unwrap();
    while proc.exit_status().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = Instant::now();
    assert!(!proc.rebooted());
    assert!(started.elapsed() < Duration::from_secs(1), "waited {:?} for a crash", started.elapsed());
}

#[test]
fn a_reset_line_with_a_failing_exit_is_not_a_reboot() {
    let dir = tempfile::tempdir().unwrap();
    let mut proc = SitlProcess::start(&launch(dir.path(), fake(&["Write-Output '[system]Reset'", "exit 1"]))).unwrap();
    assert!(!proc.rebooted());
}

#[test]
fn a_wsl_launch_whose_program_is_missing_points_at_the_path() {
    let argv = ["wsl.exe", "-d", "Ubuntu", "-e", "/home/me/wrong/betaflight_SITL.elf"].map(String::from);
    let tail = "<3>WSL (12) ERROR: CreateProcessCommon:640: execvpe(/home/me/wrong/betaflight_SITL.elf) failed: No such file or directory";
    let hint = ofs_fc::sitl::startup_hint(&argv, tail);
    assert!(hint.contains("/home/me/wrong/betaflight_SITL.elf") && hint.contains("inside WSL"), "{hint}");
    assert_eq!(ofs_fc::sitl::startup_hint(&argv, "some other failure"), "");
    let native = ["/opt/betaflight_SITL.elf"].map(String::from);
    assert_eq!(ofs_fc::sitl::startup_hint(&native, tail), "");
    let _: Option<FcError> = None;
}
