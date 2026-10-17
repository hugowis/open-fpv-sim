//! The report: what it prints for the awkward rows, and that it prints the same everywhere.
use ofs_validate::report::{max_error_pct, render, render_not_compared, unsettled};
use ofs_validate::submission::{Kind, Origin, Source, Submission, ThrustRow};
use ofs_validate::thrust::Predicted;

fn submission(source: Source) -> Submission {
    Submission {
        dir: ".".into(),
        kind: Kind::ThrustStand,
        title: "A stand".into(),
        quad: "quads/q.toml".into(),
        origin: Origin { source, note: "a note".into(), contributor: "me".into(), license: "CC-BY-SA-4.0".into() },
        motor: 0,
        data: "thrust.csv".into(),
        diff: None,
        rows: Vec::new(),
    }
}

fn predicted(rpm: f64, thrust_n: f64, current_a: f64) -> Predicted {
    Predicted { rpm, thrust_n, current_a, settle_s: 0.5 }
}

#[test]
fn errors_are_simulated_minus_measured_over_measured() {
    let rows = vec![
        (ThrustRow { command: 0.5, voltage_v: 24.0, rpm: Some(20000.0), thrust_n: Some(5.0), current_a: Some(10.0) }, Ok(predicted(22000.0, 4.0, 10.0))),
        (ThrustRow { command: 1.0, voltage_v: 23.0, rpm: Some(30000.0), thrust_n: None, current_a: Some(40.0) }, Ok(predicted(27000.0, 9.0, 30.0))),
    ];
    let text = render(&submission(Source::Measured), &rows);
    assert!(text.contains("| 0.50 | 24.0 | 20000 | 22000 | +10.0 | 5.00 | 4.00 | -20.0 | 10.0 | 10.0 | +0.0 |"), "{text}");
    assert!(text.contains("| 1.00 | 23.0 | 30000 | 27000 | -10.0 | — | 9.00 | — | 40.0 | 30.0 | -25.0 |"), "{text}");
    assert!(text.contains("| rpm | 2 | 10.0 | 10.0 |"), "{text}");
    assert!(text.contains("| thrust | 1 | 20.0 | 20.0 |"), "{text}");
    assert!(text.contains("| battery current | 2 | 12.5 | 25.0 |"), "{text}");
    assert_eq!(max_error_pct(&rows), 25.0);
}

#[test]
fn only_estimated_data_carries_the_banner() {
    let estimated = render(&submission(Source::Estimated), &[]);
    assert!(estimated.contains("These figures are estimates, not measurements"), "{estimated}");
    assert!(!render(&submission(Source::Measured), &[]).contains("estimates"));
    assert!(render_not_compared(&{
        let mut s = submission(Source::Estimated);
        s.kind = Kind::BlackboxStep;
        s
    })
    .contains("Not compared"));
}

#[test]
fn a_measured_zero_does_not_divide_and_an_unsettled_row_keeps_the_table_intact() {
    let rows = vec![
        (ThrustRow { command: 0.0, voltage_v: 24.0, rpm: Some(0.0), thrust_n: Some(0.0), current_a: Some(0.0) }, Ok(predicted(0.0, 0.0, 0.0))),
        (ThrustRow { command: 1.0, voltage_v: 24.0, rpm: Some(30000.0), thrust_n: None, current_a: None }, Err("no steady state within 10 s".to_string())),
    ];
    let text = render(&submission(Source::Measured), &rows);
    assert!(text.contains("| 0.00 | 24.0 | 0 | 0 | — | 0.00 | 0.00 | — | 0.0 | 0.0 | — |"), "{text}");
    assert!(text.contains("| 1.00 | 24.0 | 30000 | no steady state within 10 s | — | — | — | — | — | — | — |"), "{text}");
    assert!(text.contains("| rpm | 0 | — | — |"), "no comparable row: {text}");
    assert_eq!(max_error_pct(&rows), 0.0);
    assert!(!text.contains("NaN") && !text.contains("inf"), "{text}");
}

#[test]
fn unsettled_counts_the_rows_with_an_error() {
    let row = ThrustRow { command: 1.0, voltage_v: 24.0, rpm: Some(30000.0), thrust_n: None, current_a: None };
    let rows = vec![(row.clone(), Ok(predicted(30000.0, 5.0, 10.0))), (row.clone(), Err("no steady state within 10 s".to_string())), (row, Err("x".to_string()))];
    assert_eq!(unsettled(&rows), 2);
    assert_eq!(unsettled(&[]), 0);
}

#[test]
fn the_report_names_no_machine_and_no_date() {
    let text = render(&submission(Source::Measured), &[]);
    assert!(text.contains("`quads/q.toml`"));
    assert!(!text.contains(":\\") && !text.contains("/home/") && !text.contains("20"), "{text}");
}
