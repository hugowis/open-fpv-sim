//! Exporting a finished DVR recording (spec R3): an image sequence copied out of the AVI, or an MP4 made by
//! the user's own ffmpeg. The AVI stays the one source of truth and is never modified; nothing here overwrites a
//! file or a directory.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How far into the file the `movi` fourcc is searched for: the header is 220 bytes, this leaves room for a
/// recording from a muxer that wrote a few more.
const HEADER_SCAN: usize = 1024;

/// Copies the AVI's `00dc` chunks, unmodified, to `<stem>_frames/frame_%06d.jpg` (numbered from 1) next to it.
/// Returns that directory and the frame count. The chunks are found by scanning from `movi` to `idx1` or the end
/// of the file rather than trusting the header sizes, so a recording that crashed (sizes still zero) exports; a
/// chunk cut off by the end of the file is dropped. The directory must not exist.
pub fn export_frames(avi: &Path) -> Result<(PathBuf, u64), String> {
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
    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            format!("{} already exists: an export never overwrites another", dir.display())
        }
        _ => format!("{}: {e}", dir.display()),
    })?;

    let mut count = 0u64;
    let mut chunk_head = [0u8; 8];
    while pos + 8 <= len {
        input.read_exact(&mut chunk_head).map_err(name)?;
        let size = u32::from_le_bytes(chunk_head[4..].try_into().unwrap()) as u64;
        if &chunk_head[..4] == b"idx1" || pos + 8 + size > len {
            break;
        }
        if &chunk_head[..4] == b"00dc" {
            let mut jpeg = vec![0u8; size as usize];
            input.read_exact(&mut jpeg).map_err(name)?;
            count += 1;
            let path = dir.join(format!("frame_{count:06}.jpg"));
            File::create_new(&path)
                .and_then(|mut f| f.write_all(&jpeg))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            input.seek_relative(size as i64).map_err(name)?;
        }
        input.seek_relative((size & 1) as i64).map_err(name)?; // chunks are padded to even sizes
        pos += 8 + size + (size & 1);
    }
    Ok((dir, count))
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
/// anything is spawned; a failing ffmpeg is an error carrying the last line it wrote to stderr.
pub fn export_mp4(ffmpeg: &Path, avi: &Path) -> Result<PathBuf, String> {
    let mp4 = avi.with_extension("mp4");
    if mp4.exists() {
        return Err(format!("{} already exists: an export never overwrites another", mp4.display()));
    }
    let output = Command::new(ffmpeg)
        .args(ffmpeg_args(avi, &mp4))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", ffmpeg.display()))?;
    if output.status.success() {
        return Ok(mp4);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().map(str::trim).rfind(|l| !l.is_empty());
    Err(last.map_or_else(|| format!("ffmpeg failed: {}", output.status), str::to_string))
}
