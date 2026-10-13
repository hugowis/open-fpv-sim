//! Launches and supervises the Betaflight SITL process; applies the CLI diff on first boot.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::FcError;

const TAIL_LINES: usize = 200;
/// How long a missed reply waits for a Betaflight reboot to show itself (see [`SitlProcess::rebooted`]).
pub const REBOOT_GRACE: Duration = Duration::from_secs(3);
/// How long after exiting a SITL's last output lines may still be on their way through the pipes.
const EXIT_FLUSH: Duration = Duration::from_millis(200);
/// How long after the ready line a late `bind port ... failed` is still waited for.
const BIND_GRACE: Duration = Duration::from_millis(300);
/// How long after the diff-applying run exits its output may still be on its way through the pipes.
const OUTPUT_DRAIN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// argv; on Windows typically `wsl.exe -e /home/<user>/.../betaflight_SITL.elf`.
    pub launch: Vec<String>,
    /// argv run before launch and after stop to remove stray processes; may be empty.
    pub cleanup: Vec<String>,
    /// SITL's working directory: holds eeprom.bin, betaflight.diff and sitl.log.
    pub workdir: PathBuf,
    pub diff_file: PathBuf,
    pub startup_timeout: Duration,
}

/// A stale SITL still holds the ports: a new instance prints this (e.g. `bind port 5761 for UART1 failed!!`)
/// and keeps running, and the simulator would talk to the old one. Treated as a startup error.
pub fn is_bind_failure(line: &str) -> bool {
    line.contains("bind port") && line.contains("failed")
}

/// The patched SITL prints this when init has finished and its scheduler runs; only then does it accept
/// state packets (third_party/betaflight/ofs-sitl.patch: deterministic boot).
pub const READY_LINE: &str = "[SITL] ready for the simulator";

pub fn is_ready_line(line: &str) -> bool {
    line.contains(READY_LINE)
}

/// Betaflight's `systemReset()` (a reboot, e.g. after a Configurator save) prints this before `exit(0)`;
/// so does the reset to bootloader. Together with a clean exit it means "restart me", not a crash.
pub fn is_reset_line(line: &str) -> bool {
    line.contains("[system]Reset")
}

#[derive(Default)]
struct LogSink {
    tail: VecDeque<String>,
    file: Option<File>,
    bind_failed: bool,
    ready_seen: bool,
    reset_seen: bool,
    /// The `bind port ... failed` lines, kept whole: the tail alone loses them under the first boot's EEPROM writes.
    failed_binds: Vec<String>,
    /// `[FLASH_ProgramWord]` lines seen (written to sitl.log, counted instead of kept in the tail).
    flash_lines: usize,
    /// Output streams (stdout, stderr) read to their end: a process's exit does not mean its last lines were read.
    streams_ended: u8,
}

impl LogSink {
    fn push(&mut self, line: String) {
        if is_bind_failure(&line) {
            self.bind_failed = true;
            self.failed_binds.push(line.clone());
        }
        if is_ready_line(&line) {
            self.ready_seen = true;
        }
        if is_reset_line(&line) {
            self.reset_seen = true;
        }
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        if line.starts_with("[FLASH_ProgramWord]") {
            self.flash_lines += 1;
            return;
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }

    fn tail_text(&self) -> String {
        let mut text = self.tail.iter().cloned().collect::<Vec<_>>().join("\n");
        if self.flash_lines > 0 {
            text.push_str(&format!("\n({} [FLASH_ProgramWord] lines not shown; all of them are in sitl.log)", self.flash_lines));
        }
        text
    }
}

type SharedLog = Arc<Mutex<LogSink>>;

pub struct SitlProcess {
    child: Child,
    log: SharedLog,
    cleanup: Vec<String>,
    argv: Vec<String>,
    /// When the process was first seen to have exited (see [`SitlProcess::rebooted`]).
    exited_at: Option<Instant>,
}

impl SitlProcess {
    /// Launches SITL; `sitl.log` starts afresh.
    pub fn start(cfg: &LaunchConfig) -> Result<Self, FcError> {
        Self::launch(cfg, false)
    }

    /// Launches SITL again after a Betaflight reboot: `sitl.log` keeps the log from before the reboot and gets a
    /// `--- relaunch after reboot ---` line.
    pub fn relaunch(cfg: &LaunchConfig) -> Result<Self, FcError> {
        Self::launch(cfg, true)
    }

    fn launch(cfg: &LaunchConfig, append_log: bool) -> Result<Self, FcError> {
        std::fs::create_dir_all(&cfg.workdir)?;
        let workdir = std::path::absolute(&cfg.workdir)?;
        if workdir.join("eeprom.bin").exists() {
            check_diff_unchanged(cfg, &workdir)?;
        }
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
        let log_path = workdir.join("sitl.log");
        let file = if append_log {
            let file = OpenOptions::new().create(true).append(true).open(&log_path).ok();
            file.map(|mut f| {
                let _ = writeln!(f, "--- relaunch after reboot ---");
                f
            })
        } else {
            File::create(&log_path).ok()
        };
        let log: SharedLog = Arc::new(Mutex::new(LogSink { file, ..LogSink::default() }));
        let child = spawn_logged(&cfg.launch, &workdir, &log)?;
        let mut proc = SitlProcess { child, log, cleanup: cfg.cleanup.clone(), argv: cfg.launch.clone(), exited_at: None };
        proc.wait_until_ready(cfg.startup_timeout)?;
        Ok(proc)
    }

    /// Waits for SITL to announce it is ready ([`READY_LINE`]). A stale instance holding the ports makes the new one
    /// print `bind port ... failed` during init, before that line: a startup error.
    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                let tail = self.log_tail();
                let what = format!("exited with {status} during startup{}", super::startup_hint(&self.argv, &tail));
                return Err(FcError::Startup(what, tail));
            }
            let (ready, bind_failed) = self.log.lock().map(|l| (l.ready_seen, l.bind_failed)).unwrap_or((false, false));
            if bind_failed {
                return Err(self.bind_error());
            }
            if ready {
                // The TCP/MSP thread prints its `bind port 5761 ... failed` on its own schedule, which can land
                // just after the ready line: give it a moment, so a stale SITL is always a startup error.
                std::thread::sleep(BIND_GRACE);
                return if self.log.lock().map(|l| l.bind_failed).unwrap_or(false) { Err(self.bind_error()) } else { Ok(()) };
            }
            if Instant::now() >= deadline {
                let what = format!(
                    "did not print \"{READY_LINE}\" within {} ms (is it the lockstep build from scripts/build-sitl.sh?)",
                    timeout.as_millis()
                );
                return Err(FcError::Startup(what, self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn bind_error(&self) -> FcError {
        let lines = self.log.lock().map(|l| l.failed_binds.clone()).unwrap_or_default();
        let mut what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)".to_string();
        for line in &lines {
            what.push_str(&format!("\n  {line}"));
            if let Some(port) = bound_port(line) {
                let holders = port_holders(port);
                if holders.is_empty() {
                    what.push_str(&format!("\n    port {port}: no socket found (or not visible from this platform)"));
                }
                for h in holders {
                    what.push_str(&format!("\n    port {port}: {h}"));
                }
            }
        }
        FcError::Startup(what, self.log_tail())
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        let status = self.child.try_wait().ok().flatten();
        if status.is_some() && self.exited_at.is_none() {
            self.exited_at = Some(Instant::now());
        }
        status
    }

    /// True when SITL announced a reset ([`is_reset_line`]) and exited cleanly: Betaflight rebooted.
    ///
    /// Called after a missed reply, so it waits up to [`REBOOT_GRACE`] for both: before resetting, Betaflight's
    /// `motorShutdown()` sleeps 0.5 s of real time (PWM ESCs), longer than the per-exchange reply timeout, and
    /// SITL then joins its threads before exiting. A SITL that has merely hung costs this grace once.
    pub fn rebooted(&mut self) -> bool {
        let deadline = Instant::now() + REBOOT_GRACE;
        loop {
            let reset_seen = self.log.lock().map(|l| l.reset_seen).unwrap_or(false);
            match self.exit_status() {
                Some(status) if reset_seen => return status.success(),
                // Exited without the reset line: a crash, once its last lines have had time to arrive.
                Some(_) if self.exited_at.is_some_and(|t| t.elapsed() >= EXIT_FLUSH) => return false,
                _ if Instant::now() >= deadline => return false,
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    pub fn log_tail(&self) -> String {
        self.log.lock().map(|l| l.tail_text()).unwrap_or_default()
    }
}

impl Drop for SitlProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        run_cleanup(&self.cleanup);
    }
}

fn command(argv: &[String], workdir: &Path) -> Result<Command, FcError> {
    let (program, args) = argv.split_first().ok_or_else(|| FcError::Config("fc.launch is empty".into()))?;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(workdir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    Ok(cmd)
}

fn spawn_logged(argv: &[String], workdir: &Path, log: &SharedLog) -> Result<Child, FcError> {
    let mut child = command(argv, workdir)?
        .spawn()
        .map_err(|source| FcError::Launch { command: argv.join(" "), source })?;
    pipe_lines(child.stdout.take().expect("stdout is piped"), log.clone());
    pipe_lines(child.stderr.take().expect("stderr is piped"), log.clone());
    Ok(child)
}

/// The quad's diff is applied on first boot only, so an EEPROM made from an older diff no longer matches the
/// quad file. Refuse to start rather than fly a stale configuration. Line endings do not count as a change.
fn check_diff_unchanged(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    let Ok(applied) = std::fs::read_to_string(workdir.join("betaflight.diff")) else {
        // Booted before copies of the applied diff were kept (before M2a): its EEPROM may select the UDP receiver,
        // and Betaflight would fail safe without a word.
        return Err(FcError::Config(format!(
            "{} was made by an older Open FPV Sim that did not record which diff it applied; delete it to apply {} \
             again (this also discards settings changed in Betaflight Configurator)",
            workdir.join("eeprom.bin").display(),
            cfg.diff_file.display()
        )));
    };
    let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
    if lines(&applied) != lines(&std::fs::read_to_string(&cfg.diff_file)?) {
        return Err(FcError::Config(format!(
            "{} changed since this quad's first boot; delete {} to apply it again (this also discards settings \
             changed in Betaflight Configurator)",
            cfg.diff_file.display(),
            workdir.join("eeprom.bin").display()
        )));
    }
    Ok(())
}

/// First boot: `<launch> --config betaflight.diff` loads the diff, saves eeprom.bin and exits.
fn apply_diff(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    std::fs::copy(&cfg.diff_file, workdir.join("betaflight.diff"))?;
    let mut argv = cfg.launch.clone();
    argv.extend(["--config".to_string(), "betaflight.diff".to_string()]);
    let log: SharedLog = Arc::new(Mutex::new(LogSink { file: File::create(workdir.join("sitl-config.log")).ok(), ..LogSink::default() }));
    let mut child = spawn_logged(&argv, workdir, &log)?;
    let deadline = Instant::now() + cfg.startup_timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FcError::Config(format!("`{}` did not exit within {} ms", argv.join(" "), cfg.startup_timeout.as_millis())));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // The exit can be seen before the readers have the last lines (the "EEPROM saved" line comes last): wait for both
    // streams to end, within a bound (a child the run left behind could hold them open).
    let drained = Instant::now() + OUTPUT_DRAIN;
    while log.lock().map(|l| l.streams_ended < 2).unwrap_or(false) && Instant::now() < drained {
        std::thread::sleep(Duration::from_millis(10));
    }
    // SITL creates eeprom.bin at boot, so its existence proves nothing: the run must exit cleanly having said so.
    let output = log.lock().map(|l| l.tail_text()).unwrap_or_default();
    let saved = log.lock().map(|l| l.tail.iter().any(|t| t.contains("[CONFIG] Config file processed, EEPROM saved"))).unwrap_or(false);
    if !status.success() || !saved || !workdir.join("eeprom.bin").exists() {
        let _ = std::fs::remove_file(workdir.join("eeprom.bin")); // the next load applies the diff again
        return Err(FcError::Config(format!("`{}` did not apply the diff ({status}); output:\n{output}", argv.join(" "))));
    }
    Ok(())
}

fn pipe_lines(stream: impl Read + Send + 'static, log: SharedLog) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => {
                    if let Ok(mut l) = log.lock() {
                        l.streams_ended += 1;
                    }
                    break;
                }
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    tracing::debug!(target: "sitl", "{line}");
                    if let Ok(mut l) = log.lock() {
                        l.push(line);
                    }
                }
            }
        }
    });
}

/// The port of a `bind port 5762 for UART2 failed!!` line.
fn bound_port(line: &str) -> Option<u16> {
    line.split("bind port ").nth(1)?.split_whitespace().next()?.parse().ok()
}

/// Every TCP socket bound to `port`, with its state, and for a listener the process that owns it (pid, command,
/// state, parent pid, working directory): what a bind failure needs to say who holds the port. Linux only (read
/// from /proc); empty elsewhere, and under WSL from Windows (SITL's sockets live in the WSL VM).
#[cfg(target_os = "linux")]
pub fn port_holders(port: u16) -> Vec<String> {
    const STATES: [&str; 12] =
        ["?", "ESTABLISHED", "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2", "TIME_WAIT", "CLOSE", "CLOSE_WAIT", "LAST_ACK", "LISTEN", "CLOSING"];
    let mut sockets = Vec::new(); // (state, inode)
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else { continue };
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let local = f.get(1).and_then(|a| a.rsplit(':').next()).and_then(|p| u16::from_str_radix(p, 16).ok());
            if f.len() > 9 && local == Some(port) {
                let state = usize::from_str_radix(f[3], 16).ok().and_then(|s| STATES.get(s)).copied().unwrap_or("?");
                sockets.push((state, f[9].to_string()));
            }
        }
    }
    let mut out = Vec::new();
    for (state, inode) in sockets {
        let mut owner = String::new();
        if state == "LISTEN" {
            let target = format!("socket:[{inode}]");
            for p in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
                let pid = p.file_name().to_string_lossy().to_string();
                if !pid.bytes().all(|b| b.is_ascii_digit()) {
                    continue;
                }
                let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
                if fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.to_string_lossy() == target)) {
                    let comm = std::fs::read_to_string(p.path().join("comm")).unwrap_or_default();
                    let stat = std::fs::read_to_string(p.path().join("stat")).unwrap_or_default();
                    let mut rest = stat.rsplit_once(')').map_or("", |(_, s)| s).split_whitespace();
                    let (pstate, ppid) = (rest.next().unwrap_or("?"), rest.next().unwrap_or("?"));
                    let cwd = std::fs::read_link(p.path().join("cwd")).map(|c| c.display().to_string()).unwrap_or_default();
                    owner = format!(" by pid {pid} ({}, state {pstate}, parent {ppid}, cwd {cwd})", comm.trim());
                    break;
                }
            }
        }
        out.push(format!("{state}{owner}"));
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn port_holders(_port: u16) -> Vec<String> {
    Vec::new()
}

pub(crate) fn run_cleanup(argv: &[String]) {
    if let Some((program, args)) = argv.split_first() {
        let _ = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}
