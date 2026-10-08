# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace with five crates: `orin-core`, `orin-daemon`, `orin-cli`, `orin-mcp`, `orin-bench`
- CI matrix (fmt, check, clippy, test, release build) on Ubuntu, macOS and Windows
- `fmt-fix` workflow — applies `cargo fmt` on a branch without a local toolchain
- `lockfile` workflow — regenerates and commits `Cargo.lock`
- Dependency scrutiny via `cargo-deny` (`security` workflow)
- Dependabot updates for cargo and GitHub Actions
