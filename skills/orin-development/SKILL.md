1|---
2|name: orin-development
3|description: "Use when working on the orin Rust codebase itself."
4|version: 0.1.0
5|author: dnh33, Hermes Agent
6|license: MIT OR Apache-2.0
7|platforms: [windows]
8|metadata:
9|  hermes:
10|    tags: [orin, rust, ci, benchmarks]
11|    related_skills: [orin-search]
12|---
13|
14|# orin Development Skill
15|
16|orin is a Rust workspace that ships one Windows binary. This skill is for agents
17|changing it: crate layout, the exact gate commands, the testing policy, the
18|benchmark-integrity rules, the release sequence, and the rules for public copy.
19|Using orin to search files belongs in the `orin-search` skill.
20|
21|## When to Use
22|
23|- Editing anything under `crates/`, a workflow, the installer, or public copy.
24|- Reviewing a change against the gate, the benchmark rules, or the asset
25|  contract before it is called done.
26|- Deciding what may be published in README or site copy.
27|
28|Don't use for: querying files with orin (see `orin-search`).
29|
30|## Prerequisites
31|
32|- Rust stable with `rustfmt` and `clippy` (workspace MSRV 1.85, edition 2024).
33|- A committed `Cargo.lock`: every command below passes `--locked`, so a stale
34|  or missing lockfile fails the gate rather than silently resolving.
35|- A Windows build host, or CI: the crates use named pipes and process creation
36|  flags, and CI runs on `windows-latest`.
37|- Pathspec commits. Agents share this tree, so stage explicit paths
38|  (`git add <paths>`) and commit with a pathspec; never a bare `git add -A`.
39|
40|## Workspace layout
41|
42|- `orin-core` (library): the index (20-byte fixed entries, name arena, sorted
43|  u32 keys), query parse/filter/matcher/scorer, the length-prefixed wire
44|  protocol, snapshot handling for `orin.snap`, config, and platform paths. No
45|  async runtime, no UI, no network dependencies.
46|- `orin-daemon` (library): everything `orin daemon` runs: scan, watch, apply,
47|  revalidate, checkpoint, priority, and the named-pipe server that owns the
48|  live index.
49|- `orin-cli` (binary `orin`): subcommands `query`, `status`, `tui` (ratatui
50|  picker), `mcp`, and `daemon`; the IPC client with auto-spawn; and the `on`
51|  argv[0] alias that rewrites to `orin query`.
52|- `orin-mcp` (library): the stdio MCP server exposing `find`, `stat`, and
53|  `status`, plus its own daemon client.
54|- `orin-bench` (binary `orin-bench`): deterministic corpus generator, the query
55|  matrix in `crates/orin-bench/queries.json`, the timed runner with preflight,
56|  report comparison, and daemon probes.
57|
58|## The gate
59|
60|Run each command from the workspace root through `terminal(...)`. Every one must
61|exit 0 before the change is called done; CI runs the same set on
62|`windows-latest`.
63|
64|1. Format: `cargo fmt --check`. Completion: no diff. CI enforces rustfmt, and
65|   the `fmt-fix` workflow (dispatch with a branch input) runs `cargo fmt --all`
66|   and pushes `style: cargo fmt` when anything moved.
67|2. Types: `cargo check --workspace --all-targets --locked`. Completion: zero
68|   errors.
69|3. Lints: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
70|   Completion: zero warnings, because `-D warnings` turns each one into a
71|   failure.
72|4. Tests: `cargo test --workspace --locked`. Completion: all pass under the
73|   crucial-only policy below.
74|5. Release build: `cargo build --release --locked`. Completion: links cleanly;
75|   this is the shape the release job builds.
76|6. Licenses and advisories: `cargo-deny` against `deny.toml` plus RustSec
77|   against the pinned lockfile (the `security` workflow). The allowlist uses
78|   exact SPDX ids: `0BSD` and `CC0-1.0` are on it, while `BSD-0-Clause` is a
79|   different id that is not. Spell ids exactly.
80|
81|If a change needs a new dependency resolution, regenerate `Cargo.lock` through
82|the `lockfile` workflow (dispatch with a branch input) instead of editing it by
83|hand.
84|
85|## Testing policy: crucial-only
86|
87|- Only crucial tests live in the tree and run in CI: fast, deterministic,
88|  in-process. Integration-heavy and flaky suites are excluded by policy, not by
89|  oversight.
90|- Why: CI is the only build environment for this project, so every test costs
91|  shared runner time, and one flake is indistinguishable from a regression in
92|  the gate every other change is judged by.
93|- Completion: new behavior gets a deterministic unit test next to the code it
94|  exercises. If proving it needs sleeps, network, or a live daemon, the design
95|  is not ready; fix that first instead of adding a retry.
96|
97|## Benchmark integrity
98|
99|A change must not break any of these rules. They are what makes a published
100|number mean something:
101|
102|- Self-validating rows: every result row records `query_class`, `query`,
103|  `tool`, `tool_path`, `matches`, `nonzero_exits`, `spawn_us`, `status_us`,
104|  `p50_us`, `p95_us`, `p99_us`, and `qps`. `tool_path` proves which binary
105|  answered, `matches` proves the search did work, and `nonzero_exits` exposes
106|  tools that failed while appearing fast. Do not drop, rename, or round them.
107|- Preflight refuses empty searches: the runner fails a query that matches
108|  nothing for a tool, and the query matrix rejects empty needles. Every query
109|  class must keep hits in every corpus size for every compared tool, because a
110|  tool can win by returning nothing.
111|- Pinned tool binaries: the workflow verifies that `find` is GNU findutils and
112|  resolves `fd` and `rg`, then exports `BENCH_TOOL_FIND`, `BENCH_TOOL_FD`, and
113|  `BENCH_TOOL_RG` as absolute paths. Keep resolving through those variables. A
114|  bare `find` resolves to the System32 string filter, and its instant error
115|  exits would be timed as if they were fast searches.
116|- Deterministic corpus: `gen` defaults to seed `0xC0FFEE`, and the same seed
117|  must produce the identical tree. No unseeded randomness, and no corpus
118|  change that quietly invalidates comparison with earlier runs.
119|- Known floor: the CLI process spawn costs about 6.5 ms of every query, so a
120|  CLI p50 sits on that floor. Do not present a number as a query-time win
121|  without accounting for it.
122|
123|Harness shape, one subcommand per `terminal(...)` call:
124|
125|```text
126|cargo run --release -p orin-bench gen --corpus tiny --out <dir> --seed 0xC0FFEE
127|cargo run --release -p orin-bench run --tool orin --corpus <dir> --out <file> --iterations 1000
128|cargo run --release -p orin-bench probes --corpus <dir> --out <file>
129|```
130|
131|`run` is pointed at `crates/orin-bench/queries.json` for the query matrix.
132|`probes` waits out the 60-second checkpoint cadence, so it includes a
133|65-second wait; expect it to be slow.
134|
135|## Release sequence
136|
137|1. Push a tag matching `v*`, or dispatch the `release` workflow with a tag
138|   input. Completion: the job starts on `windows-latest`.
139|2. The job runs `cargo build --release --locked -p orin-cli`, then asserts that
140|   `orin.exe --version` reports the tag version and that `daemon --help`,
141|   `tui --help`, and `mcp --help` all run. One binary carries every subcommand.
142|   Completion: all four assertions pass, or the job fails by design.
143|3. Assets: bare `orin.exe`, a byte-identical `on.exe` copy, the zip holding
144|   both plus `install.ps1` and `README.md`, and `SHA256SUMS.txt`, published
145|   with `gh release create ... --latest`. Completion: the release lists all
146|   four assets.
147|4. Installer contract: the bare asset named exactly `orin.exe`, resolved from
148|   `/releases/latest` through the GitHub API. The installer stops a running
149|   `orin` process first, extracts, writes `on.exe` as a byte-identical copy,
150|   verifies `--version`, and adds the install folder to the user PATH only.
151|
152|```powershell
153|irm https://github.com/dnh33/orin/raw/main/install.ps1 | iex
154|```
155|
156|5. Never rename or drop the bare `orin.exe` asset. The installer resolves that
157|   exact name, so a zip-only release breaks the documented install line.
158|
159|## Public copy rules (README and site)
160|
161|- Outcomes first: what a user can do, and how fast, in plain sentences. No
162|  architecture tour, no crate or module names, no workflow or spec references,
163|  no internal env vars, no task ids, no machine or user paths.
164|- Every measured number names its runner and corpus. Approved headline: warm
165|  query p50 of 8-14 ms against find 45 ms, fd 61 ms, and rg 41-58 ms on
166|  `windows-latest` over a 25,926-entry corpus. Approved index numbers: cold
167|  scan 0.85 s (about 30k entries/s on the tiny corpus) and warm start from the
168|  snapshot 0.85 s.
169|- No development-state hedging: no "not tested yet", "should work", or
170|  "probably". A claim without a measurement gets cut, not softened.
171|- State decisions as decisions: Windows-only support is an owner decision
172|  recorded on 2026-10-10, and the binary is dual-licensed
173|  `MIT OR Apache-2.0` with dependency licenses allowlisted in `deny.toml`.
174|- When a number changes, re-measure on the runner and update it in the same
175|  change. A stale public number is worse than none.
176|
177|## Pitfalls
178|
179|1. Workflow `with:` values are scalar comma-separated strings, for example
180|   `components: rustfmt, clippy`. A YAML sequence there fails the workflow
181|   parse.
182|2. `interprocess` 2.4: import `use interprocess::local_socket::prelude::*;` at
183|   the top of the file, or `Listener as _` and `Stream as _` when only a method
184|   is used. There is no `interprocess::Error` type; expose errors as
185|   `std::io::Error` instead.
186|3. On a `Stream`, take the halves with `stream.split()`; `try_clone()` does not
187|   exist. `set_nonblocking` takes `ListenerNonblockingMode::Neither` or
188|   `::Both`, never a bool.
189|4. Snapshots: `orin.snap` is CRC32-versioned and written atomically through
190|   `orin.snap.tmp`, and entries are 20 bytes fixed with names in the arena. Use
191|   the existing read and write helpers instead of packing structs by hand.
192|5. Commit with pathspecs: `git add <paths>` then
193|   `git commit -m "..." -- <paths>`. Completion: `git status --porcelain`
194|   lists nothing you do not own.
195|6. Workspace dependency features are deliberate: `sysinfo` runs with
196|   `default-features = false, features = ["system"]`, so `system` is the
197|   anchor, not `process`. Changing a feature set is a decision, not a default.
198|
199|## Verification
200|
201|- All six gate commands exit 0 (see The gate).
202|- `git diff --name-only` lists only files this change owns.
203|- A harness run clears preflight without the empty-search refusal, and its
204|  rows still carry `tool_path`, `matches`, and `nonzero_exits`.
205|- A release dry run publishes bare `orin.exe`, `on.exe`, the zip, and
206|  `SHA256SUMS.txt`, and the documented install line works from that release.
207|- Public copy passes the rules above: outcomes, runner-tagged numbers, no
208|  internal identifiers, no hedging
- License is MIT ONLY (owner ruling, ADR 0009). Never write 'MIT OR Apache-2.0' in any artifact; the deny.toml dependency allowlist is unrelated.
.