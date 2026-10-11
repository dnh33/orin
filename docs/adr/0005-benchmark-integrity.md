# 0005. Benchmark integrity rules

Status: accepted (2026-10-10)

## Context

A benchmark can "win" by failing fast, matching nothing, or quietly running a
different binary. Every one of those happened during development:
System32 `find.exe` (a string filter) answered the `find` arm and its instant
error exits timed as 5.7 ms searches; 9 of 10 queries matched an unshaped
corpus and measured empty work; artifact collisions dropped results silently.

## Decision

Benchmarks are self-validating and refuse to lie:

- Every row records `matches`, `nonzero_exits`, and `tool_path` (the resolved
  binary identity, not the command name).
- The harness refuses to time an empty search: a query with zero matches in
  the warmup aborts the run as a corpus/query misalignment.
- Tool binaries are pinned via environment and identity-gated
  (`find` must be GNU findutils, never System32).
- The corpus is deterministic (seed `0xC0FFEE`) and plants known coverage for
  every query class at every size.
- Tool-specific syntax (for example `ext:`) runs as its own arm and is never
  forced onto other tools.
- Decomposition probes (`spawn_us`, `status_us`) ride in every row so latency
  can be attributed, not just totaled.

## Consequences

- Match-count drift across runs became a diagnostic signal instead of noise.
- The `spawn_us` probe is how the 6.5 ms process-spawn floor was found and
  how the query path's real cost was separated from it.
- Adding a query class without deterministic plants fails preflight by
  design.
