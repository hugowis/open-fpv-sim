//! Starting and stopping the `ofs-sim` server process.
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::error::{ClientError, ErrorKind};
use crate::model::LaunchSpec;

/// A server this client started. Dropping it kills the process; call `Client` shutdown to unload the session
/// first, so Betaflight SITL stops cleanly (killing `ofs-sim` alone orphans SITL on Windows).
pub struct ServerProcess {
    child: Child,
    log_file: Option<std::path::PathBuf>,
}

impl ServerProcess {
    pub fn spawn(spec: &LaunchSpec, listen: &str) -> Result<ServerProcess, ClientError> {
        let mut command = Command::new(&spec.program);
        command.arg("--listen").arg(listen).arg("--data-dir").arg(&spec.data_dir);
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
        Ok(ServerProcess { child, log_file: spec.log_file.clone() })
    }

    /// The exit status, once the process has ended.
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// The end of the server's log (at most about 2 kB), for error messages.
    pub fn log_tail(&self) -> String {
        self.log_file.as_deref().map(read_tail).unwrap_or_default()
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
