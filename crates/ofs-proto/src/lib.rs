//! Protocol 4 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
//! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.

pub mod pb {
    tonic::include_proto!("ofs.v1");
}

/// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
pub const PROTOCOL_VERSION: u32 = 4;
