use std::net::Ipv4Addr;

use ofs_fc::sitl::net::{default_cleanup, parse_default_gateway, parse_hostname_ips, resolve, wsl_prefix, SitlNet};

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn wsl_launches_are_detected_with_their_exec_prefix() {
    assert_eq!(wsl_prefix(&argv(&["wsl.exe", "-d", "Ubuntu", "-e", "/home/u/sitl.elf"])), Some(argv(&["wsl.exe", "-d", "Ubuntu", "-e"])));
    assert_eq!(wsl_prefix(&argv(&["C:\\Windows\\System32\\WSL.EXE", "--exec", "/x"])), Some(argv(&["C:\\Windows\\System32\\WSL.EXE", "--exec"])));
    assert_eq!(wsl_prefix(&argv(&["wsl.exe", "/x"])), Some(argv(&["wsl.exe", "-e"])));
    assert_eq!(wsl_prefix(&argv(&["/home/u/betaflight_SITL.elf"])), None);
    assert_eq!(wsl_prefix(&[]), None);
}

#[test]
fn wsl_cleanup_matches_the_process_name_not_the_command_line() {
    assert_eq!(default_cleanup(&argv(&["wsl.exe", "-d", "Ubuntu", "-e", "/x"])), argv(&["wsl.exe", "-d", "Ubuntu", "-e", "pkill", "-x", "betaflight_SITL"]));
}

#[test]
fn native_cleanup_is_pkill_on_unix_and_empty_on_plain_windows() {
    let native = default_cleanup(&argv(&["/x/betaflight_SITL.elf"]));
    if cfg!(unix) {
        assert_eq!(native, argv(&["pkill", "-x", "betaflight_SITL"]));
    } else {
        assert!(native.is_empty(), "{native:?}");
    }
}

#[test]
fn wsl_addresses_are_parsed() {
    assert_eq!(parse_hostname_ips("172.28.26.115 10.255.255.254 \n"), Some(Ipv4Addr::new(172, 28, 26, 115)));
    assert_eq!(parse_hostname_ips(""), None);
    assert_eq!(parse_default_gateway("default via 172.28.16.1 dev eth0 proto kernel\n"), Some(Ipv4Addr::new(172, 28, 16, 1)));
    assert_eq!(parse_default_gateway("10.0.0.0/8 dev eth0"), None);
}

#[test]
fn native_launch_uses_loopback_and_overrides_win() {
    let native = argv(&["/home/u/betaflight_SITL.elf"]);
    assert_eq!(resolve(&native, None, None).unwrap(), SitlNet::loopback());
    let net = resolve(&native, Some(Ipv4Addr::new(10, 0, 0, 2)), Some(Ipv4Addr::new(10, 0, 0, 1))).unwrap();
    assert_eq!(net, SitlNet { send_ip: Ipv4Addr::new(10, 0, 0, 2), bind_ip: Ipv4Addr::new(10, 0, 0, 1), sitl_ip_arg: Some(Ipv4Addr::new(10, 0, 0, 1)) });
}
