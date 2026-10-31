use ofs_audio::{AudioParams, MotorTone, Synth, Wind, SAMPLE_RATE};
use std::f64::consts::PI;

const RATE: f64 = SAMPLE_RATE as f64;

fn goertzel(samples: &[f32], freq: f64, rate: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &s) in samples.iter().enumerate() {
        let a = 2.0 * PI * freq * i as f64 / rate;
        re += f64::from(s) * a.cos();
        im += f64::from(s) * a.sin();
    }
    (re * re + im * im).sqrt() / samples.len() as f64
}

fn rms(s: &[f32]) -> f64 {
    (s.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
}

fn secs(s: f64) -> usize {
    (s * RATE) as usize
}

fn render(synth: &mut Synth, seconds: f64) -> Vec<f32> {
    let mut out = vec![0.0; secs(seconds)];
    synth.render(&mut out);
    out
}

fn tone(freq_hz: f64, gain: f64) -> AudioParams {
    AudioParams { motors: vec![MotorTone { freq_hz, gain }], ..Default::default() }
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|s| s.to_bits()).collect()
}

#[test]
fn same_calls_give_identical_samples() {
    let run = || {
        let mut s = Synth::new(SAMPLE_RATE);
        let mut p = tone(1200.0, 0.6);
        p.wind = Wind { gain: 0.5, cutoff_hz: 1000.0 };
        p.wash_gain = 0.4;
        s.set_params(&p);
        let mut all = Vec::new();
        for n in [100, 333, 1024, 7, 2000] {
            let mut b = vec![0.0; n];
            s.render(&mut b);
            all.extend(b);
        }
        s.trigger_thud(0.8);
        for n in [64, 4096] {
            let mut b = vec![0.0; n];
            s.render(&mut b);
            all.extend(b);
        }
        all
    };
    assert_eq!(bits(&run()), bits(&run()));
}

#[test]
fn a_motor_tone_peaks_at_its_blade_pass() {
    let mut s = Synth::new(SAMPLE_RATE);
    s.set_params(&tone(1500.0, 0.5));
    render(&mut s, 0.1);
    let out = render(&mut s, 0.25);
    let at = goertzel(&out, 1500.0, RATE);
    assert!(at > 5.0 * goertzel(&out, 1000.0, RATE));
    assert!(at > 5.0 * goertzel(&out, 2200.0, RATE));
}

#[test]
fn the_tone_follows_a_changed_frequency() {
    let mut s = Synth::new(SAMPLE_RATE);
    s.set_params(&tone(1000.0, 0.5));
    render(&mut s, 0.2);
    s.set_params(&tone(2000.0, 0.5));
    let out = render(&mut s, 0.2);
    let out = &out[secs(0.1)..];
    assert!(goertzel(out, 2000.0, RATE) > 5.0 * goertzel(out, 1000.0, RATE));
}

#[test]
fn silence_is_exact_after_the_slew() {
    let mut s = Synth::new(SAMPLE_RATE);
    s.set_params(&AudioParams::default());
    render(&mut s, 0.1);
    assert!(render(&mut s, 0.05).iter().all(|&x| x == 0.0));
}

#[test]
fn wind_is_quieter_low_passed_at_low_cutoff() {
    let noise = |cutoff_hz| {
        let mut s = Synth::new(SAMPLE_RATE);
        s.set_params(&AudioParams { wind: Wind { gain: 0.8, cutoff_hz }, ..Default::default() });
        render(&mut s, 0.2);
        rms(&render(&mut s, 0.3))
    };
    assert!(noise(3500.0) > noise(300.0));
}

#[test]
fn thud_is_audible_and_decays() {
    let mut s = Synth::new(SAMPLE_RATE);
    s.trigger_thud(1.0);
    let out = render(&mut s, 0.5);
    let head = rms(&out[..secs(0.05)]);
    assert!(head > 0.02, "head {head}");
    assert!(head > 10.0 * rms(&out[secs(0.4)..secs(0.5)]));

    let mut quiet = Synth::new(SAMPLE_RATE);
    quiet.trigger_thud(0.0);
    assert!(render(&mut quiet, 0.2).iter().all(|&x| x == 0.0));
}

#[test]
fn output_stays_in_range_and_finite() {
    let mut s = Synth::new(SAMPLE_RATE);
    let loud = AudioParams {
        motors: vec![MotorTone { freq_hz: 1e9, gain: 1.0 }; 4],
        wash_gain: 1.0,
        wind: Wind { gain: 1.0, cutoff_hz: 1e9 },
    };
    s.set_params(&loud);
    s.trigger_thud(1.0);
    for x in render(&mut s, 0.3) {
        assert!(x.is_finite() && x.abs() <= 1.0, "{x}");
    }

    let nan = f64::NAN;
    s.set_params(&AudioParams {
        motors: vec![MotorTone { freq_hz: nan, gain: nan }; 2],
        wash_gain: nan,
        wind: Wind { gain: nan, cutoff_hz: nan },
    });
    s.trigger_thud(nan);
    for x in render(&mut s, 0.3) {
        assert!(x.is_finite() && x.abs() <= 1.0, "{x}");
    }
}

#[test]
fn a_zero_length_buffer_is_fine() {
    let mut s = Synth::new(SAMPLE_RATE);
    s.render(&mut []);
}
