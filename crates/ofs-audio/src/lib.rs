//! Procedural flight audio: the pure sound parameters and a deterministic synth. No Godot, no I/O.
mod audio;
mod params;
mod synth;

pub use audio::Audio;
pub use params::{
    params, thud_gain, AudioConfig, AudioInput, AudioParams, MotorTone, Wind, SAMPLE_RATE,
};
pub use synth::Synth;
