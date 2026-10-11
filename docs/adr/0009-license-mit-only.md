# 0009. License: MIT only

Date: 2026-11
Status: accepted (supersedes [0008](0008-license-mit-apache.md))

## Context

The project shipped as MIT OR Apache-2.0 (ADR 0008). The owner reviewed the
choice and ruled for a single license: MIT. He considered MIT the best fit for
the project and wanted the license stated plainly and shown on the website.

## Decision

orin is licensed under the MIT license only. `LICENSE-MIT` is the license text;
`LICENSE-APACHE` is removed. Every public surface (README, website, crate
metadata, the AI context layer) states MIT and links the MIT text.

The cargo-deny dependency allowlist is unaffected: it constrains which
dependency licenses are acceptable, not the project's own license.

## Consequences

- `Cargo.toml` `license = "MIT"`.
- Reuse is under one well-understood permissive text; no dual-license
  boilerplate in downstream copies.
- ADR 0008 remains as history; this ADR supersedes it.
