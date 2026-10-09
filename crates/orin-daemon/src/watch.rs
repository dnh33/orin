//! Filesystem watcher using notify (with polling fallback).

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::state::SharedState;

/// Watcher handle for managing the background watcher thread.
pub struct WatcherHandle {
    watcher: Option<RecommendedWatcher>,
    rx: Option<mpsc::Receiver<notify::Result<Event>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WatcherHandle {
    /// Start watching the given roots.
    pub fn start(roots: Vec<PathBuf>, state: SharedState) -> Result<Self> {
        let (tx, rx) = mpsc::channel();

        let watcher: Result<RecommendedWatcher, _> = RecommendedWatcher::new(
            tx.clone(),
            Config::default().with_poll_interval(Duration::from_secs(30)),
        );
        let watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                warn!("notify watcher failed, falling back to poll: {}", e);
                let poll_watcher = notify::PollWatcher::new(
                    tx,
                    Config::default().with_poll_interval(Duration::from_secs(30)),
                )?;
                RecommendedWatcher::Poll(poll_watcher)
            }
        };

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
            rx: Some(rx),
            thread: Some(thread),
        })
    }

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

/// Handle a filesystem event.
fn handle_event(event: Event, state: &SharedState) {
    use EventKind::*;

    for path in event.paths {
        match event.kind {
            Create(_) | Modify(_) => {
                debug!("create/modify: {}", path.display());
                // Queue for apply.rs to process
            }
            Remove(_) => {
                debug!("remove: {}", path.display());
                // Queue for apply.rs to process
            }
            _ => {}
        }
    }
}

/// Polling fallback for platforms where notify doesn't work well.
pub fn start_polling_watcher(roots: Vec<PathBuf>, state: SharedState) -> Result<thread::JoinHandle<()>> {
    let handle = thread::spawn(move || {
        let mut last_modified = std::collections::HashMap::new();
        loop {
            thread::sleep(Duration::from_secs(30));
            for root in &roots {
                if let Err(e) = poll_root(root, &mut last_modified, &state) {
                    warn!("poll error for {}: {}", root.display(), e);
                }
            }
        }
    });
    Ok(handle)
}

fn poll_root(root: &PathBuf, last_modified: &mut std::collections::HashMap<PathBuf, std::time::SystemTime>, state: &SharedState) -> Result<()> {
    use ignore::WalkBuilder;
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
        let path = entry.path().to_path_buf();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = match meta.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };

        let is_new_or_modified = last_modified
            .get(&path)
            .map(|&last| modified > last)
            .unwrap_or(true);

        if is_new_or_modified {
            last_modified.insert(path.clone(), modified);
            debug!("poll detected change: {}", path.display());
            // Queue for apply.rs
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn watcher_handle_creation() {
        let dir = tempdir().unwrap();
        let state = crate::state::new_shared(&dir.path().to_path_buf()).unwrap();
        let handle = WatcherHandle::start(vec![dir.path().to_path_buf()], state).unwrap();
        handle.stop();
    }
}