//! Scripted stick and link inputs (spec §5): a CSV of `t_s`, stick columns, `tx` and `link_loss`, validated once,
//! played back sample-and-hold at tick boundaries.
use ofs_core::names::RC_AUX_COUNT;

/// The columns an inputs file may have; `t_s` is required and at least one input column is required.
pub const COLUMNS: [&str; 11] =
    ["t_s", "roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4", "tx", "link_loss"];

/// Problems listed in one error before the rest are counted.
const MAX_PROBLEMS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stick {
    Roll,
    Pitch,
    Yaw,
    Throttle,
    /// 0-based aux channel (`aux1` is `Aux(0)`).
    Aux(usize),
}

impl Stick {
    fn from_column(name: &str) -> Option<Stick> {
        Some(match name {
            "roll" => Stick::Roll,
            "pitch" => Stick::Pitch,
            "yaw" => Stick::Yaw,
            "throttle" => Stick::Throttle,
            _ => {
                let n: usize = name.strip_prefix("aux")?.parse().ok()?;
                if !(1..=RC_AUX_COUNT).contains(&n) {
                    return None;
                }
                Stick::Aux(n - 1)
            }
        })
    }

    /// The column name in an inputs file (`aux1` for `Aux(0)`).
    pub fn column(self) -> String {
        match self {
            Stick::Roll => "roll".into(),
            Stick::Pitch => "pitch".into(),
            Stick::Yaw => "yaw".into(),
            Stick::Throttle => "throttle".into(),
            Stick::Aux(i) => format!("aux{}", i + 1),
        }
    }

    /// The documented range: throttle [0, 1], every other stick and switch [-1, 1].
    fn range(self) -> (f64, f64) {
        match self {
            Stick::Throttle => (0.0, 1.0),
            _ => (-1.0, 1.0),
        }
    }
}

/// A column an inputs file can set: a stick, the transmitter, or the radio-link cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Stick(Stick),
    Transmitter,
    LinkLoss,
}

impl Column {
    fn from_column(name: &str) -> Option<Column> {
        Some(match name {
            "tx" => Column::Transmitter,
            "link_loss" => Column::LinkLoss,
            _ => Column::Stick(Stick::from_column(name)?),
        })
    }

    /// The column name in an inputs file.
    pub fn column(self) -> String {
        match self {
            Column::Stick(stick) => stick.column(),
            Column::Transmitter => "tx".into(),
            Column::LinkLoss => "link_loss".into(),
        }
    }

    /// The documented range: `tx` and `link_loss` are booleans [0, 1]; sticks keep their own range.
    fn range(self) -> (f64, f64) {
        match self {
            Column::Stick(stick) => stick.range(),
            Column::Transmitter | Column::LinkLoss => (0.0, 1.0),
        }
    }
}

/// A validated inputs file: the columns it sets and, per row, the time (s after playback starts) and one value per
/// column.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<(f64, Vec<f64>)>,
}

impl Inputs {
    /// When the last row applies, s after playback starts.
    pub fn duration_s(&self) -> f64 {
        self.rows.last().map_or(0.0, |r| r.0)
    }

    /// The file this was parsed from, written back: `parse` reads it into an equal value (floats in their
    /// shortest round-trip form).
    pub fn to_csv(&self) -> String {
        let mut out = String::from("t_s");
        for c in &self.columns {
            out.push(',');
            out.push_str(&c.column());
        }
        out.push('\n');
        for (t, values) in &self.rows {
            out.push_str(&t.to_string());
            for v in values {
                out.push(',');
                out.push_str(&v.to_string());
            }
            out.push('\n');
        }
        out
    }
}

/// Parses and validates an inputs file. Blank lines and lines starting with `#` are skipped; the first other
/// line is the header. A spreadsheet's UTF-8 byte-order mark and CRLF line endings are accepted. Every problem is
/// collected, each naming `name` and its line (1-based).
pub fn parse(name: &str, text: &str) -> Result<Inputs, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut problems = Vec::new();
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).filter(|(_, l)| !l.is_empty() && !l.starts_with('#'));
    let Some((header_line, header)) = lines.next() else {
        return Err(format!("{name}: no header line (expected t_s and input columns from {})", COLUMNS.join(", ")));
    };
    let fields: Vec<&str> = header.split(',').map(str::trim).collect();
    let mut time_index = None;
    let mut columns: Vec<(usize, Column)> = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if *f == "t_s" {
            if time_index.replace(i).is_some() {
                problems.push(format!("{name} line {header_line}: column t_s appears twice"));
            }
        } else if let Some(column) = Column::from_column(f) {
            if columns.iter().any(|(_, c)| *c == column) {
                problems.push(format!("{name} line {header_line}: column {f} appears twice"));
            }
            columns.push((i, column));
        } else {
            problems.push(format!("{name} line {header_line}: unknown column '{f}' (expected {})", COLUMNS.join(", ")));
        }
    }
    if time_index.is_none() {
        problems.push(format!("{name} line {header_line}: the header needs a t_s column"));
    }
    if columns.is_empty() {
        problems.push(format!("{name} line {header_line}: the header needs at least one input column"));
    }
    let mut rows = Vec::new();
    let mut last_t: Option<f64> = None;
    for (line, l) in lines {
        let values: Vec<&str> = l.split(',').map(str::trim).collect();
        if values.len() != fields.len() {
            problems.push(format!("{name} line {line}: {} values for {} columns", values.len(), fields.len()));
            continue;
        }
        let parsed: Vec<Option<f64>> = values.iter().map(|v| v.parse::<f64>().ok().filter(|x| x.is_finite())).collect();
        for (v, p) in values.iter().zip(&parsed) {
            if p.is_none() {
                problems.push(format!("{name} line {line}: '{v}' is not a finite number"));
            }
        }
        let Some(ti) = time_index else { continue };
        let Some(t) = parsed[ti] else { continue };
        if t < 0.0 {
            problems.push(format!("{name} line {line}: t_s {t} is negative"));
        }
        if let Some(prev) = last_t {
            if t <= prev {
                problems.push(format!("{name} line {line}: t_s {t} does not increase (previous row {prev})"));
            }
        }
        last_t = Some(t);
        let mut row = Vec::with_capacity(columns.len());
        for (i, column) in &columns {
            let Some(v) = parsed[*i] else { continue };
            let (lo, hi) = column.range();
            if !(lo..=hi).contains(&v) {
                problems.push(format!("{name} line {line}: {} {v} is outside [{lo}, {hi}]", fields[*i]));
            }
            row.push(v);
        }
        rows.push((t, row));
    }
    if rows.is_empty() && problems.is_empty() {
        problems.push(format!("{name}: no rows after the header"));
    }
    if !problems.is_empty() {
        let extra = problems.len().saturating_sub(MAX_PROBLEMS);
        problems.truncate(MAX_PROBLEMS);
        if extra > 0 {
            problems.push(format!("... and {extra} more"));
        }
        return Err(problems.join("\n"));
    }
    Ok(Inputs { name: name.to_string(), columns: columns.into_iter().map(|(_, s)| s).collect(), rows })
}

/// The first tick whose time is at or after `t_s` (a time that is a whole number of ticks up to rounding is that
/// tick: 0.3 s at 8 kHz is tick 2400, not 2401).
pub fn tick_at(t_s: f64, base_hz: u32) -> u64 {
    let x = t_s * f64::from(base_hz);
    let r = x.round();
    if (x - r).abs() < 1e-6 {
        r as u64
    } else {
        x.ceil() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subset_of_columns_parses_in_file_order() {
        let text = "# flips\n\nt_s, throttle, aux1\n0.0, 0.0, -1\n1.0, 0.0, 1\n\n1.5, 0.6, 1\n";
        let inputs = parse("flips.csv", text).unwrap();
        assert_eq!(inputs.columns, [Column::Stick(Stick::Throttle), Column::Stick(Stick::Aux(0))]);
        assert_eq!(inputs.rows, [(0.0, vec![0.0, -1.0]), (1.0, vec![0.0, 1.0]), (1.5, vec![0.6, 1.0])]);
        assert_eq!(inputs.duration_s(), 1.5);
    }

    #[test]
    fn tx_and_link_loss_parse_on_their_own_and_write_back_unchanged() {
        let text = "t_s,tx,link_loss\n0,0,0\n0.1,1,1\n";
        let inputs = parse("actions.csv", text).unwrap();
        assert_eq!(inputs.columns, [Column::Transmitter, Column::LinkLoss]);
        assert_eq!(inputs.rows, [(0.0, vec![0.0, 0.0]), (0.1, vec![1.0, 1.0])]);
        assert_eq!(inputs.to_csv(), text);
    }

    #[test]
    fn the_old_nine_column_file_still_parses_to_stick_columns() {
        let text = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4\n0,0,0,0,0.5,-1,-1,-1,-1\n1,0.25,0,0,0.6,1,1,-1,-1\n";
        let inputs = parse("old.csv", text).unwrap();
        assert_eq!(
            inputs.columns,
            [
                Column::Stick(Stick::Roll),
                Column::Stick(Stick::Pitch),
                Column::Stick(Stick::Yaw),
                Column::Stick(Stick::Throttle),
                Column::Stick(Stick::Aux(0)),
                Column::Stick(Stick::Aux(1)),
                Column::Stick(Stick::Aux(2)),
                Column::Stick(Stick::Aux(3)),
            ]
        );
        assert_eq!(inputs.to_csv(), text);
    }

    #[test]
    fn the_input_columns_have_their_own_ranges_and_the_header_lists_them_all() {
        let ranges = parse("actions.csv", "t_s,tx,link_loss\n0,2,-1\n").unwrap_err();
        assert!(ranges.contains("tx 2 is outside [0, 1]"), "{ranges}");
        assert!(ranges.contains("link_loss -1 is outside [0, 1]"), "{ranges}");
        assert!(parse("actions.csv", "t_s,tx,tx\n0,0,0\n").unwrap_err().contains("column tx appears twice"));
        assert!(parse("actions.csv", "t_s\n0\n").unwrap_err().contains("at least one input column"));
        let unknown = parse("actions.csv", "t_s,wobble\n0,0\n").unwrap_err();
        assert!(
            unknown.contains("expected t_s, roll, pitch, yaw, throttle, aux1, aux2, aux3, aux4, tx, link_loss"),
            "{unknown}"
        );
    }

    #[test]
    fn every_problem_is_reported_with_its_line() {
        let text = "t_s,throttle,roll,wobble\n0,0.5,0,1\n0.5,1.5,0\n0.5,x,0,0\n0.2,0.5,-2,0\n";
        let err = parse("bad.csv", text).unwrap_err();
        assert_eq!(
            err.lines().collect::<Vec<_>>(),
            [
                "bad.csv line 1: unknown column 'wobble' (expected t_s, roll, pitch, yaw, throttle, aux1, aux2, aux3, aux4, tx, link_loss)",
                "bad.csv line 3: 3 values for 4 columns",
                "bad.csv line 4: 'x' is not a finite number",
                "bad.csv line 5: t_s 0.2 does not increase (previous row 0.5)",
                "bad.csv line 5: roll -2 is outside [-1, 1]",
            ]
        );
    }

    #[test]
    fn the_header_needs_time_and_an_input_column_and_the_file_needs_rows() {
        assert!(parse("e.csv", "").unwrap_err().contains("e.csv: no header line"));
        assert!(parse("e.csv", "throttle\n0.5\n").unwrap_err().contains("the header needs a t_s column"));
        assert!(parse("e.csv", "t_s\n0\n").unwrap_err().contains("at least one input column"));
        assert_eq!(parse("e.csv", "t_s,yaw\n").unwrap_err(), "e.csv: no rows after the header");
        assert!(parse("e.csv", "t_s,aux5\n0,0\n").unwrap_err().contains("unknown column 'aux5'"));
        assert!(parse("e.csv", "t_s,yaw,yaw\n0,0,0\n").unwrap_err().contains("column yaw appears twice"));
        assert!(parse("e.csv", "t_s,yaw\n-1,0\n").unwrap_err().contains("t_s -1 is negative"));
        assert!(parse("e.csv", "t_s,yaw\n0,inf\n").unwrap_err().contains("'inf' is not a finite number"));
    }

    #[test]
    fn a_spreadsheet_export_with_a_bom_and_crlf_lines_parses_like_a_plain_file() {
        let plain = parse("f.csv", "t_s,throttle\n0,0.5\n1,0.2\n").unwrap();
        let excel = parse("f.csv", "\u{feff}t_s,throttle\r\n0,0.5\r\n1,0.2\r\n").unwrap();
        assert_eq!(excel, plain);
    }

    #[test]
    fn long_error_lists_are_capped() {
        let mut text = String::from("t_s,throttle\n");
        for i in 0..30 {
            text.push_str(&format!("{i},2\n"));
        }
        let err = parse("big.csv", &text).unwrap_err();
        assert_eq!(err.lines().count(), 21);
        assert_eq!(err.lines().last(), Some("... and 10 more"));
    }

    #[test]
    fn times_land_on_the_first_tick_at_or_after_them() {
        assert_eq!(tick_at(0.0, 8000), 0);
        assert_eq!(tick_at(0.3, 8000), 2400, "0.3 * 8000 is 2400.0000000000005 in floating point");
        assert_eq!(tick_at(0.00001, 8000), 1);
        assert_eq!(tick_at(1.0 / 3.0, 8000), 2667);
    }
}
