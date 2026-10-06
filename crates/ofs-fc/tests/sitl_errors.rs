use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::Duration;

use ofs_core::Bus;
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net::SitlNet;
use ofs_fc::sitl::process::{is_bind_failure, is_ready_line, is_reset_line, LaunchConfig};
use ofs_fc::sitl::FcError;

// These tests bind the fixed SITL UDP ports, so they must not run concurrently.
static PORTS: Mutex<()> = Mutex::new(());

fn config(dir: &std::path::Path, launch: &[&str]) -> BridgeConfig {
    let diff = dir.join("quad.diff");
    std::fs::write(&diff, "feature -GPS\n").unwrap();
    BridgeConfig {
        launch: LaunchConfig {
            launch: launch.iter().map(|s| s.to_string()).collect(),
            cleanup: vec![],
            workdir: dir.join("fc"),
            diff_file: diff,
            startup_timeout: Duration::from_secs(2),
        },
        net: SitlNet::loopback(),
        rate_divisor: 8,
        first_reply_timeout: Duration::from_millis(500),
        reply_timeout: Duration::from_millis(200),
        home: Home { lat_deg: 50.0, lon_deg: 4.0, alt_m: 0.0 },
        motor_count: 4,
        serial: vec![],
    }
}

#[test]
fn bind_failure_lines_are_recognised() {
    // A stale SITL keeps the ports; the new one prints this and keeps running (M0 §6).
    assert!(is_bind_failure("bind port 5761 for UART1 failed!!"));
    assert!(!is_bind_failure("bind port 5761 for UART1"));
    assert!(!is_bind_failure("[SITL] init PwmOut UDP link to gazebo 127.0.0.1:9002...0"));
}

#[test]
fn launch_failure_names_the_command() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let err = SitlBridge::start(config(dir.path(), &["ofs-definitely-missing-binary"]), &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Launch { .. }), "{err}");
    assert!(err.to_string().contains("ofs-definitely-missing-binary"), "{err}");
}

#[test]
fn busy_pwm_port_is_reported() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let _holder = UdpSocket::bind("127.0.0.1:9002").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let err = SitlBridge::start(config(dir.path(), &["ofs-definitely-missing-binary"]), &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::PortInUse { port: 9002, .. }), "{err}");
    assert!(err.to_string().contains("9002"), "{err}");
}

#[test]
fn missing_sitl_explains_how_to_fix_it() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let err = SitlBridge::start(config(dir.path(), &["ofs-definitely-missing-binary"]), &mut Bus::new()).err().unwrap();
    let shown = err.to_string();
    for hint in ["scripts/build-sitl.sh", "OFS_SITL_LAUNCH", "docs/dev-setup.md"] {
        assert!(shown.contains(hint), "missing `{hint}` in: {shown}");
    }
    if cfg!(windows) {
        assert!(shown.contains("wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"), "{shown}");
    }
}

#[test]
fn ready_lines_are_recognised() {
    assert!(is_ready_line("[SITL] ready for the simulator"));
    assert!(!is_ready_line("bind port 5761 for UART1"));
}

#[test]
fn a_changed_quad_diff_refuses_a_stale_eeprom() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    // First boot happened with another diff: eeprom.bin and the applied copy exist.
    std::fs::create_dir_all(dir.path().join("fc")).unwrap();
    std::fs::write(dir.path().join("fc/eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::write(dir.path().join("fc/betaflight.diff"), "feature GPS\n").unwrap();
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Config(_)), "{err}");
    let shown = err.to_string();
    assert!(shown.contains("changed since") && shown.contains("eeprom.bin"), "{shown}");
}

#[test]
fn reset_lines_are_recognised() {
    // Betaflight's systemReset() / systemResetToBootloader() in SITL, just before exit(0).
    assert!(is_reset_line("[system]Reset!"));
    assert!(is_reset_line("[system]ResetToBootloader!"));
    assert!(!is_reset_line("[system]Init..."));
}

/// Betaflight's MSP/TCP thread can print `bind port 5761 ... failed` just after the ready line.
#[cfg(unix)]
#[test]
fn a_bind_failure_just_after_the_ready_line_is_a_startup_error() {
    use ofs_fc::sitl::process::SitlProcess;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["sh", "-c", "echo '[SITL] ready for the simulator'; sleep 0.1; echo 'bind port 5761 for UART1 failed!!'; sleep 5"]);
    // Not a first boot: eeprom.bin and the applied diff exist, so the fake is only run as SITL.
    std::fs::create_dir_all(&cfg.launch.workdir).unwrap();
    std::fs::write(cfg.launch.workdir.join("eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::copy(&cfg.launch.diff_file, cfg.launch.workdir.join("betaflight.diff")).unwrap();
    let err = SitlProcess::start(&cfg.launch).err().unwrap();
    assert!(matches!(err, FcError::Startup(..)), "{err}");
    assert!(err.to_string().contains("could not bind its ports"), "{err}");
}
