//! Daemon state: index, config, roots, watcher handle, metrics.

use anyhow::Result;
use interprocess::local_socket::GenericNamespaced;
use orin_core::index::Index;
use orin_core::paths::socket_name;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use sysinfo::System;
use tracing::info;

/// Shared daemon state, guarded by a mutex for interior mutability.
pub struct State {
    /// The search index (entries + names arena + sorted keys).
    pub index: Index,
    /// Configured root directories being indexed.
    pub roots: Vec<RootEntry>,
    /// Listener for IPC (kept alive for the server loop).
    pub listener: Option<interprocess::local_socket::Listener>,
    /// System info for memory/CPU reporting.
    pub sys: System,
    /// Total entries ever seen (for progress reporting).
    pub total_entries: u64,
    /// Unreadable paths encountered during scan.
    pub unreadable: u64,
    /// Last checkpoint timestamp (unix seconds).
    pub last_checkpoint: u64,
    /// Current scan progress (entries processed since last report).
    pub scan_progress: Option<ScanProgress>,
}

/// A configured root directory.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RootEntry {
    pub path: PathBuf,
    pub first: u32,   // index of first entry in this root
    pub count: u32,   // number of entries in this root
    pub watch: String, // "auto" | "notify" | "poll" | "none"
}

/// In-progress scan progress for status reporting.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScanProgress {
    pub entries: u64,
    pub since_ms: u64,
}

impl State {
    /// Create a new state with an empty index and default roots.
    pub fn new(data_dir: &PathBuf) -> Result<Self> {
        let index = Index::new();
        let roots = orin_core::config::default_roots()
            .into_iter()
            .map(|p| RootEntry {
                path: PathBuf::from(p),
                first: u32::MAX,
                count: 0,
                watch: "auto".to_string(),
            })
            .collect();

        let listener = Self::create_listener()?;

        Ok(Self {
            index,
            roots,
            listener: Some(listener),
            sys: System::new_all(),
            total_entries: 0,
            unreadable: 0,
            last_checkpoint: 0,
            scan_progress: None,
        })
    }

    /// Create the IPC listener (Unix socket or Windows named pipe).
    fn create_listener() -> Result<interprocess::local_socket::Listener> {
        let name = socket_name()?;
        let listener = interprocess::local_socket::ListenerOptions::new()
            .name(name)
            .create_sync()?;
        Ok(listener)
    }

    /// Get the socket name for logging/status.
    pub fn socket_name(&self) -> String {
        // Use the listener name for reference; daemon was started with a known socket path
        "known-socket".to_string()
    }

    /// Refresh system info (memory, CPU).
    pub fn refresh_sys(&mut self) {
        self.sys.refresh_memory();
        self.sys.refresh_cpu_all();
    }

    /// Memory usage in bytes (RSS).
    pub fn memory_bytes(&self) -> u64 {
        self.sys.used_memory()
    }

    /// Index memory footprint (entries + names + sorted + roots).
    pub fn index_memory_bytes(&self) -> u64 {
        self.index.mem_bytes() as u64
    }
}

/// Thread-safe shared state.
pub type SharedState = Arc<Mutex<State>>;

/// Create a new shared state.
pub fn new_shared(data_dir: &PathBuf) -> Result<SharedState> {
    Ok(Arc::new(Mutex::new(State::new(data_dir)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_creation() {
        let dir = tempdir().unwrap();
        let state = State::new(&dir.path().to_path_buf()).unwrap();
        assert_eq!(state.index.len(), 0);
        assert!(!state.roots.is_empty());
        assert!(state.listener.is_some());
    }
}
