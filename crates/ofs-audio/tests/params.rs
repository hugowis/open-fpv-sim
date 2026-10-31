use ofs_audio::{params, thud_gain, AudioConfig, AudioInput, AudioParams};

fn input(rpm: &[f64], cmd: &[f64], speed: f64, descent: f64) -> AudioInput {
    AudioInput {
        motor_rpm: rpm.to_vec(),
        motor_cmd: cmd.to_vec(),
        airspeed_mps: speed,
        descent_mps: descent,
        active: true,
    }
}

fn p(i: &AudioInput) -> AudioParams {
    params(i, &AudioConfig::default())
}

fn all_finite(a: &AudioParams) -> bool {
    a.motors.iter().all(|m| m.freq_hz.is_finite() && m.gain.is_finite())
        && a.wash_gain.is_finite()
        && a.wind.gain.is_finite()
        && a.wind.cutoff_hz.is_finite()
}

#[test]
fn same_input_same_params() {
    let i = input(&[10000.0, 20000.0, 30000.0, 40000.0], &[0.2, 0.4, 0.6, 0.8], 12.0, 2.0);
    let c = AudioConfig::default();
    assert_eq!(params(&i, &c), params(&i, &c));
}

#[test]
fn motor_pitch_is_blade_pass_of_rpm() {
    assert_eq!(p(&input(&[30000.0], &[0.5], 0.0, 0.0)).motors[0].freq_hz, 1500.0);
    assert_eq!(p(&input(&[15000.0], &[0.5], 0.0, 0.0)).motors[0].freq_hz, 750.0);
}

#[test]
fn motor_gain_follows_the_command_and_is_silent_at_zero() {
    let g = |cmd: f64| p(&input(&[20000.0], &[cmd], 0.0, 0.0)).motors[0].gain;
    assert_eq!(g(0.0), 0.0);
    assert!(g(1.0) > g(0.5) && g(0.5) > 0.0);
    for cmd in [0.0, 0.3, 1.0, 5.0] {
        assert!((0.0..=1.0).contains(&g(cmd)));
    }
}

#[test]
fn each_motor_is_its_own_voice() {
    let a = p(&input(&[10000.0, 20000.0, 30000.0, 40000.0], &[0.5; 4], 0.0, 0.0));
    assert_eq!(a.motors.len(), 4);
    assert!(a.motors.windows(2).all(|w| w[0].freq_hz < w[1].freq_hz));
}

#[test]
fn wind_rises_with_airspeed() {
    let w = |s: f64| p(&input(&[], &[], s, 0.0)).wind;
    let (w0, w10, w30) = (w(0.0), w(10.0), w(30.0));
    assert!(w0.gain < w10.gain && w10.gain < w30.gain);
    assert!(w0.cutoff_hz < w10.cutoff_hz && w10.cutoff_hz < w30.cutoff_hz);
    assert_eq!(w0.gain, 0.0);
    assert_eq!(w0.cutoff_hz, 300.0);
    assert_eq!(w30.cutoff_hz, 3500.0);
    assert_eq!(w(60.0).cutoff_hz, 3500.0);
}

#[test]
fn wash_rises_with_descent_and_throttle() {
    let wash = |cmd: f64, descent: f64| p(&input(&[20000.0; 4], &[cmd; 4], 0.0, descent)).wash_gain;
    assert!(wash(0.5, 5.0) > wash(0.5, 0.0));
    assert!(wash(0.8, 0.0) > wash(0.2, 0.0));
    assert_eq!(wash(0.0, 5.0), 0.0);
}

#[test]
fn inactive_or_no_motors_is_silent() {
    let mut i = input(&[20000.0; 4], &[0.7; 4], 20.0, 3.0);
    i.active = false;
    let a = p(&i);
    assert!(a.motors.iter().all(|m| m.gain == 0.0));
    assert_eq!((a.wash_gain, a.wind.gain), (0.0, 0.0));

    let a = p(&input(&[], &[], 0.0, 0.0));
    assert!(a.motors.is_empty());
    assert_eq!(a.wash_gain, 0.0);
}

#[test]
fn non_finite_input_is_silent_and_params_are_finite() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let a = p(&input(&[bad, 20000.0], &[0.5, bad], bad, bad));
        assert!(all_finite(&a));
        assert_eq!((a.motors[0].freq_hz, a.motors[0].gain), (0.0, 0.0));
        assert_eq!((a.motors[1].freq_hz, a.motors[1].gain), (0.0, 0.0));
        assert_eq!(a.wind.gain, 0.0);
    }
    // only the bad motor is silenced
    let a = p(&input(&[f64::NAN, 20000.0], &[0.5, 0.5], 0.0, 0.0));
    assert_eq!(a.motors[0].gain, 0.0);
    assert!(a.motors[1].gain > 0.0);

    let a = p(&input(&[-100.0], &[-0.5], -10.0, -3.0));
    assert!(all_finite(&a));
    assert_eq!((a.motors[0].freq_hz, a.motors[0].gain), (0.0, 0.0));
    assert_eq!((a.wind.gain, a.wash_gain), (0.0, 0.0));
}

#[test]
fn mismatched_lengths_use_the_shorter() {
    assert_eq!(p(&input(&[1.0; 4], &[0.5; 2], 0.0, 0.0)).motors.len(), 2);
    assert_eq!(p(&input(&[1.0; 2], &[0.5; 4], 0.0, 0.0)).motors.len(), 2);
}

#[test]
fn thud_gain_maps_speed() {
    let c = AudioConfig::default();
    let g = |s: f64| thud_gain(s, &c);
    assert_eq!(g(0.0), 0.0);
    assert_eq!(g(-1.0), 0.0);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(g(bad), 0.0);
    }
    assert_eq!(g(12.0), 1.0);
    assert_eq!(g(100.0), 1.0);
    let mid = g(3.0);
    assert!(mid > 0.15 && mid < 0.9 && g(1.0) < mid && mid < g(12.0));
    let mut prev = 0.0;
    for k in 1..=300 {
        let v = g(k as f64 * 0.05);
        assert!(v >= prev && v >= 0.1);
        prev = v;
    }
    assert!(g(1e-6) >= 0.1);
}
