//! Launches and supervises the Betaflight SITL process; applies the CLI diff on first boot.
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::codec::MSP_TCP_PORT;
use super::FcError;

const TAIL_LINES: usize = 200;

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

#[derive(Default)]
struct LogSink {
    tail: VecDeque<String>,
    file: Option<File>,
    bind_failed: bool,
}

impl LogSink {
    fn push(&mut self, line: String) {
        if is_bind_failure(&line) {
            self.bind_failed = true;
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
    pub fn start(cfg: &LaunchConfig) -> Result<Self, FcError> {
        std::fs::create_dir_all(&cfg.workdir)?;
        let workdir = std::path::absolute(&cfg.workdir)?;
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
        let log: SharedLog = Arc::new(Mutex::new(LogSink { file: File::create(workdir.join("sitl.log")).ok(), ..LogSink::default() }));
        let child = spawn_logged(&cfg.launch, &workdir, &log)?;
        let mut proc = SitlProcess { child, log, cleanup: cfg.cleanup.clone() };
        proc.wait_until_ready(cfg.startup_timeout)?;
        Ok(proc)
    }

    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let addr = SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT));
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                return Err(FcError::Startup(format!("exited with {status} during startup"), self.log_tail()));
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                // A stale instance also answers on tcp:5761; give the new one time to report bind failures.
                std::thread::sleep(Duration::from_millis(300));
                if self.log.lock().map(|l| l.bind_failed).unwrap_or(false) {
                    let what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)";
                    return Err(FcError::Startup(what.into(), self.log_tail()));
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                let what = format!("did not open tcp:{MSP_TCP_PORT} within {} ms", timeout.as_millis());
                return Err(FcError::Startup(what, self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
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
