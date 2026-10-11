//! orin-daemon — the long-running search index daemon.
//!
//! Library behind the `orin daemon` subcommand: it runs as a background
//! service, owns the in-memory index, watches the filesystem for changes,
//! and serves queries over a Unix socket (or Windows named pipe).

pub mod apply;
pub mod checkpoint;
pub mod priority;
pub mod revalidate;
pub mod scan;
pub mod server;
pub mod state;
pub mod watch;

use anyhow::Result;
use clap::Args;
use orin_core::paths::{data_dir, socket_name};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

// Foreground arguments of `orin daemon` (the daemon's own CLI options).
#[derive(Debug, Args)]
pub struct DaemonArgs {
    /// Data directory (overrides ORIN_DATA_DIR)
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,

    /// Socket path (overrides ORIN_SOCKET)
    #[arg(long, value_name = "PATH")]
    pub socket: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Run in foreground (don't daemonize)
    #[arg(long)]
    pub foreground: bool,

    /// Exit after idle seconds (0 = never)
    #[arg(long, default_value = "0")]
    pub idle_exit: u64,
}

/// Run the daemon in the foreground until it shuts down.
///
/// `orin daemon` calls this with the parsed command-line arguments; it used
/// to be the body of a separate daemon executable.
pub fn run_daemon(args: DaemonArgs) -> Result<()> {
    // Initialize tracing
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));
    fmt().with_env_filter(filter).init();

    // Override env vars if provided
    if let Some(d) = args.data_dir {
        unsafe {
            std::env::set_var("ORIN_DATA_DIR", &d);
        }
    }
    if let Some(s) = args.socket {
        unsafe {
            std::env::set_var("ORIN_SOCKET", &s);
        }
    }

    let data_dir = data_dir();
    info!("orin daemon starting");
    info!("data_dir: {}", data_dir.display());
    let socket = socket_name()?;
    info!("socket: {:?}", socket);
    info!("idle_exit: {}s", args.idle_exit);

    // Ensure data dir exists
    std::fs::create_dir_all(&data_dir)?;

    // Set up signal handling: the handler flags shutdown and wakes the
    // accept waker through its condvar — nothing polls the flag.
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let waker = server::AcceptWaker::start(socket);
    let shutdown_clone = shutdown.clone();
    let waker_clone = waker.clone();
    ctrlc::set_handler(move || {
        shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        waker_clone.notify_shutdown();
    })?;

    // Create and run server
    let state = state::new_shared(&data_dir)?;
    let checkpoint = std::sync::Arc::new(checkpoint::Checkpoint::new(data_dir.clone(), 60));
    if let Err(e) = checkpoint.try_load(&state) {
        tracing::warn!("snapshot load failed: {}", e);
    }

    // Initial scan and the one startup revalidation run in the background
    // while the server answers: real disks take minutes, and status must
    // report "building" with progress instead of going silent. (A joining
    // scope here made the daemon unanswerable at whole-disk scale.) Queries
    // during the build see an empty-or-growing index and MCP's partial flag
    // tells callers to retry. The offline-build-then-swap inside
    // initial_scan can clobber watcher-applied entries; the startup
    // revalidation pass runs after the swap and heals exactly that drift.
    {
        let state = state.clone();
        std::thread::spawn(move || {
            priority::set_below_normal();
            state.lock().unwrap().scan_progress = Some(state::ScanProgress {
                entries: 0,
                since_ms: 0,
            });
            if let Err(e) = scan::initial_scan(&state) {
                tracing::warn!("initial scan failed: {}", e);
            }
            if let Err(e) = revalidate::Revalidate::run(&state) {
                tracing::warn!("startup revalidation failed: {}", e);
            }
            state.lock().unwrap().scan_progress = None;
        });
    }

    // Watch roots for changes -> apply. Deferred to its own thread: on
    // whole-disk roots the watcher setup can take seconds, and nothing may
    // delay the accept loop (first-contact latency is a product metric).
    let watcher_slot: std::sync::Arc<std::sync::Mutex<Option<watch::WatcherHandle>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    {
        let state = state.clone();
        let slot = watcher_slot.clone();
        let checkpoint = checkpoint.clone();
        let shutdown = shutdown.clone();
        std::thread::spawn(move || {
            let roots: Vec<PathBuf> = {
                let st = state.lock().unwrap();
                st.roots.iter().map(|r| r.path.clone()).collect()
            };
            match watch::WatcherHandle::start(roots, state.clone()) {
                Ok(w) => *slot.lock().unwrap() = Some(w),
                Err(e) => {
                    tracing::warn!("watcher start failed: {}", e);
                }
            }
            // Background checkpoint loop: the only periodic work left, kept
            // for snapshot durability (one save per interval, plus one on
            // shutdown). Revalidation is deliberately not periodic.
            let _cp_task =
                checkpoint::start_checkpoint_task(checkpoint, state.clone(), shutdown);
        });
    }

    let mut server = server::Server::new(state.clone(), args.idle_exit)?;
    server.run(shutdown.clone(), waker)?;

    // Shut down cleanly: stop watcher, final checkpoint
    if let Some(w) = watcher_slot.lock().unwrap().take() {
        w.stop();
    }
    if let Err(e) = checkpoint.save(&state) {
        tracing::warn!("final checkpoint failed: {}", e);
    }

    info!("orin daemon stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn daemon_modules_compile() {
        // Just verify all modules compile
        let _ = apply::FsEvent::Create {
            path: std::path::PathBuf::new(),
            is_dir: false,
        };
        let _ = checkpoint::Checkpoint::new(std::path::PathBuf::new(), 60);
        let _rv = revalidate::Revalidate::run;
        // Walk a small unique tempdir instead of the process cwd, which can
        // be the whole workspace (including target/) or the system dir on CI.
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let _ = scan::scan_root(dir.path(), 0, &mut orin_core::index::Index::new());
    }
}
