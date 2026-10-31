//! Starting and stopping the `ofs-sim` server process.
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{ClientError, ErrorKind};
use crate::model::LaunchSpec;

/// How often [`ServerProcess::resolved_addr`] looks for the port file.
const RESOLVE_POLL: Duration = Duration::from_millis(50);

/// Whether a listen address asks the OS for a free port (`127.0.0.1:0`), so the address to connect to can only
/// be learned from the server once it listens.
pub(crate) fn wants_auto_port(listen: &str) -> bool {
    listen.parse::<SocketAddr>().is_ok_and(|addr| addr.port() == 0)
}

/// The launch error for a server that ended before it could be reached: its status and the log tail.
pub(crate) fn exited_during_startup(program: &Path, listen: &str, status: ExitStatus, tail: &str) -> ClientError {
    let log = if tail.is_empty() { String::new() } else { format!("\n{tail}") };
    ClientError::new(
        ErrorKind::Launch,
        format!("{} exited during startup ({status}). Is the port in {listen} already taken?{log}", program.display()),
    )
}

/// A server this client started. Dropping it kills the process; call `Client` shutdown to unload the session
/// first, so Betaflight SITL stops cleanly (killing `ofs-sim` alone orphans SITL on Windows).
pub struct ServerProcess {
    /// Locked so [`resolved_addr`](ServerProcess::resolved_addr) (a `&self`) can watch for the end while its
    /// owner still holds the process.
    child: Mutex<Child>,
    log_file: Option<PathBuf>,
    program: PathBuf,
    /// The listen address of a port-0 start: the port in it is a placeholder the server replaces with the one
    /// it bound, reported in `port_file`.
    listen: Option<SocketAddr>,
    port_file: Option<PathBuf>,
}

impl ServerProcess {
    /// Starts the server on `listen`, as `ofs-sim --listen <addr> --data-dir <dir>`. A port of 0 also passes
    /// `--port-file <data-dir>/server.port`, so [`resolved_addr`](ServerProcess::resolved_addr) can report the
    /// address the OS picked; any other port starts the server exactly as before.
    pub fn spawn(spec: &LaunchSpec, listen: &str) -> Result<ServerProcess, ClientError> {
        let auto = listen.parse::<SocketAddr>().ok().filter(|addr| addr.port() == 0);
        let port_file = auto.map(|_| {
            // The server writes the port file before it creates the data dir itself, and a file an earlier
            // server of ours left behind would read as this one's port.
            let _ = fs::create_dir_all(&spec.data_dir);
            let file = spec.data_dir.join("server.port");
            let _ = fs::remove_file(&file);
            file
        });
        let mut command = Command::new(&spec.program);
        command.arg("--listen").arg(listen).arg("--data-dir").arg(&spec.data_dir);
        if let Some(file) = &port_file {
            command.arg("--port-file").arg(file);
        }
        command.envs(spec.env.iter().map(|(k, v)| (k, v)));
        command.stdin(Stdio::null()).stdout(Stdio::null());
        match &spec.log_file {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let log = OpenOptions::new().create(true).append(true).open(path).map_err(|e| {
                    ClientError::new(ErrorKind::Launch, format!("cannot open the server log {}: {e}", path.display()))
                })?;
                command.stderr(Stdio::from(log));
            }
            None => {
                command.stderr(Stdio::null());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console window next to the game
        }
        let child = command.spawn().map_err(|e| {
            ClientError::new(
                ErrorKind::Launch,
                format!(
                    "cannot start {}: {e}. Build the server with `cargo build -p ofs-sim` and point the client at it \
                     (setting `server_bin` or the OFS_SIM_BIN environment variable).",
                    spec.program.display()
                ),
            )
        })?;
        Ok(ServerProcess {
            child: Mutex::new(child),
            log_file: spec.log_file.clone(),
            program: spec.program.clone(),
            listen: auto,
            port_file,
        })
    }

    /// The exit status, once the process has ended.
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        crate::worker::lock(&self.child).try_wait().ok().flatten()
    }

    /// The address the server listens on, for a spawn whose listen port was 0: the port the server reports in
    /// its port file (written once it listens), looked for every 50 ms. A server that ends first fails at once
    /// with its status and log tail; one that neither reports nor ends fails with the log tail after `timeout`.
    pub fn resolved_addr(&self, timeout: Duration) -> Result<String, ClientError> {
        let Some(listen) = self.listen else {
            return Err(ClientError::new(
                ErrorKind::Launch,
                format!("{} was not started on port 0, so there is no port file to read", self.program.display()),
            ));
        };
        let file = self.port_file.as_deref().expect("a port-0 spawn always has a port file");
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = crate::worker::lock(&self.child).try_wait().ok().flatten() {
                return Err(exited_during_startup(&self.program, &listen.to_string(), status, &self.log_tail()));
            }
            // The server renames the file into place, so a read sees a whole port or nothing.
            if let Some(port) = fs::read_to_string(file).ok().and_then(|s| s.trim().parse::<u16>().ok()) {
                return Ok(SocketAddr::new(listen.ip(), port).to_string());
            }
            if Instant::now() > deadline {
                let tail = self.log_tail();
                let log = if tail.is_empty() { String::new() } else { format!("\n{tail}") };
                return Err(ClientError::new(
                    ErrorKind::Launch,
                    format!("{} did not report its port within {timeout:?}{log}", self.program.display()),
                ));
            }
            std::thread::sleep(RESOLVE_POLL);
        }
    }

    /// The end of the server's log (at most about 2 kB), for error messages.
    pub fn log_tail(&self) -> String {
        self.log_file.as_deref().map(read_tail).unwrap_or_default()
    }

    pub fn stop(&mut self) {
        let mut child = crate::worker::lock(&self.child);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_tail(path: &Path) -> String {
    const TAIL_BYTES: u64 = 2000;
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).trim().to_string()
}
