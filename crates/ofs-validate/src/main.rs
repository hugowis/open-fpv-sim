//! `ofs-validate <submission folder>`: checks a submission against docs/validation.md and prints the report.
//! Exit 0: valid (and within `--max-error-pct` if given); 1: over that gate; 2: the submission is invalid.
use std::io::Write;
use std::path::PathBuf;

use clap::Parser;
use ofs_validate::report::{max_error_pct, render, render_not_compared, unsettled};
use ofs_validate::submission::{load, Kind};
use ofs_validate::thrust::simulate;

#[derive(Parser)]
#[command(about = "Check a validation submission and compare it with the simulator")]
struct Args {
    /// The submission folder (submission.toml and its data file).
    dir: PathBuf,
    /// Also write the report to this file.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Append the report to $GITHUB_STEP_SUMMARY when it is set (CI's job summary).
    #[arg(long)]
    summary: bool,
    /// Exit 1 when any error is larger than this many percent. Off by default: validation is tracked, not gating.
    #[arg(long, value_name = "PERCENT")]
    max_error_pct: Option<f64>,
}

fn main() {
    let args = Args::parse();
    let sub = match load(&args.dir) {
        Ok(sub) => sub,
        Err(problems) => {
            for p in problems {
                eprintln!("ofs-validate: {p}");
            }
            std::process::exit(2);
        }
    };
    let (report, worst, unsettled) = match sub.kind {
        Kind::ThrustStand => {
            let cfg = match ofs_config::load(&sub.quad_path()) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("ofs-validate: quad '{}': {e}", sub.quad);
                    std::process::exit(2);
                }
            };
            let rows: Vec<_> = sub.rows.iter().map(|r| (r.clone(), simulate(&cfg, sub.motor, r))).collect();
            (render(&sub, &rows), max_error_pct(&rows), unsettled(&rows))
        }
        Kind::BlackboxStep => (render_not_compared(&sub), 0.0, 0),
    };
    print!("{report}");
    if let Some(out) = &args.out {
        if let Err(e) = std::fs::write(out, &report) {
            eprintln!("ofs-validate: {}: {e}", out.display());
            std::process::exit(2);
        }
    }
    if args.summary {
        if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            let appended = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| writeln!(f, "{report}"));
            if let Err(e) = appended {
                eprintln!("ofs-validate: the job summary: {e}");
            }
        }
    }
    if let Some(limit) = args.max_error_pct {
        if unsettled > 0 {
            eprintln!("ofs-validate: {unsettled} row(s) did not settle, so the {limit:.1} % gate cannot pass");
            std::process::exit(1);
        }
        if worst > limit {
            eprintln!("ofs-validate: the largest error is {worst:.1} %, over the {limit:.1} % gate");
            std::process::exit(1);
        }
    }
}
