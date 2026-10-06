//! Open FPV Sim server library: vehicle assembly, sessions, real-time pacing and the gRPC service.
pub mod pacer;
pub mod runner;
pub mod server;
pub mod session;
pub mod vehicle;

pub mod pb {
    tonic::include_proto!("ofs.v1");
}
