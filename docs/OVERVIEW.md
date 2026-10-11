# OVERVIEW

The consolidated brief for orin: what is decided, what is measured, what is
open. Detail lives in the files each section links.

## What orin is

Whole-disk instant file search for Windows. A resident daemon holds every file
name in memory and keeps the index live from filesystem events, so queries
answer from RAM in single-digit milliseconds where `find`, `fd`, and ripgrep
walk the disk and take 40-60 ms. One binary serves scripts (`orin query`),
humans (`orin tui`), and AI agents (`orin mcp`); `on` is a two-letter alias.
See `context/product.json` for the surface map.

## What is decided

Hard decisions are recorded in `docs/adr/`: one binary with the daemon as a
subcommand (0001), Windows-only by owner ruling (0002), the index memory
layout (0003), the wire protocol (0004), benchmark integrity rules (0005),
path resolution via a transient map (0006), and the MCP surface (0007).

The query language (literals, phrases, negation, `ext:`, `size:`, `type:`,
`depth:</>`, `path:`, `re:`, globs) is shared by all three surfaces and
specified in `context/query-language.json`.

## What is measured

All numbers from GitHub Actions `windows-latest` runners on the deterministic
25,926-entry corpus (basis noted in `context/product.json`):

| Measurement | Result |
|---|---|
| Warm query p50 (orin) | 8-14 ms |
| Warm query p50 (find / fd / rg) | 45 / 61 / 41-58 ms |
| Cold scan to ready | 0.85 s (30k entries/s on this corpus) |
| Warm start from snapshot | 0.85 s |
| CLI process spawn floor | 6.5 ms of every query |

The spawn floor matters for reading the table: the index answers in under
3 ms; starting `orin.exe` costs twice that. Per-row decomposition
(`spawn_us`, `status_us`) ships in every benchmark row (ADR 0005).

## What is open

1. README rewrite carrying the measured table with runner attribution
   (rule 7 in `AGENTS.md`).
2. Scale probes at 1M and 3M entries: the spec's targets (warm p50 2/5 ms,
   scan 300k entries/s, snapshot load 300 ms, RSS 60/140 MB) are verdicts for
   the large corpora, not for the tiny one.
3. Persistent child map if revalidation at 3M entries needs to shed its
   transient O(n) map (ADR 0006).
4. Website: benchmark numbers page wired to the campaign data.
