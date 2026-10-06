use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

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

/// Ctrl-C, or SIGTERM on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args = Args::parse();
    eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), args.listen);
    let service = SimService::new(args.data_dir);
    let (signalled, on_signal) = tokio::sync::oneshot::channel::<()>();
    let server = tonic::transport::Server::builder().add_service(SimServer::new(service.clone())).serve_with_shutdown(
        args.listen,
        async move {
            shutdown_signal().await;
            eprintln!("ofs-sim: shutting down");
            let _ = signalled.send(());
        },
    );
    // Open streams (Watch, Pilot, StreamState) would hold a graceful shutdown forever: give them 3 s.
    let grace = async move {
        if on_signal.await.is_ok() {
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        result = server => result?,
        _ = grace => eprintln!("ofs-sim: streams still open after 3 s; closing anyway"),
    }
    service.shutdown(); // stops the real-time runner and Betaflight SITL
    Ok(())
}
