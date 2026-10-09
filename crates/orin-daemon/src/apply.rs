//! Apply queued filesystem changes to the index.

use anyhow::Result;
use orin_core::index::NewEntry;
use std::path::Path;
use tracing::debug;

use crate::state::SharedState;

/// Apply a batch of filesystem events to the index.
#[allow(dead_code)]
pub fn apply_events(state: &SharedState, events: Vec<FsEvent>) -> Result<usize> {
    let mut applied = 0;
    for event in events {
        if apply_single(state, event)? {
            applied += 1;
        }
    }
    Ok(applied)
}

/// A single filesystem event to apply.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum FsEvent {
    Create {
        path: std::path::PathBuf,
        is_dir: bool,
    },
    Modify {
        path: std::path::PathBuf,
    },
    Remove {
        path: std::path::PathBuf,
    },
    Rename {
        from: std::path::PathBuf,
        to: std::path::PathBuf,
    },
}

#[allow(dead_code)]
fn apply_single(state: &SharedState, event: FsEvent) -> Result<bool> {
    match event {
        FsEvent::Create { path, is_dir } => apply_create(state, &path, is_dir),
        FsEvent::Modify { path } => apply_modify(state, &path),
        FsEvent::Remove { path } => apply_remove(state, &path),
        FsEvent::Rename { from, to } => apply_rename(state, &from, &to),
    }
}

#[allow(dead_code)]
fn apply_create(state: &SharedState, path: &Path, is_dir: bool) -> Result<bool> {
    let mut state = state.lock().unwrap();
    let root_idx = find_root(&state, path)?;
    let parent_idx = find_parent(&state, path, root_idx)?;

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };

    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return Ok(false),
    };

    let kind = if is_dir {
        orin_core::entry::TYPE_DIR
    } else if metadata.file_type().is_symlink() {
        orin_core::entry::TYPE_SYMLINK
    } else {
        orin_core::entry::TYPE_FILE
    };

    let hidden = name.starts_with('.') || name.starts_with('$');

    let entry = NewEntry {
        name,
        parent: Some(parent_idx),
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

    let indices = state.index.insert_batch(&[entry], 0);
    state.index.finalize();

    debug!("applied create: {} (idx={})", path.display(), indices[0]);
    Ok(true)
}

#[allow(dead_code)]
fn apply_modify(state: &SharedState, path: &Path) -> Result<bool> {
    let state = state.lock().unwrap();
    if let Some(idx) = state.index.lookup_path(path) {
        let _metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return Ok(false),
        };

        // Update entry in place (simplified - real impl would handle this better)
        let _entry = state.index.entry(idx);
        // Note: Entry is immutable, so we'd need a different approach
        // For now, just log
        debug!("apply modify: {} (idx={})", path.display(), idx);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[allow(dead_code)]
fn apply_remove(state: &SharedState, path: &Path) -> Result<bool> {
    let mut state = state.lock().unwrap();
    if let Some(idx) = state.index.lookup_path(path) {
        state.index.remove(idx);
        state.index.finalize();
        debug!("applied remove: {} (idx={})", path.display(), idx);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[allow(dead_code)]
fn apply_rename(state: &SharedState, from: &Path, to: &Path) -> Result<bool> {
    // Treat as remove + create
    apply_remove(state, from)?;
    let is_dir = std::fs::metadata(to).map(|m| m.is_dir()).unwrap_or(false);
    apply_create(state, to, is_dir)
}

#[allow(dead_code)]
fn find_root(state: &crate::state::State, path: &Path) -> Result<usize> {
    for (i, root) in state.roots.iter().enumerate() {
        if path.starts_with(&root.path) {
            return Ok(i);
        }
    }
    anyhow::bail!("no root contains path: {}", path.display())
}

#[allow(dead_code)]
fn find_parent(state: &crate::state::State, path: &Path, root_idx: usize) -> Result<u32> {
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    if parent == state.roots[root_idx].path {
        return Ok(u32::MAX); // root
    }
    state
        .index
        .lookup_path(parent)
        .ok_or_else(|| anyhow::anyhow!("parent not in index: {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn apply_create() {
        let dir = tempdir().unwrap();
        let state = crate::state::new_shared(dir.path()).unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello").unwrap();

        let event = FsEvent::Create {
            path: file.clone(),
            is_dir: false,
        };
        apply_events(&state, vec![event]).unwrap();

        let state = state.lock().unwrap();
        assert!(state.index.lookup_path(&file).is_some());
    }
}
