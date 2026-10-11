# AGENTS.md

orin is whole-disk instant file search for Windows: one binary (`orin.exe`) with
`query`, `status`, `tui`, `mcp`, and `daemon` subcommands, plus an `on.exe` copy
whose argv[0] acts as `on foo` = `orin query foo`. A resident daemon holds the
name index in memory and keeps it live from filesystem events.

Read in this order: this file, `docs/OVERVIEW.md`, `CONTEXT.md`.

## File map

| Path | What lives there |
|---|---|
| `crates/orin-core` | index (arena, 20-byte entries, sorted keys), query engine, protocol, snapshots |
| `crates/orin-daemon` | library: scan, watch, apply, checkpoint, revalidate, socket server, thread priority |
| `crates/orin-cli` | the `orin` binary: query, status, tui, mcp subcommands, auto-spawn client |
| `crates/orin-mcp` | MCP server library (tools find, stat, status) over the daemon protocol |
| `crates/orin-bench` | benchmark suite: deterministic corpus, query matrix, self-validating runner, probes |
| `site/` | the website (Astro), deployed to orin.hjermitslev.dev |
| `skills/` | agent skills: `orin-search` (using the tool), `orin-development` (working on it) |
| `docs/adr/` | decisions that are hard to reverse |
| `context/` | machine-readable facts (query grammar, protocol, surfaces) |
| `install.ps1` | the one-line Windows installer |

## Rules

1. **Windows-only is the product** (owner decision, 2026-10-10). Do not add
   Unix code paths "for portability"; the spec's cross-platform line is
   superseded.
2. **One binary.** The daemon is a subcommand, short names are installer-dropped
   copies. Every new executable must answer "why is this not a subcommand?".
3. **`cargo build --locked` everywhere.** `Cargo.lock` is committed and pinned;
   dependency changes regenerate it via `.github/workflows/lockfile.yml`.
4. **Clippy is `-D warnings`, rustfmt is CI-enforced.** `fmt-fix.yml` formats a
   branch automatically; apply its diffs verbatim.
5. **Crucial-only tests.** Test the load-bearing behavior (protocol frames,
   snapshot round-trip, query matching, path resolution, readiness). Skip
   ceremony tests. A test that encodes a defect is fixed, never weakened.
6. **Benchmark integrity is sacred.** Rows are self-validating (`matches`,
   `nonzero_exits`, `tool_path`); the harness refuses empty searches; tool
   binaries are pinned and identity-gated (System32 `find.exe` must never answer
   `find`); the corpus is deterministic (seed `0xC0FFEE`) and plants coverage
   for every query class; tool-specific syntax runs as separate arms, never
   forced onto other tools.
7. **Measured numbers only in public copy.** README and site state results as
   measured on named GitHub runners. No fake precision, no development-state
   hedging, no em dashes. Outcomes, not process.
8. **License policy.** Dual MIT/Apache-2.0. Dependencies must pass
   `cargo deny` (`deny.toml` allowlist: 0BSD, CC0-1.0, MIT, Apache-2.0, ISC,
   BSD-3-Clause, Unicode-*); RustSec `cargo audit` runs on every CI pass.
9. **Release = tag `v*`.** `release.yml` builds the single binary, stages
   `on.exe` as a copy, and publishes bare `orin.exe` (+ zip + SHA256SUMS) that
   `install.ps1` fetches from `/releases/latest` via the GitHub API. The
   installer contract: asset name is exactly `orin.exe`.
10. **Protocol is length-prefixed JSON over a named pipe**, user-scoped. Query
    wire shape lives in `crates/orin-core/src/protocol.rs`; changing it is an
    ADR, not a refactor.

## Current state (2026-10-11)

- All five surfaces exist and are wired: `query`, `status`, `tui`, `mcp`,
  `daemon`.
- Measured (GitHub `windows-latest`, 25,926-entry corpus): warm query p50
  8-14 ms vs find 45 ms, fd 61 ms, rg 41-58 ms; cold scan 0.85 s; warm start
  from snapshot 0.85 s. CLI process spawn is 6.5 ms of every query: the known
  p50 floor (see `docs/adr/0006`).
- Site lives at https://orin.hjermitslev.dev (GitHub Pages origin, Cloudflare
  DNS).
- Open: README rewrite with the measured table, scale probes at 1M/3M entries.
