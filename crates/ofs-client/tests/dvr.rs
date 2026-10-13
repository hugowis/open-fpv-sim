//! The DVR container's structure, checked by parsing what it wrote: a minimal RIFF/AVI walker.
use ofs_client::dvr::DvrWriter;
use std::path::Path;

/// One `idx1` entry: (flags, offset, size).
type IndexEntry = (u32, u32, u32);

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn tag(&mut self) -> [u8; 4] {
        let t = self.bytes[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        t
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
    fn expect(&mut self, t: &[u8; 4], what: &str) {
        assert!(self.pos + 4 <= self.bytes.len(), "truncated at {what}");
        assert_eq!(&self.tag(), t, "{what}");
    }
}

/// Walks RIFF down to the `movi` payloads and returns (frame sizes, index entries, total length).
fn parse(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<IndexEntry>, usize) {
    let mut r = Reader { bytes, pos: 0 };
    r.expect(b"RIFF", "riff");
    let riff_size = r.u32();
    r.expect(b"AVI ", "avi");
    r.expect(b"LIST", "hdrl list");
    r.u32();
    r.expect(b"hdrl", "hdrl");
    r.expect(b"avih", "avih");
    assert_eq!(r.u32(), 56, "avih size");
    let _us_per_frame = r.u32();
    r.skip(8); // max bytes, padding
    let flags = r.u32();
    assert_eq!(flags & 0x10, 0x10, "AVIF_HASINDEX");
    r.skip(4); // total frames (patched)
    r.skip(4); // initial frames
    assert_eq!(r.u32(), 1, "one stream");
    r.skip(4 + 4 + 4 + 16); // suggested buffer, width, height, reserved
    r.expect(b"LIST", "strl list");
    r.u32();
    r.expect(b"strl", "strl");
    r.expect(b"strh", "strh");
    assert_eq!(r.u32(), 56, "strh size");
    r.expect(b"vids", "stream type");
    r.expect(b"MJPG", "handler");
    r.skip(4 + 2 + 2 + 4); // flags, priority, language, initial frames
    let scale = r.u32();
    let rate = r.u32();
    assert_eq!((scale, rate), (1, 50), "50 fps");
    r.skip(4); // start
    let length = r.u32();
    r.skip(4 + 4 + 4 + 8); // suggested, quality, sample size, rc frame
    r.expect(b"strf", "strf");
    assert_eq!(r.u32(), 40, "bitmapinfoheader size");
    assert_eq!(r.u32(), 40, "biSize");
    let width = r.u32() as i32;
    let height = r.u32() as i32;
    assert_eq!((width, height), (4, 2));
    r.skip(4); // planes, bits
    assert_eq!(&bytes[r.pos..r.pos + 4], b"MJPG", "biCompression");
    r.pos = bytes.windows(4).position(|w| w == b"movi").unwrap() + 4;
    let mut payloads = Vec::new();
    loop {
        if &bytes[r.pos..r.pos + 4] == b"idx1" {
            break;
        }
        let tag = r.tag();
        assert_eq!(&tag, b"00dc", "every frame is a 00dc chunk, got {tag:?}");
        let size = r.u32() as usize;
        payloads.push(bytes[r.pos..r.pos + size].to_vec());
        r.skip(size + (size & 1)); // the pad byte on odd sizes
    }
    r.expect(b"idx1", "index");
    let index_size = r.u32() as usize;
    let mut index = Vec::new();
    for i in 0..index_size / 16 {
        let _tag = r.tag();
        let flags = r.u32();
        let offset = r.u32();
        let size = r.u32();
        assert_eq!(flags, 0x10, "frame {i}: AVIIF_KEYFRAME");
        assert_eq!(offset, 4 + payloads[..i].iter().map(|p| p.len() + 8 + (p.len() & 1)).sum::<usize>() as u32, "frame {i} offset");
        assert_eq!(size as usize, payloads[i].len(), "frame {i} size");
        index.push((flags, offset, size));
    }
    assert_eq!(length as usize, payloads.len(), "strh length matches");
    assert_eq!(riff_size as usize + 8, bytes.len(), "the outer RIFF size is patched");
    (payloads, index, bytes.len())
}

fn write_two_frames(path: &Path) -> ofs_client::dvr::DvrSummary {
    let writer = DvrWriter::start(path.to_path_buf(), 50, 4, 2).unwrap();
    writer.write_frame(b"AAA".to_vec());
    writer.write_frame(b"BBBBB".to_vec());
    writer.stop().unwrap()
}

#[test]
fn two_frames_round_trip_with_padding_and_a_patched_index() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-test-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.avi");
    let summary = write_two_frames(&path);
    assert_eq!(summary.frames, 2);
    assert_eq!(summary.dropped_frames, 0);
    let bytes = std::fs::read(&path).unwrap();
    let (payloads, index, len) = parse(&bytes);
    assert_eq!(payloads, vec![b"AAA".to_vec(), b"BBBBB".to_vec()]);
    assert_eq!(index.len(), 2);
    assert_eq!(summary.bytes as usize, len);
    assert!(bytes.windows(4).filter(|w| *w == b"00dc").count() >= 4, "movi chunks + index entries");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn zero_frames_writes_a_valid_empty_avi() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-empty-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.avi");
    let writer = DvrWriter::start(path.clone(), 50, 4, 2).unwrap();
    let summary = writer.stop().unwrap();
    assert_eq!(summary.frames, 0);
    let bytes = std::fs::read(&path).unwrap();
    let (payloads, _, _) = parse(&bytes);
    assert!(payloads.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_directory_is_created_and_a_bad_frame_size_does_not_corrupt() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-mkdir-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("nested/deep/rec.avi");
    let writer = DvrWriter::start(path.clone(), 50, 4, 2).unwrap();
    writer.write_frame(vec![0xFF; 40]); // whatever bytes: the muxer does not validate JPEG content
    let summary = writer.stop().unwrap();
    assert_eq!(summary.frames, 1);
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_start_on_an_existing_file_fails_and_leaves_it_untouched() {
    // m3e debt: File::create truncated a same-name recording (a clock without millisecond resolution, a scripted
    // caller) instead of the start failing.
    let dir = std::env::temp_dir().join(format!("ofs-dvr-exists-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rec.avi");
    write_two_frames(&path);
    let before = std::fs::read(&path).unwrap();
    let err = DvrWriter::start(path.clone(), 50, 4, 2).err().expect("a start on an existing file fails");
    assert!(err.contains(&path.display().to_string()), "the error names the file: {err}");
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "the earlier recording is untouched");
    std::fs::remove_dir_all(&dir).ok();
}
