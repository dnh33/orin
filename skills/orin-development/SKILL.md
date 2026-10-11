---
name: orin-development
description: "Use when working on the orin Rust codebase itself."
version: 0.1.0
author: dnh33, Hermes Agent
license: MIT OR Apache-2.0
platforms: [windows]
metadata:
  hermes:
    tags: [orin, rust, ci, benchmarks]
    related_skills: [orin-search]
---

# orin Development Skill

orin is a Rust workspace that ships one Windows binary. This skill is for agents
changing it: crate layout, the exact gate commands, the testing policy, the
benchmark-integrity rules, the release sequence, and the rules for public copy.
Using orin to search files belongs in the `orin-search` skill.

## When to Use

- Editing anything under `crates/`, a workflow, the installer, or public copy.
- Reviewing a change against the gate, the benchmark rules, or the asset
  contract before it is called done.
- Deciding what may be published in README or site copy.

Don't use for: querying files with orin (see `orin-search`).

## Prerequisites

- Rust stable with `rustfmt` and `clippy` (workspace MSRV 1.85, edition 2024).
- A committed `Cargo.lock`: every command below passes `--locked`, so a stale
  or missing lockfile fails the gate rather than silently resolving.
- A Windows build host, or CI: the crates use named pipes and process creation
  flags, and CI runs on `windows-latest`.
- Pathspec commits. Agents share this tree, so stage explicit paths
  (`git add <paths>`) and commit with a pathspec; never a bare `git add -A`.

## Workspace layout

- `orin-core` (library): the index (20-byte fixed entries, name arena, sorted
  u32 keys), query parse/filter/matcher/scorer, the length-prefixed wire
  protocol, snapshot handling for `orin.snap`, config, and platform paths. No
  async runtime, no UI, no network dependencies.
- `orin-daemon` (library): everything `orin daemon` runs: scan, watch, apply,
  revalidate, checkpoint, priority, and the named-pipe server that owns the
  live index.
- `orin-cli` (binary `orin`): subcommands `query`, `status`, `tui` (ratatui
  picker), `mcp`, and `daemon`; the IPC client with auto-spawn; and the `on`
  argv[0] alias that rewrites to `orin query`.
- `orin-mcp` (library): the stdio MCP server exposing `find`, `stat`, and
  `status`, plus its own daemon client.
- `orin-bench` (binary `orin-bench`): deterministic corpus generator, the query
  matrix in `crates/orin-bench/queries.json`, the timed runner with preflight,
  report comparison, and daemon probes.

## The gate

Run each command from the workspace root through `terminal(...)`. Every one must
exit 0 before the change is called done; CI runs the same set on
`windows-latest`.

1. Format: `cargo fmt --check`. Completion: no diff. CI enforces rustfmt, and
   the `fmt-fix` workflow (dispatch with a branch input) runs `cargo fmt --all`
   and pushes `style: cargo fmt` when anything moved.
2. Types: `cargo check --workspace --all-targets --locked`. Completion: zero
   errors.
3. Lints: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
   Completion: zero warnings, because `-D warnings` turns each one into a
   failure.
4. Tests: `cargo test --workspace --locked`. Completion: all pass under the
   crucial-only policy below.
5. Release build: `cargo build --release --locked`. Completion: links cleanly;
   this is the shape the release job builds.
6. Licenses and advisories: `cargo-deny` against `deny.toml` plus RustSec
   against the pinned lockfile (the `security` workflow). The allowlist uses
   exact SPDX ids: `0BSD` and `CC0-1.0` are on it, while `BSD-0-Clause` is a
   different id that is not. Spell ids exactly.

If a change needs a new dependency resolution, regenerate `Cargo.lock` through
the `lockfile` workflow (dispatch with a branch input) instead of editing it by
hand.

## Testing policy: crucial-only

- Only crucial tests live in the tree and run in CI: fast, deterministic,
  in-process. Integration-heavy and flaky suites are excluded by policy, not by
  oversight.
- Why: CI is the only build environment for this project, so every test costs
  shared runner time, and one flake is indistinguishable from a regression in
  the gate every other change is judged by.
- Completion: new behavior gets a deterministic unit test next to the code it
  exercises. If proving it needs sleeps, network, or a live daemon, the design
  is not ready; fix that first instead of adding a retry.

## Benchmark integrity

A change must not break any of these rules. They are what makes a published
number mean something:

- Self-validating rows: every result row records `query_class`, `query`,
  `tool`, `tool_path`, `matches`, `nonzero_exits`, `spawn_us`, `status_us`,
  `p50_us`, `p95_us`, `p99_us`, and `qps`. `tool_path` proves which binary
  answered, `matches` proves the search did work, and `nonzero_exits` exposes
  tools that failed while appearing fast. Do not drop, rename, or round them.
- Preflight refuses empty searches: the runner fails a query that matches
  nothing for a tool, and the query matrix rejects empty needles. Every query
  class must keep hits in every corpus size for every compared tool, because a
  tool can win by returning nothing.
- Pinned tool binaries: the workflow verifies that `find` is GNU findutils and
  resolves `fd` and `rg`, then exports `BENCH_TOOL_FIND`, `BENCH_TOOL_FD`, and
  `BENCH_TOOL_RG` as absolute paths. Keep resolving through those variables. A
  bare `find` resolves to the System32 string filter, and its instant error
  exits would be timed as if they were fast searches.
- Deterministic corpus: `gen` defaults to seed `0xC0FFEE`, and the same seed
  must produce the identical tree. No unseeded randomness, and no corpus
  change that quietly invalidates comparison with earlier runs.
- Known floor: the CLI process spawn costs about 6.5 ms of every query, so a
  CLI p50 sits on that floor. Do not present a number as a query-time win
  without accounting for it.

Harness shape, one subcommand per `terminal(...)` call:

```text
cargo run --release -p orin-bench gen --corpus tiny --out <dir> --seed 0xC0FFEE
cargo run --release -p orin-bench run --tool orin --corpus <dir> --out <file> --iterations 1000
cargo run --release -p orin-bench probes --corpus <dir> --out <file>
```

`run` is pointed at `crates/orin-bench/queries.json` for the query matrix.
`probes` waits out the 60-second checkpoint cadence, so it includes a
65-second wait; expect it to be slow.

## Release sequence

1. Push a tag matching `v*`, or dispatch the `release` workflow with a tag
   input. Completion: the job starts on `windows-latest`.
2. The job runs `cargo build --release --locked -p orin-cli`, then asserts that
   `orin.exe --version` reports the tag version and that `daemon --help`,
   `tui --help`, and `mcp --help` all run. One binary carries every subcommand.
   Completion: all four assertions pass, or the job fails by design.
3. Assets: bare `orin.exe`, a byte-identical `on.exe` copy, the zip holding
   both plus `install.ps1` and `README.md`, and `SHA256SUMS.txt`, published
   with `gh release create ... --latest`. Completion: the release lists all
   four assets.
4. Installer contract: the bare asset named exactly `orin.exe`, resolved from
   `/releases/latest` through the GitHub API. The installer stops a running
   `orin` process first, extracts, writes `on.exe` as a byte-identical copy,
   verifies `--version`, and adds the install folder to the user PATH only.

```powershell
irm https://github.com/dnh33/orin/raw/main/install.ps1 | iex
```

5. Never rename or drop the bare `orin.exe` asset. The installer resolves that
   exact name, so a zip-only release breaks the documented install line.

## Public copy rules (README and site)

- Outcomes first: what a user can do, and how fast, in plain sentences. No
  architecture tour, no crate or module names, no workflow or spec references,
  no internal env vars, no task ids, no machine or user paths.
- Every measured number names its runner and corpus. Approved headline: warm
  query p50 of 8-14 ms against find 45 ms, fd 61 ms, and rg 41-58 ms on
  `windows-latest` over a 25,926-entry corpus. Approved index numbers: cold
  scan 0.85 s (about 30k entries/s on the tiny corpus) and warm start from the
  snapshot 0.85 s.
- No development-state hedging: no "not tested yet", "should work", or
  "probably". A claim without a measurement gets cut, not softened.
- State decisions as decisions: Windows-only support is an owner decision
  recorded on 2026-10-10, and the binary is dual-licensed
  `MIT OR Apache-2.0` with dependency licenses allowlisted in `deny.toml`.
- When a number changes, re-measure on the runner and update it in the same
  change. A stale public number is worse than none.

## Pitfalls

1. Workflow `with:` values are scalar comma-separated strings, for example
   `components: rustfmt, clippy`. A YAML sequence there fails the workflow
   parse.
2. `interprocess` 2.4: import `use interprocess::local_socket::prelude::*;` at
   the top of the file, or `Listener as _` and `Stream as _` when only a method
   is used. There is no `interprocess::Error` type; expose errors as
   `std::io::Error` instead.
3. On a `Stream`, take the halves with `stream.split()`; `try_clone()` does not
   exist. `set_nonblocking` takes `ListenerNonblockingMode::Neither` or
   `::Both`, never a bool.
4. Snapshots: `orin.snap` is CRC32-versioned and written atomically through
   `orin.snap.tmp`, and entries are 20 bytes fixed with names in the arena. Use
   the existing read and write helpers instead of packing structs by hand.
5. Commit with pathspecs: `git add <paths>` then
   `git commit -m "..." -- <paths>`. Completion: `git status --porcelain`
   lists nothing you do not own.
6. Workspace dependency features are deliberate: `sysinfo` runs with
   `default-features = false, features = ["system"]`, so `system` is the
   anchor, not `process`. Changing a feature set is a decision, not a default.

## Verification

- All six gate commands exit 0 (see The gate).
- `git diff --name-only` lists only files this change owns.
- A harness run clears preflight without the empty-search refusal, and its
  rows still carry `tool_path`, `matches`, and `nonzero_exits`.
- A release dry run publishes bare `orin.exe`, `on.exe`, the zip, and
  `SHA256SUMS.txt`, and the documented install line works from that release.
- Public copy passes the rules above: outcomes, runner-tagged numbers, no
  internal identifiers, no hedging.
