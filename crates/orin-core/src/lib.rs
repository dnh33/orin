//! orin-core — core index structures, query engine, protocol, and configuration.
//!
//! This crate contains the fundamental data structures and algorithms for orin's
//! whole-disk search index. It has no async runtime, no UI dependencies, and
//! no network dependencies — it is a pure library.

pub mod entry;
pub mod arena;
pub mod fold;
pub mod index;
pub mod snapshot;
pub mod config;
pub mod paths;
pub mod protocol;
pub mod errors;

pub mod query {
    pub mod parse;
    pub mod filter;
    pub mod matcher;
    pub mod score;
}

// Re-exports for convenience
pub use entry::{Entry, ENTRY_SIZE, FLAG_TYPE_MASK, TYPE_FILE, TYPE_DIR, TYPE_SYMLINK, TYPE_OTHER,
                FLAG_HIDDEN, FLAG_NAME_TRUNCATED, FLAG_ROOT};
pub use fold::{fold_into, fold_vec, cmp_folded, prefix_cmp, normalize_nfc};
pub use index::{Index, Root, NewEntry, SearchResult, Hit, StatInfo};
pub use snapshot::{save, load, encode, decode, SNAPSHOT_MAGIC, SNAPSHOT_FORMAT};
pub use config::{Config, default_config, default_roots, load_config};
pub use paths::{data_dir, config_dir, socket_name, log_file, lock_file};
pub use protocol::{Request, Response, write_frame, read_frame, PROTOCOL_VERSION, MAX_FRAME,
                   HitWire, StatusData, RootWire, Progress};
pub use errors::Error;

/// Placeholder to make the crate compile before real implementation.
pub fn placeholder() {}