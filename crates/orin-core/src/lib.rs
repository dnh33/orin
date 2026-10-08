//! orin-core — core index structures, query engine, protocol, and configuration.
//!
//! This crate contains the fundamental data structures and algorithms for orin's
//! whole-disk search index. It has no async runtime, no UI dependencies, and
//! no network dependencies — it is a pure library.

pub mod arena;
pub mod config;
pub mod entry;
pub mod errors;
pub mod fold;
pub mod index;
pub mod paths;
pub mod protocol;
pub mod query;
pub mod snapshot;

/// Placeholder that keeps the crate compiling until the real implementation lands.
pub fn placeholder() {}
