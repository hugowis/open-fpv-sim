//! Rust client for the `ofs-sim` server. The Godot extension is a thin layer over it.
pub mod error;
pub mod frames;
pub mod interp;
pub mod model;

pub use error::{ClientError, ErrorKind};
pub use model::*;