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

/// The process *name* every SITL instance runs under, as `pkill -x` matches it (Linux truncates
/// `betaflight_SITL.elf` to 15 characters): the cleanup kills it, and the pre-launch wait looks for it.
pub(crate) const SITL_PROCESS_NAME: &str = "betaflight_SITL";

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

/// The `--port-offset` a launch argv gives SITL (its slot, see [`super::slot`]; the last one wins, as SITL's own
/// parsing does): `None` for an unscoped launch (an argv without the option, such as a hand-made `OFS_SITL_LAUNCH` in
/// a test), whose cleanup is by process name.
pub fn port_offset_in(launch: &[String]) -> Option<u16> {
    let at = launch.iter().rposition(|a| a == "--port-offset")?;
    launch.get(at + 1)?.parse().ok()
}

/// `launch` with `--port-offset <offset>` as its slot's: an option the user already wrote into `OFS_SITL_LAUNCH` or
/// `fc.launch` is dropped first, so the ports SITL binds, the lock the session holds and the processes its cleanup
/// targets are always those of the one slot.
pub fn with_port_offset(launch: &[String], offset: u16) -> Vec<String> {
    let mut out = Vec::with_capacity(launch.len() + 2);
    let mut args = launch.iter();
    while let Some(arg) = args.next() {
        if arg == "--port-offset" {
            args.next(); // its value
        } else {
            out.push(arg.clone());
        }
    }
    out.extend(["--port-offset".to_string(), offset.to_string()]);
    out
}

/// Shell text defining `slot_pids <offset>`: the pids of the live SITL processes of one slot, one per line. It
/// selects by process *name* (`pgrep -x`, which no shell or `pkill` mentioning the binary's path can match) and then
/// reads each command line (`/proc/<pid>/cmdline`, so a path with spaces is no trouble): the slot is the process's
/// `--port-offset`, and a process launched without the option is slot 0, the stock ports. Zombies hold no ports and
/// are skipped. Compiled into [`SLOT_KILL_SCRIPT`] and the WSL wait script; the offset is always an argument of the
/// script, never part of its text. [`is_victim`] is the same rule for the native Rust scan.
macro_rules! slot_pids_sh {
    () => {
        "\
slot_pids() {
  for p in $(pgrep -x betaflight_SITL); do
    a=$(tr '\\0' ' ' < /proc/$p/cmdline 2>/dev/null) || continue
    [ -n \"$a\" ] || continue
    st=$(sed 's/^.*) //' /proc/$p/stat 2>/dev/null); st=${st%% *}
    [ \"$st\" = Z ] && continue
    case \" $a\" in
      *\" --port-offset $1 \"*) echo \"$p\" ;;
      *\" --port-offset \"*) ;;
      *) [ \"$1\" = 0 ] && echo \"$p\" ;;
    esac
  done
  return 0
}
"
    };
}
pub(crate) use slot_pids_sh;

/// [`slot_pids_sh!`]'s text as a constant, for the tests that select processes by the same rule the scripts do.
pub const SLOT_PIDS_SH: &str = slot_pids_sh!();

/// The cleanup script for one slot (`sh -c SLOT_KILL_SCRIPT sh <offset>`): SIGTERM to the slot's SITL processes.
pub const SLOT_KILL_SCRIPT: &str = concat!(slot_pids_sh!(), "pids=$(slot_pids \"$1\"); [ -z \"$pids\" ] || kill $pids 2>/dev/null\nexit 0");

/// The cleanup for one slot on a plain Windows host (`powershell.exe ... -Command SCRIPT <offset>`: PowerShell makes
/// the offset the scriptblock's argument): terminates the `betaflight_SITL.exe` processes whose command line's last
/// `--port-offset` is the slot's, or that have none for slot 0 (the rule of [`is_victim`]), and waits up to 2 s for them
/// to be gone, so the SITL launched next finds its ports free. Written with single quotes only (it travels as one
/// command-line argument).
pub const WINDOWS_SLOT_KILL_SCRIPT: &str = "& { param($offset) \
if (-not (Get-Process -Name betaflight_SITL -ErrorAction SilentlyContinue)) { return }; \
Get-CimInstance Win32_Process -Filter 'Name=''betaflight_SITL.exe''' -OperationTimeoutSec 5 | ForEach-Object { \
if ($null -eq $_.CommandLine) { return }; \
$m = [regex]::Matches($_.CommandLine, '\\s--port-offset\\s+(\\d+)'); \
$o = 0; if ($m.Count -gt 0) { $o = [int]$m[$m.Count - 1].Groups[1].Value }; \
if ($o -eq [int]$offset) { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue; $_.ProcessId } \
} | ForEach-Object { Wait-Process -Id $_ -Timeout 2 -ErrorAction SilentlyContinue } }";

/// Whether the command line `args` (a process named `betaflight_SITL`, split at its NULs) is a SITL of slot `offset`:
/// its last `--port-offset` is `offset`, or it has none and the slot is 0 (the stock ports). The same rule as
/// [`slot_pids_sh`], for the native scan of `/proc`.
pub fn is_victim(args: &[&str], offset: u16) -> bool {
    if args.is_empty() {
        return false; // between fork and exec: no command line yet
    }
    match args.iter().rposition(|a| *a == "--port-offset") {
        Some(at) => args.get(at + 1).and_then(|v| v.parse::<u16>().ok()) == Some(offset),
        None => offset == 0,
    }
}

/// Cleanup argv that removes stray SITL processes (e.g. left behind by a simulator that was killed).
/// A launch with a `--port-offset` (a slot) removes only that slot's processes ([`SLOT_KILL_SCRIPT`]); that is what
/// lets several simulators share a machine. An unscoped launch matches the process *name* (`pkill -x`; Linux
/// truncates `betaflight_SITL.elf` to `betaflight_SITL`), every SITL on the machine.
/// Under WSL it runs inside the VM; natively on unix it is the same rule in `sh`; on a plain Windows host it is a
/// PowerShell command ([`WINDOWS_SLOT_KILL_SCRIPT`]) that also waits for its victims, which a scoped launch needs.
pub fn default_cleanup(launch: &[String]) -> Vec<String> {
    let kill = match port_offset_in(launch) {
        Some(offset) => ["sh", "-c", SLOT_KILL_SCRIPT, "sh"].into_iter().map(String::from).chain([offset.to_string()]).collect(),
        None => ["pkill", "-x", SITL_PROCESS_NAME].map(String::from).to_vec(),
    };
    match wsl_prefix(launch) {
        Some(mut prefix) => {
            prefix.extend(kill);
            prefix
        }
        None if cfg!(unix) => kill,
        None => windows_cleanup(port_offset_in(launch)),
    }
}

/// [`default_cleanup`] on a plain Windows host: a slot's processes by [`WINDOWS_SLOT_KILL_SCRIPT`], or every
/// `betaflight_SITL.exe` by name for an unscoped launch. Empty off Windows (a Linux launch never gets here).
fn windows_cleanup(scope: Option<u16>) -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    match scope {
        Some(offset) => ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", WINDOWS_SLOT_KILL_SCRIPT]
            .into_iter()
            .map(String::from)
            .chain([offset.to_string()])
            .collect(),
        None => ["taskkill", "/F", "/IM", "betaflight_SITL.exe"].map(String::from).to_vec(),
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
