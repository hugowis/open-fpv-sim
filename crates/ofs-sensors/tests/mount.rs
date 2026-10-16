//! The frame response between the motors and the IMU: a second-order transmissibility
//! `T(s) = (1 + 2ζ s/ωn) / (1 + 2ζ s/ωn + s²/ωn²)`, checked against its analytic gain.
use std::f64::consts::PI;

use ofs_sensors::mount::{MountResponse, SecondOrder};

const DT: f64 = 1.0 / 8000.0;

fn soft() -> MountResponse {
    MountResponse { natural_hz: 100.0, damping_ratio: 0.1 }
}

/// The analytic gain |T(jω)| at `f_hz`.
fn analytic(m: MountResponse, f_hz: f64) -> f64 {
    let r = f_hz / m.natural_hz;
    let x = 2.0 * m.damping_ratio * r;
    (1.0 + x * x).sqrt() / ((1.0 - r * r).powi(2) + x * x).sqrt()
}

/// Drives the filter with a unit sine for one second and returns the peak output over the last 0.1 s.
fn gain(m: MountResponse, f_hz: f64) -> f64 {
    let mut filter = SecondOrder::transmissibility(m, DT);
    let steps = (1.0 / DT) as usize;
    let mut peak = 0.0f64;
    for k in 0..steps {
        let y = filter.step((2.0 * PI * f_hz * k as f64 * DT).sin());
        if k >= steps * 9 / 10 {
            peak = peak.max(y.abs());
        }
    }
    peak
}

#[test]
fn the_gain_is_unity_at_dc() {
    let mut filter = SecondOrder::transmissibility(soft(), DT);
    let mut y = 0.0;
    for _ in 0..16_000 {
        y = filter.step(1.0);
    }
    assert!((y - 1.0).abs() < 1e-6, "settled at {y}");
}

#[test]
fn the_gain_peaks_at_resonance() {
    let m = soft();
    let peak = (1.0 + 4.0 * m.damping_ratio * m.damping_ratio).sqrt() / (2.0 * m.damping_ratio);
    let got = gain(m, 100.0);
    assert!(((got - peak) / peak).abs() < 0.03, "resonance gain {got} vs analytic {peak}");
}

#[test]
fn the_gain_rolls_off_above_resonance() {
    let m = soft();
    for f in [40.0, 400.0] {
        let (got, want) = (gain(m, f), analytic(m, f));
        assert!(((got - want) / want).abs() < 0.03, "{f} Hz: gain {got} vs analytic {want}");
    }
    assert!(gain(m, 400.0) < 0.1, "the soft mount cuts a 400 Hz buzz to a tenth");
}

#[test]
fn a_stiff_mount_passes_the_vibration() {
    let m = MountResponse { natural_hz: 800.0, damping_ratio: 0.7 };
    let got = gain(m, 50.0);
    assert!((got - 1.0).abs() < 0.01, "gain {got}");
}

#[test]
fn a_fresh_filter_is_idle_and_a_driven_one_is_not() {
    let mut filter = SecondOrder::transmissibility(soft(), DT);
    assert!(filter.is_idle());
    filter.step(1.0);
    assert!(!filter.is_idle());
}
