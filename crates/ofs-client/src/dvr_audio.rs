//! The DVR's audio sidecar: what the flight sound reads, logged next to the AVI, and its replay into PCM.
//!
//! Text, LF, every number a Rust `f64` `Display` (shortest form that parses back to the same bits):
//!
//! ```text
//! ofs-dvr-audio 1
//! C <event_time_s>
//! F <state_time_s> <collision_speed_mps> <airspeed_mps> <descent_mps> <n> <rpm_1..rpm_n> <cmd_1..cmd_n>
//! ```
use crate::dvr::DVR_FPS;
use ofs_audio::{Audio, AudioInput, SAMPLE_RATE};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// One video frame of sound: 44100 / 50, exact.
pub const SAMPLES_PER_FRAME: usize = 882;
const _: () = assert!(SAMPLE_RATE % DVR_FPS == 0 && SAMPLES_PER_FRAME == (SAMPLE_RATE / DVR_FPS) as usize);

const HEADER: &str = "ofs-dvr-audio 1";

/// Appends the audio inputs of a recording. A write error is kept and returned by `finish`.
pub struct AudioLog {
    out: BufWriter<File>,
    error: Option<String>,
}

impl AudioLog {
    /// Creates (truncating) the sidecar and writes the header.
    pub fn create(path: &Path) -> Result<AudioLog, String> {
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut log = AudioLog { out: BufWriter::new(file), error: None };
        log.put(format_args!("{HEADER}\n"));
        log.error.take().map_or(Ok(log), Err)
    }

    fn put(&mut self, args: std::fmt::Arguments) {
        if self.error.is_none() {
            if let Err(e) = self.out.write_fmt(args) {
                self.error = Some(format!("writing the audio log: {e}"));
            }
        }
    }

    pub fn collision(&mut self, time_s: f64) {
        self.put(format_args!("C {time_s}\n"));
    }

    /// One video frame. `None` (no telemetry yet) is the silent record.
    pub fn frame(&mut self, input: Option<(&AudioInput, f64, f64)>) {
        let Some((input, time_s, speed_mps)) = input else {
            return self.put(format_args!("F NaN 0 0 0 0\n"));
        };
        let n = input.motor_rpm.len().min(input.motor_cmd.len());
        self.put(format_args!(
            "F {time_s} {speed_mps} {} {} {n}",
            input.airspeed_mps, input.descent_mps
        ));
        for v in input.motor_rpm[..n].iter().chain(&input.motor_cmd[..n]) {
            self.put(format_args!(" {v}"));
        }
        self.put(format_args!("\n"));
    }

    pub fn finish(mut self) -> Result<(), String> {
        if let Err(e) = self.out.flush() {
            self.error.get_or_insert(format!("writing the audio log: {e}"));
        }
        self.error.map_or(Ok(()), Err)
    }
}

enum Record {
    Collision(f64),
    Frame(AudioInput, f64, f64),
}

fn parse_line(line: &str) -> Result<Record, String> {
    let mut parts = line.split_whitespace();
    let num = |s: &str| s.parse::<f64>().map_err(|_| format!("not a number: {s:?}"));
    match parts.next() {
        Some("C") => {
            let t = parts.next().ok_or("C needs a time")?;
            match parts.next() {
                None => Ok(Record::Collision(num(t)?)),
                Some(_) => Err("C has extra fields".into()),
            }
        }
        Some("F") => {
            let rest: Vec<&str> = parts.collect();
            let n: usize = rest.get(4).ok_or("F is too short")?.parse().map_err(|_| "bad motor count")?;
            if rest.len() != 5 + n.checked_mul(2).ok_or("bad motor count")? {
                return Err(format!("F says {n} motors but has {} numbers after the count", rest.len() - 5));
            }
            let nums = rest.iter().enumerate().filter(|&(i, _)| i != 4).map(|(_, s)| num(s)).collect::<Result<Vec<_>, _>>()?;
            let input = AudioInput {
                motor_rpm: nums[4..4 + n].to_vec(),
                motor_cmd: nums[4 + n..].to_vec(),
                airspeed_mps: nums[2],
                descent_mps: nums[3],
                active: true,
            };
            Ok(Record::Frame(input, nums[0], nums[1]))
        }
        _ => Err("unknown record".into()),
    }
}

/// Replays the sidecar through the audio facade and writes mono s16le PCM, one chunk per `F` record.
/// Returns the number of `F` records.
pub fn write_pcm(sidecar: &Path, out: &mut dyn Write) -> Result<u64, String> {
    let name = sidecar.display();
    let mut reader = BufReader::new(File::open(sidecar).map_err(|e| format!("{name}: {e}"))?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| format!("{name}: {e}"))?;
    if line.trim_end() != HEADER {
        return Err(format!("{name}: line 1: expected {HEADER:?}"));
    }
    let mut audio = Audio::new();
    let mut bytes = Vec::with_capacity(SAMPLES_PER_FRAME * 2);
    let mut frames = 0;
    let mut lineno = 1;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|e| format!("{name}: line {}: {e}", lineno + 1))? == 0 {
            return Ok(frames);
        }
        lineno += 1;
        let record = match parse_line(&line) {
            Ok(r) => r,
            // A crash mid-write leaves a last line without its LF.
            Err(_) if !line.ends_with('\n') => return Ok(frames),
            Err(e) => return Err(format!("{name}: line {lineno}: {e}")),
        };
        match record {
            Record::Collision(t) => audio.collision_event(t),
            Record::Frame(input, time_s, speed_mps) => {
                bytes.clear();
                for s in audio.frame(&input, time_s, speed_mps, SAMPLES_PER_FRAME) {
                    bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
                }
                out.write_all(&bytes).map_err(|e| format!("writing the audio: {e}"))?;
                frames += 1;
            }
        }
    }
}
