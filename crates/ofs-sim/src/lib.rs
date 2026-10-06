//! Open FPV Sim server library: vehicle assembly and the gRPC service.
pub mod pacer;
pub mod server;
pub mod vehicle;

pub mod pb {
    tonic::include_proto!("ofs.v1");
}
