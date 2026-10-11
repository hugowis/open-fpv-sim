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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

/// Frames in flight to the writer thread before the capture starts dropping: a second's worth.
const FRAME_CHANNEL_CAPACITY: usize = 50;
/// The capture cadence: recordings are tied to received fields.
pub const DVR_FPS: u32 = 50;
const AVIIF_KEYFRAME: u32 = 0x10;

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvrSummary {
    pub frames: u64,
    pub dropped_frames: u64,
    pub bytes: u64,
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

/// Writes the fixed layout. Returns the file offsets of the four fields patched at stop: the outer RIFF size,
/// the `movi` LIST size, `avih.dwTotalFrames` and `strh.dwLength`. (The index's offsets are relative to the
/// position right after the `movi` fourcc, counting the fourcc: the first frame sits at 4.)
fn write_header(file: &mut File, h: &Header) -> std::io::Result<[usize; 4]> {
    let mut b = Vec::with_capacity(224);
    put(&mut b, b"RIFF");
    let riff_size_at = 4;
    put32(&mut b, 0);
    put(&mut b, b"AVI ");
    put(&mut b, b"LIST");
    put32(&mut b, 192); // hdrl: "hdrl" + avih + strl
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
    debug_assert_eq!(b.len(), 212);
    put(&mut b, b"LIST");
    let movi_size_at = 216;
    put32(&mut b, 0); // patched
    put(&mut b, b"movi");
    file.write_all(&b)?;
    Ok([riff_size_at, movi_size_at, avih_frames_at, strh_length_at])
}

/// Seeks to each `(offset, value)` pair and patches it in, then flushes: the four size/length fields that make
/// the file a valid AVI. A failure anywhere leaves the file unfinalized, so the caller reports it instead of
/// claiming the frames were saved.
fn patch_sizes(file: &mut File, patches: [(u64, u32); 4]) -> std::io::Result<()> {
    for (at, value) in patches {
        file.seek(SeekFrom::Start(at))?;
        file.write_all(&value.to_le_bytes())?;
    }
    file.flush()
}

fn writer(mut file: File, header: Header, rx: Receiver<Vec<u8>>) -> Result<DvrSummary, String> {
    let [riff_size_at, movi_size_at, avih_frames_at, strh_length_at] =
        write_header(&mut file, &header).expect("the DVR header write failed");
    let mut frames = 0u64;
    let mut index: Vec<u32> = Vec::new();
    while let Ok(jpeg) = rx.recv() {
        let size = jpeg.len() as u32;
        file.write_all(b"00dc").and_then(|_| file.write_all(&size.to_le_bytes())).and_then(|_| file.write_all(&jpeg)).expect("the DVR frame write failed");
        if jpeg.len() % 2 == 1 {
            file.write_all(&[0u8]).expect("the DVR pad write failed");
        }
        index.push(size);
        frames += 1;
    }
    // The sender is gone: the queue is drained, finalize. The index write and the position read panic into the
    // join's Err on failure; the patching returns its first failure to `stop()` instead of a summary.
    let mut idx = Vec::with_capacity(8 + index.len() * 16);
    put(&mut idx, b"idx1");
    put32(&mut idx, (index.len() * 16) as u32);
    let mut offset = 4u32;
    for size in &index {
        put(&mut idx, b"00dc");
        put32(&mut idx, AVIIF_KEYFRAME);
        put32(&mut idx, offset);
        put32(&mut idx, *size);
        offset += 8 + size + (size & 1);
    }
    file.write_all(&idx).expect("the DVR index write failed");
    let end = file.stream_position().expect("the DVR position failed");
    let movi_end = end - idx.len() as u64;
    // The movi LIST's size counts the 'movi' fourcc plus every chunk (movi_data_at = movi_size_at + 4).
    let patches = [
        (avih_frames_at as u64, frames as u32),
        (strh_length_at as u64, frames as u32),
        (movi_size_at as u64, (movi_end - (movi_size_at + 4) as u64) as u32),
        (riff_size_at as u64, (end - 8) as u32),
    ];
    patch_sizes(&mut file, patches).map_err(|e| format!("the DVR finalize failed: {e}"))?;
    Ok(DvrSummary { frames, dropped_frames: 0, bytes: end })
}

/// A recording in progress. `stop` finalizes; dropping without `stop` also finalizes (the writer thread joins).
pub struct DvrWriter {
    tx: Option<SyncSender<Vec<u8>>>,
    handle: Option<JoinHandle<Result<DvrSummary, String>>>,
    dropped: std::sync::Arc<AtomicU64>,
    path: PathBuf,
}

impl DvrWriter {
    /// Starts a recording at `fps` frames a second with `width` × `height` pixels; every `write_frame` must
    /// hand JPEG bytes of that size (Godot's encoder does). Creates the file's directory.
    pub fn start(path: PathBuf, fps: u32, width: u32, height: u32) -> Result<DvrWriter, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let header = Header { microsec_per_frame: 1_000_000 / fps.max(1), fps, width, height };
        let (tx, rx) = sync_channel(FRAME_CHANNEL_CAPACITY);
        let dropped = std::sync::Arc::new(AtomicU64::new(0));
        let dropped_in_thread = dropped.clone();
        let handle = std::thread::Builder::new()
            .name("ofs-dvr".into())
            .spawn(move || {
                writer(file, header, rx).map(|mut summary| {
                    summary.dropped_frames += dropped_in_thread.load(Ordering::Relaxed);
                    summary
                })
            })
            .map_err(|e| e.to_string())?;
        Ok(DvrWriter { tx: Some(tx), handle: Some(handle), dropped, path })
    }

    /// The recording's file, for toasts.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Hands one frame to the writer. JPEG bytes of the declared size. When the writer thread is saturated the
    /// frame is dropped and counted.
    pub fn write_frame(&self, jpeg: Vec<u8>) {
        let Some(tx) = &self.tx else { return };
        if tx.try_send(jpeg).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
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
