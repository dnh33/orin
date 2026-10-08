//! orin-cli — command-line interface and TUI for orin.
//!
//! This binary provides the human-facing CLI (`orin`), the interactive
//! TUI (`orin tui`), and the MCP server subcommand (`orin mcp`).

fn main() {
    println!("orin {}", env!("CARGO_PKG_VERSION"));
}
