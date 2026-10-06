use std::net::SocketAddr;
use std::process::Command;

use ofs_sim::listen::check_listen;

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn loopback_addresses_are_always_allowed() {
    for a in ["127.0.0.1:50051", "127.0.0.2:1", "[::1]:50051"] {
        assert!(check_listen(addr(a), false).is_ok(), "{a}");
    }
}

#[test]
fn other_addresses_need_the_explicit_flag() {
    for a in ["0.0.0.0:50051", "192.168.1.20:50051", "[::]:50051", "10.0.0.1:1"] {
        let message = check_listen(addr(a), false).unwrap_err();
        assert!(message.contains("--allow-remote") && message.contains(a), "{message}");
        assert!(check_listen(addr(a), true).is_ok(), "{a} with the flag");
    }
}

#[test]
fn the_binary_refuses_to_start_on_a_remote_address_without_the_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_ofs-sim")).args(["--listen", "0.0.0.0:0"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing to listen on 0.0.0.0:0") && stderr.contains("--allow-remote"), "{stderr}");
}