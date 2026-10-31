//! Procedural flight audio: the pure sound parameters and a deterministic synth. No Godot, no I/O.
mod params;

pub use params::{
    params, thud_gain, AudioConfig, AudioInput, AudioParams, MotorTone, Wind, SAMPLE_RATE,
};
