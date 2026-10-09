//! Initial filesystem scan and incremental re-scan.

use anyhow::Result;
use ignore::WalkBuilder;
use orin_core::config::default_roots;
use orin_core::index::{Index, NewEntry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::state::{RootEntry, ScanProgress, SharedState};

/// Scan a single root directory and insert entries into index.
/// Returns the number of entries inserted.
pub fn scan_root(root: &Path, root_idx: usize, index: &mut Index) -> Result<u64> {
    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_ignore(false)
        .build();

    let mut entries: Vec<NewEntry<'_>> = Vec::new();

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
            None => {
                warn!("skipping entry with invalid file name: {}", path.display());
                continue;
            }
        };

        let kind = if metadata.is_dir() {
            orin_core::entry::TYPE_DIR
        } else if metadata.file_type().is_symlink() {
            orin_core::entry::TYPE_SYMLINK
        } else {
            orin_core::entry::TYPE_FILE
        };

        let hidden = name.starts_with('.') || name.starts_with('$');

        let new_entry = NewEntry {
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
        };

        entries.push(new_entry);
    }

    // Insert directly into the index (arena owns the names)
    index.insert_batch(&entries, 0);
    index.finalize();

    Ok(entries.len() as u64)
}

/// Full initial scan of all roots.
pub fn initial_scan(state: &SharedState) -> Result<u64> {
    info!("starting initial scan");
    let start = std::time::Instant::now();

    let roots: Vec<(PathBuf, usize)> = {
        let state = state.lock().unwrap();
        state.roots.iter()
            .enumerate()
            .map(|(i, r)| (r.path.clone(), i))
            .collect()
    };

    let mut total_entries = Arc::new(AtomicU64::new(0));
    let mut idx = Index::new();

    for (root_path, root_idx) in roots {
        let count = scan_root(&root_path, root_idx, &mut idx)?;
        total_entries.fetch_add(count as u64, Ordering::Relaxed);
    }

    {
        let mut state = state.lock().unwrap();
        state.index = idx; // replace index with the one we built
        state.total_entries = total_entries.load(Ordering::Relaxed);
    }

    Ok(total_entries.load(Ordering::Relaxed))
}

/// Incremental re-scan of a single root.
pub fn rescan_root(state: &SharedState, root_idx: usize) -> Result<u64> {
    info!("rescanning root {}", root_idx);
    let (root_path, _) = {
        let state = state.lock().unwrap();
        (state.roots[root_idx].path.clone(), root_idx)
    };

    let mut idx = Index::new();
    let count = scan_root_seq(&root_path, root_idx, &mut idx)?;

    {
        let mut state = state.lock().unwrap();
        // Replace root's entries entirely (simplified - real impl uses tombstones)
        state.index.insert_batch(&idx.entries.iter().copied().collect::<Vec<_>>(), 0);
        state.index.finalize();
        state.roots[root_idx].count = idx.len() as u32;
    }

    Ok(count)
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

        let mut idx = Index::new();
        let count = scan_root(dir.path(), 0, &mut idx).unwrap();
        assert_eq!(count, 2); // file.txt + subdir
    }
}
