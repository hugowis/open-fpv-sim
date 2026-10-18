//! The submission format (docs/validation.md): a folder with `submission.toml` and a data file. Loading checks
//! everything it can and reports every problem together, each naming its file and line.
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The schema version of `submission.toml` this build reads.
pub const SCHEMA_VERSION: i64 = 1;
/// The licence community data comes under: the component library's own.
pub const LICENSE: &str = "CC-BY-SA-4.0";

const THRUST_COLUMNS: [&str; 5] = ["command", "voltage_v", "rpm", "thrust_n", "current_a"];
const MEASURED: [&str; 3] = ["rpm", "thrust_n", "current_a"];
/// What a `blackbox_decode` CSV must carry for a step-response comparison (blackbox-tools' header names).
pub const BLACKBOX_COLUMNS: [&str; 7] =
    ["time (us)", "gyroADC[0]", "gyroADC[1]", "gyroADC[2]", "setpoint[0]", "setpoint[1]", "setpoint[2]"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ThrustStand,
    BlackboxStep,
}

/// How much the figures can be trusted; the vocabulary of the component library's provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Datasheet,
    Measured,
    Estimated,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Datasheet => "datasheet",
            Source::Measured => "measured",
            Source::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    pub source: Source,
    pub note: String,
    pub contributor: String,
    pub license: String,
}

/// One line of a thrust-stand log: the ESC command and the pack voltage it ran at, and what was measured.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrustRow {
    pub command: f64,
    pub voltage_v: f64,
    pub rpm: Option<f64>,
    pub thrust_n: Option<f64>,
    /// Battery-side current.
    pub current_a: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Submission {
    pub dir: PathBuf,
    pub kind: Kind,
    pub title: String,
    /// The quad file as `submission.toml` wrote it (relative to the folder).
    pub quad: String,
    pub origin: Origin,
    /// Thrust stand: which of the quad's motors (and its propeller and ESC) was on the stand.
    pub motor: usize,
    /// The data file's name.
    pub data: String,
    /// Blackbox: the Betaflight diff of the tune that flew.
    pub diff: Option<String>,
    /// Thrust stand: the rows. Empty for a Blackbox submission.
    pub rows: Vec<ThrustRow>,
}

impl Submission {
    pub fn quad_path(&self) -> PathBuf {
        self.dir.join(&self.quad)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema_version: Option<i64>,
    kind: Option<String>,
    title: Option<String>,
    quad: Option<String>,
    data: Option<String>,
    motor: Option<i64>,
    diff: Option<String>,
    origin: Option<RawOrigin>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOrigin {
    source: Option<String>,
    note: Option<String>,
    contributor: Option<String>,
    license: Option<String>,
}

/// The non-blank, non-comment lines of a text with their physical line numbers (1-based); a leading byte-order mark
/// and CRLF endings are accepted.
fn lines(text: &str) -> Vec<(usize, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).filter(|(_, l)| !l.is_empty() && !l.starts_with('#')).collect()
}

fn present<'a>(value: &'a Option<String>, key: &str, problems: &mut Vec<String>) -> Option<&'a str> {
    match value.as_deref().map(str::trim) {
        Some(v) if !v.is_empty() => Some(v),
        _ => {
            problems.push(format!("submission.toml: missing {key}"));
            None
        }
    }
}

/// A TOML error as one problem line, naming the line it is on when the parser says where.
fn toml_error(text: &str, e: &toml::de::Error) -> String {
    match e.span() {
        Some(span) => {
            let line = text.as_bytes()[..span.start.min(text.len())].iter().filter(|b| **b == b'\n').count() + 1;
            format!("submission.toml line {line}: {}", e.message())
        }
        None => format!("submission.toml: {}", e.message()),
    }
}

fn text_of(t: &toml::Table, key: &str, problems: &mut Vec<String>) -> Option<String> {
    match t.get(key).map(toml::Value::as_str) {
        None => None,
        Some(None) => {
            problems.push(format!("submission.toml: {key} must be text"));
            None
        }
        Some(Some(s)) => Some(s.to_string()),
    }
}

fn int_of(t: &toml::Table, key: &str, problems: &mut Vec<String>) -> Option<i64> {
    match t.get(key).map(toml::Value::as_integer) {
        None => None,
        Some(None) => {
            problems.push(format!("submission.toml: {key} must be a whole number"));
            None
        }
        Some(Some(i)) => Some(i),
    }
}

fn origin_of(t: &toml::Table, problems: &mut Vec<String>) -> Option<RawOrigin> {
    let o = t.get("origin")?.as_table()?;
    const KEYS: [&str; 4] = ["source", "note", "contributor", "license"];
    for key in o.keys() {
        if !KEYS.contains(&key.as_str()) {
            problems.push(format!("submission.toml: unknown key 'origin.{key}'"));
        }
    }
    Some(RawOrigin {
        source: text_of(o, "source", problems),
        note: text_of(o, "note", problems),
        contributor: text_of(o, "contributor", problems),
        license: text_of(o, "license", problems),
    })
}

/// The `Raw` fields, one key at a time, so every wrong type is its own problem (a serde pass would stop at the
/// first) and every unknown key was named before this ran.
fn raw_from(t: &toml::Table, problems: &mut Vec<String>) -> Raw {
    if let Some(v) = t.get("origin") {
        if v.as_table().is_none() {
            problems.push("submission.toml: [origin] must be a table".into());
        }
    }
    Raw {
        schema_version: int_of(t, "schema_version", problems),
        kind: text_of(t, "kind", problems),
        title: text_of(t, "title", problems),
        quad: text_of(t, "quad", problems),
        data: text_of(t, "data", problems),
        motor: int_of(t, "motor", problems),
        diff: text_of(t, "diff", problems),
        origin: t.get("origin").and_then(toml::Value::as_table).map(|_| origin_of(t, problems)).flatten(),
    }
}

/// Loads and checks the submission in `dir`. Every problem found is returned together.
pub fn load(dir: &Path) -> Result<Submission, Vec<String>> {
    let toml_path = dir.join("submission.toml");
    let text = std::fs::read_to_string(&toml_path).map_err(|e| vec![format!("submission.toml: {}: {e}", toml_path.display())])?;
    // Syntax first (one problem, with its line): a file that does not parse can say nothing else. Then every key is
    // read on its own, so unknown keys and wrong types all come back together instead of the first one alone.
    let table: toml::Table = toml::from_str(&text).map_err(|e| vec![toml_error(&text, &e)])?;
    let mut problems = Vec::new();
    const TOP: [&str; 8] = ["schema_version", "kind", "title", "quad", "data", "motor", "diff", "origin"];
    for key in table.keys() {
        if !TOP.contains(&key.as_str()) {
            problems.push(format!("submission.toml: unknown key '{key}'"));
        }
    }
    let raw = raw_from(&table, &mut problems);

    match raw.schema_version {
        Some(SCHEMA_VERSION) => {}
        Some(v) => problems.push(format!("submission.toml: schema_version {v} (this build reads {SCHEMA_VERSION})")),
        None => problems.push("submission.toml: missing schema_version".into()),
    }
    let kind = match raw.kind.as_deref() {
        Some("thrust_stand") => Some(Kind::ThrustStand),
        Some("blackbox_step") => Some(Kind::BlackboxStep),
        Some(other) => {
            problems.push(format!("submission.toml: kind '{other}' (expected thrust_stand or blackbox_step)"));
            None
        }
        None => {
            problems.push("submission.toml: missing kind".into());
            None
        }
    };
    let title = present(&raw.title, "title", &mut problems).unwrap_or_default().to_string();
    let quad = present(&raw.quad, "quad", &mut problems).unwrap_or_default().to_string();
    let data = present(&raw.data, "data", &mut problems).unwrap_or_default().to_string();

    let mut origin = Origin { source: Source::Estimated, note: String::new(), contributor: String::new(), license: String::new() };
    match &raw.origin {
        None => problems.push("submission.toml: missing the [origin] table".into()),
        Some(o) => {
            match o.source.as_deref() {
                Some("datasheet") => origin.source = Source::Datasheet,
                Some("measured") => origin.source = Source::Measured,
                Some("estimated") => origin.source = Source::Estimated,
                Some(other) => problems.push(format!("submission.toml: origin.source '{other}' (expected datasheet, measured or estimated)")),
                None => problems.push("submission.toml: missing origin.source".into()),
            }
            origin.note = present(&o.note, "origin.note", &mut problems).unwrap_or_default().to_string();
            origin.contributor = present(&o.contributor, "origin.contributor", &mut problems).unwrap_or_default().to_string();
            match o.license.as_deref() {
                Some(LICENSE) => origin.license = LICENSE.into(),
                Some(other) => problems.push(format!("submission.toml: origin.license '{other}' (community data comes under {LICENSE})")),
                None => problems.push(format!("submission.toml: missing origin.license (it must be {LICENSE})")),
            }
        }
    }

    // The quad the figures were taken on.
    let quad_path = dir.join(&quad);
    let cfg = if quad.is_empty() {
        None
    } else {
        match ofs_config::load(&quad_path) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                problems.push(format!("submission.toml: quad '{quad}': {e}"));
                None
            }
        }
    };

    let motor = match raw.motor {
        None => 0,
        Some(m) if m >= 0 => m as usize,
        Some(m) => {
            problems.push(format!("submission.toml: motor {m} must be >= 0"));
            0
        }
    };
    let mut rows = Vec::new();
    let mut diff = None;
    match kind {
        Some(Kind::ThrustStand) => {
            if let Some(cfg) = &cfg {
                if motor >= cfg.motors.len() {
                    problems.push(format!("submission.toml: motor {motor} does not exist (the quad has {})", cfg.motors.len()));
                }
            }
            if !data.is_empty() {
                match std::fs::read_to_string(dir.join(&data)) {
                    Ok(text) => rows = thrust_rows(&data, &text, &mut problems),
                    Err(e) => problems.push(format!("{data}: {e}")),
                }
            }
        }
        Some(Kind::BlackboxStep) => {
            if let Some(d) = present(&raw.diff, "diff (the Betaflight diff of the tune that flew)", &mut problems) {
                if !dir.join(d).is_file() {
                    problems.push(format!("submission.toml: diff '{d}': file not found"));
                }
                diff = Some(d.to_string());
            }
            if !data.is_empty() {
                match std::fs::read_to_string(dir.join(&data)) {
                    Ok(text) => check_blackbox(&data, &text, &mut problems),
                    Err(e) => problems.push(format!("{data}: {e}")),
                }
            }
        }
        None => {}
    }

    match (problems.is_empty(), kind) {
        (true, Some(kind)) => Ok(Submission { dir: dir.to_path_buf(), kind, title, quad, origin, motor, data, diff, rows }),
        _ => Err(problems),
    }
}

fn number(cell: &str) -> Option<f64> {
    cell.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn thrust_rows(file: &str, text: &str, problems: &mut Vec<String>) -> Vec<ThrustRow> {
    let all = lines(text);
    let Some((header_line, header)) = all.first().copied() else {
        problems.push(format!("{file}: no header line (expected columns from {})", THRUST_COLUMNS.join(", ")));
        return Vec::new();
    };
    let fields: Vec<&str> = header.split(',').map(str::trim).collect();
    for f in &fields {
        if !THRUST_COLUMNS.contains(f) {
            problems.push(format!("{file} line {header_line}: unknown column '{f}' (expected {})", THRUST_COLUMNS.join(", ")));
        }
    }
    let index = |name: &str| fields.iter().position(|f| *f == name);
    let (command_at, voltage_at) = (index("command"), index("voltage_v"));
    let measured_at: Vec<(usize, &str)> = MEASURED.iter().filter_map(|m| index(m).map(|i| (i, *m))).collect();
    if command_at.is_none() {
        problems.push(format!("{file} line {header_line}: the header needs a command column"));
    }
    if voltage_at.is_none() {
        problems.push(format!("{file} line {header_line}: the header needs a voltage_v column"));
    }
    if measured_at.is_empty() {
        problems.push(format!("{file} line {header_line}: the header needs at least one of {}", MEASURED.join(", ")));
    }
    if all.len() == 1 {
        problems.push(format!("{file}: no rows after the header"));
    }
    let (Some(command_at), Some(voltage_at)) = (command_at, voltage_at) else { return Vec::new() };
    let mut rows = Vec::new();
    for (line, l) in &all[1..] {
        let cells: Vec<&str> = l.split(',').map(str::trim).collect();
        if cells.len() != fields.len() {
            problems.push(format!("{file} line {line}: {} values for {} columns", cells.len(), fields.len()));
            continue;
        }
        let mut ok = true;
        let mut required = |at: usize, name: &str| -> f64 {
            match number(cells[at]) {
                Some(v) => v,
                None => {
                    problems.push(format!("{file} line {line}: '{}' is not a finite number ({name})", cells[at]));
                    ok = false;
                    0.0
                }
            }
        };
        let command = required(command_at, "command");
        let voltage_v = required(voltage_at, "voltage_v");
        // A cell that did not parse has been reported already (and reads as 0): range-check only what did.
        let parsed = ok;
        if parsed && !(0.0..=1.0).contains(&command) {
            problems.push(format!("{file} line {line}: command {command} is outside [0, 1]"));
            ok = false;
        }
        if parsed && voltage_v <= 0.0 {
            problems.push(format!("{file} line {line}: voltage_v {voltage_v} must be > 0"));
            ok = false;
        }
        let mut values = [None; 3];
        for (slot, (at, name)) in values.iter_mut().zip(MEASURED.iter().map(|m| (index(m), *m))) {
            let Some(at) = at else { continue };
            if cells[at].is_empty() {
                continue;
            }
            match number(cells[at]) {
                Some(v) if v >= 0.0 => *slot = Some(v),
                Some(v) => {
                    problems.push(format!("{file} line {line}: {name} {v} must be >= 0"));
                    ok = false;
                }
                None => {
                    problems.push(format!("{file} line {line}: '{}' is not a finite number ({name})", cells[at]));
                    ok = false;
                }
            }
        }
        if ok && values.iter().all(Option::is_none) {
            problems.push(format!("{file} line {line}: no measured value (fill in at least one of {})", MEASURED.join(", ")));
            ok = false;
        }
        if ok {
            rows.push(ThrustRow { command, voltage_v, rpm: values[0], thrust_n: values[1], current_a: values[2] });
        }
    }
    rows
}

fn check_blackbox(file: &str, text: &str, problems: &mut Vec<String>) {
    let all = lines(text);
    let Some((header_line, header)) = all.first().copied() else {
        problems.push(format!("{file}: no header line (expected blackbox_decode's CSV)"));
        return;
    };
    let fields: Vec<&str> = header.split(',').map(str::trim).collect();
    let found = {
        let shown: Vec<&str> = fields.iter().take(12).copied().collect();
        format!("{}{}", shown.join(", "), if fields.len() > 12 { ", ..." } else { "" })
    };
    for want in BLACKBOX_COLUMNS {
        if !fields.contains(&want) {
            problems.push(format!("{file} line {header_line}: missing column '{want}' (found: {found})"));
        }
    }
    if all.len() == 1 {
        problems.push(format!("{file}: no rows after the header"));
    }
}
