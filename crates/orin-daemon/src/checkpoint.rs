//! Periodic atomic snapshots (checkpoint) of the index to disk.

use anyhow::Result;
use orin_core::snapshot::{SnapshotHeader, load_snapshot, save_snapshot};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::state::{SharedState, State};

/// Checkpoint manager: handles periodic snapshots and recovery.
pub struct Checkpoint {
    data_dir: PathBuf,
    interval_secs: u64,
    last_checkpoint: AtomicU64,
}

impl Checkpoint {
    /// Create a new checkpoint manager.
    pub fn new(data_dir: PathBuf, interval_secs: u64) -> Self {
        Self {
            data_dir,
            interval_secs,
            last_checkpoint: AtomicU64::new(0),
        }
    }

    /// Try to load a snapshot on startup.
    pub fn try_load(&self, state: &SharedState) -> Result<bool> {
        let snapshot_path = self.data_dir.join("orin.snap");
        if !snapshot_path.exists() {
            info!("no snapshot found, starting fresh");
            return Ok(false);
        }

        info!("loading snapshot from {}", snapshot_path.display());
        let mut state = state.lock().unwrap();
        let loaded = load_snapshot(&snapshot_path, &mut state.index)?;
        if loaded {
            state.last_checkpoint = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.last_checkpoint
                .store(state.last_checkpoint, Ordering::Relaxed);
            info!("snapshot loaded: {} entries", state.index.len());
        }
        Ok(loaded)
    }

    /// Save a snapshot if interval has elapsed.
    pub fn maybe_checkpoint(&self, state: &SharedState) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last = self.last_checkpoint.load(Ordering::Relaxed);
        if now - last < self.interval_secs {
            return Ok(false);
        }

        self.save(state)?;
        Ok(true)
    }

    /// Force save a snapshot (called on shutdown).
    pub fn save(&self, state: &SharedState) -> Result<()> {
        let snapshot_path = self.data_dir.join("orin.snap");
        let tmp_path = self.data_dir.join("orin.snap.tmp");

        let mut state = state.lock().unwrap();
        save_snapshot(&state.index, &tmp_path)?;

        // Atomic rename
        std::fs::rename(&tmp_path, &snapshot_path)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        state.last_checkpoint = now;
        self.last_checkpoint.store(now, Ordering::Relaxed);

        info!(
            "checkpoint saved: {} entries, {} bytes",
            state.index.len(),
            state.index.mem_bytes()
        );
        Ok(())
    }

    /// Get snapshot info for status.
    pub fn snapshot_info(&self) -> Option<SnapshotHeader> {
        let snapshot_path = self.data_dir.join("orin.snap");
        if snapshot_path.exists() {
            let mut file = File::open(&snapshot_path).ok()?;
            let mut header_bytes = [0u8; 64];
            file.read_exact(&mut header_bytes).ok()?;
            Some(orin_core::snapshot::read_header(&header_bytes))
        } else {
            None
        }
    }
}

/// Background checkpoint task.
pub fn start_checkpoint_task(
    checkpoint: std::sync::Arc<Checkpoint>,
    state: SharedState,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(10));
            if let Err(e) = checkpoint.maybe_checkpoint(&state) {
                warn!("checkpoint error: {}", e);
            }
        }
        // Final checkpoint on shutdown
        let _ = checkpoint.save(&state);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn checkpoint_save_load() {
        let dir = tempdir().unwrap();
        let state = crate::state::new_shared(&dir.path().to_path_buf()).unwrap();
        let cp = Checkpoint::new(dir.path().to_path_buf(), 60);

        // Save
        cp.save(&state).unwrap();

        // Load into new state
        let state2 = crate::state::new_shared(&dir.path().to_path_buf()).unwrap();
        let loaded = cp.try_load(&state2).unwrap();
        assert!(loaded);
    }
}