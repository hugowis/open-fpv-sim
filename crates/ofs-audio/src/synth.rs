use crate::params::{AudioParams, MotorTone};
use std::f64::consts::TAU;

const SLEW_TAU_S: f64 = 0.02;
const WASH_SPLIT_HZ: f64 = 600.0;
const THUD_HZ: f64 = 90.0;
const THUD_TAU_S: f64 = 0.04;
const CLICK_TAU_S: f64 = 0.005;
const CLICK_LOWPASS_HZ: f64 = 800.0;
/// Below this the thud envelope counts as finished (-60 dB), so silence is exact again.
const THUD_FLOOR: f64 = 1e-3;
/// A slewed gain this close to a zero target snaps to it.
const SNAP: f64 = 5e-4;

/// Non-finite counts as 0; the rest is clamped to `[0, max]`.
fn clean(x: f64, max: f64) -> f64 {
    if x.is_finite() {
        x.clamp(0.0, max)
    } else {
        0.0
    }
}

fn slew(value: &mut f64, target: f64, k: f64) {
    *value += (target - *value) * k;
    if target == 0.0 && value.abs() < SNAP {
        *value = 0.0;
    }
}

/// One-pole low-pass step; the coefficient follows the cutoff.
fn lowpass(state: &mut f64, x: f64, cutoff_hz: f64, rate: f64) -> f64 {
    *state += (x - *state) * (1.0 - (-TAU * cutoff_hz / rate).exp());
    *state
}

#[derive(Clone, Copy, Default)]
struct Voice {
    /// Phase in cycles, wrapped to [0, 1).
    phase: f64,
    freq_hz: f64,
    gain: f64,
}

/// Mono, deterministic sound generator: motor tones, wind and prop wash noise, collision thud.
pub struct Synth {
    rate: u32,
    voices: Vec<Voice>,
    targets: Vec<MotorTone>,
    wash_target: f64,
    wind_target: (f64, f64),
    wash: f64,
    wind: (f64, f64),
    wind_lp: f64,
    wash_lp: f64,
    click_lp: f64,
    thud_env: f64,
    thud_gain: f64,
    thud_phase: f64,
    click_env: f64,
    noise: u64,
}

impl Synth {
    pub fn new(sample_rate: u32) -> Synth {
        Synth {
            rate: sample_rate.max(1),
            voices: Vec::new(),
            targets: Vec::new(),
            wash_target: 0.0,
            wind_target: (0.0, 0.0),
            wash: 0.0,
            wind: (0.0, 0.0),
            wind_lp: 0.0,
            wash_lp: 0.0,
            click_lp: 0.0,
            thud_env: 0.0,
            thud_gain: 0.0,
            thud_phase: 0.0,
            click_env: 0.0,
            noise: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Silence, and the noise seed starts over: a fresh synth.
    pub fn reset(&mut self) {
        *self = Synth::new(self.rate);
    }

    pub fn set_params(&mut self, p: &AudioParams) {
        let nyquist = f64::from(self.rate) / 2.0;
        self.targets = p
            .motors
            .iter()
            .map(|m| MotorTone { freq_hz: clean(m.freq_hz, nyquist), gain: clean(m.gain, 1.0) })
            .collect();
        // shortcut: a motor that disappears is cut at once, not faded; the count only changes between flights.
        self.voices.truncate(self.targets.len());
        for t in &self.targets[self.voices.len()..] {
            self.voices.push(Voice { phase: 0.0, freq_hz: t.freq_hz, gain: 0.0 });
        }
        self.wash_target = clean(p.wash_gain, 1.0);
        self.wind_target = (clean(p.wind.gain, 1.0), clean(p.wind.cutoff_hz, nyquist));
    }

    /// A non-finite gain is ignored; the rest is clamped to [0, 1].
    pub fn trigger_thud(&mut self, gain: f64) {
        if gain.is_finite() {
            self.thud_gain = gain.clamp(0.0, 1.0);
            self.thud_env = 1.0;
            self.click_env = 1.0;
            self.thud_phase = 0.0;
        }
    }

    fn white(&mut self) -> f64 {
        let mut x = self.noise;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.noise = x;
        (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    pub fn render(&mut self, out: &mut [f32]) {
        let rate = f64::from(self.rate);
        let k = 1.0 - (-1.0 / (SLEW_TAU_S * rate)).exp();
        let thud_decay = (-1.0 / (THUD_TAU_S * rate)).exp();
        let click_decay = (-1.0 / (CLICK_TAU_S * rate)).exp();
        for sample in out.iter_mut() {
            let mut mix = 0.0;
            for (v, t) in self.voices.iter_mut().zip(&self.targets) {
                slew(&mut v.freq_hz, t.freq_hz, k);
                slew(&mut v.gain, t.gain, k);
                v.phase = (v.phase + v.freq_hz / rate).fract();
                let a = v.phase * TAU;
                mix += v.gain * (a.sin() + 0.35 * (2.0 * a).sin() + 0.15 * (3.0 * a).sin());
            }

            slew(&mut self.wash, self.wash_target, k);
            slew(&mut self.wind.0, self.wind_target.0, k);
            slew(&mut self.wind.1, self.wind_target.1, k);
            let n = self.white();
            mix += self.wind.0 * lowpass(&mut self.wind_lp, n, self.wind.1, rate);
            mix += 0.5 * self.wash * (n - lowpass(&mut self.wash_lp, n, WASH_SPLIT_HZ, rate));

            if self.thud_env >= THUD_FLOOR {
                let click = lowpass(&mut self.click_lp, n * self.click_env, CLICK_LOWPASS_HZ, rate);
                mix += self.thud_gain
                    * (self.thud_env * (TAU * THUD_HZ * self.thud_phase / rate).sin() + click);
                self.thud_phase += 1.0;
                self.thud_env *= thud_decay;
                self.click_env *= click_decay;
            } else {
                self.thud_env = 0.0;
            }

            // The soft clip bounds the sum to [-1, 1] however many motors or how loud.
            *sample = (0.5 * mix).tanh() as f32;
        }
    }
}
