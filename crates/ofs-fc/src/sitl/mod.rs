//! Betaflight SITL bridge.
pub mod bridge;
pub mod codec;
pub mod esc_telemetry;
pub mod frames;
pub mod net;
pub mod process;

#[derive(Debug, thiserror::Error)]
pub enum FcError {
    #[error("UDP port {port} is already in use: {hint}")]
    PortInUse { port: u16, hint: &'static str },
    #[error("failed to launch Betaflight SITL `{command}`: {source}{}", launch_hint(.source))]
    Launch { command: String, source: std::io::Error },
    #[error("Betaflight SITL config step failed: {0}")]
    Config(String),
    #[error("Betaflight SITL {0}; last output:\n{1}")]
    Startup(String, String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// How to fix a launch program that does not exist (spec §7: platform-specific instructions).
fn launch_hint(source: &std::io::Error) -> &'static str {
    if source.kind() != std::io::ErrorKind::NotFound {
        return "";
    }
    if cfg!(windows) {
        "\nBuild Betaflight SITL inside WSL with scripts/build-sitl.sh, then set OFS_SITL_LAUNCH to \
         `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf` \
         (or fc.launch in the quad file). See docs/dev-setup.md."
    } else {
        "\nBuild Betaflight SITL with scripts/build-sitl.sh, then set OFS_SITL_LAUNCH to the path of \
         obj/main/betaflight_SITL.elf (or fc.launch in the quad file). See docs/dev-setup.md."
    }
}
