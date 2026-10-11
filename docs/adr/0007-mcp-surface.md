# 0007. MCP surface: official SDK, paginated tools, honest partial flag

Status: accepted (2026-10-10)

## Context

orin's agent story needs a real MCP server, not a wrapper script. The surface
must be usable within token budgets and must not let a still-converging index
read as authoritative.

## Decision

- Built on `rmcp`, the official Model Context Protocol Rust SDK, pinned via
  `Cargo.lock` (stable spec 2026-07-28, with negotiation back to 2025-11-25).
- Tools: `find` (query, limit, cursor), `stat` (path), `status` (no args).
- `find` clamps `limit` to 500 per call, pages with an opaque `cursor` and
  `next_cursor`, and returns `partial: true` while the index is converging so
  callers can retry.
- Responses carry paths and metadata only, never file contents.
- Failures return `isError` results with the message; no panics on request
  paths.

## Consequences

- `partial` costs one extra status round trip per `find`; accepted because
  silent wrong answers are worse for agents than a small latency tax.
- `#[derive(JsonSchema)]` expands to `schemars::` paths, so `schemars` is a
  declared direct dependency even though `rmcp` re-exports it.
- Tool-specific query syntax (filters, globs, regex) rides through unchanged:
  the same language serves CLI, TUI, and MCP.
