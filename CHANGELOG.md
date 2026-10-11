# Changelog

All notable changes to orin are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v0.1.0] - 2026-10-11

### Added

- `orin query`: one-shot whole-disk search, with `--json` for JSON Lines output and exit codes 0 (results), 1 (no match), 2 (error).
- `orin status`: daemon health at a glance: state, entries, memory and scanned roots.
- `orin tui`: an interactive picker; Enter prints the selected path, so it drops straight into a pipeline.
- `orin mcp`: an MCP server over stdio with `find`, `stat` and `status` tools, cursor pagination, and a `partial` flag for results gathered while the index is still filling.
- `orin daemon`: the resident indexer that holds the file names in memory, auto-spawned on first use so you never start it yourself.
- `on.exe`: a byte-identical copy of `orin.exe` created by the installer, so `on budget` searches for `budget`.
- Query language shared by every surface: literals, quoted phrases, `!negation`, `ext:` (with `ext:!` and comma lists), `size:` comparisons and ranges, `type:` for files, directories and symlinks, `depth:` prefix bounds, `path:` segments, `re:` regular expressions, and globs.
- One-line Windows installer, no admin rights needed: it installs to `%LOCALAPPDATA%\orin` and adds that folder to your user PATH.
- Benchmarks that prove their own results: each row records how many matches it found, how many runs failed, and which binary produced it; a search that matches nothing is refused, and the comparison tools are identity-checked.

## [v0.1.1] - 2026-10-11

### Fixed

- Queries are answered while the first scan runs. On a large disk the daemon used to go quiet for minutes, because the scan finished before it started serving; it now starts serving first and fills the index behind it.
- The daemon reports its state honestly while the index is still being built, so you can tell a filling index from a ready one instead of guessing.

## [v0.1.2] - 2026-10-11

### Fixed

- First contact with the daemon now succeeds on large disks: the wait for a freshly spawned daemon was too short for a big index, so the first command could fail before the daemon was ready. The wait now covers that case.
- Shell pipelines no longer hang after orin auto-spawns its daemon: the spawned daemon could inherit the shell's pipe handles, which kept the pipeline open after orin itself had finished.

## [v0.1.3] - 2026-10-11

### Fixed

- A spawned daemon can always be found now: the pipe name contained the process id, so no client could compute the daemon's name and reach the one already running.
