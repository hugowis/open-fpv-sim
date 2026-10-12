//! Betaflight's own Blackbox logs (spec §4). With `set blackbox_device = VIRTUAL` in the quad's diff, SITL writes
//! `LOGnnnnn.BFL` into the quad's firmware directory each time it is armed; when the session ends, the logs it
//! wrote are copied into the logs directory, beside the recordings.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Copies the session's Blackbox logs when dropped. A `Session` declares it last: fields drop in declaration
/// order, so the vehicle (and Betaflight SITL with it) is gone, and its files closed, before the copy runs.
pub struct BlackboxCopy {
    firmware_dir: PathBuf,
    logs_dir: PathBuf,
    /// Logs modified since then belong to this session (with a little slack for coarse file times).
    since: SystemTime,
}

fn is_blackbox_log(name: &str) -> bool {
    // LOG00001.BFL: what SITL's VIRTUAL device names its files.
    name.len() == 12 && name.starts_with("LOG") && name.ends_with(".BFL") && name[3..8].bytes().all(|b| b.is_ascii_digit())
}

impl BlackboxCopy {
    pub fn new(firmware_dir: PathBuf, logs_dir: PathBuf) -> Self {
        let since = SystemTime::now() - Duration::from_secs(2);
        Self { firmware_dir, logs_dir, since }
    }

    /// Copies each `LOGnnnnn.BFL` the session wrote to `<logs dir>/<firmware dir name>-LOGnnnnn.BFL`. An identical
    /// copy already there is left alone; a different file under that name gets a `-2`, `-3`, ... before `.BFL`.
    /// Returns the copies made.
    pub fn copy(&self) -> std::io::Result<Vec<PathBuf>> {
        let Ok(entries) = std::fs::read_dir(&self.firmware_dir) else { return Ok(Vec::new()) };
        let prefix = self.firmware_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut logs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| is_blackbox_log(&e.file_name().to_string_lossy()))
            .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= self.since))
            .map(|e| e.path())
            .collect();
        logs.sort();
        let mut copies = Vec::new();
        for log in logs {
            std::fs::create_dir_all(&self.logs_dir)?;
            let name = log.file_name().expect("listed files have names").to_string_lossy().into_owned();
            if let Some(dest) = destination(&self.logs_dir, &prefix, &name, &std::fs::read(&log)?)? {
                std::fs::copy(&log, &dest)?;
                copies.push(dest);
            }
        }
        Ok(copies)
    }
}

/// Where `name` from firmware directory `prefix` goes: `None` when an identical copy is already there.
fn destination(logs_dir: &Path, prefix: &str, name: &str, bytes: &[u8]) -> std::io::Result<Option<PathBuf>> {
    let stem = name.trim_end_matches(".BFL");
    for n in 1.. {
        let candidate = if n == 1 { format!("{prefix}-{stem}.BFL") } else { format!("{prefix}-{stem}-{n}.BFL") };
        let path = logs_dir.join(candidate);
        match std::fs::read(&path) {
            Ok(existing) if existing == bytes => return Ok(None),
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(path)),
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop returns")
}

impl Drop for BlackboxCopy {
    fn drop(&mut self) {
        match self.copy() {
            Ok(copies) if !copies.is_empty() => tracing::info!("copied {} Blackbox log(s) to {}", copies.len(), self.logs_dir.display()),
            Ok(_) => {}
            Err(e) => tracing::warn!("copying Blackbox logs from {} failed: {e}", self.firmware_dir.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_logs_are_copied_once_under_their_quads_name() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("quad-0badf00d");
        let logs = tmp.path().join("logs");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00001.BFL"), b"H Product:Blackbox flight data recorder").unwrap();
        std::fs::write(fw.join("eeprom.bin"), b"not a log").unwrap();
        std::fs::write(fw.join("LOG1.BFL"), b"not SITL's naming").unwrap();
        let copy = BlackboxCopy::new(fw.clone(), logs.clone());
        assert_eq!(copy.copy().unwrap(), [logs.join("quad-0badf00d-LOG00001.BFL")]);
        assert!(copy.copy().unwrap().is_empty(), "an identical copy is not made twice");
        // A different log under the same number (the firmware directory was reset) is kept beside the first.
        std::fs::write(fw.join("LOG00001.BFL"), b"H Product:Blackbox flight data recorder, another flight").unwrap();
        assert_eq!(copy.copy().unwrap(), [logs.join("quad-0badf00d-LOG00001-2.BFL")]);
        assert_eq!(std::fs::read(logs.join("quad-0badf00d-LOG00001.BFL")).unwrap(), b"H Product:Blackbox flight data recorder");
    }

    #[test]
    fn logs_from_before_the_session_and_a_missing_directory_copy_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("fw");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00001.BFL"), b"old").unwrap();
        let later = BlackboxCopy { firmware_dir: fw, logs_dir: tmp.path().join("logs"), since: SystemTime::now() + Duration::from_secs(60) };
        assert!(later.copy().unwrap().is_empty());
        let missing = BlackboxCopy::new(tmp.path().join("nope"), tmp.path().join("logs"));
        assert!(missing.copy().unwrap().is_empty());
    }

    #[test]
    fn dropping_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let fw = tmp.path().join("q-1");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("LOG00002.BFL"), b"x").unwrap();
        drop(BlackboxCopy::new(fw, tmp.path().join("logs")));
        assert!(tmp.path().join("logs").join("q-1-LOG00002.BFL").exists());
    }
}
