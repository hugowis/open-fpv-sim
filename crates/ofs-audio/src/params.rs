/// Sample rate of the synth, defined once here; Godot reads it through the client.
pub const SAMPLE_RATE: u32 = 44100;

/// Tuning of the sound mapping.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioConfig {
    /// Propeller blades: the motor tone is the blade-pass frequency.
    pub blades: f64,
    /// Gain of one motor at full command, before the 1/sqrt(n) mix.
    pub motor_gain: f64,
    pub wash_descent_ref_mps: f64,
    pub wind_ref_mps: f64,
    pub wind_exponent: f64,
    pub wind_cutoff_min_hz: f64,
    pub wind_cutoff_max_hz: f64,
    pub wind_cutoff_full_mps: f64,
    pub thud_ref_mps: f64,
    pub thud_exponent: f64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            blades: 3.0,
            motor_gain: 0.5,
            wash_descent_ref_mps: 5.0,
            wind_ref_mps: 25.0,
            wind_exponent: 1.5,
            wind_cutoff_min_hz: 300.0,
            wind_cutoff_max_hz: 3500.0,
            wind_cutoff_full_mps: 30.0,
            thud_ref_mps: 12.0,
            thud_exponent: 0.7,
        }
    }
}

/// What the sim tells the audio each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInput {
    pub motor_rpm: Vec<f64>,
    pub motor_cmd: Vec<f64>,
    pub airspeed_mps: f64,
    pub descent_mps: f64,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotorTone {
    pub freq_hz: f64,
    pub gain: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wind {
    pub gain: f64,
    pub cutoff_hz: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioParams {
    pub motors: Vec<MotorTone>,
    pub wash_gain: f64,
    pub wind: Wind,
}

/// A non-finite value counts as 0; negatives clamp to 0.
fn nonneg(x: f64) -> f64 {
    if x.is_finite() {
        x.max(0.0)
    } else {
        0.0
    }
}

/// Pure map from the flight state to sound parameters. Never returns a non-finite value.
pub fn params(input: &AudioInput, cfg: &AudioConfig) -> AudioParams {
    let n = input.motor_rpm.len().min(input.motor_cmd.len());
    let mix = if n > 0 { cfg.motor_gain / (n as f64).sqrt() } else { 0.0 };
    let on = if input.active { 1.0 } else { 0.0 };

    let mut motors = Vec::with_capacity(n);
    let (mut sum, mut lo, mut hi, mut count) = (0.0, f64::MAX, 0.0_f64, 0u32);
    for (&rpm, &cmd) in input.motor_rpm.iter().zip(&input.motor_cmd) {
        if rpm.is_finite() && cmd.is_finite() {
            let cmd = cmd.clamp(0.0, 1.0);
            motors.push(MotorTone {
                freq_hz: rpm.max(0.0) / 60.0 * cfg.blades,
                gain: cmd.powf(1.5) * mix * on,
            });
        } else {
            motors.push(MotorTone::default());
        }
        if cmd.is_finite() {
            let cmd = cmd.clamp(0.0, 1.0);
            sum += cmd;
            lo = lo.min(cmd);
            hi = hi.max(cmd);
            count += 1;
        }
    }

    let wash_gain = if count > 0 {
        let mean = sum / f64::from(count);
        let descent = (nonneg(input.descent_mps) / cfg.wash_descent_ref_mps).clamp(0.0, 1.0);
        (mean * mean * (0.25 + 0.75 * descent) + 0.5 * (hi - lo) * mean).clamp(0.0, 1.0) * on
    } else {
        0.0
    };

    let speed = nonneg(input.airspeed_mps);
    let wind = Wind {
        gain: (speed / cfg.wind_ref_mps).powf(cfg.wind_exponent).clamp(0.0, 1.0) * on,
        cutoff_hz: cfg.wind_cutoff_min_hz
            + (cfg.wind_cutoff_max_hz - cfg.wind_cutoff_min_hz)
                * (speed / cfg.wind_cutoff_full_mps).clamp(0.0, 1.0),
    };

    AudioParams { motors, wash_gain, wind }
}

/// Thud loudness for a collision at this speed; any real hit is at least audible.
pub fn thud_gain(collision_speed_mps: f64, cfg: &AudioConfig) -> f64 {
    if !collision_speed_mps.is_finite() || collision_speed_mps <= 0.0 {
        return 0.0;
    }
    (collision_speed_mps / cfg.thud_ref_mps)
        .clamp(0.0, 1.0)
        .powf(cfg.thud_exponent)
        .max(0.1)
}
