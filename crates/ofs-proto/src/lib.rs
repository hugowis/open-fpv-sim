//! Protocol 5 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
//! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.
// tonic's `Status` is the error type of every gRPC call (the generated traits fix it); boxing it would only move
// the size elsewhere.
#![allow(clippy::result_large_err)]

pub mod pb {
    tonic::include_proto!("ofs.v1");
}

/// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
pub const PROTOCOL_VERSION: u32 = 5;

/// The unit vector (north, east, down) a heading and an elevation point along: how the server builds an antenna's
/// aim and how clients draw it (`ReceiverAntenna.aim_az_deg` is added to the pilot's facing to give the heading).
/// Heading 0 is north, 90 east; elevation is up.
pub fn aim_ned(heading_deg: f64, elevation_deg: f64) -> [f64; 3] {
    let (h, e) = (heading_deg.to_radians(), elevation_deg.to_radians());
    [e.cos() * h.cos(), e.cos() * h.sin(), -e.sin()]
}

#[cfg(test)]
mod tests {
    #[test]
    fn aim_ned_points_along_heading_and_elevation() {
        let close = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12);
        assert!(close(super::aim_ned(0.0, 0.0), [1.0, 0.0, 0.0]), "north");
        assert!(close(super::aim_ned(90.0, 0.0), [0.0, 1.0, 0.0]), "east");
        assert!(close(super::aim_ned(0.0, 90.0), [0.0, 0.0, -1.0]), "up is -down");
    }
}
