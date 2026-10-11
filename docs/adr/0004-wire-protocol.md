# 0004. Wire protocol: length-prefixed JSON over a user-scoped named pipe

Status: accepted (2026-10-08)

## Context

CLI, TUI, MCP server, and tooling all need a stable channel to the resident
daemon. The channel must be local-only, user-scoped, debuggable, and cheap.

## Decision

- Transport: a Windows named pipe scoped to the user
  (`\\.\pipe\orin-<user>-<hash>`).
- Frame: `u32` little-endian length plus one UTF-8 JSON object. JSON keeps
  the protocol inspectable; framing leaves room to swap encodings later.
- Requests: `ping`, `status`, `query`, `stat`, `roots.list|add|remove`,
  `rescan`, `stop`.
- The client auto-spawns the daemon on first contact (`DETACHED_PROCESS`),
  honors `ORIN_NO_SPAWN`, and retries with backoff.

## Consequences

- Changing the wire shape is an ADR, not a refactor.
- Clients parse JSON defensively and treat a missing field as absent, which
  let `partial` and pagination fields ship without version bumps.
- Accepted cost: JSON serialization is a small share of query latency; the
  measured floor is process spawn, not the wire.
