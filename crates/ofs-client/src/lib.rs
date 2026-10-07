//! Rust client for the `ofs-sim` server. The Godot extension is a thin layer over it.
//!
//! `Client::start` launches a supervisor on its own threads: it finds (or starts) the server, loads the quad
//! in real time, opens the event stream and the pilot link, and keeps them alive. A game loop calls `poll` for
//! what happened, `pose` and `telemetry` for what to draw, and `set_sticks` with what the pilot did; none of
//! them blocks.
mod client;
pub mod error;
pub mod frames;
pub mod interp;
pub mod launch;
pub mod model;
mod worker;

pub use client::Client;
pub use error::{ClientError, ErrorKind};
pub use interp::Pose;
pub use model::*;
