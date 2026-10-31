//! Exporting a finished DVR recording (spec R3): an image sequence copied out of the AVI, or an MP4 made by
//! the user's own ffmpeg. The AVI stays the one source of truth and is never modified; nothing here overwrites a
//! file or a directory.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// How far into the file the `movi` fourcc is searched for: the header is 220 bytes, this leaves room for a
/// recording from a muxer that wrote a few more.
const HEADER_SCAN: usize = 1024;

/// Copies the AVI's `00dc` chunks, unmodified, to `<stem>_frames/frame_%06d.jpg` (numbered from 1) next to it.
/// Returns that directory and the frame count. The chunks are found by scanning from `movi` to `idx1` or the end
/// of the file rather than trusting the header sizes, so a recording that crashed (sizes still zero) exports; a
/// chunk cut off by the end of the file is dropped. The directory must not exist; it is made at the first frame, so
/// an AVI without frames is an error that leaves nothing behind, and a failure later removes what this call made.
pub fn export_frames(avi: &Path) -> Result<(PathBuf, u64), String> {
    export_frames_with(avi, &mut |path, jpeg| File::create_new(path).and_then(|mut f| f.write_all(jpeg)))
}

/// `export_frames` with the frame writer injected (it is handed the new file's path and the JPEG bytes).
fn export_frames_with(avi: &Path, write: &mut dyn FnMut(&Path, &[u8]) -> std::io::Result<()>) -> Result<(PathBuf, u64), String> {
    let name = |e: std::io::Error| format!("{}: {e}", avi.display());
    let file = File::open(avi).map_err(name)?;
    let len = file.metadata().map_err(name)?.len();
    let mut input = BufReader::new(file);
    let mut head = vec![0u8; HEADER_SCAN.min(len as usize)];
    input.read_exact(&mut head).map_err(name)?;
    if head.len() < 12 || &head[..4] != b"RIFF" || &head[8..12] != b"AVI " {
        return Err(format!("{} is not an AVI recording", avi.display()));
    }
    let movi = head[12..]
        .windows(4)
        .position(|w| w == b"movi")
        .ok_or_else(|| format!("{} has no movi list", avi.display()))?;
    let mut pos = (12 + movi + 4) as u64;
    input.seek(SeekFrom::Start(pos)).map_err(name)?;

    let stem = avi.file_stem().unwrap_or(OsStr::new("recording")).to_string_lossy();
    let dir = avi.with_file_name(format!("{stem}_frames"));
    let mut made = false;
    let mut count = 0u64;
    let mut chunk_head = [0u8; 8];
    let result = (|| {
        while pos + 8 <= len {
            input.read_exact(&mut chunk_head).map_err(name)?;
            let size = u32::from_le_bytes(chunk_head[4..].try_into().unwrap()) as u64;
            if &chunk_head[..4] == b"idx1" || pos + 8 + size > len {
                break;
            }
            if &chunk_head[..4] == b"00dc" {
                let mut jpeg = vec![0u8; size as usize];
                input.read_exact(&mut jpeg).map_err(name)?;
                if !made {
                    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
                        std::io::ErrorKind::AlreadyExists => {
                            format!("{} already exists: an export never overwrites another", dir.display())
                        }
                        _ => format!("{}: {e}", dir.display()),
                    })?;
                    made = true;
                }
                count += 1;
                let path = dir.join(format!("frame_{count:06}.jpg"));
                write(&path, &jpeg).map_err(|e| format!("{}: {e}", path.display()))?;
            } else {
                input.seek_relative(size as i64).map_err(name)?;
            }
            input.seek_relative((size & 1) as i64).map_err(name)?; // chunks are padded to even sizes
            pos += 8 + size + (size & 1);
        }
        if count == 0 {
            return Err(format!("{} holds no frames", avi.display()));
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok((dir, count)),
        Err(e) => {
            if made {
                std::fs::remove_dir_all(&dir).ok();
            }
            Err(e)
        }
    }
}

/// An export job's outcome: `kind` is "frames" (`result` is the directory) or "mp4" (the file).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDone {
    pub avi: PathBuf,
    pub kind: &'static str,
    pub result: Result<PathBuf, String>,
}

/// Runs exports off the caller's thread (an MP4 takes seconds) and hands the outcomes back through `poll`.
pub struct Exporter {
    tx: Sender<ExportDone>,
    rx: Mutex<Receiver<ExportDone>>,
}

impl Default for Exporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter {
    pub fn new() -> Exporter {
        let (tx, rx) = channel();
        Exporter { tx, rx: Mutex::new(rx) }
    }

    /// Exports `avi` as the requested kinds on a new thread, frames first, with ffmpeg from the `PATH`.
    pub fn spawn(&self, avi: PathBuf, frames: bool, mp4: bool) {
        self.spawn_with(if mp4 { find_ffmpeg() } else { None }, avi, frames, mp4);
    }

    /// `spawn` with ffmpeg given: `None` with `mp4` set reports "ffmpeg was not found on PATH". A panic in the
    /// thread is caught and reported as one export error instead of dying silently.
    pub fn spawn_with(&self, ffmpeg: Option<PathBuf>, avi: PathBuf, frames: bool, mp4: bool) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let outcomes = catch_panics(avi.clone(), move || {
                let mut done = Vec::new();
                if frames {
                    let result = export_frames(&avi).map(|(dir, _)| dir);
                    done.push(ExportDone { avi: avi.clone(), kind: "frames", result });
                }
                if mp4 {
                    let result = match ffmpeg {
                        Some(ffmpeg) => export_mp4(&ffmpeg, &avi),
                        None => Err("ffmpeg was not found on PATH".to_string()),
                    };
                    done.push(ExportDone { avi, kind: "mp4", result });
                }
                done
            });
            for outcome in outcomes {
                tx.send(outcome).ok();
            }
        });
    }

    /// The outcomes that arrived since the last call.
    pub fn poll(&self) -> Vec<ExportDone> {
        self.rx.lock().map_or_else(|_| Vec::new(), |rx| rx.try_iter().collect())
    }
}

/// Runs an export job's body under a panic guard: the outcomes it returns pass through untouched, and a panic
/// becomes one `ExportDone` of kind "export" whose error names the panic, so the poller always hears an outcome
/// instead of waiting on a thread that died silently.
fn catch_panics(avi: PathBuf, job: impl FnOnce() -> Vec<ExportDone>) -> Vec<ExportDone> {
    match catch_unwind(AssertUnwindSafe(job)) {
        Ok(outcomes) => outcomes,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "an unknown panic".to_string());
            vec![ExportDone { avi, kind: "export", result: Err(format!("the export panicked: {message}")) }]
        }
    }
}

/// The first `ffmpeg` (`ffmpeg.exe` on Windows) file in a `PATH`-style list of directories.
pub fn find_ffmpeg_in(path_var: &OsStr) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::split_paths(path_var).map(|dir| dir.join(exe)).find(|candidate| candidate.is_file())
}

/// ffmpeg on the `PATH`, if any.
pub fn find_ffmpeg() -> Option<PathBuf> {
    find_ffmpeg_in(&std::env::var_os("PATH")?)
}

/// The conversion: `-n` keeps the never-overwrite rule and the pad keeps odd window sizes legal for yuv420p.
pub fn ffmpeg_args(avi: &Path, mp4: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-nostdin", "-n", "-loglevel", "error", "-i"].iter().map(OsString::from).collect();
    args.push(avi.into());
    args.extend(
        ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-movflags", "+faststart"]
            .iter()
            .map(OsString::from),
    );
    args.push(mp4.into());
    args
}

/// Converts the AVI to `<stem>.mp4` next to it with the given ffmpeg. An existing mp4 is an error before
/// anything is spawned; a failing ffmpeg is an error carrying the last line it wrote to stderr, and a failing
/// run removes the partial it wrote, so the name is free for a retry.
pub fn export_mp4(ffmpeg: &Path, avi: &Path) -> Result<PathBuf, String> {
    let mp4 = avi.with_extension("mp4");
    if mp4.exists() {
        return Err(format!("{} already exists: an export never overwrites another", mp4.display()));
    }
    let mut command = Command::new(ffmpeg);
    command.args(ffmpeg_args(avi, &mp4)).stdin(Stdio::null()).stdout(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000); // CREATE_NO_WINDOW: no console flash from a GUI parent
    let output = command.output().map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    if output.status.success() {
        return Ok(mp4);
    }
    let _ = std::fs::remove_file(&mp4); // the partial this run wrote (the pre-check saw no file): best-effort
    Err(last_line(&output.stderr).unwrap_or_else(|| format!("ffmpeg failed: {}", output.status)))
}

/// The last non-blank line of `stderr`, trimmed (invalid UTF-8 is replaced, not an error).
fn last_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr).lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dvr::DvrWriter;

    #[test]
    fn last_line_is_the_last_non_blank_line_trimmed() {
        assert_eq!(last_line(b""), None);
        assert_eq!(last_line(b"\n  \r\n\n"), None);
        assert_eq!(last_line(b"first\nsecond  \n\n\n"), Some("second".to_string()));
        assert_eq!(last_line(b"first\r\nUnknown encoder 'libx264'\r\n\r\n"), Some("Unknown encoder 'libx264'".to_string()));
        assert_eq!(last_line(b"only"), Some("only".to_string()));
        let non_utf8 = last_line(b"ok\nbad \xff\xfe byte\n").unwrap();
        assert!(non_utf8.starts_with("bad ") && non_utf8.ends_with(" byte"), "{non_utf8}");
    }

    #[test]
    fn catch_panics_turns_a_panic_into_one_reported_export_error() {
        let outcomes = catch_panics(PathBuf::from("a.avi"), || panic!("synthetic boom"));
        assert_eq!(outcomes.len(), 1, "one outcome, not a thread that dies silently");
        assert_eq!(outcomes[0].avi, PathBuf::from("a.avi"));
        assert_eq!(outcomes[0].kind, "export");
        let err = outcomes[0].result.as_ref().unwrap_err();
        assert!(err.contains("synthetic boom"), "{err}");
    }

    #[test]
    fn a_failure_after_the_directory_was_made_removes_it() {
        let dir = std::env::temp_dir().join(format!("ofs-dvr-export-cleanup-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let avi = dir.join("rec.avi");
        let writer = DvrWriter::start(avi.clone(), 50, 4, 2).unwrap();
        writer.write_frame(b"AAA".to_vec());
        writer.write_frame(b"BBB".to_vec());
        writer.stop().unwrap();
        let mut written = 0;
        let err = export_frames_with(&avi, &mut |path, jpeg| {
            written += 1;
            if written == 2 {
                return Err(std::io::Error::other("disk full"));
            }
            File::create_new(path).and_then(|mut f| f.write_all(jpeg))
        })
        .unwrap_err();
        assert!(err.contains("disk full"), "{err}");
        assert!(!dir.join("rec_frames").exists(), "the half-made directory is removed");
        assert!(avi.exists());
    }
}
