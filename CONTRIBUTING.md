# Contributing to orin

Thanks for your interest in orin! These notes keep contributions easy to review
and merge.

## Development setup

A Rust toolchain is required (stable; `rust-toolchain.toml` pins components and
`rustup` will fetch the right version).

```sh
cargo test --workspace        # run the test suite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all               # please run before committing
```

## CI gates

Every PR runs:

1. `cargo fmt --check` — formatting
2. `cargo check` / `cargo clippy -D warnings` — lints
3. `cargo test --workspace` + `cargo build --release` on Ubuntu, macOS, Windows
4. `cargo-deny check` — licenses, advisories, banned crates (`security` workflow)

All gates must pass before merge.

### No local toolchain?

- **Formatting fails?** Dispatch the `fmt-fix` workflow
  (Actions → fmt-fix → Run workflow → branch name); it applies `cargo fmt` and
  commits the result back to the branch.
- **Changed `Cargo.toml`?** Dispatch the `lockfile` workflow for the branch so
  `Cargo.lock` stays in sync.

## Testing policy

Tests are written **only where failure would be silent or costly**: index
invariants, snapshot round-trips, end-to-end scenarios, and the benchmark
artifact. Do not add tests for getters, trivial formatting, or anything CI
already catches loudly. When in doubt, ask in the PR.

## Pull requests

- One logical change per PR.
- Fill in the PR template.
- Commit subjects: short and imperative (`fix: ...`, `feat: ...`, `docs: ...`,
  `ci: ...`, `chore: ...`).

## Reporting bugs and requesting features

Use the issue templates. For anything security-sensitive, see
[SECURITY.md](SECURITY.md) — never a public issue.
