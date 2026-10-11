# 0003. Index memory layout: 20-byte entries, name arena, sorted keys

Status: accepted (2026-10-08), amended 2026-10-10 (alignment handling)

## Context

The index must stay resident for millions of entries inside a documented
memory budget (60 MB at 1M entries), answer queries in single-digit
milliseconds, and survive crashes without corruption.

## Decision

- Entries are fixed 20 bytes (`#[repr(C)]`): name offset and length, flags,
  depth, parent id, size, mtime.
- Names live once in a byte arena; entries reference them by offset and
  length. Truncation past 64 KB is flagged in the entry.
- A sorted array of live entry ids (folded-name order, ties by id) drives the
  search fold; inserts maintain it incrementally, removals tombstone.
- Snapshots (`orin.snap`) are versioned with a CRC32 header and written
  atomically (temp file plus rename).

## Consequences

- Per-entry accounting is published so the memory claim is auditable.
- Alignment amendment (2026-10-10): file buffers are byte-aligned, so typed
  slice casts over them were undefined behavior. Snapshot decode reads fields
  with `read_unaligned` in copy loops. `Pod`/`repr(C)` describes layout, not
  the alignment of arbitrary byte buffers.
- Root directory entries sit at the head of their root's id range; path
  resolution keys children under that entry (see ADR 0006).
