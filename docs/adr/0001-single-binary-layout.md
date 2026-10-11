# 0001. One binary: daemon as subcommand, alias as copy

Status: accepted (2026-10-10)

## Context

v0.1 shipped as three executables: `orin.exe` (CLI), `orind.exe` (daemon),
`on.exe` (short alias as its own build target). The install surface read as
"three things installed", and the first review question was exactly that.
Separate crates for an alias existed only because Windows symlinks need
privilege.

## Decision

One built product binary, `orin.exe`, with subcommands `query`, `status`,
`tui`, `mcp`, `daemon`. The short name `on` is a byte-identical copy of the
binary dropped by the installer; when argv[0]'s stem is `on` (case-insensitive)
and the first argument is not an explicit subcommand or root flag, the command
behaves as `orin query` with the remaining arguments. `orin-daemon` became a
library the CLI calls.

## Consequences

- Every additional executable must answer "why is this not a subcommand?".
- `install.ps1` downloads the Windows archive asset (`orin-*-windows-x86_64.zip`,
  whose only product binary is `orin.exe`) and creates `on.exe` as a
  `Copy-Item`. The release workflow publishes that zip plus a bare `orin.exe`
  and `SHA256SUMS.txt`.
- `orin mcp` auto-spawns `orin daemon` from its own executable path.
- Historical name `orind` survives only in on-disk filenames
  (`orind.log`/`orind.lock`) to avoid orphaning existing installs.
