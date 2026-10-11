# 0008. License: MIT OR Apache-2.0

Status: accepted (2026-10-11, owner asked for the analysis)

## Context

The license was set early as dual MIT/Apache-2.0 without a recorded
reasoning. The owner asked for a deliberate re-derivation. orin's adoption
goal is unusual: the primary integrators are AI agents and the tools that
host them, so the license's job is to remove every reason NOT to embed it.

Competitor landscape (verified 2026-10-11): fd is MIT OR Apache-2.0,
ripgrep is Unlicense OR MIT, fzf is MIT, fsearch is GPL-2.0, Everything is
proprietary freeware.

## Decision

Keep MIT OR Apache-2.0. Rejected alternatives and why:

- **GPL-2.0/3.0 (fsearch's choice):** reciprocity is the wrong goal for an
  agent-integration tool. Hosts embedding `orin mcp` or shipping `orin.exe`
  inside closed products would simply avoid it. fsearch can afford GPL as a
  standalone desktop app; orin cannot as a platform primitive.
- **MPL-2.0:** file-level copyleft is friendlier but still gives corporate
  counsel a file to read twice. No upside over dual permissive here.
- **Unlicense OR MIT (ripgrep's choice):** maximally permissive, but no
  patent grant. orin is a resident daemon that touches a lot of system API
  surface; the Apache patent clause costs nothing and covers real ground.
- **0BSD:** drops both the patent grant and the attribution request.
  Attribution in docs is cheap and is how the tool spreads.

## Consequences

- `LICENSE-MIT` + `LICENSE-APACHE` ship in the repo and in the release zip;
  README and the website state the license visibly.
- Dependencies must keep passing the `cargo deny` allowlist (rule 8 in
  `AGENTS.md`); several deps (0BSD, CC0-1.0) are more permissive than orin,
  which is fine in the dependency direction and would be wrong as the
  project license.
- Sole authorship means this stays a one-commit decision to revisit; if it
  is ever changed, this ADR gets a superseding successor, not an edit.
