//! The DVR export: an image sequence copied out of the AVI, and the ffmpeg argument list. No test needs ffmpeg.
use ofs_audio::AudioInput;
use ofs_client::dvr::DvrWriter;
use ofs_client::dvr_audio::AudioLog;
use ofs_client::dvr_export::{export_frames, export_mp4, ffmpeg_args, find_ffmpeg_in, Exporter};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-export-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_avi(path: &Path, frames: &[&[u8]]) {
    let writer = DvrWriter::start(path.to_path_buf(), 50, 4, 2).unwrap();
    for f in frames {
        writer.write_frame(f.to_vec());
    }
    writer.stop().unwrap();
}

/// An ffmpeg stand-in that takes its last argument as the output path, writes `PARTIAL` into it, prints
/// `synthetic ffmpeg failure` to stderr and exits 1: a conversion that dies halfway through its output.
/// Windows runs no script under a `.exe` name, so there the flavor is a `.cmd` batch (the name never reaches
/// the code under test: the path is handed straight to `export_mp4`).
fn failing_ffmpeg(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join("ffmpeg.cmd");
        std::fs::write(
            &path,
            "@echo off\r\n\
             set \"out=\"\r\n\
             :loop\r\n\
             if \"%~1\"==\"\" goto done\r\n\
             set \"out=%~1\"\r\n\
             shift\r\n\
             goto loop\r\n\
             :done\r\n\
             echo PARTIAL> \"%out%\"\r\n\
             echo synthetic ffmpeg failure 1>&2\r\n\
             exit /b 1\r\n",
        )
        .unwrap();
        path
    }
    #[cfg(unix)]
    {
        let path = dir.join("ffmpeg");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             for out in \"$@\"; do :; done\n\
             printf 'PARTIAL\\n' > \"$out\"\n\
             echo synthetic ffmpeg failure >&2\n\
             exit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[test]
fn frames_are_copied_verbatim_and_numbered_from_one() {
    let dir = fresh_dir("verbatim");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA", b"BBBBB"]);
    let (out, count) = export_frames(&avi).unwrap();
    assert_eq!(out, dir.join("rec_frames"));
    assert_eq!(count, 2);
    assert_eq!(std::fs::read(out.join("frame_000001.jpg")).unwrap(), b"AAA");
    assert_eq!(std::fs::read(out.join("frame_000002.jpg")).unwrap(), b"BBBBB");
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2);
}

#[test]
fn an_unfinalized_avi_still_exports() {
    let dir = fresh_dir("unfinalized");
    let avi = dir.join("rec.avi");
    let frame = [7u8; 10]; // chunks of 18 bytes after the 224-byte header (movi fourcc at 220)
    write_avi(&avi, &[&frame, &frame, &frame]);
    let mut bytes = std::fs::read(&avi).unwrap();
    for at in [4, 216, 48, 140] {
        bytes[at..at + 4].copy_from_slice(&[0; 4]);
    }
    let movi_end = 224 + 3 * 18;
    let crashed = dir.join("crashed.avi");
    std::fs::write(&crashed, &bytes[..movi_end]).unwrap(); // no idx1
    assert_eq!(export_frames(&crashed).unwrap().1, 3);

    let cut = dir.join("cut.avi");
    std::fs::write(&cut, &bytes[..movi_end - 5]).unwrap(); // the third chunk is partial
    let (out, count) = export_frames(&cut).unwrap();
    assert_eq!(count, 2);
    assert!(!out.join("frame_000003.jpg").exists());

    let indexed = dir.join("indexed.avi"); // header still zero, idx1 present
    std::fs::write(&indexed, &bytes).unwrap();
    assert_eq!(export_frames(&indexed).unwrap().1, 3);
}

#[test]
fn an_existing_frames_directory_is_an_error_and_untouched() {
    let dir = fresh_dir("exists");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    let out = dir.join("rec_frames");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("keep.txt"), b"mine").unwrap();
    let err = export_frames(&avi).unwrap_err();
    assert!(err.contains("rec_frames"), "{err}");
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
    assert_eq!(std::fs::read(out.join("keep.txt")).unwrap(), b"mine");
}

#[test]
fn a_file_that_is_not_an_avi_is_an_error() {
    let dir = fresh_dir("notavi");
    let junk = dir.join("junk.avi");
    std::fs::write(&junk, (0..300u32).map(|i| (i * 7 % 251) as u8).collect::<Vec<_>>()).unwrap();
    assert!(export_frames(&junk).is_err());
    assert!(!dir.join("junk_frames").exists());
}

#[test]
fn ffmpeg_args_are_the_documented_list() {
    let args = ffmpeg_args(Path::new("in.avi"), None, Path::new("out.mp4"));
    let s: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(s[0], "-nostdin");
    assert!(!s.contains(&"-c:a".to_string()) && !s.contains(&"-shortest".to_string()), "no audio arguments without a pcm");
    assert_eq!(s.iter().filter(|a| *a == "-i").count(), 1);
    assert!(s.contains(&"-n".to_string()));
    assert_eq!(s[s.iter().position(|a| a == "-i").unwrap() + 1], "in.avi");
    assert_eq!(s[s.iter().position(|a| a == "-vf").unwrap() + 1], "pad=ceil(iw/2)*2:ceil(ih/2)*2");
    assert_eq!(s[s.iter().position(|a| a == "-c:v").unwrap() + 1], "libx264");
    assert_eq!(s[s.iter().position(|a| a == "-pix_fmt").unwrap() + 1], "yuv420p");
    assert_eq!(s[s.iter().position(|a| a == "-crf").unwrap() + 1], "18");
    assert_eq!(s[s.iter().position(|a| a == "-movflags").unwrap() + 1], "+faststart");
    assert_eq!(s.last().unwrap(), "out.mp4");
}

#[test]
fn ffmpeg_args_with_audio_adds_the_pcm_input_and_aac() {
    let args = ffmpeg_args(Path::new("in.avi"), Some(Path::new("a.pcm")), Path::new("out.mp4"));
    let s: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let silent: Vec<String> = ffmpeg_args(Path::new("in.avi"), None, Path::new("out.mp4"))
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let at = |name: &str| s.iter().position(|a| a == name).unwrap();
    let inputs: Vec<usize> = s.iter().enumerate().filter(|(_, a)| *a == "-i").map(|(i, _)| i).collect();
    assert_eq!(inputs.len(), 2);
    assert_eq!(s[inputs[0] + 1], "in.avi");
    assert_eq!(s[inputs[1] + 1], "a.pcm");
    assert_eq!(&s[inputs[1] - 6..inputs[1]], ["-f", "s16le", "-ar", "44100", "-ac", "1"]);
    assert!(inputs[0] < inputs[1] - 6, "the pcm input follows the AVI input");
    assert_eq!(s[at("-c:a") + 1], "aac");
    assert_eq!(s[at("-b:a") + 1], "128k");
    for video in ["-vf", "-c:v", "-pix_fmt", "-crf", "-movflags"] {
        assert_eq!(s[at(video) + 1], silent[silent.iter().position(|a| a == video).unwrap() + 1], "{video}");
    }
    assert!(at("-shortest") > at("-c:a") && at("-shortest") < s.len() - 1);
    assert_eq!(s.last().unwrap(), "out.mp4");
}

fn write_sidecar(path: &Path, frames: usize) {
    let mut log = AudioLog::create(path).unwrap();
    let input = AudioInput { motor_rpm: vec![9000.0; 4], motor_cmd: vec![0.5; 4], active: true, ..AudioInput::default() };
    for i in 0..frames {
        log.frame(Some((&input, i as f64 / 50.0, 3.0)));
    }
    log.finish().unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn a_bad_sidecar_fails_before_ffmpeg_and_leaves_no_temp_file() {
    let dir = fresh_dir("badsidecar");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    std::fs::write(dir.join("rec.audio"), b"not-a-sidecar 9
F 0 0 0 0 0
").unwrap();
    let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
    assert!(err.contains("rec.audio") && !err.contains("no-such-ffmpeg"), "{err}");
    assert_eq!(names(&dir), ["rec.audio", "rec.avi"]);
    // An empty sidecar and a half-written header are bad headers like any other.
    for bytes in [&b""[..], b"ofs-dvr-au"] {
        std::fs::write(dir.join("rec.audio"), bytes).unwrap();
        let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
        assert!(err.contains("rec.audio"), "{err}");
        assert_eq!(names(&dir), ["rec.audio", "rec.avi"]);
    }
}

#[test]
fn a_missing_ffmpeg_with_a_good_sidecar_leaves_no_temp_file() {
    let dir = fresh_dir("goodsidecar");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    write_sidecar(&dir.join("rec.audio"), 3);
    let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
    assert!(err.contains("no-such-ffmpeg"), "{err}");
    assert_eq!(names(&dir), ["rec.audio", "rec.avi"]);
}

#[test]
fn a_sidecar_without_frames_is_a_silent_export_not_an_error_about_audio() {
    let dir = fresh_dir("noaudioframes");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    write_sidecar(&dir.join("rec.audio"), 0);
    let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
    assert!(err.contains("no-such-ffmpeg"), "{err}");
    assert_eq!(names(&dir), ["rec.audio", "rec.avi"]);
}

#[test]
fn an_existing_mp4_is_refused_before_the_sidecar_is_read() {
    let dir = fresh_dir("mp4firstaudio");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    std::fs::write(dir.join("rec.audio"), b"garbage").unwrap();
    std::fs::write(dir.join("rec.mp4"), b"mine").unwrap();
    let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
    assert!(err.contains("rec.mp4") && err.contains("already exists"), "{err}");
}

#[test]
fn export_frames_ignores_the_sidecar() {
    let dir = fresh_dir("framesaudio");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA", b"BBBBB"]);
    write_sidecar(&dir.join("rec.audio"), 2);
    let (out, count) = export_frames(&avi).unwrap();
    assert_eq!(count, 2);
    assert_eq!(std::fs::read(out.join("frame_000002.jpg")).unwrap(), b"BBBBB");
    assert_eq!(names(&dir), ["rec.audio", "rec.avi", "rec_frames"]);
}

#[test]
fn find_ffmpeg_in_finds_the_first_match_and_none_when_absent() {
    let dir = fresh_dir("find");
    let empty = dir.join("empty");
    let first = dir.join("first");
    let second = dir.join("second");
    for d in [&empty, &first, &second] {
        std::fs::create_dir_all(d).unwrap();
    }
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::fs::write(first.join(name), b"").unwrap();
    std::fs::write(second.join(name), b"").unwrap();
    let path = std::env::join_paths([&empty, &first, &second]).unwrap();
    assert_eq!(find_ffmpeg_in(&path), Some(first.join(name)));
    let none: OsString = std::env::join_paths([&empty]).unwrap();
    assert_eq!(find_ffmpeg_in(&none), None);
}

#[test]
fn export_mp4_with_a_missing_program_is_an_error_and_leaves_the_avi() {
    let dir = fresh_dir("noprog");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    let before = std::fs::read(&avi).unwrap();
    assert!(export_mp4(Path::new("no-such-ffmpeg"), &avi).is_err());
    assert_eq!(std::fs::read(&avi).unwrap(), before);
    assert!(!dir.join("rec.mp4").exists());
}

#[test]
fn export_mp4_refuses_an_existing_mp4() {
    let dir = fresh_dir("mp4exists");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    std::fs::write(dir.join("rec.mp4"), b"mine").unwrap();
    let err = export_mp4(Path::new("no-such-ffmpeg"), &avi).unwrap_err();
    assert!(err.contains("rec.mp4") && err.contains("already exists"), "{err}");
    assert_eq!(std::fs::read(dir.join("rec.mp4")).unwrap(), b"mine");
}

#[test]
fn a_failed_mp4_export_removes_its_partial_so_a_retry_runs_again() {
    let dir = fresh_dir("mp4retry");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA"]);
    let fake = failing_ffmpeg(&dir);
    let err = export_mp4(&fake, &avi).unwrap_err();
    assert!(err.contains("synthetic ffmpeg failure"), "{err}");
    assert!(!dir.join("rec.mp4").exists(), "the partial the failed run wrote is removed");
    let retry = export_mp4(&fake, &avi).unwrap_err();
    assert!(!retry.contains("already exists"), "the retry is not refused on the dead partial: {retry}");
    assert!(retry.contains("synthetic ffmpeg failure"), "the retry ran the conversion again: {retry}");
}

#[test]
fn an_avi_without_frames_is_an_error_and_no_directory_is_made() {
    let dir = fresh_dir("noframes");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[]);
    let err = export_frames(&avi).unwrap_err();
    assert!(err.contains("holds no frames"), "{err}");
    assert!(!dir.join("rec_frames").exists());
}

#[test]
fn the_exporter_reports_a_frames_job_and_a_missing_ffmpeg() {
    let dir = fresh_dir("exporter");
    let avi = dir.join("rec.avi");
    write_avi(&avi, &[b"AAA", b"BBB"]);
    let exporter = Exporter::new();
    exporter.spawn_with(None, avi.clone(), true, true);
    let mut done = Vec::new();
    let start = Instant::now();
    while done.len() < 2 && start.elapsed() < Duration::from_secs(5) {
        done.extend(exporter.poll());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(done.len(), 2, "two results within 5 s");
    let frames = done.iter().find(|d| d.kind == "frames").unwrap();
    assert_eq!(frames.avi, avi);
    assert_eq!(frames.result, Ok(dir.join("rec_frames")));
    let mp4 = done.iter().find(|d| d.kind == "mp4").unwrap();
    assert!(mp4.result.as_ref().unwrap_err().contains("ffmpeg"), "{:?}", mp4.result);
    assert!(exporter.poll().is_empty(), "each result is delivered once");
}
