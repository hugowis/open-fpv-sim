//! Where SITL lives on the network: natively on loopback, or inside WSL2 behind NAT (Windows).
//! See docs/research/sitl-interface.md §6.
use std::io::Read;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Cleanup argv that removes stray SITL processes (e.g. left behind by a simulator that was killed).
/// Matches the process *name* (`pkill -x`; Linux truncates `betaflight_SITL.elf` to `betaflight_SITL`):
/// `pkill -f` would also match (and kill) any shell whose command line contains the binary path.
/// Under WSL it runs inside the VM; natively it needs a unix host, so plain Windows gets none.
pub fn default_cleanup(launch: &[String]) -> Vec<String> {
    let pkill = ["pkill", "-x", "betaflight_SITL"].map(String::from);
    match wsl_prefix(launch) {
        Some(mut prefix) => {
            prefix.extend(pkill);
            prefix
        }
        None if cfg!(unix) => pkill.to_vec(),
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

/// How long a `wsl.exe` query may take before the vehicle build gives up (a hung WSL would stall it forever).
pub const WSL_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs `prefix args` and returns its stdout, killing it if it has not finished within `timeout`.
fn run_with_timeout(prefix: &[String], args: &[&str], timeout: Duration) -> Result<String, FcError> {
    let (program, rest) = prefix.split_first().expect("wsl prefix is never empty");
    let command = format!("{} {}", prefix.join(" "), args.join(" "));
    let mut child = Command::new(program)
        .args(rest)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| FcError::Launch { command: command.clone(), source })?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + timeout;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FcError::Config(format!("`{}` did not finish within {} ms", command.trim_end(), timeout.as_millis())));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = reader.join().unwrap_or_default();
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn run(prefix: &[String], args: &[&str]) -> Result<String, FcError> {
    run_with_timeout(prefix, args, WSL_QUERY_TIMEOUT)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sleeper() -> Vec<String> {
        if cfg!(windows) {
            ["ping", "-n", "6", "127.0.0.1"].map(String::from).to_vec()
        } else {
            ["sleep", "5"].map(String::from).to_vec()
        }
    }

    #[test]
    fn a_hung_wsl_command_times_out() {
        let started = std::time::Instant::now();
        let err = run_with_timeout(&sleeper(), &[], std::time::Duration::from_millis(300)).unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "waited {:?}", started.elapsed());
        assert!(err.to_string().contains("did not finish within 300 ms"), "{err}");
    }

    #[test]
    fn a_quick_command_returns_its_output() {
        let echo: Vec<String> = if cfg!(windows) {
            ["cmd", "/C", "echo", "172.28.1.2"].map(String::from).to_vec()
        } else {
            ["echo", "172.28.1.2"].map(String::from).to_vec()
        };
        let out = run_with_timeout(&echo, &[], std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(parse_hostname_ips(&out), Some("172.28.1.2".parse().unwrap()));
    }
}
