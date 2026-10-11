# 0006. Path resolution: transient lookup map, not linear scans

Status: accepted (2026-10-10)

## Context

`lookup_path` shipped as a linear scan with a comment saying "slow but correct
for tests". Production revalidation called it per walked entry, and each call
rebuilt a full path allocation per index entry. On a 25,926-entry corpus a
startup revalidation took ~6.5 minutes (measured twice). Warm starts paid the
same cost. The benchmark's readiness gate masked it by accepting first-entry
answers against a still-scanning index.

## Decision

- `lookup_map_owned()` builds a transient `(parent, name) -> id` map in one
  pass with owned keys, so the map outlives the lock it was built under.
- `lookup_in()` resolves a path component-wise: strip the owning root, then
  walk `(parent, name)` steps. O(depth) per lookup.
- `lookup_path()` keeps its signature and spends one map build per call;
  bulk callers build the map once per pass. Revalidation carries a depth
  stack over its pre-order walk and seeds added entries, so children resolve
  against fresh parents.
- Seeding: the root directory entry is identified by its NAME matching the
  root path (an entry with `parent == MAX` at the head of the root's range is
  not sufficient, since root-level files share that shape). Indexes without a
  root entry seed virtually at `u32::MAX`, matching how incremental inserts
  key root-level children.

## Consequences

- Cold scan and warm start dropped from ~400 s to ~0.85 s at 25,926 entries
  (measured, GitHub `windows-latest`, replication runs).
- The map is O(n) transient memory during a pass and dropped after; a
  maintained persistent map is the v0.2 path if revalidation at 3M entries
  needs it.
- The benchmark readiness gate now requires the daemon's `ready` state, not
  first entries, so partial indexes can never be timed as complete ones.
