use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::Duration;

use ofs_core::Bus;
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net::SitlNet;
use ofs_fc::sitl::process::{is_bind_failure, LaunchConfig};
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
