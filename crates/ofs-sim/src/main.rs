use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::server::SimService;

#[derive(Parser)]
#[command(about = "Open FPV Sim headless server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen: SocketAddr,
    /// Per-quad firmware working directories (EEPROM, SITL log) are created here.
    #[arg(long, default_value = ".ofs-data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args = Args::parse();
    eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), args.listen);
    tonic::transport::Server::builder()
        .add_service(SimServer::new(SimService::new(args.data_dir)))
        .serve(args.listen)
        .await?;
    Ok(())
}
