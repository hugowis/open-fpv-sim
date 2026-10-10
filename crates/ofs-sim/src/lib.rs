//! Open FPV Sim server library: vehicle assembly, sessions, real-time pacing and the gRPC service.
// tonic's `Status` is the error type of every gRPC call (the generated traits fix it); boxing it would only move
// the size elsewhere.
#![allow(clippy::result_large_err)]
pub mod listen;
pub mod pacer;
pub mod runner;
pub mod server;
pub mod session;
pub mod streams;
pub mod vehicle;

/// The protocol messages and stubs live in `ofs-proto`; re-exported so server code and tests keep their paths.
pub use ofs_proto::pb;
