//! Launches and supervises the Betaflight SITL process; applies the CLI diff on first boot.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::net;
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
/// How long a `config.lock` may sit untouched before a waiter removes it: a holder killed mid-apply cannot release
/// it, and its age is the only portable sign that it died.
const LOCK_STALE: Duration = Duration::from_secs(300);

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

/// A stale SITL still holds the ports: a new instance prints this (e.g. `bind port 5761 for UART1 failed!! after 20
/// attempts (errno 98)`) once its bind retries are spent, and keeps running, and the simulator would talk to the old
/// one. Treated as a startup error. The `bind port 5761 for UART1 retry 3/20 (errno 98)` lines before it are
/// progress, not a verdict, and carry no "failed".
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
    /// What each failed bind's port showed the moment its line arrived: `Some((port, holders))` — possibly empty,
    /// meaning checked and nobody there — or `None` when the line named no port. A holder that releases the port
    /// before the error is built is still named (run 38453432260 attempt 5 was checked too late and said "no socket
    /// found"); a port that was free at the bind and is held when the error is built is reported as a late holder.
    bind_holders: Vec<Option<(u16, Vec<String>)>>,
    /// `[FLASH_ProgramWord]` lines seen (written to sitl.log, counted instead of kept in the tail).
    flash_lines: usize,
    /// Output streams (stdout, stderr) read to their end: a process's exit does not mean its last lines were read.
    streams_ended: u8,
}

impl LogSink {
    fn push(&mut self, line: String) {
        let mut bind_snapshot: Option<(u16, String)> = None;
        if is_bind_failure(&line) {
            self.bind_failed = true;
            if let Some(port) = bound_port(&line) {
                let holders = port_holders(port);
                let text = holders_text(port, &holders);
                self.bind_holders.push(Some((port, holders)));
                bind_snapshot = Some((port, text));
            } else {
                self.bind_holders.push(None);
            }
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
            if let Some((_, holders)) = &bind_snapshot {
                let _ = writeln!(f, "  when the bind failed: {holders}");
            }
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
        let awaited = await_cleanup(&cfg.launch);
        if awaited != AwaitCleanup::NONE {
            eprintln!("ofs-sitl: waited for the cleanup's victims: {awaited:?}");
        }
        if !workdir.join("eeprom.bin").exists() {
            // Someone else may configure it while we wait for the lock (`None`): then skip the diff they applied.
            if let Some(_config_lock) = lock_config(&workdir, cfg.startup_timeout)? {
                apply_diff(cfg, &workdir)?;
            }
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
        FcError::Startup(bind_error_text(&self.log), self.log_tail())
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
        let awaited = await_cleanup(&self.argv);
        if awaited != AwaitCleanup::NONE {
            eprintln!("ofs-sitl: waited for the cleanup's victims on drop: {awaited:?}");
        }
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

/// The first boot's serialization: `config.lock` in the workdir, held while the diff applies, so two simulators
/// first-booting one quad's directory do not both run `--config` in it (and cannot interleave a partial EEPROM into
/// the other's boot). `Some(file)` while holding; `None` when a holder finished first (`eeprom.bin` appeared). A
/// lock untouched for [`LOCK_STALE`] is a holder killed mid-apply: removed so the boot can go ahead.
fn lock_config(workdir: &Path, timeout: Duration) -> Result<Option<File>, FcError> {
    let path = workdir.join("config.lock");
    let deadline = Instant::now() + timeout;
    loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok(Some(file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        if workdir.join("eeprom.bin").exists() {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err(FcError::Config(format!(
                "{} has been locked for {} ms: another simulator's first boot is stuck, or a stale config.lock \
                 survived a hard kill and is younger than {} s",
                path.display(),
                timeout.as_millis(),
                LOCK_STALE.as_secs()
            )));
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.modified().ok().is_some_and(|m| m.elapsed().unwrap_or_default() > LOCK_STALE) {
                let _ = std::fs::remove_file(&path);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
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

/// The holders rendered for sitl.log: what `port_holders` saw at the moment of the failure.
fn holders_text(port: u16, holders: &[String]) -> String {
    if holders.is_empty() {
        format!("port {port}: no socket found (or not visible from this platform)")
    } else {
        holders.iter().map(|h| format!("port {port}: {h}")).collect::<Vec<_>>().join("\n    ")
    }
}

/// The bind-failure error's text: each failing line, and what its port showed when the line arrived. A bind that
/// found no holder is checked again now, and a holder that appeared since is reported as such.
fn bind_error_text(log: &SharedLog) -> String {
    let (lines, findings) = log
        .lock()
        .map(|l| (l.failed_binds.clone(), l.bind_holders.clone()))
        .unwrap_or_default();
    let mut what =
        "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)".to_string();
    for (line, finding) in lines.iter().zip(findings.iter()) {
        what.push_str(&format!("\n  {line}"));
        let Some((port, holders)) = finding else { continue };
        if !holders.is_empty() {
            for h in holders {
                what.push_str(&format!("\n    port {port}: {h}"));
            }
            continue;
        }
        let now = port_holders(*port);
        if now.is_empty() {
            what.push_str(&format!(
                "\n    port {port}: no socket found when the bind failed (or not visible from this platform)"
            ));
        } else {
            what.push_str(&format!("\n    port {port}: no socket when the bind failed, held now by:"));
            for h in now {
                what.push_str(&format!("\n    port {port}: {h}"));
            }
        }
    }
    what
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

pub fn run_cleanup(argv: &[String]) {
    if let Some((program, args)) = argv.split_first() {
        let _ = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// What a cleanup wait saw: `victims_seen` live processes at the start, whether the SIGTERM grace ran out and
/// the SIGKILL had to fire, and how long the wait took. [`AwaitCleanup::NONE`] is the every-launch case:
/// nobody was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwaitCleanup {
    pub victims_seen: u32,
    pub escalated: bool,
    pub waited_ms: u64,
}

impl AwaitCleanup {
    pub const NONE: AwaitCleanup = AwaitCleanup { victims_seen: 0, escalated: false, waited_ms: 0 };
}

/// How long a victim has to die of the cleanup's SIGTERM before it is SIGKILLed, and how long the SIGKILL then
/// has to finish. A victim that is merely slow costs the first grace once; the poll is 10 ms.
#[cfg(target_os = "linux")]
const CLEANUP_TERM_GRACE: Duration = Duration::from_millis(300);
#[cfg(target_os = "linux")]
const CLEANUP_KILL_GRACE: Duration = Duration::from_secs(1);

/// The victims' view from inside the WSL VM, where the Windows host cannot read /proc: wait for the last victim to be
/// gone — a zombie holds no ports, but `pgrep` matches it, so init's reap ends the wait — escalating to SIGKILL
/// after [`CLEANUP_TERM_GRACE`], then report what was seen as `seen=<n> escalated=<0|1> ms=<t>` for
/// [`await_cleanup`] to parse. The victims are chosen by the script's arguments (never spliced into its text):
/// `$1` is `x` for every process named `betaflight_SITL`, or `s` for the processes of the slot whose
/// `--port-offset` is `$2` (see [`net::slot_pids_sh`]).
pub const WSL_WAIT_SCRIPT: &str = concat!(
    net::slot_pids_sh!(),
    "\
live() { if [ \"$1\" = s ]; then [ -n \"$(slot_pids \"$2\")\" ]; else pkill -0 -x betaflight_SITL 2>/dev/null; fi; }
slay() { if [ \"$1\" = s ]; then pids=$(slot_pids \"$2\"); [ -z \"$pids\" ] || kill -9 $pids 2>/dev/null; else pkill -9 -x betaflight_SITL 2>/dev/null; fi; }
live \"$@\" || { echo 'seen=0 escalated=0 ms=0'; exit 0; }
t0=$(date +%s%N)
i=0; while live \"$@\" && [ $i -lt 30 ]; do sleep 0.01; i=$((i+1)); done
esc=0
if live \"$@\"; then
  slay \"$@\"
  esc=1
  i=0; while live \"$@\" && [ $i -lt 100 ]; do sleep 0.01; i=$((i+1)); done
fi
t1=$(date +%s%N)
echo \"seen=1 escalated=$esc ms=$(( (t1-t0)/1000000 ))\"
exit 0"
);

/// Waits for the cleanup's victims to actually die before a new SITL binds. The `pkill` in the cleanup only
/// *signals* them, and their sockets close one by one during teardown, so a launch that immediately follows can
/// fail to bind (`bind port ... failed!!` — the port-bind flake, see
/// `docs/superpowers/history/closeout-evidence.md`). A victim that ignores SIGTERM is SIGKILLed after
/// [`CLEANUP_TERM_GRACE`]; the whole wait gives up after [`CLEANUP_KILL_GRACE`] and the launch proceeds — the
/// startup diagnostics name any holder if the bind still fails. Under WSL the victims live in the VM, so the
/// wait runs there too; on plain Windows there is nothing to wait for. Returns what was seen, so a recurrence
/// of the flake says whether a victim existed at launch at all.
pub fn await_cleanup(launch: &[String]) -> AwaitCleanup {
    let scope = net::port_offset_in(launch);
    if let Some(prefix) = net::wsl_prefix(launch) {
        let mut argv = prefix;
        argv.extend(["sh", "-c", WSL_WAIT_SCRIPT, "sh"].map(String::from));
        match scope {
            Some(offset) => argv.extend(["s".to_string(), offset.to_string()]),
            None => argv.push("x".to_string()),
        }
        let output = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).output();
        return parse_wait_output(output.ok().map(|o| o.stdout).unwrap_or_default());
    }
    #[cfg(target_os = "linux")]
    return await_cleanup_native(scope);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = scope;
        AwaitCleanup::NONE
    }
}

/// Reads the WSL wait script's `seen=<n> escalated=<0|1> ms=<t>` line; anything unreadable is the no-victim case
/// (the wait ran, the launch proceeds, and the launch's own diagnostics cover a holder).
fn parse_wait_output(stdout: Vec<u8>) -> AwaitCleanup {
    let text = String::from_utf8_lossy(&stdout);
    let (mut seen, mut escalated, mut ms) = (0, false, 0);
    for field in text.lines().last().unwrap_or("").split_whitespace() {
        match field.split_once('=') {
            Some(("seen", n)) => seen = n.parse().unwrap_or(0),
            Some(("escalated", e)) => escalated = e == "1",
            Some(("ms", t)) => ms = t.parse().unwrap_or(0),
            _ => {}
        }
    }
    AwaitCleanup { victims_seen: seen, escalated, waited_ms: ms }
}

/// The native-Linux half of [`await_cleanup`]: poll /proc for live victims: every `betaflight_SITL` process, or
/// with a `scope` (a slot's `--port-offset`) only that slot's.
#[cfg(target_os = "linux")]
fn await_cleanup_native(scope: Option<u16>) -> AwaitCleanup {
    let first = live_victims(scope).len() as u32;
    if first == 0 {
        return AwaitCleanup::NONE;
    }
    let started = Instant::now();
    let mut escalated = false;
    loop {
        let alive = live_victims(scope);
        if alive.is_empty() {
            return AwaitCleanup {
                victims_seen: first,
                escalated,
                waited_ms: started.elapsed().as_millis() as u64,
            };
        }
        if !escalated && started.elapsed() >= CLEANUP_TERM_GRACE {
            let _ = Command::new("kill").arg("-9").args(alive.iter().map(u32::to_string)).stdin(Stdio::null()).stderr(Stdio::null()).status();
            escalated = true;
        }
        if started.elapsed() >= CLEANUP_TERM_GRACE + CLEANUP_KILL_GRACE {
            return AwaitCleanup {
                victims_seen: first,
                escalated,
                waited_ms: started.elapsed().as_millis() as u64,
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The pids of the live victims: processes named `betaflight_SITL`, with a `scope` only those of that slot
/// ([`net::is_victim`]; a zombie holds no ports and does not count).
#[cfg(target_os = "linux")]
fn live_victims(scope: Option<u16>) -> Vec<u32> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else { continue };
        if comm.trim() != net::SITL_PROCESS_NAME {
            continue;
        }
        if let Some(offset) = scope {
            let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
            let text = String::from_utf8_lossy(&cmdline);
            let args: Vec<&str> = text.split('\0').filter(|a| !a.is_empty()).collect();
            if !net::is_victim(&args, offset) {
                continue;
            }
        }
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
        let Some((_, rest)) = stat.rsplit_once(')') else { continue };
        if rest.trim_start().starts_with('Z') {
            continue;
        }
        pids.push(pid);
    }
    pids
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// A port that was free when the bind failed but is held by the time the error is built (something grabbed it
    /// in between): the late holder is reported as such, not lost.
    #[test]
    fn a_late_holder_is_reported_when_the_bind_failed_on_a_free_port() {
        let log: SharedLog = Arc::new(Mutex::new(LogSink::default()));
        // The bind line is pushed while the port is free, then the port is taken: on a busy CI runner another
        // process can snatch the released port first, so the rebind retries with a fresh port until it wins.
        let (port, _listener) = loop {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = probe.local_addr().unwrap().port();
            drop(probe);
            log.lock().unwrap_or_else(|e| e.into_inner()).push(format!("bind port {port} for UART1 failed!!"));
            match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => break (port, listener),
                Err(_) => continue,
            }
        };
        let text = bind_error_text(&log);
        drop(_listener);
        assert!(text.contains(&format!("port {port}: no socket when the bind failed, held now by:")), "{text}");
        assert!(text.contains(&format!("port {port}: LISTEN by pid {}", std::process::id())), "{text}");
    }

    /// CI (run 38453432260, attempt 5) caught the flake as `no socket found`: the holders were read when the error
    /// was built, after SITL had booted well past the failure, and the holder had released the port by then. They
    /// must be read the moment the bind line arrives.
    #[test]
    fn the_holder_is_named_even_if_it_releases_the_port_before_the_error_is_built() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log: SharedLog = Arc::new(Mutex::new(LogSink::default()));
        log.lock().unwrap_or_else(|e| e.into_inner()).push(format!("bind port {port} for UART1 failed!!"));
        drop(listener);
        std::thread::sleep(Duration::from_millis(50));
        let text = bind_error_text(&log);
        assert!(
            text.contains(&format!("port {port}: LISTEN by pid {}", std::process::id())),
            "{text}"
        );
    }
}
