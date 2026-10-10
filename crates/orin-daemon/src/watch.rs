//! Filesystem watcher using notify: the live source of truth for the index.

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use tracing::{debug, info, warn};

use crate::state::SharedState;

/// Watcher handle for managing the background watcher thread.
#[allow(dead_code)]
pub struct WatcherHandle {
    watcher: Option<RecommendedWatcher>,
    #[allow(dead_code)]
    rx: Option<mpsc::Receiver<notify::Result<Event>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WatcherHandle {
    #[allow(dead_code)]
    /// Start watching the given roots.
    pub fn start(roots: Vec<PathBuf>, state: SharedState) -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(tx.clone(), Config::default())?;

        // Watch each root
        for root in roots {
            if root.exists() {
                if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
                    warn!("failed to watch {}: {}", root.display(), e);
                } else {
                    info!("watching: {}", root.display());
                }
            }
        }

        // Spawn event processing thread
        let thread = thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                match event {
                    Ok(event) => handle_event(event, &state),
                    Err(e) => warn!("watch error: {}", e),
                }
            }
        });

        Ok(Self {
            watcher: Some(watcher),
            rx: None, // rx only used internally, no need to expose
            thread: Some(thread),
        })
    }

    #[allow(dead_code)]
    /// Stop the watcher.
    pub fn stop(mut self) {
        if let Some(w) = self.watcher.take() {
            drop(w); // stops watching
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[allow(dead_code)]
/// Handle a filesystem event: translate to FsEvent and apply to the index.
fn handle_event(event: Event, state: &SharedState) {
    use EventKind::*;

    let mut events = Vec::new();
    let paths = event.paths.clone();
    match event.kind {
        Create(_) => {
            for path in paths {
                let is_dir = std::fs::metadata(&path)
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                debug!("create: {}", path.display());
                events.push(crate::apply::FsEvent::Create { path, is_dir });
            }
        }
        Modify(notify::event::ModifyKind::Name(_)) if paths.len() == 2 => {
            debug!("rename: {} -> {}", paths[0].display(), paths[1].display());
            events.push(crate::apply::FsEvent::Rename {
                from: paths[0].clone(),
                to: paths[1].clone(),
            });
        }
        Modify(_) => {
            for path in paths {
                debug!("modify: {}", path.display());
                events.push(crate::apply::FsEvent::Modify { path });
            }
        }
        Remove(_) => {
            for path in paths {
                debug!("remove: {}", path.display());
                events.push(crate::apply::FsEvent::Remove { path });
            }
        }
        _ => {}
    }

    if !events.is_empty()
        && let Err(e) = crate::apply::apply_events(state, events)
    {
        warn!("apply_events failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_support::new_shared_hermetic;
    use tempfile::tempdir;

    #[test]
    fn watcher_handle_creation() {
        let dir = tempdir().unwrap();
        let state = new_shared_hermetic(dir.path(), "watcher_handle_creation").unwrap();
        let handle = WatcherHandle::start(vec![dir.path().to_path_buf()], state).unwrap();
        handle.stop();
    }
}
