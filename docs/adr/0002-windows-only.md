# 0002. Windows-only is the product

Status: accepted (2026-10-10, owner ruling)

## Context

The design spec positioned orin as cross-platform (Windows/macOS/Linux) and
the benchmark plan targeted Windows and Linux. During the benchmark push the
owner directed the work Windows-first, then ruled explicitly: Windows-only is
the product, not a gap.

## Decision

orin targets Windows only. The spec's cross-platform text is superseded by
this record. Unix code paths, `nix`-style dependencies, and `:`-separated
`ORIN_ROOTS` parsing were removed; roots split on `;`. CI and benchmarks run
on `windows-latest`.

## Consequences

- Do not add platform abstractions "for portability"; they are dead weight.
- README and the website state Windows plainly.
- Reopening this is an owner decision, recorded as a new ADR that supersedes
  this one.
