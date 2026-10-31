use crate::params::{params, thud_gain, AudioConfig, AudioInput};
use crate::synth::Synth;
use crate::SAMPLE_RATE;

const MAX_PENDING: usize = 16;

/// The sim-facing side: queued collisions, flight state in, samples out.
pub struct Audio {
    cfg: AudioConfig,
    synth: Synth,
    /// Collision times (sim seconds) waiting for the state that carries the impact speed.
    pending: Vec<f64>,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    pub fn new() -> Audio {
        Audio { cfg: AudioConfig::default(), synth: Synth::new(SAMPLE_RATE), pending: Vec::new() }
    }

    pub fn collision_event(&mut self, time_s: f64) {
        if time_s.is_finite() {
            if self.pending.len() == MAX_PENDING {
                self.pending.remove(0);
            }
            self.pending.push(time_s);
        }
    }

    pub fn frame(
        &mut self,
        input: &AudioInput,
        state_time_s: f64,
        collision_speed_mps: f64,
        frames: usize,
    ) -> Vec<f32> {
        let before = self.pending.len();
        // A NaN time compares false with everything, so it releases nothing.
        self.pending.retain(|&t| t > state_time_s || state_time_s.is_nan());
        if self.pending.len() < before && collision_speed_mps > 0.0 {
            self.synth.trigger_thud(thud_gain(collision_speed_mps, &self.cfg));
        }
        self.synth.set_params(&params(input, &self.cfg));
        let mut out = vec![0.0; frames];
        self.synth.render(&mut out);
        out
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.synth.reset();
    }
}
