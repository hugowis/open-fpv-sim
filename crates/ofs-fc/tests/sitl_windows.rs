//! The stray-SITL cleanup on a plain Windows host (no WSL): a slot's cleanup takes down that slot's `betaflight_SITL.exe`
//! and nobody else's. The same rule as the Linux decoy tests of `sitl_errors.rs`, with a copy of `cmd.exe` under the SITL
//! name as the decoy (its command line carries `--port-offset N`, which is all the cleanup reads).
#![cfg(windows)]

use std::path::Path;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::Duration;

use ofs_fc::sitl::net::default_cleanup;
use ofs_fc::sitl::process::{await_cleanup, run_cleanup};

/// The decoys are named processes of the whole machine: tests that start and clean them take turns.
static DECOYS: Mutex<()> = Mutex::new(());

/// `<dir>\betaflight_SITL.exe /c ping ... & rem <extra...>`: sleeps about 30 s; the copy of cmd.exe is made once per
/// directory (a copy over a running executable is refused).
fn spawn_decoy(dir: &Path, extra: &[&str]) -> Child {
    let fake = dir.join("betaflight_SITL.exe");
    if !fake.exists() {
        let cmd = std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        std::fs::copy(cmd, &fake).unwrap();
    }
    Command::new(&fake).args(["/c", "ping -n 30 127.0.0.1 > nul & rem decoy"]).args(extra).spawn().unwrap()
}

fn alive(child: &mut Child) -> bool {
    child.try_wait().unwrap().is_none()
}

fn launch_for(dir: &Path, offset: &str) -> Vec<String> {
    vec![dir.join("betaflight_SITL.exe").display().to_string(), "--port-offset".into(), offset.into()]
}

fn clean(launch: &[String]) {
    run_cleanup(&default_cleanup(launch));
    await_cleanup(launch);
}

fn kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// A slot's cleanup takes down that slot's SITL and leaves every other slot's running.
#[test]
fn a_cleanup_for_one_slot_leaves_another_slots_sitl_alive_on_windows() {
    let _turn = DECOYS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut slot0 = spawn_decoy(dir.path(), &["--port-offset", "0"]);
    let mut slot1 = spawn_decoy(dir.path(), &["--port-offset", "10"]);
    std::thread::sleep(Duration::from_millis(300)); // let both start
    clean(&launch_for(dir.path(), "10"));
    assert!(!alive(&mut slot1), "slot 1's SITL is gone");
    assert!(alive(&mut slot0), "slot 0's SITL survives slot 1's cleanup");
    kill(slot0);
}

/// A SITL started without the option holds the stock ports, so it is slot 0's.
#[test]
fn a_sitl_without_the_option_belongs_to_slot_zero_on_windows() {
    let _turn = DECOYS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let mut stock = spawn_decoy(dir.path(), &[]);
    let mut slot1 = spawn_decoy(dir.path(), &["--port-offset", "10"]);
    std::thread::sleep(Duration::from_millis(300));
    clean(&launch_for(dir.path(), "20"));
    assert!(alive(&mut stock) && alive(&mut slot1), "slot 2's cleanup touches neither");
    clean(&launch_for(dir.path(), "0"));
    assert!(!alive(&mut stock), "slot 0's cleanup took down the SITL started without the option");
    assert!(alive(&mut slot1), "slot 1's SITL survives slot 0's cleanup");
    kill(slot1);
}

/// A binary in a directory with spaces in its path is found by its slot all the same.
#[test]
fn a_slots_sitl_is_found_when_its_path_has_spaces_on_windows() {
    let _turn = DECOYS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let mut slot0 = spawn_decoy(dir.path(), &["--port-offset", "0"]);
    let mut slot1 = spawn_decoy(dir.path(), &["--port-offset", "10"]);
    std::thread::sleep(Duration::from_millis(300));
    clean(&launch_for(dir.path(), "10"));
    assert!(!alive(&mut slot1), "slot 1's SITL, whose path has spaces, is gone");
    assert!(alive(&mut slot0), "slot 0's SITL survives");
    kill(slot0);
}
