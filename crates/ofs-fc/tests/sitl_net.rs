use std::net::Ipv4Addr;

use ofs_fc::sitl::net::{
    default_cleanup, is_victim, parse_default_gateway, parse_hostname_ips, port_offset_in, resolve, with_port_offset, wsl_prefix, SitlNet,
};

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
fn the_port_offset_is_read_from_the_launch_argv() {
    assert_eq!(port_offset_in(&argv(&["/x/betaflight_SITL.elf", "--ip", "1.2.3.4", "--port-offset", "10"])), Some(10));
    assert_eq!(port_offset_in(&argv(&["wsl.exe", "-e", "/x/b", "--port-offset", "0"])), Some(0));
    assert_eq!(port_offset_in(&argv(&["/x/betaflight_SITL.elf", "--ip", "1.2.3.4"])), None, "an unscoped launch");
    assert_eq!(port_offset_in(&argv(&["/x/b", "--port-offset"])), None, "no value");
    assert_eq!(port_offset_in(&argv(&["/x/b", "--port-offset", "ten"])), None, "not a number");
}

#[test]
fn a_user_supplied_port_offset_is_replaced_by_the_slots() {
    let sitl = "/x/betaflight_SITL.elf";
    assert_eq!(with_port_offset(&argv(&[sitl, "--ip", "1.2.3.4"]), 10), argv(&[sitl, "--ip", "1.2.3.4", "--port-offset", "10"]));
    assert_eq!(
        with_port_offset(&argv(&["wsl.exe", "-e", sitl, "--port-offset", "3", "--ip", "1.2.3.4"]), 10),
        argv(&["wsl.exe", "-e", sitl, "--ip", "1.2.3.4", "--port-offset", "10"]),
        "the user's option is dropped, so SITL, the lock and the cleanup all mean slot 1"
    );
    assert_eq!(with_port_offset(&argv(&[sitl, "--port-offset"]), 0), argv(&[sitl, "--port-offset", "0"]), "a dangling option too");
    let twice = with_port_offset(&argv(&[sitl, "--port-offset", "1", "--port-offset", "2"]), 30);
    assert_eq!(port_offset_in(&twice), Some(30));
    assert_eq!(twice.iter().filter(|a| *a == "--port-offset").count(), 1, "{twice:?}");
}

#[test]
fn a_slots_cleanup_runs_the_slot_script_with_its_offset_as_an_argument() {
    let script = ofs_fc::sitl::net::SLOT_KILL_SCRIPT;
    assert!(!script.contains("10"), "the offset is an argument, never part of the script text");
    let native = default_cleanup(&argv(&["/x/betaflight_SITL.elf", "--ip", "1.2.3.4", "--port-offset", "10"]));
    let wsl = default_cleanup(&argv(&["wsl.exe", "-d", "Ubuntu", "-e", "/x/betaflight_SITL.elf", "--port-offset", "10"]));
    assert_eq!(wsl, argv(&["wsl.exe", "-d", "Ubuntu", "-e", "sh", "-c", script, "sh", "10"]));
    if cfg!(unix) {
        assert_eq!(native, argv(&["sh", "-c", script, "sh", "10"]));
    } else {
        assert!(native.is_empty(), "{native:?}");
    }
}

#[test]
fn the_native_matcher_decides_a_slot_by_its_last_option_and_no_option_is_slot_zero() {
    let sitl = "/home/u/ofs/betaflight/obj/main/betaflight_SITL.elf";
    let yes = |args: &[&str], offset| assert!(is_victim(args, offset), "{args:?} should be a victim of slot {offset}");
    let no = |args: &[&str], offset| assert!(!is_victim(args, offset), "{args:?} must not be a victim of slot {offset}");
    yes(&[sitl, "--ip", "1.2.3.4", "--port-offset", "10"], 10);
    yes(&[sitl, "--port-offset", "10", "--config", "betaflight.diff"], 10);
    yes(&[sitl, "--port-offset", "0"], 0);
    yes(&["/home/u/My Projects/betaflight_SITL.elf", "--port-offset", "10"], 10);
    yes(&[sitl, "--ip", "1.2.3.4"], 0);
    yes(&[sitl], 0);
    no(&[sitl, "--port-offset", "100"], 10);
    no(&[sitl, "--port-offset", "1"], 10);
    no(&[sitl, "--port-offset", "10"], 0);
    no(&[sitl, "--ip", "1.2.3.4"], 10);
    no(&[sitl, "--port-offset", "ten"], 0);
    no(&[], 0);
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
