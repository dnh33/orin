# CONTEXT.md

Canonical vocabulary for orin. Glossary only: each term names one thing and its
one meaning here.

| Term | Meaning |
|---|---|
| orin | The product and its single binary, `orin.exe`. Lowercase wordmark. |
| orind | Historical name for the daemon executable. Now a subcommand: `orin daemon`. Never a product name. |
| on | A byte-identical copy of `orin.exe`. argv[0] stem `on` makes `on foo` mean `orin query foo`. |
| daemon | The resident process (`orin daemon`) that owns the index and serves the pipe. Auto-spawned on first use. |
| index | The resident in-memory structure: fixed-size entries, a name arena, and sorted keys. |
| entry | One indexed file, directory, or link. 20 bytes: name offset+len, flags, depth, parent, size, mtime. |
| arena | The names arena: file names stored once as bytes; entries point in by offset and length. |
| sorted keys | The live entry ids ordered by folded name; the search fold walks this order. |
| fold | Case-insensitive comparison form of a name used for matching and ordering. |
| snapshot | `orin.snap`: a CRC32-checked, versioned, atomically written image of the index for warm starts. |
| checkpoint | The periodic writer of snapshots (crash durability). |
| revalidate | A pass reconciling disk against index. Runs at startup and on rescan, never on a timer. |
| scan | The initial walk of a root that builds the index. |
| root | A directory subtree the daemon indexes and watches. Configured by `ORIN_ROOTS` (`;`-separated). |
| hit | One query result: an absolute path plus metadata. |
| query language | The search syntax (literals, phrases, `!negation`, `ext:`, `size:`, `type:`, `depth:<`/`depth:>`, `path:`, `re:`, globs). See `context/query-language.json`. |
| wire | The length-prefixed JSON request/response protocol over a named pipe. |
| MCP | Model Context Protocol. `orin mcp` serves `find`, `stat`, `status` over stdio. |
| cursor | Opaque pagination token returned as `next_cursor` by MCP `find`. |
| partial | MCP `find` flag: the index was still converging when the page was served; retry later. |
| p50 | Median end-to-end CLI latency including process spawn, as measured by `orin-bench`. |
| spawn floor | The 6.5 ms cost of starting `orin.exe` per invocation; the known p50 floor for CLI queries. |
| self-validating rows | Benchmark rows that record `matches`, `nonzero_exits`, and `tool_path` so fast failures can never score as fast searches. |
| planted coverage | Deterministic corpus files placed so every query class has known hits at every corpus size. |
