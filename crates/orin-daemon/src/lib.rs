//! orin-daemon — the long-running search index daemon.
//!
//! This crate provides the `orind` binary which runs as a background service,
//! owns the in-memory index, watches the filesystem for changes, and serves
//! queries over a Unix socket (or Windows named pipe).

pub mod apply;
pub mod checkpoint;
pub mod revalidate;
pub mod scan;
pub mod server;
pub mod state;
pub mod watch;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn daemon_modules_compile() {
        // Just verify all modules compile
        let _ = apply::FsEvent::Create {
            path: std::path::PathBuf::new(),
            is_dir: false,
        };
        let _ = checkpoint::Checkpoint::new(std::path::PathBuf::new(), 60);
        let _ = revalidate::Revalidate::new(60);
        let _ = scan::scan_root(std::path::Path::new("."), 0, &mut orin_core::index::Index::new());
    }
}