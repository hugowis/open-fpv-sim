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

#[test]
fn a_sitl_that_rejects_the_port_offset_points_at_the_rebuild() {
    let tail = "Unknown argument: --port-offset";
    let wsl = ["wsl.exe", "-d", "Ubuntu", "-e", "/home/me/betaflight_SITL.elf", "--port-offset", "10"].map(String::from);
    let hint = ofs_fc::sitl::startup_hint(&wsl, tail);
    assert!(hint.contains("--port-offset") && hint.contains("scripts/build-sitl.sh"), "{hint}");
    let native = ["/opt/betaflight_SITL.elf", "--port-offset", "0"].map(String::from);
    assert!(ofs_fc::sitl::startup_hint(&native, tail).contains("scripts/build-sitl.sh"), "{hint}");
    // The same rejection from a launch without our --port-offset is the user's own old binary: still a rebuild.
    let unscoped = ["/opt/betaflight_SITL.elf"].map(String::from);
    assert!(ofs_fc::sitl::startup_hint(&unscoped, tail).contains("scripts/build-sitl.sh"), "{hint}");
    // A patched binary rejecting some other unknown option of a hand-written launch is not this failure.
    assert_eq!(ofs_fc::sitl::startup_hint(&native, "Unknown argument: --badflag"), "");
}

#[cfg(unix)]
mod first_boot_lock {
    //! Two simulators first-booting one quad's directory at once: the diff applies once (the config.lock serialization).
    use std::sync::Arc;

    use ofs_fc::sitl::process::{LaunchConfig, SitlProcess};

    /// A fake SITL whose first boot (`--config`, no eeprom.bin yet) sleeps, saves the EEPROM, counts the apply and
    /// exits; its real boot prints the ready line and stays alive like a running SITL.
    fn sh_first_boot() -> Vec<String> {
        vec!["sh".into(), "-c".into(), "\
             if [ ! -f eeprom.bin ]; then \
               sleep 0.3; echo '[CONFIG] Config file processed, EEPROM saved'; echo x >> ../applied-count.txt; touch eeprom.bin; exit 0; \
             fi; \
             echo '[SITL] ready for the simulator'; sleep 30"
            .into()]
    }

    fn first_boot(dir: &std::path::Path) -> LaunchConfig {
        let workdir = dir.join("fc");
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(dir.join("quad.diff"), "feature -GPS\n").unwrap();
        LaunchConfig { launch: sh_first_boot(), cleanup: vec![], workdir, diff_file: dir.join("quad.diff"), startup_timeout: std::time::Duration::from_secs(20) }
    }

    #[test]
    fn two_first_boots_of_one_quad_apply_the_diff_once() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Arc::new(first_boot(dir.path()));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let cfg = Arc::clone(&cfg);
                std::thread::spawn(move || SitlProcess::start(&cfg))
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
        let count = std::fs::read_to_string(dir.path().join("applied-count.txt")).unwrap();
        assert_eq!(count.lines().count(), 1, "the diff applied once, not once per racing boot");
    }

    #[test]
    fn a_finished_first_boot_leaves_no_lock_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = first_boot(dir.path());
        let _proc = SitlProcess::start(&cfg).unwrap();
        assert!(!cfg.workdir.join("config.lock").exists(), "the lock is removed when the first boot ends");
    }

    #[test]
    fn a_stale_config_lock_does_not_wedge_the_boot() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = first_boot(dir.path());
        let lock = cfg.workdir.join("config.lock");
        std::fs::write(&lock, b"").unwrap();
        // Ten minutes old (not `touch -d`, which BSD/macOS `touch` does not have).
        let ten_minutes_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::OpenOptions::new().write(true).open(&lock).unwrap().set_modified(ten_minutes_ago).unwrap();
        let _proc = SitlProcess::start(&cfg).unwrap();
        let count = std::fs::read_to_string(dir.path().join("applied-count.txt")).unwrap();
        assert_eq!(count.lines().count(), 1, "the stale lock was removed and the boot went ahead");
    }
}
