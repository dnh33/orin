---
name: orin-search
description: "Use when an agent needs to search files with orin or on."
version: 0.1.0
author: dnh33, Hermes Agent
license: MIT OR Apache-2.0
platforms: [windows]
metadata:
  hermes:
    tags: [orin, search, mcp]
    related_skills: [orin-development]
---

# orin Search Skill

orin is instant whole-disk file search on Windows: one binary (`orin.exe`) with a
resident indexer daemon, plus `on.exe`, a byte-identical copy of that same binary.
This skill covers the consumer interface: which entry point to reach for, the query
grammar, the exit-code and JSON Lines contracts, and the MCP tools and their
pagination protocol. Changing the orin codebase itself belongs in the
`orin-development` skill.

## When to Use

- You need file paths and orin is installed: search before walking directories.
- You need structured results or pages of hits inside an agent session: `orin mcp`.
- You need one answer from a shell, with a machine-checkable exit code:
  `orin query`.
- You want the shortest spelling of a search: `on`.

Don't use for: reading or editing the orin source tree (see `orin-development`),
or for searching a tree you have no shell access to.

## Prerequisites

- `orin.exe` and `on.exe` on PATH. The installer puts both in the user install
  folder and adds that folder to the user PATH.
- Windows only: the daemon serves a user-scoped named pipe and clients start it
  with `DETACHED_PROCESS`. No other platform is supported.
- Nothing to start by hand: the first query auto-spawns the daemon (below).

## Choosing an entry point

| Need | Reach for | Why |
|---|---|---|
| Session search with tools | `orin mcp` | `find`, `stat`, `status`, cursor paging |
| One-shot shell search | `orin query <terms>` | stdout text or JSON Lines, exit codes |
| Shortest one-liner | `on <terms>` | byte-identical copy; `on foo` == `orin query foo` |
| Interactive browsing | `orin tui` | ratatui picker; `--open` launches the pick |
| Daemon health | `orin status` | version, state, entry count, roots |

Register the MCP server with your host as `{"command": "orin", "args": ["mcp"]}`.
The MCP client process stays up between calls, so the per-query process-spawn cost
that every `orin query` pays does not apply to `find` calls.

## Exit-code contract

| Code | Meaning |
|---|---|
| 0 | command succeeded, results printed |
| 1 | valid query, no matches (an answer, not a failure) |
| 2 | error: daemon unreachable, parse or protocol failure (message on stderr) |

Branch on 1 only to detect an empty result set. Only 2 is a failure.

## Query language

Terms are separated by spaces, and a hit must satisfy every term. Double quotes
group a phrase. Filters are `key:value`; a bare word searches names.

| Term | Matches | Example |
|---|---|---|
| literal word | names containing the word | `orin query report` |
| quoted phrase | names containing the phrase | `orin query "release notes"` |
| `!term` | excludes what the inner term matches | `orin query report !draft` |
| `ext:rs` | extension is rs | `orin query ext:rs` |
| `ext:!lock` | extension is not lock | `orin query ext:!lock` |
| `ext:jpg,png` | extension is jpg or png | `orin query ext:jpg,png` |
| `size:>1M` | at least 1 MiB | `orin query size:>1M` |
| `size:100K..1M` | between 100 KiB and 1 MiB | `orin query size:100K..1M` |
| `size:<10K` | at most 10 KiB | `orin query size:<10K` |
| `type:f` | regular file | `orin query type:f` |
| `type:d` | directory | `orin query type:d` |
| `type:l` | link | `orin query type:l` |
| `depth:<3` | depth 3 or shallower | `orin query depth:<3` |
| `depth:>1` | deeper than depth 1 | `orin query depth:>1` |
| `path:segment` | path contains the segment | `orin query path:src` |
| `re:regex` | name matches the regex | `orin query re:^report` |
| glob `*.rs` | name matches the glob | `orin query *.rs` |

Depth has no exact form. `depth:3` is not a filter: the parser only accepts the
`<` and `>` prefixes, so `depth:3` falls through to a literal search for the text
`depth:3`. Ask for exactly depth 3 by combining both bounds,
`depth:>2 depth:<4`, which is the only spelling the grammar has.

Size units are binary: `K`, `M`, `G`, `T`, so `1M` is 1,048,576 bytes.

## JSON Lines

`orin query <terms> --json` writes one compact JSON object per hit, one per line
(JSON Lines, not a JSON array):

```json
{"path":"C:/w/r.md","name":"r.md","type":"file","size":2048,"mtime":1712345678,"score":512.5}
```

- `type` is `file`, `dir`, `link`, or `other`.
- `size` is bytes, `mtime` is unix seconds, `score` is relevance.
- Default cap is 10,000 hits; change it with `--limit`.
- Without `--json` each line is `path`, `score`, then size.
- `orin status --json` prints one pretty JSON object instead: `version`,
  `protocol`, `state`, `entries`, `mem_bytes`, `unreadable`, `roots`,
  `progress`.

## MCP tools

`orin mcp` serves three tools over stdio. Tool failures come back as MCP error
results, not panics.

### find

Arguments:

- `query` (string, required): the query language from the table above.
- `limit` (integer, optional): default 20, hard clamp 500. Never assume one
  page holds every hit.
- `cursor` (string, optional): the opaque `next_cursor` from the previous call.
  Omit it for page one.

Result:

- `query`: the query that ran.
- `hits`: objects with `path`, `name`, `type`, `size`, `mtime`, `score`,
  best match first.
- `partial` (boolean): true while the index is still converging.
- `next_cursor` (string, absent on the last page): pass it back to continue.

### stat

Arguments: `path` (string, required, an absolute path).

Result: `path`, `exists`, `kind`, `size`, `mtime`, `depth`. Here `kind` is
numeric (0 file, 1 dir, 2 link), and when `exists` is false the fields after it
are null.

### status

Arguments: none.

Result: `version`, `protocol`, `state`, `entries`, `mem_bytes`, `unreadable`,
`roots`, `progress`. `state` is `building`, `ready`, or `revalidating`; each
root is `path`, `entries`, `watch`, with `watch` in `polling`, `notify`, `off`;
`progress` appears only while a scan runs.

### Pagination protocol

1. Call `find` with `query` and no `cursor`. The first page holds up to
   `limit` hits (20 by default).
2. While `next_cursor` is present, call again with the same `query` and that
   `cursor`. Treat the cursor as opaque and pass it back verbatim; a cursor
   that does not parse is an error, not a fresh page one.
3. Stop when `next_cursor` is absent. An exact boundary can produce one extra
   empty page, so keep paging while a cursor comes back even if `hits` is
   empty.
4. When `partial` is true the index is still converging: call `status`, wait
   for `state` `ready` with no `progress`, then run the query again from page
   one. A partial result set is not the final answer.

## Auto-spawn

The first `orin query`, `orin status`, or `orin mcp` call connects to the
user-scoped named pipe. If no daemon answers, the client starts `orin daemon`
detached, with no console window, then retries the connection with backoff for
up to 3 seconds. The wire protocol is a 4-byte little-endian length prefix
followed by a JSON frame, capped at 16 MiB.

- `ORIN_NO_SPAWN=1` forbids spawning. With no daemon running, the call fails
  with `daemon not running; ORIN_NO_SPAWN=1 forbids auto-spawn` and exit 2.
- Set it in tests, CI steps, and scripts where an unnoticed background indexer
  would be a side effect you did not ask for.

## Quick Reference

```text
terminal(command="orin query report --json")   JSON Lines hits, exit 0/1/2
terminal(command="on report")                  same as orin query report
terminal(command="orin status --json")         daemon health as JSON
terminal(command="orin tui")                   interactive picker
terminal(command="orin mcp")                   stdio MCP server (find/stat/status)
```

## Pitfalls

1. Exit code 1 is a real answer, "no matches". Reporting it as an error loses
   the distinction between an empty result and a broken daemon.
2. The `on` alias keys off the executable name: `on foo` runs
   `orin query foo`, while `on status` runs `orin status` and `on --help` shows
   root help. Explicit subcommands and root flags always win over the alias.
3. Limits differ per entry point: `orin query` defaults to 10,000 hits, MCP
   `find` to 20 with a hard clamp of 500. Page with cursors instead of assuming
   you have everything.
4. `--json` on `query` is JSON Lines; `--json` on `status` is one pretty
   object. Do not parse the whole query output as a single JSON value.
5. `partial: true` and an empty page that still carries `next_cursor` are both
   normal states with defined follow-ups, not failures.

## Verification

- `terminal(command="orin status")` prints version, state, entry count, and
  roots, and exits 0.
- `terminal(command="orin query report --json")` emits one JSON object per
  line, and every line parses on its own.
- Contract checks: `orin query ext:rs --json` returns only `.rs` paths, and
  `orin query type:d` returns only directories. A mismatch is a bug in orin to
  report, not a pattern to code around.
- `on report` and `orin query report` print the same hits.
- With the MCP server registered, page two continues page one, and the final
  page has no `next_cursor`.
