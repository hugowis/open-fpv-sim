//! Where the server may listen. `Load` reads any file on the host and starts the quad file's `fc.launch` program,
//! so a server that other machines can reach lets them run programs here.
use std::net::SocketAddr;

pub fn check_listen(addr: SocketAddr, allow_remote: bool) -> Result<(), String> {
    if addr.ip().is_loopback() || allow_remote {
        return Ok(());
    }
    Err(format!(
        "refusing to listen on {addr}: it is not a loopback address, and any client that can reach it could make this \
         machine run programs (a quad file's `fc.launch`). Listen on 127.0.0.1, or pass --allow-remote if you trust \
         every machine that can connect."
    ))
}