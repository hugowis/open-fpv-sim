//! The session journal: everything that steers a run (stick changes, transmitter, faults, played inputs), stamped
//! with the tick it was applied at, so a failed session can be replayed call for call (the repro bundle, spec §7).
//! Plain text, one entry per line; floats are written in Rust's shortest round-trip form, so they read back exactly.
use std::path::Path;

use crate::inputs::Inputs;
use crate::vehicle::Fault;

/// Entries kept: about an hour of 50 Hz stick changes. Past it the journal stops growing and says so.
pub const MAX_ENTRIES: usize = 250_000;

/// The journal format this build writes and reads; `HEADER` must say the same number (a test holds them together).
const FORMAT_VERSION: u32 = 1;
/// The first line of `journal.txt`.
const HEADER: &str = "# ofs-journal 1";
const TRUNCATED: &str = "# truncated";

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Roll, pitch, yaw, throttle, aux1..4.
    Sticks([f64; 8]),
    Transmitter(bool),
    Fault { fault: Fault, active: bool },
    ScheduleFault { fault: Fault, at_s: f64 },
    ClearFaults,
    PlayInputs(Inputs),
    StopInputs,
    /// A fresh race was asked for, on the course with this index or the current one (the race model applies it at
    /// the next tick).
    ResetRace(Option<usize>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub tick: u64,
    pub action: Action,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    entries: Vec<Entry>,
    truncated: bool,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an entry; past [`MAX_ENTRIES`] it is dropped and the journal is marked truncated.
    pub fn push(&mut self, tick: u64, action: Action) {
        if self.entries.len() >= MAX_ENTRIES {
            self.truncated = true;
            return;
        }
        self.entries.push(Entry { tick, action });
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Writes `journal.txt` (and one `inputs-<n>.csv` per played file) into `dir`, creating it.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut text = format!("{HEADER}\n");
        if self.truncated {
            text.push_str(TRUNCATED);
            text.push('\n');
        }
        let mut files = 0;
        for e in &self.entries {
            text.push_str(&e.tick.to_string());
            text.push(' ');
            match &e.action {
                Action::Sticks(v) => {
                    text.push_str("sticks");
                    for x in v {
                        text.push(' ');
                        text.push_str(&x.to_string());
                    }
                }
                Action::Transmitter(on) => text.push_str(if *on { "tx 1" } else { "tx 0" }),
                Action::Fault { fault, active } => {
                    text.push_str(if *active { "fault on " } else { "fault off " });
                    text.push_str(&fault_tokens(fault));
                }
                Action::ScheduleFault { fault, at_s } => {
                    text.push_str(&format!("schedule {at_s} {}", fault_tokens(fault)));
                }
                Action::ClearFaults => text.push_str("clear"),
                Action::PlayInputs(inputs) => {
                    files += 1;
                    let name = format!("inputs-{files}.csv");
                    std::fs::write(dir.join(&name), inputs.to_csv())?;
                    text.push_str(&format!("play {name}"));
                }
                Action::StopInputs => text.push_str("stop"),
                Action::ResetRace(None) => text.push_str("race_reset"),
                Action::ResetRace(Some(course)) => text.push_str(&format!("race_reset {course}")),
            }
            text.push('\n');
        }
        std::fs::write(dir.join("journal.txt"), text)
    }

    /// Reads what `write` wrote. Every problem is reported with its file and line.
    pub fn read(dir: &Path) -> Result<Journal, String> {
        let path = dir.join("journal.txt");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).find(|(_, l)| !l.is_empty()) {
            Some((_, HEADER)) => {}
            Some((_, found)) if found.starts_with("# ofs-journal ") => {
                let version = found.trim_start_matches("# ofs-journal ");
                return Err(format!("journal.txt: written by format {version}, this build reads format {FORMAT_VERSION}"));
            }
            Some((n, _)) => return Err(format!("journal.txt line {n}: not a journal (expected \"{HEADER}\")")),
            None => return Err(format!("journal.txt: not a journal (expected \"{HEADER}\")")),
        }
        let mut journal = Journal::new();
        let mut problems = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == TRUNCATED {
                journal.truncated = true;
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            match parse_entry(line, dir) {
                Ok(entry) => journal.entries.push(entry),
                Err(why) => problems.push(format!("journal.txt line {n}: {why}")),
            }
        }
        if problems.is_empty() {
            Ok(journal)
        } else {
            Err(problems.join("\n"))
        }
    }
}

/// `motor_failure motor=2 scale=0`: the fault's name and its fields as `key=value`.
fn fault_tokens(f: &Fault) -> String {
    let v3 = |v: &glam::DVec3| format!("{},{},{}", v.x, v.y, v.z);
    let args = match f {
        Fault::RadioLinkLoss => String::new(),
        Fault::RadioLoss { fraction } => format!(" fraction={fraction}"),
        Fault::DownlinkLoss { fraction } => format!(" fraction={fraction}"),
        Fault::BurstLoss { on_s, off_s } => format!(" on_s={on_s} off_s={off_s}"),
        Fault::MotorFailure { motor, scale } => format!(" motor={motor} scale={scale}"),
        Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
            format!(" prop={prop} thrust_scale={thrust_scale} imbalance_kgm={imbalance_kgm}")
        }
        Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
            format!(" cell={cell} r_offset_ohm={r_offset_ohm} v_offset_v={v_offset_v}")
        }
        Fault::GyroFault { bias_radps, noise_scale } => format!(" bias_radps={} noise_scale={noise_scale}", v3(bias_radps)),
        Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
            format!(" freq_mhz={freq_mhz} power_mw={power_mw} position_ned_m={}", v3(position_ned_m))
        }
    };
    format!("{}{args}", f.name())
}

fn number(s: &str) -> Result<f64, String> {
    s.parse::<f64>().map_err(|_| format!("'{s}' is not a number"))
}

fn index(s: &str) -> Result<usize, String> {
    s.parse::<usize>().map_err(|_| format!("'{s}' is not an index"))
}

/// `name key=value...` back into a fault; every expected key must be present and nothing else.
fn parse_fault(name: &str, args: &[&str]) -> Result<Fault, String> {
    let mut pairs = Vec::new();
    for a in args {
        let (k, v) = a.split_once('=').ok_or_else(|| format!("'{a}' is not key=value"))?;
        pairs.push((k, v));
    }
    let mut take = |key: &str| -> Result<&str, String> {
        let i = pairs.iter().position(|(k, _)| *k == key).ok_or_else(|| format!("{name} needs {key}="))?;
        Ok(pairs.remove(i).1)
    };
    let v3 = |s: &str| -> Result<glam::DVec3, String> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 3 {
            return Err(format!("'{s}' is not x,y,z"));
        }
        Ok(glam::DVec3::new(number(parts[0])?, number(parts[1])?, number(parts[2])?))
    };
    let fault = match name {
        "radio_link_loss" => Fault::RadioLinkLoss,
        "radio_loss" => Fault::RadioLoss { fraction: number(take("fraction")?)? },
        "downlink_loss" => {
            let fraction = number(take("fraction")?)?;
            if !(0.0..=1.0).contains(&fraction) {
                return Err(format!("downlink_loss fraction must be in [0, 1], got {fraction}"));
            }
            Fault::DownlinkLoss { fraction }
        }
        "burst_loss" => Fault::BurstLoss { on_s: number(take("on_s")?)?, off_s: number(take("off_s")?)? },
        "motor_failure" => Fault::MotorFailure { motor: index(take("motor")?)?, scale: number(take("scale")?)? },
        "prop_damage" => Fault::PropDamage {
            prop: index(take("prop")?)?,
            thrust_scale: number(take("thrust_scale")?)?,
            imbalance_kgm: number(take("imbalance_kgm")?)?,
        },
        "weak_cell" => Fault::WeakCell {
            cell: index(take("cell")?)?,
            r_offset_ohm: number(take("r_offset_ohm")?)?,
            v_offset_v: number(take("v_offset_v")?)?,
        },
        "gyro_fault" => Fault::GyroFault { bias_radps: v3(take("bias_radps")?)?, noise_scale: number(take("noise_scale")?)? },
        "vtx_interference" => Fault::VtxInterference {
            freq_mhz: number(take("freq_mhz")?)?,
            power_mw: number(take("power_mw")?)?,
            position_ned_m: v3(take("position_ned_m")?)?,
        },
        other => return Err(format!("unknown fault '{other}'")),
    };
    if let Some((k, _)) = pairs.first() {
        return Err(format!("{name} has no key '{k}'"));
    }
    Ok(fault)
}

fn parse_entry(line: &str, dir: &Path) -> Result<Entry, String> {
    let mut words = line.split_whitespace();
    let tick = words.next().ok_or("empty")?.parse::<u64>().map_err(|_| "the line must start with a tick number".to_string())?;
    let verb = words.next().ok_or("no verb after the tick")?;
    let rest: Vec<&str> = words.collect();
    let action = match verb {
        "sticks" => {
            if rest.len() != 8 {
                return Err(format!("sticks needs 8 values, got {}", rest.len()));
            }
            let mut v = [0.0; 8];
            for (slot, s) in v.iter_mut().zip(&rest) {
                *slot = number(s)?;
            }
            Action::Sticks(v)
        }
        "tx" => match rest.as_slice() {
            ["0"] => Action::Transmitter(false),
            ["1"] => Action::Transmitter(true),
            _ => return Err("tx needs 0 or 1".into()),
        },
        "fault" => match rest.split_first() {
            Some((&state @ ("on" | "off"), tail)) => match tail.split_first() {
                Some((name, args)) => Action::Fault { fault: parse_fault(name, args)?, active: state == "on" },
                None => return Err("fault needs a name".into()),
            },
            _ => return Err("fault needs on or off".into()),
        },
        "schedule" => match rest.split_first() {
            Some((at, tail)) => match tail.split_first() {
                Some((name, args)) => Action::ScheduleFault { fault: parse_fault(name, args)?, at_s: number(at)? },
                None => return Err("schedule needs a time and a fault".into()),
            },
            None => return Err("schedule needs a time and a fault".into()),
        },
        "clear" if rest.is_empty() => Action::ClearFaults,
        "stop" if rest.is_empty() => Action::StopInputs,
        "race_reset" => match rest.as_slice() {
            [] => Action::ResetRace(None),
            [course] => Action::ResetRace(Some(course.parse().map_err(|_| format!("race_reset: '{course}' is not a course index"))?)),
            _ => return Err("race_reset takes at most a course index".into()),
        },
        "play" => {
            let [file] = rest.as_slice() else { return Err("play needs one file name".into()) };
            let path = dir.join(file);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Action::PlayInputs(crate::inputs::parse(file, &text)?)
        }
        other => return Err(format!("unknown verb '{other}'")),
    };
    Ok(Entry { tick, action })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs;
    use glam::DVec3;

    fn all_faults() -> Vec<Fault> {
        vec![
            Fault::RadioLinkLoss,
            Fault::RadioLoss { fraction: 0.1 },
            Fault::DownlinkLoss { fraction: 0.3 },
            Fault::BurstLoss { on_s: 1e-300, off_s: f64::MAX },
            Fault::MotorFailure { motor: 2, scale: -0.0 },
            Fault::PropDamage { prop: 3, thrust_scale: 0.1, imbalance_kgm: 1e305 },
            Fault::WeakCell { cell: 5, r_offset_ohm: 0.123456789012345, v_offset_v: 0.2 },
            Fault::GyroFault { bias_radps: DVec3::new(0.1, -0.2, 1e-9), noise_scale: 3.0 },
            Fault::VtxInterference { freq_mhz: 5800.0, power_mw: 25.0, position_ned_m: DVec3::new(10.0, -2.5, -1.0) },
        ]
    }

    fn round_trip(j: &Journal) -> Journal {
        let dir = tempfile::tempdir().unwrap();
        j.write(dir.path()).unwrap();
        Journal::read(dir.path()).unwrap()
    }

    #[test]
    fn every_fault_survives_a_text_round_trip() {
        let mut j = Journal::new();
        for (i, f) in all_faults().into_iter().enumerate() {
            j.push(i as u64 * 10, Action::Fault { fault: f.clone(), active: i % 2 == 0 });
            j.push(i as u64 * 10 + 1, Action::ScheduleFault { fault: f, at_s: 0.1 + i as f64 });
        }
        j.push(500, Action::ClearFaults);
        j.push(501, Action::Transmitter(false));
        j.push(502, Action::StopInputs);
        j.push(503, Action::ResetRace(None));
        j.push(504, Action::ResetRace(Some(1)));
        let back = round_trip(&j);
        assert_eq!(back.entries().len(), j.entries().len());
        for (a, b) in j.entries().iter().zip(back.entries()) {
            assert_eq!(a.tick, b.tick);
            match (&a.action, &b.action) {
                (Action::Fault { fault: fa, active: x }, Action::Fault { fault: fb, active: y }) => {
                    assert_eq!(x, y);
                    assert_eq!(format!("{fa:?}"), format!("{fb:?}"), "bit-exact, -0.0 included");
                }
                (Action::ScheduleFault { fault: fa, at_s: x }, Action::ScheduleFault { fault: fb, at_s: y }) => {
                    assert_eq!(x.to_bits(), y.to_bits());
                    assert_eq!(format!("{fa:?}"), format!("{fb:?}"));
                }
                (x, y) => assert_eq!(x, y),
            }
        }
    }

    #[test]
    fn the_downlink_loss_text_reads_back_and_rejects_a_bad_fraction() {
        let mut j = Journal::new();
        j.push(7, Action::Fault { fault: Fault::DownlinkLoss { fraction: 0.25 }, active: true });
        j.push(8, Action::Fault { fault: Fault::DownlinkLoss { fraction: 1.0 }, active: false });
        let dir = tempfile::tempdir().unwrap();
        j.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("journal.txt")).unwrap();
        assert!(text.contains("7 fault on downlink_loss fraction=0.25"), "{text}");
        assert!(text.contains("8 fault off downlink_loss fraction=1"), "{text}");
        let back = Journal::read(dir.path()).unwrap();
        assert_eq!(back.entries()[0].action, j.entries()[0].action);
        assert_eq!(back.entries()[1].action, j.entries()[1].action);

        let missing = read_text("# ofs-journal 1\n1 fault on downlink_loss\n").unwrap_err();
        assert!(missing.contains("downlink_loss needs fraction="), "{missing}");
        let nan = read_text("# ofs-journal 1\n1 fault on downlink_loss fraction=NaN\n").unwrap_err();
        assert!(nan.contains("downlink_loss fraction"), "{nan}");
    }

    #[test]
    fn sticks_with_nan_and_infinity_survive() {
        let mut j = Journal::new();
        j.push(1, Action::Sticks([0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1.0, 0.3]));
        let back = round_trip(&j);
        let Action::Sticks(v) = &back.entries()[0].action else { panic!("{:?}", back.entries()[0]) };
        assert_eq!(v[0].to_bits(), 0.1f64.to_bits());
        assert!(v[1].is_nan());
        assert_eq!((v[2], v[3]), (f64::INFINITY, f64::NEG_INFINITY));
    }

    #[test]
    fn a_played_inputs_file_is_written_beside_the_journal_and_read_back_equal() {
        let inputs = inputs::parse("climb.csv", "t_s,throttle,aux1\n0,0.3,-1\n0.5,1,1\n").unwrap();
        let mut j = Journal::new();
        j.push(40, Action::PlayInputs(inputs.clone()));
        let dir = tempfile::tempdir().unwrap();
        j.write(dir.path()).unwrap();
        assert!(dir.path().join("inputs-1.csv").exists());
        assert!(std::fs::read_to_string(dir.path().join("journal.txt")).unwrap().contains("40 play inputs-1.csv"));
        let back = Journal::read(dir.path()).unwrap();
        let Action::PlayInputs(read) = &back.entries()[0].action else { panic!("{:?}", back.entries()[0]) };
        assert_eq!((&read.columns, &read.rows), (&inputs.columns, &inputs.rows));
    }

    #[test]
    fn a_journal_past_the_cap_is_truncated_and_says_so() {
        let mut j = Journal::new();
        for i in 0..=MAX_ENTRIES {
            j.push(i as u64, Action::ClearFaults);
        }
        assert!(j.truncated());
        assert_eq!(j.entries().len(), MAX_ENTRIES);
        assert!(round_trip(&j).truncated(), "the flag survives the file");
        assert!(!Journal::new().truncated());
    }

    #[test]
    fn read_reports_every_bad_line_with_its_number() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("journal.txt"), "# ofs-journal 1\n1 teleport 3\n2 tx\n3 sticks 1 2 x 4 5 6 7 8\n4 fault on motor_failure motor=2\n").unwrap();
        let message = Journal::read(dir.path()).unwrap_err();
        for needle in ["journal.txt line 2", "journal.txt line 3", "journal.txt line 4", "journal.txt line 5"] {
            assert!(message.contains(needle), "{needle} missing in:\n{message}");
        }
    }

    fn read_text(text: &str) -> Result<Journal, String> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("journal.txt"), text).unwrap();
        Journal::read(dir.path())
    }

    #[test]
    fn a_journal_without_the_header_is_refused() {
        let message = read_text("1 tx 1\n").unwrap_err();
        assert!(message.starts_with("journal.txt line 1: not a journal"), "{message}");
        assert!(message.contains(HEADER), "the message names the header it wants: {message}");
    }

    #[test]
    fn a_journal_of_another_version_is_refused() {
        let message = read_text("# ofs-journal 2\n1 tx 1\n").unwrap_err();
        assert_eq!(message, "journal.txt: written by format 2, this build reads format 1");
    }

    #[test]
    fn the_header_is_the_version_constant() {
        assert_eq!(HEADER, format!("# ofs-journal {FORMAT_VERSION}"));
    }

    #[test]
    fn a_header_with_a_byte_order_mark_and_crlf_reads() {
        let journal = read_text("\u{feff}# ofs-journal 1\r\n1 tx 1\r\n").unwrap();
        assert_eq!(journal.entries().len(), 1);
    }

    #[test]
    fn a_truncated_journal_still_reads() {
        let journal = read_text("# ofs-journal 1\n1 tx 1\n# truncated\n").unwrap();
        assert!(journal.truncated());
        assert_eq!(journal.entries().len(), 1);
    }

    #[test]
    fn a_missing_inputs_file_is_named() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("journal.txt"), "# ofs-journal 1\n1 play inputs-7.csv\n").unwrap();
        assert!(Journal::read(dir.path()).unwrap_err().contains("inputs-7.csv"));
    }
}
