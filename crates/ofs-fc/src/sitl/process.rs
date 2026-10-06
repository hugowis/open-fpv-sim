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
/// How long after the ready line a late `bind port ... failed` is still waited for.
const BIND_GRACE: Duration = Duration::from_millis(300);

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
}

impl LogSink {
    fn push(&mut self, line: String) {
        if is_bind_failure(&line) {
            self.bind_failed = true;
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
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

type SharedLog = Arc<Mutex<LogSink>>;

pub struct SitlProcess {
    child: Child,
    log: SharedLog,
    cleanup: Vec<String>,
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
        let mut proc = SitlProcess { child, log, cleanup: cfg.cleanup.clone() };
        proc.wait_until_ready(cfg.startup_timeout)?;
        Ok(proc)
    }

    /// Waits for SITL to announce it is ready ([`READY_LINE`]). A stale instance holding the ports makes the new one
    /// print `bind port ... failed` during init, before that line: a startup error.
    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                return Err(FcError::Startup(format!("exited with {status} during startup"), self.log_tail()));
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
        let what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)";
        FcError::Startup(what.into(), self.log_tail())
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
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
                _ if Instant::now() >= deadline => return false,
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    pub fn log_tail(&self) -> String {
        self.log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default()
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
/// quad file. Refuse to start rather than fly a stale configuration.
fn check_diff_unchanged(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    let Ok(applied) = std::fs::read(workdir.join("betaflight.diff")) else {
        return Ok(()); // first booted before copies of the applied diff were kept
    };
    if applied != std::fs::read(&cfg.diff_file)? {
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
    let log: SharedLog = Arc::new(Mutex::new(LogSink::default()));
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
    if !workdir.join("eeprom.bin").exists() {
        let tail = log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
        return Err(FcError::Config(format!("`{}` exited with {status} without writing eeprom.bin; output:\n{tail}", argv.join(" "))));
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
                Ok(0) | Err(_) => break,
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

pub(crate) fn run_cleanup(argv: &[String]) {
    if let Some((program, args)) = argv.split_first() {
        let _ = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}
