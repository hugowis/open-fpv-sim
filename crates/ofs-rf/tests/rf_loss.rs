use ofs_rf::{RfLoss, RfLossCurve};
use serde::Deserialize;

fn close(a: f64, b: f64) { assert!((a - b).abs() < 1e-9, "{a} != {b}"); }

#[test]
fn a_scalar_is_the_same_at_every_frequency() {
    let loss = RfLoss::from(25.0);
    for f in [900.0, 2400.0, 5800.0] { close(loss.db_at(f), 25.0); }
    assert!(loss.is_obstacle());
}

#[test]
fn a_curve_interpolates_as_a_power_law() {
    // 10 dB at 100 MHz, 20 dB at 1000 MHz: 14.142… dB at the geometric middle.
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![100.0, 1000.0], db: vec![10.0, 20.0] });
    close(loss.db_at(316.22776601683796), 10.0 * 2f64.sqrt());
}

#[test]
fn a_curve_clamps_outside_its_points() {
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] });
    close(loss.db_at(100.0), 12.0);
    close(loss.db_at(10000.0), 25.0);
}

#[test]
fn a_zero_point_falls_back_to_linear_db() {
    // ln(f) 900..5800, dB 0..25: half way in ln(f) is 12.5, where log-log is undefined.
    let loss = RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 25.0] });
    let middle = (900f64.ln() + 5800f64.ln()) / 2.0;
    close(loss.db_at(middle.exp()), 12.5);
}

#[test]
fn only_a_positive_curve_is_an_obstacle() {
    assert!(!RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 0.0] }).is_obstacle());
    assert!(RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![0.0, 1.0] }).is_obstacle());
    close(RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![3.0, 25.0] }).max_db(), 25.0);
}

#[test]
fn a_curve_validates_its_shape() {
    let ok = RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] };
    assert!(ok.validate().is_ok());
    for bad in [
        RfLossCurve { at_mhz: vec![900.0], db: vec![12.0] },
        RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0] },
        RfLossCurve { at_mhz: vec![5800.0, 900.0], db: vec![12.0, 25.0] },
        RfLossCurve { at_mhz: vec![0.0, 5800.0], db: vec![12.0, 25.0] },
        RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![-1.0, 25.0] },
    ] { assert!(bad.validate().is_err(), "{bad:?}"); }
}

#[test]
fn a_negative_scalar_is_not_an_obstacle() {
    let loss = RfLoss::Db(-5.0);
    close(loss.max_db(), -5.0);
    assert!(!loss.is_obstacle());
}

#[test]
fn serde_accepts_a_scalar_and_a_table() {
    assert_eq!(RfLoss::deserialize(toml::de::ValueDeserializer::new("25.0")).unwrap(), RfLoss::Db(25.0));
    assert_eq!(
        RfLoss::deserialize(toml::de::ValueDeserializer::new("{ at_mhz = [900.0, 5800.0], db = [12.0, 25.0] }")).unwrap(),
        RfLoss::Curve(RfLossCurve { at_mhz: vec![900.0, 5800.0], db: vec![12.0, 25.0] })
    );
}
