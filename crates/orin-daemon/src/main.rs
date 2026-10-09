//! orin-daemon — the long-running search index daemon.
//!
//! This binary runs as a background service, owns the in-memory index,
//! watches the filesystem for changes, and serves queries over a
//! Unix socket (or Windows named pipe).

mod state;
mod server;
mod scan;
mod watch;
mod apply;
mod checkpoint;
mod revalidate;

use anyhow::Result;
use clap::Parser;
use orin_core::paths::{data_dir, socket_name};
use std::path::PathBuf;
use tracing::{info};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser, Debug)]
#[command(name = "orind", version, about = "orin daemon — whole-disk instant file search")]
struct Args {
    /// Data directory (overrides ORIN_DATA_DIR)
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Socket path (overrides ORIN_SOCKET)
    #[arg(long, value_name = "PATH")]
    socket: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Run in foreground (don't daemonize)
    #[arg(long)]
    foreground: bool,

    /// Exit after idle seconds (0 = never)
    #[arg(long, default_value = "0")]
    idle_exit: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&args.log_level));
    fmt().with_env_filter(filter).init();

    // Override env vars if provided
    if let Some(d) = args.data_dir {
        unsafe { std::env::set_var("ORIN_DATA_DIR", &d); }
    }
    if let Some(s) = args.socket {
        unsafe { std::env::set_var("ORIN_SOCKET", &s); }
    }

    let data_dir = data_dir();
    info!("orin daemon starting");
    info!("data_dir: {}", data_dir.display());
    info!("socket: {:?}", socket_name()?);
    info!("idle_exit: {}s", args.idle_exit);

    // Ensure data dir exists
    std::fs::create_dir_all(&data_dir)?;

    // Set up signal handling
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    })?;

    // Create and run server
    let mut server = server::Server::new(data_dir, args.idle_exit)?;
    server.run(shutdown)?;

    info!("orin daemon stopped");
    Ok(())
}
