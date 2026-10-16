//! The worked example under `validation/`: it loads, it says it is estimated, and its committed report is what the
//! tool prints now (so the example cannot go stale).
use std::path::Path;

use ofs_validate::report::render;
use ofs_validate::submission::{load, Source};
use ofs_validate::thrust::simulate;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../validation/thrust-stand/example-2207-5in");

fn printed() -> String {
    let sub = load(Path::new(EXAMPLE)).unwrap_or_else(|p| panic!("the shipped example is invalid:\n{}", p.join("\n")));
    let cfg = ofs_config::load(&sub.quad_path()).unwrap();
    let rows: Vec<_> = sub.rows.iter().map(|r| (r.clone(), simulate(&cfg, sub.motor, r))).collect();
    render(&sub, &rows)
}

#[test]
fn the_example_says_it_is_estimated() {
    let sub = load(Path::new(EXAMPLE)).unwrap();
    assert_eq!(sub.origin.source, Source::Estimated);
    assert!(sub.origin.note.contains("NOT a measurement"), "{}", sub.origin.note);
    assert!(printed().contains("These figures are estimates, not measurements"));
}

#[test]
fn the_committed_report_is_what_the_tool_prints() {
    let committed = std::fs::read_to_string(Path::new(EXAMPLE).join("REPORT.md")).unwrap().replace("\r\n", "\n");
    assert_eq!(
        printed(),
        committed,
        "REPORT.md is stale: regenerate it with\n  cargo run -p ofs-validate --locked -- validation/thrust-stand/example-2207-5in --out validation/thrust-stand/example-2207-5in/REPORT.md"
    );
}
