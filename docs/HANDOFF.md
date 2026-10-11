# HANDOFF

Entry point for the next session on orin. Read `AGENTS.md`, then this file,
then `CONTEXT.md`.

## Starting prompt

> Continue the orin project at github.com/dnh33/orin. Read AGENTS.md for the
> rules and file map, docs/OVERVIEW.md for decided/measured/open, and
> docs/adr/ before changing anything those records govern. Do not re-derive:
> the measured numbers in context/product.json, the Windows-only ruling
> (ADR 0002), the single-binary layout (ADR 0001), or the benchmark integrity
> rules (ADR 0005).

## Execution loop

1. Check CI on the branch before assuming anything works: the gate is
   `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and
   `cargo deny`, all with `--locked` where it applies.
2. Small commits, pathspec form (`git commit -m msg -- <paths>`), because
   multiple agents share the tree.
3. Any latency claim needs a benchmark row; any benchmark change must keep
   rows self-validating (ADR 0005).
4. Public copy: outcomes only, measured numbers with their runner named.

## Constraints that bite

- No local Rust toolchain is assumed; CI is the compiler. A change is
  "verified" when the gate is green, not when it looks right.
- The release contract is `install.ps1`'s: a bare `orin.exe` asset on the
  GitHub release, fetched via the API (ADR 0001).
- The spec's cross-platform text is superseded (ADR 0002).

## Definition of done

- Green CI on the branch and on `main`.
- New behavior covered by one crucial test (not ceremony).
- `AGENTS.md` "Current state" and `docs/OVERVIEW.md` updated in the same
  change when a recorded fact moves.
- Nothing in public copy that is unmeasured, unverified, or internal.
