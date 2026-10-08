//! orin-bench — benchmark suite for orin.
//!
//! This binary generates synthetic filesystem trees and measures
//! index build time, query latency, memory usage, and snapshot performance.

fn main() {
    println!("orin-bench {}", env!("CARGO_PKG_VERSION"));
}
