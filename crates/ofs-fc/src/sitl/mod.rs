//! Betaflight SITL bridge.
pub mod bridge;
pub mod codec;
pub mod frames;
pub mod net;
pub mod process;

#[derive(Debug, thiserror::Error)]
pub enum FcError {
    #[error("UDP port {port} is already in use: {hint}")]
    PortInUse { port: u16, hint: &'static str },
    #[error("failed to launch Betaflight SITL `{command}`: {source}")]
    Launch { command: String, source: std::io::Error },
    #[error("Betaflight SITL config step failed: {0}")]
    Config(String),
    #[error("Betaflight SITL {0}; last output:\n{1}")]
    Startup(String, String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
