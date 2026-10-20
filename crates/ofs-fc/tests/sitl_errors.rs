use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::Duration;

use ofs_core::Bus;
use ofs_fc::sitl::bridge::{BridgeConfig, SitlBridge};
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net::SitlNet;
use ofs_fc::sitl::process::{is_bind_failure, is_ready_line, is_reset_line, LaunchConfig};
use ofs_fc::sitl::FcError;

// These tests bind the fixed SITL UDP ports, so they must not run concurrently. Tests that spawn a process
// take the lock too: a fork briefly inherits the sockets another test holds, which makes a later bind fail.
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
        ports: ofs_fc::sitl::codec::PortMap::default(),
        rate_divisor: 8,
        first_reply_timeout: Duration::from_millis(500),
        reply_timeout: Duration::from_millis(200),
        home: Home { lat_deg: 50.0, lon_deg: 4.0, alt_m: 0.0 },
        motor_count: 4,
        serial: vec![],
        erpm: Vec::new(),
        pole_pairs: Vec::new(),
        taps: vec![],
    }
}

#[test]
fn bind_failure_lines_are_recognised() {
    // A stale SITL keeps the ports; the new one prints this and keeps running (M0 §6).
    assert!(is_bind_failure("bind port 5761 for UART1 failed!!"));
    assert!(is_bind_failure("bind port 5761 for UART1 failed!! after 20 attempts (errno 98)"));
    assert!(!is_bind_failure("bind port 5761 for UART1 retry 3/20 (errno 98)"));
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
        assert!(shown.contains("scripts/build-sitl-windows.sh"), "the native route is named too: {shown}");
    }
}

/// The cleanup is a courtesy before a launch: a command that hangs (a stuck WMI query behind the Windows cleanup, a
/// wedged `wsl.exe`) must not hang the load with it.
#[test]
fn a_cleanup_that_hangs_is_given_up_on() {
    let sleeper: Vec<String> = if cfg!(windows) {
        ["ping", "-n", "8", "127.0.0.1"].map(String::from).to_vec()
    } else {
        ["sleep", "8"].map(String::from).to_vec()
    };
    let started = std::time::Instant::now();
    ofs_fc::sitl::process::run_cleanup_within(&sleeper, Duration::from_millis(300));
    assert!(started.elapsed() < Duration::from_secs(3), "waited {:?}", started.elapsed());
    // A cleanup that finishes is waited for in full.
    let quick: Vec<String> = if cfg!(windows) {
        ["cmd", "/C", "exit", "0"].map(String::from).to_vec()
    } else {
        vec!["true".to_string()]
    };
    let started = std::time::Instant::now();
    ofs_fc::sitl::process::run_cleanup_within(&quick, Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(3), "a quick cleanup returns when it ends: {:?}", started.elapsed());
}

/// A native Windows SITL that cannot find its Cygwin runtime DLLs exits at once with `0xC0000135` and prints nothing.
#[test]
fn a_missing_runtime_dll_is_named() {
    let hint = ofs_fc::sitl::exit_hint("exit code: 0xc0000135");
    assert!(hint.contains("msys-2.0.dll") && hint.contains("next to betaflight_SITL.exe"), "{hint}");
    assert!(ofs_fc::sitl::exit_hint("exit code: 0x1").is_empty());
    assert!(ofs_fc::sitl::exit_hint("exit status: 1").is_empty());
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
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
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

/// A relaunch after a Betaflight reboot keeps the log from before it; the first start truncates.
#[cfg(unix)]
#[test]
fn a_relaunch_appends_to_the_sitl_log() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["sh", "-c", "echo '[SITL] ready for the simulator'; sleep 5"]);
    std::fs::create_dir_all(&cfg.launch.workdir).unwrap();
    std::fs::write(cfg.launch.workdir.join("eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::copy(&cfg.launch.diff_file, cfg.launch.workdir.join("betaflight.diff")).unwrap();
    let log = cfg.launch.workdir.join("sitl.log");
    std::fs::write(&log, "stale log of an earlier run
").unwrap();
    drop(SitlProcess::start(&cfg.launch).unwrap());
    let first = std::fs::read_to_string(&log).unwrap();
    assert_eq!(first, "[SITL] ready for the simulator
", "the first start truncates");
    drop(SitlProcess::relaunch(&cfg.launch).unwrap());
    let both = std::fs::read_to_string(&log).unwrap();
    assert_eq!(both, format!("{first}--- relaunch after reboot ---
{first}"), "the relaunch appends");
}

#[test]
fn more_than_four_motors_is_a_config_error_not_a_panic() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    cfg.motor_count = 6;
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Config(_)), "{err}");
    assert!(err.to_string().contains("4 motors"), "{err}");
}

/// A firmware directory that has already booted once with `applied` as its diff.
fn booted_with(dir: &std::path::Path, applied: Option<&str>) {
    std::fs::create_dir_all(dir.join("fc")).unwrap();
    std::fs::write(dir.join("fc/eeprom.bin"), [0u8; 16]).unwrap();
    if let Some(text) = applied {
        std::fs::write(dir.join("fc/betaflight.diff"), text).unwrap();
    }
}

#[test]
fn an_unchanged_diff_reuses_the_eeprom() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    booted_with(dir.path(), Some("feature -GPS\n"));
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Launch { .. }), "the guard let it through to the launch: {err}");
}

#[test]
fn a_diff_that_only_changed_its_line_endings_is_unchanged() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    booted_with(dir.path(), Some("feature -GPS\r\n")); // e.g. checked out with CRLF on Windows since
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Launch { .. }), "{err}");
}

#[test]
fn an_eeprom_whose_diff_is_unknown_is_refused() {
    // Firmware directories from before M2a kept no copy of the applied diff: their EEPROM may still select the UDP
    // receiver, and Betaflight would then fail safe with no explanation.
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    booted_with(dir.path(), None);
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Config(_)), "{err}");
    assert!(err.to_string().contains("eeprom.bin"), "{err}");
}

/// The CI flake's evidence was lost: the bind line scrolled out of a tail full of EEPROM writes, and nothing said
/// who held the port. The error must carry the failing bind lines themselves, the flash writes only as a count, and
/// (on Linux) the process listening on the port.
#[cfg(target_os = "linux")]
#[test]
fn a_bind_failure_names_the_port_and_who_holds_it() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let holder = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = holder.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let script = format!(
        "echo 'bind port {port} for UART2 failed!!' >&2; i=0; while [ $i -lt 400 ]; do echo \"[FLASH_ProgramWord]0x0 = 0\"; i=$((i+1)); done; echo '[SITL] ready for the simulator'; sleep 5"
    );
    let cfg = config(dir.path(), &["sh", "-c", &script]);
    std::fs::create_dir_all(&cfg.launch.workdir).unwrap();
    std::fs::write(cfg.launch.workdir.join("eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::copy(&cfg.launch.diff_file, cfg.launch.workdir.join("betaflight.diff")).unwrap();
    let err = SitlProcess::start(&cfg.launch).err().unwrap().to_string();
    assert!(err.contains(&format!("bind port {port} for UART2 failed!!")), "the failing line itself: {err}");
    assert!(err.contains(&format!("pid {}", std::process::id())), "the listener on the port: {err}");
    assert!(err.contains("400 [FLASH_ProgramWord] lines"), "flash writes as a count: {err}");
    assert!(!err.contains("[FLASH_ProgramWord]0x0"), "not the lines themselves");
}

#[cfg(target_os = "linux")]
#[test]
fn port_holders_finds_a_listener_and_nothing_on_a_free_port() {
    use ofs_fc::sitl::process::port_holders;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let found = port_holders(port);
    assert!(found.iter().any(|h| h.contains("LISTEN") && h.contains(&format!("pid {}", std::process::id()))), "{found:?}");
    drop(listener);
    // Not every LISTEN on the port: another test in this process may take the freed ephemeral port at once, or a child
    // another test thread is forking may hold a copy for the instant before it execs (it failed so once in CI). This
    // process's own listener is what must be gone.
    let mine = format!("pid {}", std::process::id());
    assert!(port_holders(port).iter().all(|h| !(h.contains("LISTEN") && h.contains(&mine))), "a closed listener is gone");
}

/// SITL creates eeprom.bin at boot, so its existence does not prove the diff was applied: a config run that exits
/// non-zero (Betaflight's `[CONFIG] prepareSave failed`) must be a startup error, not a quad flying the defaults.
#[cfg(unix)]
#[test]
fn a_config_run_that_fails_is_an_error_even_if_eeprom_bin_exists() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["sh", "-c", "head -c 64 /dev/zero > eeprom.bin; echo '[CONFIG] prepareSave failed'; exit 1"]);
    let err = SitlProcess::start(&cfg.launch).err().unwrap();
    assert!(matches!(err, FcError::Config(_)), "{err}");
    let shown = err.to_string();
    assert!(shown.contains("prepareSave failed") && shown.contains("exit"), "{shown}");
}

/// The config run's last line ("EEPROM saved") comes after a flood of EEPROM writes; the process can exit before the
/// pipe readers have read it. Found live: a good first boot refused as "did not apply the diff" (1 run in 12).
#[cfg(unix)]
#[test]
fn a_config_run_is_judged_on_its_whole_output() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    // The saved line reaches the pipe 300 ms after the process exits (a background writer still holds stdout): the
    // check must wait for the end of the output, not for the exit.
    let config_run = "head -c 64 /dev/zero > eeprom.bin; (sleep 0.3; echo '[CONFIG] Config file processed, EEPROM saved') & exit 0";
    let script = format!("if [ \"$1\" = --config ]; then {config_run}; else echo '[SITL] ready for the simulator'; sleep 5; fi");
    let cfg = config(dir.path(), &["sh", "-c", &script, "sitl"]);
    let started = SitlProcess::start(&cfg.launch);
    assert!(started.is_ok(), "a config run that saved is accepted: {:?}", started.err().map(|e| e.to_string()));
}

// The port-bind flake (docs/superpowers/history/closeout-evidence.md): the cleanup's `pkill` only *signals* its
// victim, whose sockets close one by one while it dies, so a launch that immediately follows can fail to bind.
// The cleanup must wait for the victim to be really gone — escalating to SIGKILL when it ignores SIGTERM — and
// report what it saw, so a recurrence says whether a victim existed at launch at all.

/// A spawned fake SITL that is killed when dropped: an assertion firing between the spawn and the test's own
/// cleanup cannot leak the process (the M6d review's deferred minor).
#[cfg(unix)]
struct FakeSitl(std::process::Child);

#[cfg(unix)]
impl FakeSitl {
    /// The child's pid (a test that kills by hand, before the drop's own kill).
    fn id(&self) -> u32 {
        self.0.id()
    }

    /// Reaps the child early (the drop's kill and wait still run on an already-reaped child).
    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.0.wait()
    }
}

#[cfg(unix)]
impl Drop for FakeSitl {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawns `<dir>/betaflight_SITL -c <script>`: a copy of /bin/sh under the SITL name, so the process's comm is
/// the name the cleanup waits for, without touching a real SITL.
#[cfg(unix)]
fn spawn_fake_sitl(dir: &std::path::Path, sh_script: &str) -> FakeSitl {
    let fake = dir.join("betaflight_SITL");
    std::fs::copy("/bin/sh", &fake).unwrap();
    FakeSitl(std::process::Command::new(&fake).arg("-c").arg(sh_script).spawn().unwrap())
}

/// As [`spawn_fake_sitl`], launched with SITL's `--port-offset <offset>` after the script (the shell takes the
/// arguments as its positionals, so its command line reads like a slot's SITL).
#[cfg(unix)]
fn spawn_fake_sitl_in_slot(dir: &std::path::Path, sh_script: &str, offset: u16) -> FakeSitl {
    spawn_fake_sitl_with(dir, sh_script, &["--port-offset", &offset.to_string()])
}

/// A fake SITL with `extra` after the script; the binary is copied once per directory (a copy over a running binary
/// is "text file busy").
#[cfg(unix)]
fn spawn_fake_sitl_with(dir: &std::path::Path, sh_script: &str, extra: &[&str]) -> FakeSitl {
    let fake = dir.join("betaflight_SITL");
    if !fake.exists() {
        std::fs::copy("/bin/sh", &fake).unwrap();
    }
    FakeSitl(std::process::Command::new(&fake).arg("-c").arg(sh_script).args(extra).spawn().unwrap())
}

#[cfg(unix)]
fn is_alive(child: &mut FakeSitl) -> bool {
    // A killed child stays a zombie until waited for, which holds no ports: try_wait reaps it.
    child.0.try_wait().unwrap().is_none()
}


/// A slot's cleanup and wait take down that slot's SITL and leave every other slot's running: a second session
/// starting (or a crashed one's slot being reused) must not kill the first.
#[cfg(target_os = "linux")]
#[test]
fn a_cleanup_for_one_slot_leaves_another_slots_sitl_alive() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 10);
    std::thread::sleep(Duration::from_millis(100)); // let both exec
    let launch: Vec<String> = [dir.path().join("betaflight_SITL").display().to_string(), "--port-offset".into(), "10".into()].to_vec();
    ofs_fc::sitl::process::run_cleanup(&ofs_fc::sitl::net::default_cleanup(&launch));
    let outcome = ofs_fc::sitl::process::await_cleanup(&launch);
    // The TERM usually kills the fake before the wait looks (0 seen); never more than slot 1's one process.
    assert!(outcome.victims_seen <= 1, "only slot 1's SITL is a victim of slot 1's cleanup: {outcome:?}");
    assert!(!outcome.escalated, "{outcome:?}");
    assert!(!is_alive(&mut slot1), "slot 1's SITL is gone");
    assert!(is_alive(&mut slot0), "slot 0's SITL survives slot 1's cleanup");
}

/// A SITL started without the option (an older simulator, a hand-made launch) holds the stock ports, so it is slot 0's:
/// slot 0's cleanup takes it down, and another slot's cleanup leaves it alone.
#[cfg(target_os = "linux")]
#[test]
fn a_sitl_without_the_option_belongs_to_slot_zero() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut stock = spawn_fake_sitl_with(dir.path(), "sleep 30; sleep 30", &[]);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 10);
    std::thread::sleep(Duration::from_millis(100)); // let both exec
    let sitl = dir.path().join("betaflight_SITL").display().to_string();
    let launch_for = |offset: &str| [sitl.clone(), "--port-offset".to_string(), offset.to_string()].to_vec();
    ofs_fc::sitl::process::run_cleanup(&ofs_fc::sitl::net::default_cleanup(&launch_for("20")));
    std::thread::sleep(Duration::from_millis(100));
    assert!(is_alive(&mut stock) && is_alive(&mut slot1), "slot 2's cleanup touches neither");
    ofs_fc::sitl::process::run_cleanup(&ofs_fc::sitl::net::default_cleanup(&launch_for("0")));
    ofs_fc::sitl::process::await_cleanup(&launch_for("0"));
    assert!(!is_alive(&mut stock), "slot 0's cleanup took down the SITL started without the option");
    assert!(is_alive(&mut slot1), "slot 1's SITL survives slot 0's cleanup");
}

/// A SITL binary in a directory with spaces in its path (`C:\Program Files`, a home folder named "My Projects" under
/// WSL) is still found by its slot: the match is on the process name and the `--port-offset` argument, not on the
/// shape of the whole command line.
#[cfg(unix)]
#[test]
fn a_slots_sitl_is_found_when_its_path_has_spaces() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 10);
    std::thread::sleep(Duration::from_millis(100)); // let both exec
    let launch: Vec<String> = [dir.path().join("betaflight_SITL").display().to_string(), "--port-offset".into(), "10".into()].to_vec();
    ofs_fc::sitl::process::run_cleanup(&ofs_fc::sitl::net::default_cleanup(&launch));
    ofs_fc::sitl::process::await_cleanup(&launch);
    assert!(!is_alive(&mut slot1), "slot 1's SITL, whose path has spaces, is gone");
    assert!(is_alive(&mut slot0), "slot 0's SITL survives");
}

/// macOS has no `/proc` (and names a process by its whole executable name): the slot script finds its victims with `ps`
/// there. Forced on Linux with the script's second argument, the same decoys give the same answers as through `/proc`:
/// a slot's cleanup takes only that slot's SITL, a SITL without the option is slot 0's, a path with spaces is no trouble.
#[cfg(target_os = "linux")]
#[test]
fn the_slot_script_through_ps_alone_takes_the_same_victims() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let mut stock = spawn_fake_sitl_with(dir.path(), "sleep 30; sleep 30", &[]);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 10);
    std::thread::sleep(Duration::from_millis(100)); // let both exec
    let run = |offset: &str| {
        std::process::Command::new("sh")
            .args(["-c", ofs_fc::sitl::net::SLOT_KILL_SCRIPT, "sh", offset, "ps"])
            .status()
            .unwrap();
        std::thread::sleep(Duration::from_millis(150)); // let the TERM land
    };
    run("20");
    assert!(is_alive(&mut stock) && is_alive(&mut slot1), "slot 2's cleanup touches neither");
    run("10");
    assert!(!is_alive(&mut slot1), "slot 1's SITL, whose path has spaces, is gone");
    assert!(is_alive(&mut stock), "the SITL without the option is slot 0's, not slot 1's");
    run("0");
    assert!(!is_alive(&mut stock), "slot 0's cleanup took the SITL without the option");
}

/// The unscoped cleanup (a launch without `--port-offset`) is `pkill -x betaflight_SITL` on Linux. BSD's `pkill -x` matches
/// the kernel's 16-character process name, which for `betaflight_SITL.elf` is `betaflight_SITL.`, so on macOS it
/// finds nothing: there the cleanup is a script over the same `ps` search as the slot script, shown here on Linux with
/// its `ps` argument. It takes every SITL of the machine, in any slot, and nothing else.
#[cfg(target_os = "linux")]
#[test]
fn the_unscoped_script_through_ps_kills_every_sitl_and_only_those() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut stock = spawn_fake_sitl_with(dir.path(), "sleep 30; sleep 30", &[]);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), "sleep 30; sleep 30", 10);
    let bystander = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let mut bystander = FakeSitl(bystander);
    std::thread::sleep(Duration::from_millis(100)); // let them exec
    std::process::Command::new("sh").args(["-c", ofs_fc::sitl::net::SITL_KILL_ALL_SCRIPT, "sh", "ps"]).status().unwrap();
    std::thread::sleep(Duration::from_millis(150)); // let the TERM land
    assert!(!is_alive(&mut stock) && !is_alive(&mut slot1), "every SITL is gone, whatever its slot");
    assert!(is_alive(&mut bystander), "a process that is not a SITL is left alone");
}

/// The escalation to SIGKILL is scoped too: slot 1's cleanup SIGKILLs slot 1's TERM-ignoring SITL, not slot 0's.
#[cfg(unix)]
#[test]
fn the_sigkill_escalation_only_reaches_its_own_slot() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let trap = |ready: &std::path::Path| format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display());
    let (ready0, ready1) = (dir.path().join("ready0"), dir.path().join("ready1"));
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready0), 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready1), 10);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !(ready0.exists() && ready1.exists()) {
        assert!(std::time::Instant::now() < deadline, "the fake SITLs never installed their traps");
        std::thread::sleep(Duration::from_millis(5));
    }
    let launch: Vec<String> = [dir.path().join("betaflight_SITL").display().to_string(), "--port-offset".into(), "10".into()].to_vec();
    ofs_fc::sitl::process::run_cleanup(&ofs_fc::sitl::net::default_cleanup(&launch));
    std::thread::sleep(Duration::from_millis(50)); // let the TERM land (and be ignored)
    let outcome = ofs_fc::sitl::process::await_cleanup(&launch);
    assert!(outcome.escalated && outcome.victims_seen == 1, "{outcome:?}");
    assert!(!is_alive(&mut slot1), "slot 1's TERM-ignoring SITL was SIGKILLed");
    assert!(is_alive(&mut slot0), "slot 0's TERM-ignoring SITL was left alone");
}

/// The script WSL runs inside the VM (`sh -c SCRIPT sh s OFFSET`) is scoped like the native wait: run here on Linux
/// with the same arguments, it SIGKILLs slot 1's TERM-ignoring SITL and reports it, and leaves slot 0's alone.
#[cfg(target_os = "linux")]
#[test]
fn the_wsl_wait_script_is_scoped_to_its_slot_too() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let trap = |ready: &std::path::Path| format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display());
    let (ready0, ready1) = (dir.path().join("ready0"), dir.path().join("ready1"));
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready0), 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready1), 10);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !(ready0.exists() && ready1.exists()) {
        assert!(std::time::Instant::now() < deadline, "the fake SITLs never installed their traps");
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::WSL_WAIT_SCRIPT, "sh", "s", "10"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("seen=1 escalated=1"), "slot 1's victim was seen and SIGKILLed: {text:?}");
    assert!(!is_alive(&mut slot1), "slot 1's SITL is gone");
    assert!(is_alive(&mut slot0), "slot 0's SITL is left alone");
    let none = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::WSL_WAIT_SCRIPT, "sh", "s", "20"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&none.stdout).contains("seen=0"), "no slot-2 victims: {:?}", none.stdout);
}

/// The wait script macOS runs (there is no `/proc`): the same decoys, forced through the `ps` path on Linux.
/// It sees a TERM-ignoring slot-1 SITL, escalates to SIGKILL, leaves slot 0 alone, and reports what it did.
#[cfg(target_os = "linux")]
#[test]
fn the_macos_wait_script_escalates_and_is_scoped() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let trap = |ready: &std::path::Path| format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display());
    let (ready0, ready1) = (dir.path().join("ready0"), dir.path().join("ready1"));
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready0), 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready1), 10);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !(ready0.exists() && ready1.exists()) {
        assert!(std::time::Instant::now() < deadline, "the fake SITLs never installed their traps");
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::UNIX_WAIT_SCRIPT, "sh", "s", "10", "ps"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("seen=1 escalated=1"), "slot 1's victim was seen and SIGKILLed: {text:?}");
    assert!(!is_alive(&mut slot1), "slot 1's SITL is gone");
    assert!(is_alive(&mut slot0), "slot 0's SITL is left alone");
    let none = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::UNIX_WAIT_SCRIPT, "sh", "s", "20", "ps"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&none.stdout).contains("seen=0"), "no slot-2 victims: {:?}", none.stdout);
}

/// A victim that ignores SIGTERM (a SITL in a slow teardown) must not survive the cleanup: the wait escalates to
/// SIGKILL and returns only when it is gone (or a zombie, which holds no ports).
#[cfg(target_os = "linux")]
#[test]
fn a_victim_that_ignores_sigterm_is_sigkilled() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    let victim = spawn_fake_sitl(dir.path(), &format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display()));
    let pid = victim.id();
    // TERM before the trap is installed kills the shell outright (the race that made this test fail 2 runs in 5).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline, "the victim never installed its trap");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::process::Command::new("kill").arg(pid.to_string()).status().unwrap();
    std::thread::sleep(Duration::from_millis(50)); // let the signal land
    let outcome = ofs_fc::sitl::process::await_cleanup(&[dir.path().join("betaflight_SITL").display().to_string()]);
    assert!(outcome.escalated, "a TERM-ignoring victim must be escalated: {outcome:?}");
    assert_eq!(outcome.victims_seen, 1, "{outcome:?}");
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    assert!(
        state.is_empty() || state.rsplit_once(')').is_some_and(|(_, s)| s.trim_start().starts_with('Z')),
        "the victim ignored SIGTERM and was not escalated: {state}"
    );
}

/// The unscoped macOS path has no `--port-offset`: the wait must see every SITL through `ps`, SIGKILL a
/// TERM-ignoring one, and report it. (`a_victim_that_ignores_sigterm_is_sigkilled` keeps the `/proc` assertion.)
#[cfg(target_os = "macos")]
#[test]
fn the_unscoped_wait_sigkills_a_term_ignoring_victim_on_macos() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let ready = dir.path().join("ready");
    let mut victim = spawn_fake_sitl(
        dir.path(),
        &format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display()),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline, "the victim never installed its trap");
        std::thread::sleep(Duration::from_millis(5));
    }
    let outcome = ofs_fc::sitl::process::await_cleanup(&[dir.path().join("betaflight_SITL").display().to_string()]);
    assert!(outcome.escalated && outcome.victims_seen == 1, "{outcome:?}");
    assert!(victim.0.try_wait().unwrap().is_some(), "the SIGKILLed victim is gone");
}

/// The common path — the victim is already dying or dead — must fall through fast, not cost every launch.
#[cfg(unix)]
#[test]
fn the_cleanup_does_not_hang_when_nobody_survives() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut victim = spawn_fake_sitl(dir.path(), "sleep 30");
    let _ = victim.0.kill();
    victim.wait().unwrap(); // dead and reaped BEFORE the measurement: the drop guard alone would measure the wait
    let started = std::time::Instant::now();
    let outcome = ofs_fc::sitl::process::await_cleanup(&[dir.path().join("betaflight_SITL").display().to_string()]);
    assert!(
        started.elapsed() < Duration::from_millis(600),
        "no survivors: the wait must fall through fast, took {:?}",
        started.elapsed()
    );
    assert_eq!(outcome, ofs_fc::sitl::process::AwaitCleanup::NONE, "the victim was already reaped: {outcome:?}");
}

/// The SITL patch retries a busy port for about half a second, printing `retry n/m` lines, and a plain success line
/// when one lands (the port-bind flake's dying holder). A session must start then: a retry line is not a verdict.
#[test]
fn a_bind_that_succeeds_after_retries_starts_the_session() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let script = "echo 'bind port 5761 for UART1 retry 1/20 (errno 98)' >&2; \
                  echo 'bind port 5761 for UART1 retry 2/20 (errno 98)' >&2; \
                  echo 'bind port 5761 for UART1' >&2; \
                  echo '[SITL] ready for the simulator'; sleep 5";
    let cfg = config(dir.path(), &["sh", "-c", script]);
    std::fs::create_dir_all(&cfg.launch.workdir).unwrap();
    std::fs::write(cfg.launch.workdir.join("eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::copy(&cfg.launch.diff_file, cfg.launch.workdir.join("betaflight.diff")).unwrap();
    let started = SitlProcess::start(&cfg.launch);
    assert!(
        started.is_ok(),
        "a bind that landed on a retry starts the session: {:?}",
        started.err().map(|e| e.to_string())
    );
}

/// A permanent bind failure (the 20 retries are spent) must fail the start and keep the errno on the record.
/// This is the parser/error-mapping half of the live test, so it runs on macOS where SITL's own BSD bind
/// check cannot refuse a held port.
#[test]
fn a_sitl_that_reports_a_spent_bind_fails_with_the_errno() {
    use ofs_fc::sitl::process::SitlProcess;
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let script = "i=1; while [ $i -le 20 ]; do \
                  echo \"bind port 5761 for UART1 retry $i/20 (errno 48)\" >&2; \
                  i=$((i+1)); done; \
                  echo 'bind port 5761 for UART1 failed!! after 20 attempts (errno 48)' >&2; \
                  sleep 5";
    let cfg = config(dir.path(), &["sh", "-c", script]);
    std::fs::create_dir_all(&cfg.launch.workdir).unwrap();
    std::fs::write(cfg.launch.workdir.join("eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::copy(
        &cfg.launch.diff_file,
        cfg.launch.workdir.join("betaflight.diff"),
    )
    .unwrap();
    let err = SitlProcess::start(&cfg.launch)
        .err()
        .expect("a spent bind fails the start");
    let text = err.to_string();
    assert!(
        text.contains("bind port 5761 for UART1 failed!! after 20 attempts"),
        "the spent retries are on the record: {text}"
    );
    assert!(text.contains("errno 48"), "the failure names the errno: {text}");
}
