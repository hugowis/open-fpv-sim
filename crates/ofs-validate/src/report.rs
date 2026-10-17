//! The Markdown report of a comparison: the figures side by side and the error per quantity. It carries no dates and
//! no machine paths, so the same submission prints the same report anywhere.
use crate::submission::{Kind, Source, Submission, ThrustRow};
use crate::thrust::Predicted;

/// One row's measured figures and what the models settled at (or why they did not).
pub type Compared = (ThrustRow, Result<Predicted, String>);

fn error_pct(sim: f64, measured: Option<f64>) -> Option<f64> {
    measured.filter(|m| *m > 0.0).map(|m| (sim - m) / m * 100.0)
}

/// The simulated minus measured error of each quantity of each row, in percent, rows the models settled for only.
fn errors(rows: &[Compared]) -> [Vec<f64>; 3] {
    let mut out: [Vec<f64>; 3] = Default::default();
    for (row, predicted) in rows {
        let Ok(p) = predicted else { continue };
        for (slot, e) in out.iter_mut().zip([
            error_pct(p.rpm, row.rpm),
            error_pct(p.thrust_n, row.thrust_n),
            error_pct(p.current_a, row.current_a),
        ]) {
            slot.extend(e);
        }
    }
    out
}

/// The largest absolute error, in percent, of any quantity of any row (0 when nothing was compared).
pub fn max_error_pct(rows: &[Compared]) -> f64 {
    errors(rows).iter().flatten().fold(0.0, |m, e| f64::max(m, e.abs()))
}

fn cell(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "—".to_string(), |v| format!("{v:.decimals$}"))
}

fn pct(e: Option<f64>) -> String {
    e.map_or_else(|| "—".to_string(), |e| format!("{e:+.1}"))
}

/// The report of a thrust-stand comparison.
pub fn render(sub: &Submission, rows: &[Compared]) -> String {
    let mut out = format!("# Thrust-stand comparison: {}\n\n", sub.title);
    if sub.origin.source == Source::Estimated {
        out.push_str(
            "> **These figures are estimates, not measurements.** The comparison shows the format and the report, \
             not how accurate the model is.\n\n",
        );
    }
    out.push_str(&format!(
        "- Source: `{}` — {}\n- Contributor: {} ({})\n- Quad: `{}`, motor {}\n- Rows: {}\n\n",
        sub.origin.source.name(),
        sub.origin.note,
        sub.origin.contributor,
        sub.origin.license,
        sub.quad,
        sub.motor,
        rows.len()
    ));
    out.push_str("Errors are (simulated − measured) / measured, in percent. A dash is a figure the stand did not record.\n\n");
    out.push_str("| command | voltage (V) | rpm meas | rpm sim | err % | thrust meas (N) | thrust sim | err % | current meas (A) | current sim | err % |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|\n");
    for (row, predicted) in rows {
        match predicted {
            Ok(p) => out.push_str(&format!(
                "| {:.2} | {:.1} | {} | {:.0} | {} | {} | {:.2} | {} | {} | {:.1} | {} |\n",
                row.command,
                row.voltage_v,
                cell(row.rpm, 0),
                p.rpm,
                pct(error_pct(p.rpm, row.rpm)),
                cell(row.thrust_n, 2),
                p.thrust_n,
                pct(error_pct(p.thrust_n, row.thrust_n)),
                cell(row.current_a, 1),
                p.current_a,
                pct(error_pct(p.current_a, row.current_a)),
            )),
            Err(why) => out.push_str(&format!(
                "| {:.2} | {:.1} | {} | {why} | — | {} | — | — | {} | — | — |\n",
                row.command,
                row.voltage_v,
                cell(row.rpm, 0),
                cell(row.thrust_n, 2),
                cell(row.current_a, 1),
            )),
        }
    }
    out.push_str("\n| quantity | rows compared | mean absolute error % | maximum absolute error % |\n|---|---|---|---|\n");
    let e = errors(rows);
    for (name, list) in ["rpm", "thrust", "battery current"].iter().zip(&e) {
        if list.is_empty() {
            out.push_str(&format!("| {name} | 0 | — | — |\n"));
        } else {
            let mean = list.iter().map(|x| x.abs()).sum::<f64>() / list.len() as f64;
            let max = list.iter().fold(0.0, |m, x| f64::max(m, x.abs()));
            out.push_str(&format!("| {name} | {} | {mean:.1} | {max:.1} |\n", list.len()));
        }
    }
    out
}

/// The report of a Blackbox submission: it follows the format, and nothing compares it yet.
pub fn render_not_compared(sub: &Submission) -> String {
    debug_assert_eq!(sub.kind, Kind::BlackboxStep);
    let mut out = format!("# Blackbox step response: {}\n\n", sub.title);
    if sub.origin.source == Source::Estimated {
        out.push_str("> **These figures are estimates, not measurements.**\n\n");
    }
    out.push_str(&format!(
        "- Source: `{}` — {}\n- Contributor: {} ({})\n- Quad: `{}`\n- Log: `{}`, tune: `{}`\n\n",
        sub.origin.source.name(),
        sub.origin.note,
        sub.origin.contributor,
        sub.origin.license,
        sub.quad,
        sub.data,
        sub.diff.as_deref().unwrap_or("")
    ));
    out.push_str("The submission follows the format. Not compared: the step-response comparator waits for real data (docs/roadmap.md, Validation).\n");
    out
}
