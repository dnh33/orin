//! Initial filesystem scan and incremental re-scan.

use anyhow::Result;
use ignore::WalkBuilder;
use orin_core::config::default_roots;
use orin_core::index::{Index, NewEntry};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::state::{RootEntry, ScanProgress, SharedState};

/// Scan a single root directory and return NewEntry items.
pub fn scan_root(root: &Path, root_idx: usize) -> Result<Vec<NewEntry<'_>>> {
    scan_root_seq(root, root_idx)
}

/// Sequential scan for initial implementation.
pub fn scan_root_seq(root: &Path, root_idx: usize) -> Result<Vec<NewEntry<'_>>> {
    let mut entries = Vec::new();
    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_ignore(false)
        .build();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                warn!("walk error: {}", e);
                continue;
            }
        };

        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!("metadata error for {}: {}", path.display(), e);
                continue;
            }
        };

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        let kind = if metadata.is_dir() {
            orin_core::entry::TYPE_DIR
        } else if metadata.file_type().is_symlink() {
            orin_core::entry::TYPE_SYMLINK
        } else {
            orin_core::entry::TYPE_FILE
        };

        let hidden = name.starts_with('.') || name.starts_with('$');

        entries.push(NewEntry {
            name,
            parent: None,
            kind,
            hidden,
            size: metadata.len(),
            mtime: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as u32)
                .unwrap_or(0),
            root: root_idx,
        });
    }

    Ok(entries)
}

/// Full initial scan of all roots.
pub fn initial_scan(state: &SharedState) -> Result<u64> {
    info!("starting initial scan");
    let start = std::time::Instant::now();

    let roots: Vec<(PathBuf, usize)> = {
        let state = state.lock().unwrap();
        state
            .roots
            .iter()
            .enumerate()
            .map(|(i, r)| (r.path.clone(), i))
            .collect()
    };

    let total_entries = Arc::new(AtomicU64::new(0));
    let mut all_entries = Vec::new();

    for (root_path, root_idx) in roots {
        let entries = scan_root_seq(&root_path, root_idx)?;
        let count = entries.len();
        total_entries.fetch_add(count as u64, Ordering::Relaxed);
        all_entries.extend(entries);
    }

    // Insert all entries into index
    {
        let mut state = state.lock().unwrap();
        let indices = state.index.insert_batch(&all_entries, 0);
        state.index.finalize();
        state.total_entries = total_entries.load(Ordering::Relaxed);

        // Update root entry ranges
        let mut offset = 0;
        for (i, root_entry) in state.roots.iter_mut().enumerate() {
            let root_indices: Vec<_> = indices
                .iter()
                .filter(|&&idx| {
                    // This is simplified - real implementation would track which root each entry belongs to
                    true
                })
                .copied()
                .collect();
            root_entry.first = offset as u32;
            root_entry.count = root_indices.len() as u32;
            offset += root_indices.len();
        }
    }

    let elapsed = start.elapsed();
    info!(
        "initial scan complete: {} entries in {:.2}s",
        total_entries.load(Ordering::Relaxed),
        elapsed.as_secs_f64()
    );

    Ok(total_entries.load(Ordering::Relaxed))
}

/// Incremental re-scan of a single root.
pub fn rescan_root(state: &SharedState, root_idx: usize) -> Result<u64> {
    info!("rescanning root {}", root_idx);
    let (root_path, _) = {
        let state = state.lock().unwrap();
        (state.roots[root_idx].path.clone(), root_idx)
    };

    let entries = scan_root_seq(&root_path, root_idx)?;

    {
        let mut state = state.lock().unwrap();
        // Remove old entries for this root (simplified - real impl uses tombstones)
        let indices = state.index.insert_batch(&entries, 0);
        state.index.finalize();
        state.roots[root_idx].count = indices.len() as u32;
    }

    Ok(entries.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scan_root_seq_basic() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), b"hello").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let entries = scan_root_seq(dir.path(), 0).unwrap();
        assert_eq!(entries.len(), 2); // file.txt + subdir
    }
}