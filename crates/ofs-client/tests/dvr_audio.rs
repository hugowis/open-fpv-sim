//! The DVR audio sidecar: the log's text format, and its replay into PCM. No test needs ffmpeg.
use ofs_audio::AudioInput;
use ofs_client::dvr_audio::{write_pcm, AudioLog, SAMPLES_PER_FRAME};
use std::path::PathBuf;

const CHUNK_BYTES: usize = SAMPLES_PER_FRAME * 2;

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-audio-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pcm(path: &std::path::Path) -> (Vec<u8>, u64) {
    let mut out = Vec::new();
    let frames = write_pcm(path, &mut out).unwrap();
    (out, frames)
}

fn energy(chunk: &[u8]) -> i64 {
    chunk.chunks(2).map(|s| i64::from(i16::from_le_bytes([s[0], s[1]])).abs()).sum()
}

fn idle() -> AudioInput {
    AudioInput { active: true, ..AudioInput::default() }
}

#[test]
fn the_log_round_trips_exactly() {
    let dir = fresh_dir("roundtrip");
    let path = dir.join("rec.audio");
    let input = AudioInput {
        motor_rpm: vec![0.1 + 0.2, 1e-300],
        motor_cmd: vec![0.25, 1.0 / 3.0],
        airspeed_mps: 0.1 + 0.2,
        descent_mps: -1e-300,
        active: true,
    };
    let mut log = AudioLog::create(&path).unwrap();
    log.collision(0.1 + 0.2);
    log.frame(Some((&input, 1e-300, 7.5)));
    log.frame(None);
    log.frame(Some((&idle(), 2.0, 0.0)));
    log.finish().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "ofs-dvr-audio 1");
    assert_eq!(lines.len(), 5);
    let num = |s: &str| s.parse::<f64>().unwrap();

    let c: Vec<&str> = lines[1].split(' ').collect();
    assert_eq!(c[0], "C");
    assert_eq!(c.len(), 2);
    assert_eq!(num(c[1]).to_bits(), (0.1_f64 + 0.2).to_bits());

    let f: Vec<&str> = lines[2].split(' ').collect();
    assert_eq!(f[0], "F");
    assert_eq!(f.len(), 1 + 5 + 4);
    assert_eq!(num(f[1]).to_bits(), 1e-300_f64.to_bits());
    assert_eq!(num(f[2]).to_bits(), 7.5_f64.to_bits());
    assert_eq!(num(f[3]).to_bits(), input.airspeed_mps.to_bits());
    assert_eq!(num(f[4]).to_bits(), input.descent_mps.to_bits());
    assert_eq!(f[5], "2");
    let rest: Vec<u64> = f[6..].iter().map(|s| num(s).to_bits()).collect();
    let want: Vec<u64> = input.motor_rpm.iter().chain(&input.motor_cmd).map(|v| v.to_bits()).collect();
    assert_eq!(rest, want);

    assert_eq!(lines[3], "F NaN 0 0 0 0");
    assert_eq!(lines[4], "F 2 0 0 0 0");
}

fn write_log(path: &std::path::Path, frames: usize) {
    let mut log = AudioLog::create(path).unwrap();
    for i in 0..frames {
        if i == 40 {
            log.collision(0.8);
        }
        let k = i as f64 / frames as f64;
        let input = AudioInput {
            motor_rpm: vec![10_000.0 + 20_000.0 * k; 4],
            motor_cmd: vec![0.1 + 0.8 * k; 4],
            airspeed_mps: 20.0 * k,
            descent_mps: 0.0,
            active: true,
        };
        log.frame(Some((&input, i as f64 * 0.02, 5.0)));
    }
    log.finish().unwrap();
}

#[test]
fn replaying_twice_gives_identical_pcm() {
    let dir = fresh_dir("twice");
    let path = dir.join("rec.audio");
    write_log(&path, 100);
    let (a, frames) = pcm(&path);
    let (b, _) = pcm(&path);
    assert_eq!(frames, 100);
    assert_eq!(a, b);
    assert_eq!(a.len(), 100 * CHUNK_BYTES);
    assert!(a.iter().any(|&x| x != 0));
}

#[test]
fn a_silent_frame_is_silence_and_a_collision_is_a_thud() {
    let dir = fresh_dir("thud");
    let silent = dir.join("silent.audio");
    let mut log = AudioLog::create(&silent).unwrap();
    for _ in 0..20 {
        log.frame(None);
    }
    log.finish().unwrap();
    let (out, frames) = pcm(&silent);
    assert_eq!(frames, 20);
    assert!(out.iter().all(|&x| x == 0));

    let hit = dir.join("hit.audio");
    let mut log = AudioLog::create(&hit).unwrap();
    for i in 0..20 {
        if i == 2 {
            log.collision(0.04);
        }
        log.frame(Some((&idle(), i as f64 * 0.02, 8.0)));
    }
    log.finish().unwrap();
    let (out, _) = pcm(&hit);
    let chunks: Vec<i64> = out.chunks(CHUNK_BYTES).map(energy).collect();
    assert_eq!(chunks[..2], [0, 0]);
    assert!(chunks[2] > 0);
    assert!(chunks[19] < chunks[2]);
}

#[test]
fn a_truncated_tail_is_tolerated_and_a_bad_line_is_an_error() {
    let dir = fresh_dir("bad");
    let path = dir.join("rec.audio");
    write_log(&path, 5);
    let text = std::fs::read_to_string(&path).unwrap();

    let cut = dir.join("cut.audio");
    let tail = text[..text.len() - 1].rfind('\n').unwrap() + 1; // halfway into the last line
    std::fs::write(&cut, &text[..(tail + text.len()) / 2]).unwrap();
    assert_eq!(pcm(&cut).1, 4);

    let mut lines: Vec<&str> = text.lines().collect();
    lines[2] = "F 1 2";
    let bad = dir.join("bad.audio");
    std::fs::write(&bad, lines.join("\n") + "\n").unwrap();
    let err = write_pcm(&bad, &mut Vec::new()).unwrap_err();
    assert!(err.contains("line 3"), "{err}");
    assert!(err.contains("bad.audio"), "{err}");

    let header = dir.join("header.audio");
    std::fs::write(&header, text.replacen("ofs-dvr-audio 1", "ofs-dvr-audio 2", 1)).unwrap();
    assert!(write_pcm(&header, &mut Vec::new()).is_err());

    let mismatch = dir.join("mismatch.audio");
    std::fs::write(&mismatch, "ofs-dvr-audio 1\nF 0 0 0 0 2 1 2 3\nF 0 0 0 0 0\n").unwrap();
    let err = write_pcm(&mismatch, &mut Vec::new()).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
}

#[test]
fn a_collision_waits_for_its_state() {
    let dir = fresh_dir("waits");
    let path = dir.join("rec.audio");
    let mut log = AudioLog::create(&path).unwrap();
    log.collision(1.0);
    log.frame(Some((&idle(), 0.5, 8.0)));
    log.frame(Some((&idle(), 1.5, 8.0)));
    log.finish().unwrap();
    let (out, _) = pcm(&path);
    assert_eq!(energy(&out[..CHUNK_BYTES]), 0);
    assert!(energy(&out[CHUNK_BYTES..]) > 0);
}
