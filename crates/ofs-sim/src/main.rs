use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use ofs_sim::server::{sim_server, SimService};

#[derive(Parser)]
#[command(about = "Open FPV Sim headless server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen: SocketAddr,
    /// Per-quad firmware working directories (EEPROM, SITL log) are created here (default `.ofs-data`; `--replay`
    /// defaults to a temporary directory it removes afterwards).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Write the port the server bound to this file once it listens (atomically, so a reader sees all of it or
    /// nothing). With `--listen 127.0.0.1:0` the OS picks the port: a launcher cannot lose a race for it.
    #[arg(long, value_name = "FILE")]
    port_file: Option<PathBuf>,
    /// Allow listening on a non-loopback address. Any client that can connect can make this machine run programs.
    #[arg(long)]
    allow_remote: bool,
    /// Run a repro bundle (a `*.repro` folder a failed session wrote) again instead of serving, print what happened as
    /// `key=value` lines and exit: 0 reproduced, 1 not reproduced, 2 the bundle could not be run.
    #[arg(long, value_name = "BUNDLE")]
    replay: Option<PathBuf>,
}

/// `--replay`: the exit code.
fn replay(bundle: &std::path::Path, data_dir: Option<PathBuf>) -> i32 {
    let scratch = data_dir.is_none();
    let data_dir = data_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("ofs-replay-{}", std::process::id())));
    let outcome = ofs_sim::repro::replay(bundle, &data_dir);
    if scratch {
        let _ = std::fs::remove_dir_all(&data_dir);
    }
    let flat = |s: &str| s.replace(['\r', '\n'], " ");
    match outcome {
        Ok(o) => {
            println!("reproduced={}", o.reproduced);
            println!("exact={}", o.exact);
            println!("reached_tick={}", o.reached_tick);
            println!("expected_tick={}", o.expected_tick);
            println!("digest={:016x}", o.digest);
            println!("expected_digest={:016x}", o.expected_digest);
            println!("failure={}", flat(o.failure.as_deref().unwrap_or("")));
            println!("expected_failure={}", flat(&o.expected_failure));
            i32::from(!o.reproduced)
        }
        Err(message) => {
            eprintln!("ofs-sim: {message}");
            2
        }
    }
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
    if let Some(bundle) = &args.replay {
        std::process::exit(replay(bundle, args.data_dir.clone()));
    }
    if let Err(message) = ofs_sim::listen::check_listen(args.listen, args.allow_remote) {
        eprintln!("ofs-sim: {message}");
        std::process::exit(2);
    }
    if !args.listen.ip().is_loopback() {
        eprintln!("ofs-sim: warning: listening on {} (--allow-remote): every client that can connect can run programs here", args.listen);
    }
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    let bound = listener.local_addr()?;
    eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), bound);
    if let Some(file) = &args.port_file {
        let partial = PathBuf::from(format!("{}.partial", file.display()));
        std::fs::write(&partial, bound.port().to_string())?;
        let _ = std::fs::remove_file(file); // Windows refuses to rename over a file
        std::fs::rename(&partial, file)?;
    }
    let service = SimService::new(args.data_dir.unwrap_or_else(|| PathBuf::from(".ofs-data")));
    let (signalled, on_signal) = tokio::sync::oneshot::channel::<()>();
    let server = tonic::transport::Server::builder().add_service(sim_server(service.clone())).serve_with_incoming_shutdown(
        tokio_stream::wrappers::TcpListenerStream::new(listener),
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
    let result = tokio::select! {
        result = server => result.map_err(Into::into),
        _ = grace => {
            eprintln!("ofs-sim: streams still open after 3 s; closing anyway");
            Ok(())
        }
    };
    service.shutdown(); // stops the real-time runner and Betaflight SITL, also when the server failed
    result
}
