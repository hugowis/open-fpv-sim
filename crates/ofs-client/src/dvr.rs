//! The goggles' DVR container (M3e spec §5): an MJPEG AVI written frame by frame. Standard RIFF/AVI — a `LIST
//! hdrl` (avih + strh/strf declaring one 'vids'/'MJPG' stream at the fixed frame rate), a `LIST movi` with one
//! `00dc` JPEG chunk per frame (padded to even sizes), an `idx1` index, and the four size/length fields patched
//! when the recording stops. Encoding is Godot's (`Image.save_jpg_to_buffer`); this module only muxes. File I/O
//! runs on a worker thread so a slow disk cannot stall the render loop; at most [`FRAME_CHANNEL_CAPACITY`]
//! frames are in flight and further ones are dropped and counted (a recording that hiccups is fine, a flight
//! that stutters is not).
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

/// Frames in flight to the writer thread before the capture starts dropping: a second's worth.
const FRAME_CHANNEL_CAPACITY: usize = 50;
/// The capture cadence: recordings are tied to received fields.
pub const DVR_FPS: u32 = 50;
const AVIIF_KEYFRAME: u32 = 0x10;
/// AVI 1.0's 32-bit sizes cap one RIFF segment; a longer recording starts a `RIFF/AVIX` segment.
const AVI_SEGMENT_BYTES: u64 = u32::MAX as u64 + 8;
/// The total cap: about 70 hours at 50 fps and typical 640x480 frames.
const MAX_DVR_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// A recording feed: each has its own writer in the client. `Goggles` is the FPV composite; `Clean` is the
/// camera without the video degradation or OSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvrFeed {
    Goggles,
    Clean,
}

impl DvrFeed {
    pub fn parse(name: &str) -> Option<DvrFeed> {
        match name {
            "goggles" => Some(DvrFeed::Goggles),
            "clean" => Some(DvrFeed::Clean),
            _ => None,
        }
    }
}

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvrSummary {
    pub frames: u64,
    pub dropped_frames: u64,
    pub bytes: u64,
    pub truncated: bool,
}

struct Header {
    microsec_per_frame: u32,
    fps: u32,
    width: u32,
    height: u32,
}

fn put(bytes: &mut Vec<u8>, tag: &[u8; 4]) {
    bytes.extend_from_slice(tag);
}

fn put32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Writes the fixed layout. Returns the file offsets of the five fields patched at stop: the outer RIFF size,
/// the `movi` LIST size, `avih.dwTotalFrames`, `strh.dwLength` and the OpenDML total frame count. (The index's
/// offsets are relative to the position right after the `movi` fourcc, counting the fourcc: the first frame sits at 4.)
fn write_header(file: &mut File, h: &Header) -> std::io::Result<[usize; 5]> {
    let mut b = Vec::with_capacity(224);
    put(&mut b, b"RIFF");
    let riff_size_at = 4;
    put32(&mut b, 0);
    put(&mut b, b"AVI ");
    put(&mut b, b"LIST");
    put32(&mut b, 448); // hdrl: "hdrl" + avih + strl + dmlh
    put(&mut b, b"hdrl");
    put(&mut b, b"avih");
    put32(&mut b, 56);
    put32(&mut b, h.microsec_per_frame);
    put32(&mut b, 0); // max bytes per second
    put32(&mut b, 0); // padding granularity
    put32(&mut b, 0x10); // AVIF_HASINDEX
    let avih_frames_at = 48;
    put32(&mut b, 0); // dwTotalFrames, patched
    put32(&mut b, 0); // initial frames
    put32(&mut b, 1); // streams
    put32(&mut b, 0); // suggested buffer
    put32(&mut b, h.width);
    put32(&mut b, h.height);
    for _ in 0..4 {
        put32(&mut b, 0); // reserved
    }
    put(&mut b, b"LIST");
    put32(&mut b, 116); // strl: "strl" + strh + strf
    put(&mut b, b"strl");
    put(&mut b, b"strh");
    put32(&mut b, 56);
    put(&mut b, b"vids");
    put(&mut b, b"MJPG");
    put32(&mut b, 0); // flags
    put16(&mut b, 0); // priority
    put16(&mut b, 0); // language
    put32(&mut b, 0); // initial frames
    put32(&mut b, 1); // scale
    put32(&mut b, h.fps); // rate
    put32(&mut b, 0); // start
    let strh_length_at = 140;
    put32(&mut b, 0); // dwLength, patched
    put32(&mut b, 0); // suggested buffer
    put32(&mut b, 10_000); // quality
    put32(&mut b, 0); // sample size
    put16(&mut b, 0); // rcFrame
    put16(&mut b, 0);
    put16(&mut b, h.width as u16);
    put16(&mut b, h.height as u16);
    put(&mut b, b"strf");
    put32(&mut b, 40); // BITMAPINFOHEADER
    put32(&mut b, 40); // biSize
    put32(&mut b, h.width as i32 as u32);
    put32(&mut b, h.height as i32 as u32);
    put16(&mut b, 1); // planes
    put16(&mut b, 24); // bits
    put(&mut b, b"MJPG"); // biCompression
    put32(&mut b, h.width * h.height * 3); // biSizeImage
    for _ in 0..4 {
        put32(&mut b, 0); // X/Y pels, colors used, colors important
    }
    put(&mut b, b"dmlh");
    put32(&mut b, 248); // dwTotalFrames + 61 reserved DWORDs
    let dmlh_frames_at = b.len();
    put32(&mut b, 0); // dwTotalFrames, patched
    for _ in 0..61 {
        put32(&mut b, 0);
    }
    debug_assert_eq!(b.len(), 468);
    put(&mut b, b"LIST");
    let movi_size_at = 472;
    put32(&mut b, 0); // patched
    put(&mut b, b"movi");
    file.write_all(&b)?;
    Ok([riff_size_at, movi_size_at, avih_frames_at, strh_length_at, dmlh_frames_at])
}

/// Seeks to each `(offset, value)` pair and patches it in, then flushes: the size/length fields that make the
/// file a valid AVI. A failure anywhere leaves the file unfinalized, so the caller reports it instead of claiming
/// the frames were saved.
fn patch_sizes(file: &mut File, patches: &[(u64, u32)]) -> std::io::Result<()> {
    for &(at, value) in patches {
        file.seek(SeekFrom::Start(at))?;
        file.write_all(&value.to_le_bytes())?;
    }
    file.flush()
}

/// Starts a `RIFF/AVIX` segment: a `movi` LIST for the next frames. Returns the absolute offsets of the
/// segment's RIFF size and `movi` size fields.
fn write_segment_header(file: &mut File) -> std::io::Result<[usize; 2]> {
    let start = file.stream_position()? as usize;
    let mut b = Vec::with_capacity(24);
    put(&mut b, b"RIFF");
    put32(&mut b, 0);
    put(&mut b, b"AVIX");
    put(&mut b, b"LIST");
    put32(&mut b, 0);
    put(&mut b, b"movi");
    file.write_all(&b)?;
    Ok([start + 4, start + 16])
}

/// Writes a segment's `idx1` and patches its RIFF and `movi` sizes, then leaves the position at the segment's
/// end. The index offsets are relative to the segment's `movi` fourcc, as AVI 1.0 requires.
fn finish_segment(
    file: &mut File,
    index: &[u32],
    segment_start: u64,
    riff_size_at: usize,
    movi_size_at: usize,
) -> std::io::Result<u64> {
    let mut idx = Vec::with_capacity(8 + index.len() * 16);
    put(&mut idx, b"idx1");
    put32(&mut idx, (index.len() * 16) as u32);
    let mut offset = 4u32;
    for size in index {
        put(&mut idx, b"00dc");
        put32(&mut idx, AVIIF_KEYFRAME);
        put32(&mut idx, offset);
        put32(&mut idx, *size);
        offset += 8 + size + (size & 1);
    }
    file.write_all(&idx)?;
    let end = file.stream_position()?;
    let movi_end = end - idx.len() as u64;
    // The movi LIST's size counts the 'movi' fourcc plus every chunk (movi_data_at = movi_size_at + 4).
    let patches = [
        (movi_size_at as u64, (movi_end - (movi_size_at + 4) as u64) as u32),
        (riff_size_at as u64, (end - segment_start - 8) as u32),
    ];
    patch_sizes(file, &patches)?;
    file.seek(SeekFrom::Start(end))?;
    Ok(end)
}

fn writer_with_limit(
    mut file: File,
    header: Header,
    rx: Receiver<Vec<u8>>,
    segment_bytes: u64,
    max_bytes: u64,
    truncated: &AtomicBool,
) -> Result<DvrSummary, String> {
    let [mut riff_size_at, mut movi_size_at, avih_frames_at, strh_length_at, dmlh_frames_at] =
        write_header(&mut file, &header).expect("the DVR header write failed");
    let mut segment_start = 0u64;
    let mut segment_frames = 0u64;
    let mut total_frames = 0u64;
    let mut first_frames = None;
    let mut index: Vec<u32> = Vec::new();
    let mut data_end = file.stream_position().expect("the DVR position failed");
    while let Ok(jpeg) = rx.recv() {
        let size = jpeg.len() as u32;
        let chunk = 8 + u64::from(size) + u64::from(size & 1);
        // The final file includes this frame plus one more 16-byte index entry; stop before any field would wrap.
        let index_cost = 8 + 16 * (segment_frames + 1);
        if data_end + chunk + index_cost > max_bytes {
            truncated.store(true, Ordering::Relaxed);
            break;
        }
        if segment_frames > 0 && data_end + chunk + index_cost > segment_start + segment_bytes {
            data_end = finish_segment(&mut file, &index, segment_start, riff_size_at, movi_size_at)
                .expect("the DVR segment finalize failed");
            first_frames.get_or_insert(segment_frames);
            segment_start = data_end;
            [riff_size_at, movi_size_at] = write_segment_header(&mut file).expect("the DVR segment header failed");
            data_end = file.stream_position().expect("the DVR position failed");
            segment_frames = 0;
            index.clear();
        }
        file.write_all(b"00dc").and_then(|_| file.write_all(&size.to_le_bytes())).and_then(|_| file.write_all(&jpeg)).expect("the DVR frame write failed");
        if jpeg.len() % 2 == 1 {
            file.write_all(&[0u8]).expect("the DVR pad write failed");
        }
        data_end += chunk;
        index.push(size);
        segment_frames += 1;
        total_frames += 1;
    }
    // The sender is gone: the queue is drained, finalize the last segment. The index write and the position
    // read panic into the join's Err on failure; the patching returns its first failure to `stop()`.
    let end = finish_segment(&mut file, &index, segment_start, riff_size_at, movi_size_at)
        .expect("the DVR segment finalize failed");
    let first_frames = first_frames.unwrap_or(total_frames);
    let patches = [
        (avih_frames_at as u64, first_frames as u32),
        (strh_length_at as u64, first_frames as u32),
        (dmlh_frames_at as u64, total_frames as u32),
    ];
    patch_sizes(&mut file, &patches).map_err(|e| format!("the DVR finalize failed: {e}"))?;
    Ok(DvrSummary { frames: total_frames, dropped_frames: 0, bytes: end, truncated: truncated.load(Ordering::Relaxed) })
}

/// A recording in progress. `stop` finalizes; dropping without `stop` also finalizes (the writer thread joins).
pub struct DvrWriter {
    tx: Option<SyncSender<Vec<u8>>>,
    handle: Option<JoinHandle<Result<DvrSummary, String>>>,
    dropped: std::sync::Arc<AtomicU64>,
    truncated: std::sync::Arc<AtomicBool>,
    path: PathBuf,
}

impl DvrWriter {
    /// Starts a recording at `fps` frames a second with `width` × `height` pixels; every `write_frame` must
    /// hand JPEG bytes of that size (Godot's encoder does). Creates the file's directory; fails if the file exists
    /// (a recording never overwrites another).
    pub fn start(path: PathBuf, fps: u32, width: u32, height: u32) -> Result<DvrWriter, String> {
        if fps == 0 {
            return Err("fps must be greater than 0".into());
        }
        if width == 0 || height == 0 {
            return Err("width and height must be greater than 0".into());
        }
        if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
            return Err(format!("width and height must be at most {} (the AVI frame rectangle is 16-bit)", u16::MAX));
        }
        if u64::from(width) * u64::from(height) * 3 > u64::from(u32::MAX) {
            return Err(format!("{width}x{height} is too large for a 32-bit AVI frame size"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let file = File::create_new(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                format!("{} already exists: a recording never overwrites another", path.display())
            }
            _ => format!("{}: {e}", path.display()),
        })?;
        let header = Header { microsec_per_frame: 1_000_000 / fps, fps, width, height };
        let (tx, rx) = sync_channel(FRAME_CHANNEL_CAPACITY);
        let dropped = std::sync::Arc::new(AtomicU64::new(0));
        let dropped_in_thread = dropped.clone();
        let truncated = std::sync::Arc::new(AtomicBool::new(false));
        let truncated_in_thread = truncated.clone();
        let handle = std::thread::Builder::new()
            .name("ofs-dvr".into())
            .spawn(move || {
                writer_with_limit(file, header, rx, AVI_SEGMENT_BYTES, MAX_DVR_BYTES, &truncated_in_thread).map(|mut summary| {
                    summary.dropped_frames += dropped_in_thread.load(Ordering::Relaxed);
                    summary
                })
            })
            .map_err(|e| {
                let _ = std::fs::remove_file(&path);
                e.to_string()
            })?;
        Ok(DvrWriter { tx: Some(tx), handle: Some(handle), dropped, truncated, path })
    }

    /// The recording's file, for toasts.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// True once the writer hit the AVI size limit and stopped accepting frames.
    pub fn truncated(&self) -> bool {
        self.truncated.load(Ordering::Relaxed)
    }

    /// Hands one frame to the writer. JPEG bytes of the declared size. When the writer thread is saturated the
    /// frame is dropped and counted. Returns whether the frame was accepted (the audio log follows the video).
    pub fn write_frame(&self, jpeg: Vec<u8>) -> bool {
        let Some(tx) = &self.tx else { return false };
        if self.truncated() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let accepted = tx.try_send(jpeg).is_ok();
        if !accepted {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        accepted
    }

    /// Stops: drops the sender (the worker drains the queue and patches the headers) and joins. A summary means
    /// the file was finalized; an Err means it was not (the pilot's toast must be the failure, not "frames saved").
    pub fn stop(mut self) -> Result<DvrSummary, String> {
        self.tx.take();
        let handle = self.handle.take().ok_or_else(|| "the recorder already stopped".to_string())?;
        match handle.join() {
            Ok(summary) => summary,
            Err(_) => Err("the DVR writer panicked".to_string()),
        }
    }
}

impl Drop for DvrWriter {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_stops_at_the_total_size_cap_and_finalizes_a_valid_file() {
        let dir = std::env::temp_dir().join(format!("ofs-dvr-limit-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("limit.avi");
        let file = File::create_new(&path).unwrap();
        let header = Header { microsec_per_frame: 20_000, fps: 50, width: 4, height: 2 };
        let (tx, rx) = sync_channel(4);
        let truncated = std::sync::atomic::AtomicBool::new(false);
        let handle = std::thread::spawn(move || writer_with_limit(file, header, rx, 1024, 1024, &truncated));
        for _ in 0..50 {
            let _ = tx.send(vec![0xFF; 100]);
        }
        drop(tx);
        let summary = handle.join().unwrap().unwrap();

        assert!(summary.truncated, "the writer reports the cap");
        assert!(summary.frames > 0 && summary.frames < 50, "some frames, not all: {}", summary.frames);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() as u64 <= 1024, "the file stays within the cap: {}", bytes.len());
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64, bytes.len() as u64 - 8, "RIFF size");
        let idx = bytes.len() - (8 + 16 * summary.frames as usize);
        assert_eq!(&bytes[idx..idx + 4], b"idx1", "the index is the last block");
        assert_eq!(
            u32::from_le_bytes(bytes[idx + 4..idx + 8].try_into().unwrap()) as u64 / 16,
            summary.frames,
            "one index entry per frame"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_recording_rolls_to_avix_segments_and_keeps_every_frame() {
        let dir = std::env::temp_dir().join(format!("ofs-dvr-roll-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roll.avi");
        let file = File::create_new(&path).unwrap();
        let header = Header { microsec_per_frame: 20_000, fps: 50, width: 4, height: 2 };
        let (tx, rx) = sync_channel(64);
        let truncated = std::sync::atomic::AtomicBool::new(false);
        let handle = std::thread::spawn(move || writer_with_limit(file, header, rx, 1024, MAX_DVR_BYTES, &truncated));
        for i in 0..50u8 {
            tx.send(vec![i; 100]).unwrap();
        }
        tx.send(vec![0xFF; 2048]).unwrap(); // larger than a whole segment
        drop(tx);
        let summary = handle.join().unwrap().unwrap();

        assert_eq!(summary.frames, 51, "every frame is kept, including the oversized one");
        assert!(!summary.truncated);

        let bytes = std::fs::read(&path).unwrap();
        let mut pos = 0usize;
        let mut segments = 0usize;
        let mut total = 0usize;
        while pos + 12 <= bytes.len() {
            assert_eq!(&bytes[pos..pos + 4], b"RIFF", "segment {segments}");
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let form = &bytes[pos + 8..pos + 12];
            assert_eq!(form, if segments == 0 { b"AVI " } else { b"AVIX" }, "segment {segments} form");
            let end = pos + 8 + size;
            assert!(end <= bytes.len(), "segment {segments} fits");
            let segment = &bytes[pos..end];
            let idx = segment.windows(4).position(|w| w == b"idx1").expect("each segment has an index");
            total += u32::from_le_bytes(segment[idx + 4..idx + 8].try_into().unwrap()) as usize / 16;
            segments += 1;
            pos = end;
        }
        assert!(segments >= 2, "the tiny limit rolls at least once");
        assert_eq!(total, 51, "the segments' indexes add up");
        let dmlh = bytes.windows(4).position(|w| w == b"dmlh").expect("the OpenDML total");
        assert_eq!(u32::from_le_bytes(bytes[dmlh + 8..dmlh + 12].try_into().unwrap()), 51, "dmlh total frames");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_start_with_a_bad_size_or_rate_creates_no_file() {
        let dir = std::env::temp_dir().join(format!("ofs-dvr-start-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (fps, width, height) in [(50, 0, 2), (50, 4, 0), (0, 4, 2), (50, 65_536, 2), (50, 4, 65_536), (50, 65_535, 65_535)] {
            let path = dir.join(format!("bad-{fps}-{width}-{height}.avi"));
            let _ = std::fs::remove_file(&path);
            assert!(DvrWriter::start(path.clone(), fps, width, height).is_err(), "{fps} {width} {height}");
            assert!(!path.exists(), "no file for {fps} {width} {height}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_frame_after_the_cap_is_refused_and_counted() {
        let (tx, _rx) = sync_channel(1);
        let writer = DvrWriter {
            tx: Some(tx),
            handle: None,
            dropped: std::sync::Arc::new(AtomicU64::new(0)),
            truncated: std::sync::Arc::new(AtomicBool::new(true)),
            path: PathBuf::from("unused.avi"),
        };
        assert!(!writer.write_frame(vec![0xFF]));
        assert_eq!(writer.dropped.load(Ordering::Relaxed), 1, "the refused frame is counted as dropped");
    }
}
