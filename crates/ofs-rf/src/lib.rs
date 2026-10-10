//! Shared radio-frequency models: propagation, Rician fading, receiver diversity and body shadow. The analog
//! video link and the ELRS control link both build on them. No bus and no state below the fader level.
pub mod fading;
pub mod propagation;
