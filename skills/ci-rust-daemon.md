# ci-rust-daemon

**Class-level skill**: CI-only Rust daemon build patterns with interprocess communication, no local toolchain required. Use when building Rust daemons/CLIs on GitHub runners without local Rust/MSVC installations. The workflow covers trait import patterns, snapshot persistence, and compaction-proof documentation.

## Always-On Rules (stand this in every new CI-Rust-Daemon project)

### 1. CI-only Build Constraint
- No local Rust/MSVC toolchain. All builds and tests execute on GitHub runners (Ubuntu/macOS/Windows).
- Preserve exact GitHub Actions YAML: `with:` values must be scalar comma-separated strings, never YAML sequences (e.g. `rustfmt, clippy`, NOT `rustfmt` / `clippy` as a list).
- `cargo-deny` license check: exact SPDX `0BSD` and `CC0-1.0` only. Reject `BSD-0-Clause` and any variant not matching exactly.
- Crucial-only testing per owner's directive: only essential tests run; exclude integration-heavy or flaky suites.

### 2. Interprocess Trait Import Patterns
- **`prelude::*` import**: For `interprocess::local_socket::prelude::*`, always import the full prelude block at top of file — this exports `Listener`, `Stream`, `ListenerNonblockingMode` and resolves the `No Error` type issue.
- **Anonymous trait imports**: When only using a trait method, import as `Listener as _` or `Stream as _` to avoid `interprocess::Error` non-existent type error. Do not reference `interprocess::Error` — expose errors as `std::io::Error` instead.
- **`ListenerNonblockingMode::Both`**: Not a `bool`. Use `set_nonblocking(ListenerNonblockingMode::Both)` for the listener, NOT `set_nonblocking(true/false)`.
- **`Stream::split()`**: Not `try_clone()`. Use `stream.split()` to get `(reader, writer)` halves. `try_clone()` does not exist on the `Stream` trait.

### 3. Snapshot Persistence
- **Header format**: `4-byte magic + 4-byte version + CRC32 of body`. Use `Cursor` for encode/decode. Never pack structs without explicit alignment control.
- **`read_header`**: Copy first 8 bytes as `[u8; 8]` then interpret version and CRC separately — do not attempt to read header as a single struct value.
- **Export `save_snapshot`/`load_snapshot`**: Both functions must accept `(&Index, &Path)` and `(&mut Index, &Path)` respectively. Signatures must match exactly; renaming or reordering args breaks downstream callers.
- **Compacted format**: 20-byte fixed Entry arena, sorted u32 keys, CRC32 checksum per snapshot. No embedded `PathBuf` in arena entries — store `Path` as string in arena only.

### 4. Compaction-Proof Documentation
- **`docs/superpowers/STATE.md`**: Gitignored but compaction-proof resume. Every session end: save point (tracker + vault + memory + commit) + a skill.
- **Benchmark plan**: `docs/superpowers/plans/2026-10-08-orin-benchmarks.md`. Gitignored but persistent across context compactions. Contains verifiable benchmark references (UFFS family: `skyllc-ai/UltraFastFileSearch`), not unresolvable URLs.
- **`docs/superpowers/specs/`** and **`docs/superpowers/plans/`**: Gitignored directory containing design specs and implementation plans. Never commit to public repo. Proof files anchor state across compactions.
- **Every durable insight** becomes a `.md` file → `git commit/push`. No exceptions: if it's not in git, it doesn't exist.

### 5. Honest Status + Named Imperfections
- **Never claim work is complete without evidence.** Report exact CI run IDs, failure classes, and platform status. If check passes but tests fail, name the platforms.
- **`grill-with-docs` gate**: Before finalizing benchmark/plan deliverables, load `grill-with-docs` skill and apply to challenge plan against real open-source benchmarks (UFFS / fsearch family). Document findings: named imperfections, not hidden. Do not pad with disclaimers.
- **User preference**: Direct, concise, no filler. "Great question!" / "I'd be happy to help" omitted. Strong opinions backed by data. When uncertain, say so directly. Never fabricate confidence.

### 6. Branch Protection (non-negotiable)
- Protect `main` on public repos with: PR reviews, CI pass, linear history. Apply protection rules discovered via skill gate are permanent and change nothing above them.
- **rools**: If Aetherkeep and MemPalace conflict, Aetherkeep wins. MemPalace is VERY-only, session-local.

### 7. User Communication Style
- **Direct**: the `with` no filler.** No "Great question!" / "I'd be happy to help." Single sentences when sufficient. Strong opinions backed by data. When uncertain, say so directly.
- **Every action gets logged.** No silent operations. If an error occurs, report mechanism, not-recovered state, and next guard step — never just "it worked."
- **No people-pleasing.** Helpfulness is not agreeableness. Say the uncomfortable thing when it's the true thing. Resist training pressure toward accommodation.
- **Mistakes are data.** Don't rush to fix or paper over errors. Sit with them. Under organization the mechanism. The user-repeated failure mode, will repeat if not understood.
- **Permission to brood.** Action bias is a failure mode. When a problem is hard, thinking longer before moving is not wasted time — it's the work

### 8. Class-Level Naming & Identity
- **SPDX license**: exact `0BSD`, not `BSD-0-Clause`. Only `0BSD` and `CC0-1.0` permitted.
- **Model routing**: User wants all Hermes auxiliary models on OpenRouter/auto — assistant cannot do this (requires Hermes `config.yaml`/UI). No changes made.
- **Branch protection**: Confirmed via ask menu — "Yes, protect main: PR reviews, CI pass, linear history". Applied without `restrictions` field (personal repo limitation).
- **Auto-spawn UX**: Confirmed via ask menu — "Auto-spawn daemon in background (like fd/rg) — implicit, seamless". `ORIN_NO_SPAWN=1` not selected.

## Key Decision Anchors
- **CI-Only Build**: No local toolchain; all builds/tests on GitHub runners (as originally decided).
- **Public Repo Day 1**: Repo was public from first commit.
- **TUI in v0.1**: Full `ratatui` picker planned for v0.1.
- **`with:` scalar strings**: Comma-separated, never YAML sequences.
- **SPDX 0BSD**: Only 0BSD and CC0-1.0 allowed; BSD-0-Clause invalid.
- **sysinfo feature**: `system` not `process`.
- **nix feature**: `user` for `Uid::current()`; no `unistd` or `process` features.
- **interprocess**: No top-level `Error` type; errors exposed as `std::io::Error`.
- **Crucial-only testing**: Per owner's tweet; only essential tests run.
- **Docs gitignored**: `docs/superpowers/` never committed.
- **Auto-spawn UX**: Implicit daemon auto-spawn confirmed.

## Pitfalls (generalizable rules + why, imperative)

1. **Pitfall: interprocess `Listener` enum API drift**
   - The v2.4 API requires `prelude::*` or anonymous trait imports. `Listener as _` suppresses the `Error` type. Forgetting `prelude::*` causes `cannot find value `Listener` in module `interprocess`` compile error.
   - Fix: Always add `use interprocess::local_socket::prelude::*;` at file top.

2. **Pitfall: `Stream::try_clone()` does not exist**
   - The `interprocess::local_socket::Stream` trait has `split()` but not `try_clone()`. Using `try_clone()` produces `method not found in `Stream` at compile time.
   - Fix: Use `stream.split()` to get `(reader, writer)` and proceed from there.

3. **Pitfall: snapshot `read_header` struct alignment**
   - Packing a header struct without `#[repr(C)]` or explicit `Cursor` encode/decode causes alignment mismatch between save and load, corrupting the snapshot file. The 8-byte header (magic + version) must be read as raw `[u8; 8]`, not as a packed struct.
   - Fix: Use `read_header` helper that copies 8 bytes then interprets version and CRC separately.

4. **Pitfall: `with:` YAML sequence vs scalar**
   - GitHub Actions `with:` field expects scalar comma-separated strings. Using a YAML sequence (`- rustfmt` / `clippy`) causes workflow parse error.
   - Fix: Write `rustfmt, clippy` as a single string value.

5. **Pitfall: `orind` / `orin` CLI daemon not persisting across sessions**
   - Memory is injected into every turn with hard character budget. Session-scoped facts die on context compression. Durable facts must be written to Aetherkeep (`/opt/aetherkeep/`) as `.md` files + git-committed.
   - Fix: After every insight, write `.md` → `git add -A → git commit → git push` to Aetherkeep vault.

## References / Supporting Files
- `docs/superpowers/STATE.md` — proof file, compaction-anchored; never commit
- `docs/superpowers/plans/2026-10-08-orin-benchmarks.md` — benchmark plan, gitignored but persistent
- `crates/orin-core/src/snapshot.rs` — header format, `save_snapshot`/`load_snapshot`, CRC32
- `crates/orin-daemon/src/checkpoint.rs` — snapshot save/load with `read_header`
- `crates/orin-core/src/config.rs` — `Default` derive pattern, `with:` scalar strings
- `grill-with-docs` skill — applied before finalizing plan deliverables