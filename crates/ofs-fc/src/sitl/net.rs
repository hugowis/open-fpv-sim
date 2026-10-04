//! Where SITL lives on the network: natively on loopback, or inside WSL2 behind NAT (Windows).
//! See docs/research/sitl-interface.md §6.
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};

use super::FcError;

/// Addresses for the lockstep exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SitlNet {
    /// Where state datagrams are sent (SITL's UDP 9003).
    pub send_ip: Ipv4Addr,
    /// Local address the motor socket binds (SITL sends motor packets to it, UDP 9002).
    pub bind_ip: Ipv4Addr,
    /// Passed to SITL as `--ip` when it must reply somewhere other than its own loopback.
    pub sitl_ip_arg: Option<Ipv4Addr>,
}

impl SitlNet {
    pub fn loopback() -> Self {
        Self { send_ip: Ipv4Addr::LOCALHOST, bind_ip: Ipv4Addr::LOCALHOST, sitl_ip_arg: None }
    }
}

/// The `wsl.exe … -e` prefix of a launch argv, if SITL is launched through WSL.
pub fn wsl_prefix(launch: &[String]) -> Option<Vec<String>> {
    let first = launch.first()?;
    let exe = first.rsplit(['/', '\\']).next().unwrap_or(first).to_ascii_lowercase();
    if exe != "wsl.exe" && exe != "wsl" {
        return None;
    }
    match launch.iter().position(|a| a == "-e" || a == "--exec") {
        Some(i) => Some(launch[..=i].to_vec()),
        None => Some(vec![first.clone(), "-e".to_string()]),
    }
}

/// Cleanup argv that removes stray SITL processes. Matches the process *name* (`pkill -x`): `pkill -f`
/// would also match (and kill) any shell whose command line contains the binary path.
pub fn default_cleanup(launch: &[String]) -> Vec<String> {
    match wsl_prefix(launch) {
        Some(mut prefix) => {
            prefix.extend(["pkill", "-x", "betaflight_SITL"].map(String::from));
            prefix
        }
        None => Vec::new(),
    }
}

/// First IPv4 address of `hostname -I` output.
pub fn parse_hostname_ips(out: &str) -> Option<Ipv4Addr> {
    out.split_whitespace().find_map(|w| w.parse().ok())
}

/// Gateway of `ip route show default` output (`default via 172.28.16.1 dev eth0 …`).
pub fn parse_default_gateway(out: &str) -> Option<Ipv4Addr> {
    let mut words = out.split_whitespace();
    while let Some(w) = words.next() {
        if w == "via" {
            return words.next()?.parse().ok();
        }
    }
    None
}

fn run(prefix: &[String], args: &[&str]) -> Result<String, FcError> {
    let (program, rest) = prefix.split_first().expect("wsl prefix is never empty");
    let out = Command::new(program)
        .args(rest)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| FcError::Launch { command: format!("{} {}", prefix.join(" "), args.join(" ")), source })?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Addresses for a launch argv. Native: loopback. WSL2 (NAT): send to the VM IP, bind and `--ip` the
/// Windows host IP as seen from WSL. Overrides: `send_override` (OFS_SITL_HOST), `reply_override`
/// (OFS_SITL_REPLY_IP; also passed as `--ip`).
pub fn resolve(launch: &[String], send_override: Option<Ipv4Addr>, reply_override: Option<Ipv4Addr>) -> Result<SitlNet, FcError> {
    let mut net = match wsl_prefix(launch) {
        None => SitlNet::loopback(),
        Some(prefix) => {
            let vm = parse_hostname_ips(&run(&prefix, &["hostname", "-I"])?)
                .ok_or_else(|| FcError::Config("could not read the WSL VM IP (`hostname -I`)".into()))?;
            let host = parse_default_gateway(&run(&prefix, &["ip", "route", "show", "default"])?)
                .ok_or_else(|| FcError::Config("could not read the Windows host IP from WSL (`ip route show default`)".into()))?;
            SitlNet { send_ip: vm, bind_ip: host, sitl_ip_arg: Some(host) }
        }
    };
    if let Some(ip) = send_override {
        net.send_ip = ip;
    }
    if let Some(ip) = reply_override {
        net.bind_ip = ip;
        net.sitl_ip_arg = Some(ip);
    }
    Ok(net)
}
