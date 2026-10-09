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
pub fn scan_root(root: &Path, root_idx: usize) -> Result<(Vec<NewEntry<'_>>, u64)> {
    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_ignore(false)
        .build();

    let mut names = Vec::new();
    let mut kinds = Vec::new();
    let mut hiddens = Vec::new();
    let mut sizes = Vec::new();
    let mut mtimes = Vec::new();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(e) => {
                warn!("walk error: {}", e);
                continue;
            }
        };

        let path = entry.path().to_path_buf();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!("metadata error for {}: {}", path.display(), e);
                continue;
            }
        };

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                warn!("skipping entry with invalid file name: {}", path.display());
                continue;
            }
        };

        let kind = if metadata.is_dir() {
            1
        } else if metadata.file_type().is_symlink() {
            2
        } else {
            0
        };
        let hidden = name.starts_with('.') || name.starts_with('$');
        let size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);

        names.push(name);
        kinds.push(kind);
        hiddens.push(hidden);
        sizes.push(size);
        mtimes.push(mtime);
    }

    let entries: Vec<NewEntry<'_>> = names.iter().enumerate().map(|(i, name)| {
        NewEntry {
            name: name.as_str(),
            parent: None,
            kind: kinds[i],
            hidden: hiddens[i],
            size: sizes[i],
            mtime: mtimes[i],
            root: root_idx,
        }
    }).collect();

    Ok((entries, entries.len() as u64))
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
        let (entries, count) = scan_root(&root_path, root_idx)?;
        index.insert_batch(&entries, 0);
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

    let (entries, count) = scan_root(&root_path, root_idx)?;

    {
        let mut state = state.lock().unwrap();
        state.index.insert_batch(&entries, 0);
        state.index.finalize();
        state.roots[root_idx].count = entries.len() as u32;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scan_root_basic() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), b"hello").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let mut idx = Index::new();
        let (entries, count) = scan_root(dir.path(), 0).unwrap();
        idx.insert_batch(&entries, 0);
        assert_eq!(count, 2); // file.txt + subdir
    }
}
