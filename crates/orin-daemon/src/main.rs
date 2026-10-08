//! orin-daemon — the long-running search index daemon.
//!
//! This binary runs as a background service, owns the in-memory index,
//! watches the filesystem for changes, and serves queries over a
//! Unix socket (or Windows named pipe).

fn main() {
    println!("orind {}", env!("CARGO_PKG_VERSION"));
}