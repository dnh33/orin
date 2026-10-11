# orin

orin is whole-disk instant file search for Windows: a resident index answers warm queries at a median p50 of 7 ms (5-9 ms for most queries), while `find`, `fd`, and `rg` walk the disk at 45-65 ms. Measured on named GitHub Actions `windows-latest` runners over a 25,926-entry deterministic corpus.

One binary, `orin.exe`, is a CLI, an interactive picker, and an MCP server for AI agents. `on.exe` is the same bytes under a two-letter name.

## Install

```powershell
irm https://github.com/dnh33/orin/raw/main/install.ps1 | iex
```

No admin rights required. The installer puts `orin.exe` and `on.exe` (a byte-identical copy of `orin.exe`) in `%LOCALAPPDATA%\orin` and adds that folder to your user PATH. Run it again any time to update in place.

To install by hand, download [`orin-<version>-windows-x86_64.zip`](https://github.com/dnh33/orin/releases/latest), unzip it anywhere, and add that folder to your PATH. The zip carries both binaries.

## Usage

| Command | What it does | Example |
| --- | --- | --- |
| `orin query` | one-shot search; `--json` prints JSON Lines | `orin query "budget 2026" --json` |
| `orin status` | daemon health: state, entries, memory, roots | `orin status` |
| `orin tui` | interactive picker; Enter prints the selected path | `orin tui` |
| `orin mcp` | MCP server over stdio for AI agents | `orin mcp` |
| `orin daemon` | the resident indexer, started for you[^daemon] | first search starts it |

Your first command starts the daemon if it is not running, waits until the index is ready, and answers from there; every command after that talks to the same resident process.

`on` is the two-letter alias: `on budget` is exactly `orin query budget`.

[^daemon]: You can run `orin daemon` yourself to watch it work in the foreground. Nothing in normal use requires it.

## Query language

Every surface speaks the same query language.

| Term | Matches | Example |
| --- | --- | --- |
| literal | bare words, case-insensitive; several words match together as one phrase | `Cargo.toml` |
| quoted phrase | words in order, exactly as written | `"budget 2026"` |
| negation | everything that does not contain the term | `!test_` |
| extension | `ext:rs`, `ext:!lock`, `ext:jpg,png` | `ext:jpg,png` |
| size | `size:>1M`, `size:<10K`, `size:100K..1M` (K, M, G; a bare number is bytes) | `size:>1M` |
| kind | `type:f` file, `type:d` directory, `type:l` symlink | `type:f` |
| depth | `depth:<3`, `depth:>1`: prefix forms only, there is no exact-depth form | `depth:<3` |
| path segment | a term that must appear as a path segment | `path:src` |
| regex | regular expression over the name | `re:^app_.*` |
| glob | `*` or `?` inside a term | `*.rs` |

Defaults: case-insensitive, best matches first, up to 1000 results.

## For AI agents

**MCP tools.** `orin mcp` speaks MCP over stdio and exposes three tools:

- `find`: search the index and return matching paths. Takes `query`, `limit` and `cursor`; results arrive a page at a time, with a `next_cursor` while more remain.
- `stat`: exists, kind, size and mtime for one path.
- `status`: entries, roots, version and state. Its `partial` flag says the index is still being built, so you know a result set may be short and worth retrying.

**JSON Lines contract.** `orin query --json` prints one JSON object per line, one per match: `path`, `name`, `type`, `size`, `mtime`, `score`. No headers, no decoration, ready to stream into a parser.

**Exit codes.**

| Code | Meaning |
| --- | --- |
| 0 | results printed |
| 1 | no match |
| 2 | error |

**Why an agent should prefer `orin mcp`.** Parsing CLI output means reading a human-formatted line, guessing at columns, and folding exit codes into your control flow. Over MCP you get typed tool calls with named arguments, pages you can resume with a cursor, and an explicit `partial` flag, so incompleteness is data instead of something you infer from a short list. Use `--json` inside a shell pipeline; use `orin mcp` when you are an agent deciding what to do next.

## Benchmarks

Every number below was measured on GitHub Actions `windows-latest` runners, over a 25,926-entry deterministic corpus.

| Measurement | Result |
| --- | --- |
| Warm query p50, orin | 7 ms median, 5-9 ms typical, 23 ms widest scans |
| Warm query p50, find | 45 ms |
| Warm query p50, fd | 62 ms median (60-69) |
| Warm query p50, rg | 52 ms median (45-66) |
| Cold scan to ready | 0.85 s (30k entries/s on this corpus) |
| Warm start from snapshot | 0.85 s |
| Process spawn per CLI query | 6.5 ms |
| Snapshot load on a real machine | about 130 ms (Windows 11 desktop, 3,638,273-entry index) |

**The spawn floor.** Starting `orin.exe` costs 6.5 ms on every CLI query, and that cost is inside the 5-9 ms above: at the fast end it is most of the total. It is the honest floor for one-shot commands. Long-lived callers, the daemon itself and `orin mcp`, do not pay it per query.

**Real machine.** The corpus rows are a controlled benchmark. The last row is one Windows 11 desktop holding a 3,638,273-entry index, where loading the snapshot takes about 130 ms.

**Why these numbers are trustworthy.** Every row is self-validating: it records how many matches the search produced, how many runs exited nonzero, and the exact path of the binary that produced the row. The harness refuses searches that match nothing, so a suspiciously fast row cannot be a tool answering with an empty result, and the comparison binaries are pinned and identity-checked, so `find` cannot be answered by some other program of the same name. The corpus is deterministic and plants coverage for every query class, so rows reproduce run to run.

## How it works

orin keeps every indexed file name in memory inside one resident process, which is a subcommand of the same `orin.exe`. A first scan fills the index (30k entries/s measured on the benchmark corpus), NTFS change events keep it current afterwards, and versioned atomic snapshots mean the next start loads instead of rescanning. Scripts, the TUI and the MCP server all reach that one index over length-prefixed JSON on a user-scoped named pipe, so every surface answers the same way.

## Windows only

orin is Windows only, by design. Cross-platform support is not planned.

## License

Licensed under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.