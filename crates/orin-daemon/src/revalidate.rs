//! On-demand revalidation: scan roots and reconcile with index.
//!
//! Revalidation is never periodic: it runs only when something asks for it —
//! once at daemon startup after the initial scan/snapshot load, and when a
//! client requests a Rescan. The filesystem watcher stays the live source of
//! truth in between, so an idle daemon never wakes up to revalidate.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tracing::{debug, info, warn};

use crate::state::SharedState;

/// Revalidation entry point. No timer and no background thread: callers
/// decide when a pass runs.
#[allow(dead_code)]
pub struct Revalidate;

impl Revalidate {
    /// Run a full revalidation pass over every root right now.
    pub fn run(state: &SharedState) -> Result<()> {
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

fn revalidate_root(state: &SharedState, root: &Path) -> Result<(u64, u64)> {
    use ignore::WalkBuilder;

    let mut checked = 0u64;
    let mut fixed = 0u64;

    // One lookup map for the whole pass. A per-entry lookup_path call was
    // O(n) with full-path allocations: revalidation became O(n^2) and a
    // 25k-entry corpus needed ~6.5 minutes (measured 2026-10-10).
    let map: std::collections::HashMap<(u32, String), u32> = {
        let state = state.lock().unwrap();
        state.index.lookup_map_owned()
    };

    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_ignore(false)
        .build();

    // Pre-order walk: a stack of (depth, entry idx) resolves parents without
    // touching the index. Entries added during the pass are stacked too, so
    // their children resolve against the fresh parent instead of the stale
    // map and get added in order.
    let mut stack: Vec<(usize, u32)> = Vec::new();
    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let depth = entry.depth();
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            stack.pop();
        }
        checked += 1;

        let found = match entry.file_name().to_str() {
            Some(name) => match stack.last() {
                Some(&(_, parent)) => map.get(&(parent, name.to_string())).copied(),
                // The walk root: its entry key is (u32::MAX, name).
                None => map.get(&(u32::MAX, name.to_string())).copied(),
            },
            None => None,
        };

        match found {
            Some(idx) => stack.push((depth, idx)),
            None => {
                // File exists on disk but not in index - add it.
                let parent = stack.last().map(|&(_, i)| i).unwrap_or(u32::MAX);
                match add_missing(state, path, parent) {
                    Ok(Some(idx)) => {
                        stack.push((depth, idx));
                        fixed += 1;
                        debug!("revalidate added: {}", path.display());
                    }
                    Ok(None) => {}
                    Err(e) => warn!("failed to add missing {}: {}", path.display(), e),
                }
            }
        }
    }

    // TODO: Also check for index entries that no longer exist on disk
    // This requires iterating the index and checking each path

    Ok((checked, fixed))
}

/// Insert one on-disk entry under `parent_idx` (`u32::MAX` = root level).
/// Returns the new entry's index, or `None` when metadata was unreadable.
fn add_missing(state: &SharedState, path: &Path, parent_idx: u32) -> Result<Option<u32>> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return Ok(None),
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

    let new_entry = orin_core::index::NewEntry {
        name,
        parent: if parent_idx == u32::MAX {
            None
        } else {
            Some(parent_idx)
        },
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

    let mut state = state.lock().unwrap();
    let indices = state.index.insert_batch(&[new_entry], 0);
    // Incremental order maintenance: a full finalize() re-sort per added
    // entry made sparse additions O(n log n) each.
    let idx = indices[0];
    state.index.sorted_insert(idx);
    debug!("revalidate added entry at idx={idx}");
    Ok(Some(idx))
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

        std::fs::write(dir.path().join("newfile.txt"), b"test").unwrap();

        let (checked, _fixed) = revalidate_root(&state, dir.path()).unwrap();
        assert!(checked > 0);
        // Note: fixed may be 0 if initial scan already ran
    }
}
