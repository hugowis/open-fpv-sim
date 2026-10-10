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

/// How to fix a SITL that exited during startup because `wsl.exe` could not find the program after `-e` (WSL itself
/// started, so this is not a [`FcError::Launch`]). Empty when the output does not show that.
pub fn startup_hint(argv: &[String], output: &str) -> String {
    let Some(prefix) = net::wsl_prefix(argv) else {
        return String::new();
    };
    let missing = output.contains("No such file or directory") || output.contains("execvpe");
    match argv.get(prefix.len()) {
        Some(program) if missing => format!(
            "\nWSL could not run {program}: the path after `-e` must be the Betaflight SITL binary's Linux path inside \
             WSL (e.g. /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf, built by scripts/build-sitl.sh). \
             See docs/dev-setup.md."
        ),
        _ => String::new(),
    }
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
