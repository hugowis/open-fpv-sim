//! Typed client errors, mapped from the server's `ofs-error-kind` metadata (the same kinds the Python client maps).
use tonic::{Code, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Firmware,
    Numerical,
    Protocol,
    NotLoaded,
    InvalidArgument,
    InvalidState,
    PilotBusy,
    Internal,
    /// Nothing answered, or the connection dropped.
    Unavailable,
    /// The server process could not be started or exited during startup.
    Launch,
    Other,
}

impl ErrorKind {
    /// The `ofs-error-kind` string for server kinds; `unavailable`, `launch` and `other` are client-side.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Config => "config",
            ErrorKind::Firmware => "firmware",
            ErrorKind::Numerical => "numerical",
            ErrorKind::Protocol => "protocol",
            ErrorKind::NotLoaded => "not_loaded",
            ErrorKind::InvalidArgument => "invalid_argument",
            ErrorKind::InvalidState => "invalid_state",
            ErrorKind::PilotBusy => "pilot_busy",
            ErrorKind::Internal => "internal",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::Launch => "launch",
            ErrorKind::Other => "other",
        }
    }

    fn from_server(kind: &str) -> Option<ErrorKind> {
        Some(match kind {
            "config" => ErrorKind::Config,
            "firmware" => ErrorKind::Firmware,
            "numerical" => ErrorKind::Numerical,
            "protocol" => ErrorKind::Protocol,
            "not_loaded" => ErrorKind::NotLoaded,
            "invalid_argument" => ErrorKind::InvalidArgument,
            "invalid_state" => ErrorKind::InvalidState,
            "pilot_busy" => ErrorKind::PilotBusy,
            "internal" => ErrorKind::Internal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct ClientError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ClientError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    /// Maps a gRPC status: the server's `ofs-error-kind` metadata wins. Without it, the connection going away
    /// (which tonic reports as Unavailable, Unknown "h2 protocol error", Cancelled or DeadlineExceeded) is
    /// `Unavailable`; anything else is `Other`.
    pub fn from_status(status: &Status) -> Self {
        let kind = status.metadata().get("ofs-error-kind").and_then(|v| v.to_str().ok()).and_then(ErrorKind::from_server);
        match kind {
            Some(kind) => Self::new(kind, status.message()),
            None if matches!(status.code(), Code::Unavailable | Code::Unknown | Code::Cancelled | Code::DeadlineExceeded) => {
                Self::new(ErrorKind::Unavailable, status.message())
            }
            None => Self::new(ErrorKind::Other, format!("{:?}: {}", status.code(), status.message())),
        }
    }
}