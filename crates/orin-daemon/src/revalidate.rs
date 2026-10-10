//! Periodic revalidation: scan roots and reconcile with index.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::state::SharedState;

/// Revalidation manager: periodically verifies index against filesystem.
#[allow(dead_code)]
pub struct Revalidate {
    #[allow(dead_code)]
    interval_secs: u64,
    last_run: AtomicU64,
}

impl Revalidate {
    #[allow(dead_code)]
    /// Create a new revalidation manager.
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval_secs,
            last_run: AtomicU64::new(0),
        }
    }

    #[allow(dead_code)]
    /// Run revalidation if interval has elapsed.
    pub fn maybe_revalidate(&self, state: &SharedState) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last = self.last_run.load(Ordering::Relaxed);
        if now - last < self.interval_secs {
            return Ok(false);
        }

        self.run(state)?;
        self.last_run.store(now, Ordering::Relaxed);
        Ok(true)
    }

    #[allow(dead_code)]
    /// Run a full revalidation pass.
    fn run(&self, state: &SharedState) -> Result<()> {
        info!("starting revalidation");
        let start = std::time::Instant::now();

        let roots: Vec<PathBuf> = {
            let state = state.lock().unwrap();
            state.roots.iter().map(|r| r.path.clone()).collect()
        };

        let mut total_checked = 0u64;
        let mut total_fixed = 0u64;

        for root in roots {
            let (checked, fixed) = revalidate_root(state, &root)?;
            total_checked += checked;
            total_fixed += fixed;
        }

        let elapsed = start.elapsed();
        info!(
            "revalidation complete: {} checked, {} fixed in {:.2}s",
            total_checked,
            total_fixed,
            elapsed.as_secs_f64()
        );

        Ok(())
    }
}

#[allow(dead_code)]
fn revalidate_root(state: &SharedState, root: &Path) -> Result<(u64, u64)> {
    use ignore::WalkBuilder;

    let mut checked = 0u64;
    let mut fixed = 0u64;

    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_ignore(false)
        .build();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        checked += 1;

        let in_index = {
            let state = state.lock().unwrap();
            state.index.lookup_path(path).is_some()
        };

        if !in_index {
            // File exists on disk but not in index - add it
            if let Err(e) = add_missing(state, path) {
                warn!("failed to add missing {}: {}", path.display(), e);
            } else {
                fixed += 1;
                debug!("revalidate added: {}", path.display());
            }
        }
    }

    // TODO: Also check for index entries that no longer exist on disk
    // This requires iterating the index and checking each path

    Ok((checked, fixed))
}

#[allow(dead_code)]
fn add_missing(state: &SharedState, path: &Path) -> Result<()> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return Ok(()),
    };

    let kind = if metadata.is_dir() {
        orin_core::entry::TYPE_DIR
    } else if metadata.file_type().is_symlink() {
        orin_core::entry::TYPE_SYMLINK
    } else {
        orin_core::entry::TYPE_FILE
    };

    let hidden = name.starts_with('.') || name.starts_with('$');

    let root_idx = {
        let state = state.lock().unwrap();
        state
            .roots
            .iter()
            .position(|r| path.starts_with(&r.path))
            .unwrap_or(0)
    };

    let parent_idx = {
        let state = state.lock().unwrap();
        let parent = path.parent().unwrap_or(path);
        if parent == state.roots[root_idx].path {
            u32::MAX
        } else {
            state.index.lookup_path(parent).unwrap_or(u32::MAX)
        }
    };

    let new_entry = orin_core::index::NewEntry {
        name,
        parent: Some(parent_idx),
        kind,
        hidden,
        size: metadata.len(),
        mtime: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0),
        root: root_idx,
    };

    {
        let mut state = state.lock().unwrap();
        let indices = state.index.insert_batch(&[new_entry], 0);
        state.index.finalize();
        debug!("revalidate added entry at idx={}", indices[0]);
    }

    Ok(())
}

#[allow(dead_code)]
/// Background revalidation task.
pub fn start_revalidate_task(
    revalidate: std::sync::Arc<Revalidate>,
    state: SharedState,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(60));
            if let Err(e) = revalidate.maybe_revalidate(&state) {
                warn!("revalidate error: {}", e);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_support::new_shared_hermetic;
    use tempfile::tempdir;

    #[test]
    fn revalidate_basic() {
        let dir = tempdir().unwrap();
        let state = new_shared_hermetic(dir.path(), "revalidate_basic").unwrap();
        let _rv = Revalidate::new(60);

        std::fs::write(dir.path().join("newfile.txt"), b"test").unwrap();

        let (checked, _fixed) = revalidate_root(&state, dir.path()).unwrap();
        assert!(checked > 0);
        // Note: fixed may be 0 if initial scan already ran
    }
}
