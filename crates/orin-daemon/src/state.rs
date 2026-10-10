//! Daemon state: index, config, roots, watcher handle, metrics.

use anyhow::Result;
use orin_core::index::Index;
use orin_core::paths::socket_name;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use sysinfo::System;

/// Shared daemon state, guarded by a mutex for interior mutability.
#[allow(dead_code)]
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
    #[allow(dead_code)]
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
    pub first: u32,    // index of first entry in this root
    pub count: u32,    // number of entries in this root
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
    pub fn new(_data_dir: &Path) -> Result<Self> {
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
pub fn new_shared(data_dir: &Path) -> Result<SharedState> {
    Ok(Arc::new(Mutex::new(State::new(data_dir)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_creation() {
        let dir = tempdir().unwrap();
        let shared = test_support::new_shared_hermetic(dir.path(), "state_creation").unwrap();
        let state = shared.lock().unwrap();
        assert_eq!(state.index.len(), 0);
        assert!(!state.roots.is_empty());
        assert!(state.listener.is_some());
    }
}

/// Test-only helpers for hermetic listener creation.
///
/// `State::new` binds an IPC listener whose name comes from `ORIN_SOCKET`
/// (or a fixed per-user default). Unit tests run in parallel threads of one
/// process, so every test must bind a unique name. The env var is
/// process-global, so its mutation is serialized by `LISTENER_LOCK` and the
/// live value is visible only for the duration of the bind.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{RootEntry, SharedState, State};
    use anyhow::Result;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};
    use sysinfo::System;

    /// Serializes ORIN_SOCKET mutation + listener binding across all tests
    /// that create a listener in this test binary.
    static LISTENER_LOCK: Mutex<()> = Mutex::new(());
    /// Distinguishes successive binds within the test process.
    static NEXT_BIND_ID: AtomicU32 = AtomicU32::new(0);

    /// Create shared state with a unique IPC socket/pipe name.
    ///
    /// Windows: a bare namespaced name — interprocess's `GenericNamespaced`
    /// prepends `\\.\pipe\` itself, and a pre-prefixed value would be doubled
    /// into an invalid pipe path (backslashes are rejected inside the pipe
    /// name; that doubling is the historical source of "Access is denied").
    /// Unix: a socket file inside the caller's own tempdir.
    ///
    /// The name is unique per call (pid + bind counter + test label), so
    /// parallel tests never collide with "Address already in use" (os error
    /// 98) or "Access is denied" (os error 5).
    ///
    /// The listener is bound directly (mirroring `State::create_listener`)
    /// instead of going through `State::new`, so the env-var window contains
    /// only the name lookup + bind — not the slow `System::new_all()` pass —
    /// and parallel tests reading the ambient `ORIN_SOCKET` (e.g.
    /// `server::tests`) essentially never observe a hermetic name.
    pub(crate) fn new_shared_hermetic(data_dir: &Path, label: &str) -> Result<SharedState> {
        let _lock = LISTENER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("ORIN_SOCKET").ok();
        let pid = std::process::id();
        let id = NEXT_BIND_ID.fetch_add(1, Ordering::Relaxed);
        let name = if cfg!(windows) {
            format!("orin-test-{pid}-{id}-{label}")
        } else {
            data_dir.join(format!("orin-test-{pid}-{id}.sock")).display().to_string()
        };
        // SAFETY: env mutation is serialized across all listener-creating
        // tests by LISTENER_LOCK, and the previous value is restored below
        // before the lock is released.
        unsafe { std::env::set_var("ORIN_SOCKET", &name) };
        let listener = interprocess::local_socket::ListenerOptions::new()
            .name(orin_core::paths::socket_name()?)
            .create_sync()?;
        if let Some(prev) = prev {
            unsafe { std::env::set_var("ORIN_SOCKET", prev) };
        } else {
            unsafe { std::env::remove_var("ORIN_SOCKET") };
        }

        // Same shape as State::new minus the listener creation (done above)
        // and with a cheaper System::new(); tests don't inspect sys.
        let state = State {
            index: orin_core::index::Index::new(),
            roots: orin_core::config::default_roots()
                .into_iter()
                .map(|p| RootEntry {
                    path: p.into(),
                    first: u32::MAX,
                    count: 0,
                    watch: "auto".to_string(),
                })
                .collect(),
            listener: Some(listener),
            sys: System::new(),
            total_entries: 0,
            unreadable: 0,
            last_checkpoint: 0,
            scan_progress: None,
        };
        Ok(Arc::new(Mutex::new(state)))
    }
}
