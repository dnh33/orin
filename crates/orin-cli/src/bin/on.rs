//! `on` - the short alias for `orin`.
//!
//! Same binary body, one keystroke: `on query foo` == `orin query foo`.
//! Compiles the same source via a path-module (main.rs is a mod-rs file, so
//! its `mod client;` resolves the same as when built as the `orin` bin).

#[path = "../main.rs"]
mod app;

fn main() -> std::process::ExitCode {
    app::main()
}
