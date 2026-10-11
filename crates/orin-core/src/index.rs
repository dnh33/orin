//! The in-memory search index: entries, names, sorted keys, and roots.

use crate::arena::NamesArena;
use crate::entry::{ENTRY_SIZE, Entry};
use rayon::prelude::*;
use std::path::PathBuf;

/// Root directory metadata.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Root {
    pub path: PathBuf,
    pub first: u32,
    pub count: u32,
}

/// Builder-side row; the index assigns indices.
#[derive(Clone, Debug)]
pub struct NewEntry<'a> {
    pub name: &'a str,
    pub parent: Option<u32>,
    pub kind: u8,
    pub hidden: bool,
    pub size: u64,
    pub mtime: u32,
    pub root: usize,
}

/// Search result containing hits and metadata.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub total: u64,
    pub partial: bool,
    pub took_us: u64,
}

/// A search hit with scored entry data.
#[derive(Clone, Debug)]
pub struct Hit {
    pub idx: u32,
    pub score: f32,
    pub path: PathBuf,
    pub name: String,
    pub kind: u8,
    pub size: u64,
    pub mtime: u32,
}

/// File metadata for stat operations.
#[derive(Clone, Debug, serde::Serialize)]
pub struct StatInfo {
    pub path: String,
    pub exists: bool,
    pub kind: Option<u8>,
    pub size: Option<u64>,
    pub mtime: Option<u32>,
    pub depth: Option<u8>,
}

/// The in-memory search index.
pub struct Index {
    pub entries: Vec<Entry>,
    pub names: NamesArena,
    pub sorted: Vec<u32>, // indices into entries, sorted by folded name
    pub roots: Vec<Root>,
    pub tombstones: u32, // count of removed entries
    pub unreadable: u64, // count of unreadable dirs during scan
}

impl Index {
    /// Create a new empty index.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            names: NamesArena::default(),
            sorted: Vec::new(),
            roots: Vec::new(),
            tombstones: 0,
            unreadable: 0,
        }
    }

    /// Number of live entries (excluding tombstones).
    pub fn len(&self) -> usize {
        self.entries.len() - self.tombstones as usize
    }

    /// Returns true if the index has no live entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total bytes in the names arena.
    pub fn names_bytes(&self) -> usize {
        self.names.bytes.len()
    }

    /// Approximate memory usage of the index (entries + names + sorted + roots).
    pub fn mem_bytes(&self) -> usize {
        self.entries.len() * ENTRY_SIZE
            + self.names.bytes.len()
            + self.sorted.len() * 4
            + self.roots.len() * std::mem::size_of::<Root>()
    }

    /// Get the entry range for a root index.
    pub fn root_range(&self, root: u32) -> (u32, u32) {
        if root as usize >= self.roots.len() {
            return (0, 0);
        }
        let r = &self.roots[root as usize];
        (r.first, r.count)
    }

    /// Insert a batch of entries, returning their assigned indices.
    ///
    /// Entries must be in pre-order (parent before children). `base_depth` is
    /// the depth to assign to entries whose parent is `None` (roots).
    pub fn insert_batch(&mut self, items: &[NewEntry<'_>], base_depth: u8) -> Vec<u32> {
        let mut indices = Vec::with_capacity(items.len());
        let mut parent_map: std::collections::HashMap<(usize, u32), u32> =
            std::collections::HashMap::new(); // (root, local_parent_idx) -> global_idx

        for item in items {
            let global_idx = self.entries.len() as u32;

            // Determine parent index. Keys are batch-local (pre-order batches
            // see their parent first); a parent absent from the batch is a
            // pre-existing global entry (incremental inserts).
            let parent_idx = match item.parent {
                Some(parent_key) => parent_map
                    .get(&(item.root, parent_key))
                    .copied()
                    .unwrap_or(parent_key),
                None => u32::MAX,
            };

            // Depth
            let depth = if parent_idx == u32::MAX {
                base_depth
            } else {
                self.entries[parent_idx as usize].depth.saturating_add(1)
            };

            // Flags
            let mut flags = match item.kind {
                0 => 0, // TYPE_FILE
                1 => 1, // TYPE_DIR
                2 => 2, // TYPE_SYMLINK
                _ => 3, // TYPE_OTHER
            };
            if item.hidden {
                flags |= 4; // FLAG_HIDDEN
            }
            if item.root > 0 && parent_idx == u32::MAX {
                flags |= 16; // FLAG_ROOT
            }

            // Name handling
            let name_bytes = item.name.as_bytes();
            let name_len = name_bytes.len().min(u16::MAX as usize) as u16;
            let name_truncated = name_bytes.len() > u16::MAX as usize;
            if name_truncated {
                flags |= 8; // FLAG_NAME_TRUNCATED
            }
            let (name_off, _stored_len) = self.names.push(item.name);

            // Size (saturate at u32::MAX)
            let size = if item.size > u32::MAX as u64 {
                u32::MAX
            } else {
                item.size as u32
            };

            let entry = crate::entry::Entry {
                name_off,
                name_len,
                flags,
                depth,
                parent: parent_idx,
                size,
                mtime: item.mtime,
            };

            self.entries.push(entry);
            indices.push(global_idx);

            // Track for children
            if item.kind == 1 {
                // directory
                parent_map.insert((item.root, global_idx), global_idx);
            }
        }

        indices
    }

    /// Rebuild the sorted index array from live entries.
    ///
    /// Sorted by folded name (case-insensitive), ties by parent index.
    pub fn finalize(&mut self) {
        let live: Vec<u32> = (0..self.entries.len() as u32)
            .filter(|&i| self.is_live(i))
            .collect();

        self.sorted = live;
        self.sorted.par_sort_unstable_by(|&a, &b| {
            let name_a = self.names.get(
                self.entries[a as usize].name_off,
                self.entries[a as usize].name_len,
            );
            let name_b = self.names.get(
                self.entries[b as usize].name_off,
                self.entries[b as usize].name_len,
            );
            crate::fold::cmp_folded(name_a, name_b).then_with(|| a.cmp(&b))
        });
    }

    /// Insert a single entry into the sorted array (maintains order).
    pub fn sorted_insert(&mut self, idx: u32) {
        let pos = self.sorted.partition_point(|&i| {
            let name_i = self.names.get(
                self.entries[i as usize].name_off,
                self.entries[i as usize].name_len,
            );
            let name_new = self.names.get(
                self.entries[idx as usize].name_off,
                self.entries[idx as usize].name_len,
            );
            crate::fold::cmp_folded(name_i, name_new).is_lt()
        });
        self.sorted.insert(pos, idx);
    }

    /// Mark an entry as removed (tombstone) and remove from sorted array.
    pub fn remove(&mut self, idx: u32) {
        if !self.is_live(idx) {
            return;
        }
        self.entries[idx as usize].flags |= 0x80; // custom tombstone flag
        self.tombstones += 1;
        self.sorted.retain(|&i| i != idx);
    }

    /// Check if an entry index is live (not tombstoned).
    pub fn is_live(&self, idx: u32) -> bool {
        (idx as usize) < self.entries.len() && (self.entries[idx as usize].flags & 0x80) == 0
    }

    /// Get an entry by index.
    pub fn entry(&self, idx: u32) -> &crate::entry::Entry {
        &self.entries[idx as usize]
    }

    /// Get the name of an entry by index.
    pub fn name_of(&self, idx: u32) -> &str {
        let e = &self.entries[idx as usize];
        self.names.get(e.name_off, e.name_len)
    }

    /// Resolve an absolute path to an entry index via a prebuilt lookup map.
    ///
    /// Component-wise: strip the owning root, then walk `(parent, name)`
    /// steps. O(depth) once the map exists. The previous implementation
    /// scanned every entry and rebuilt a full path per comparison: O(n)
    /// allocations per call and O(n^2) for a revalidation pass.
    pub fn lookup_in(
        &self,
        map: &std::collections::HashMap<(u32, String), u32>,
        abs: &std::path::Path,
    ) -> Option<u32> {
        for r in &self.roots {
            let Ok(rest) = abs.strip_prefix(&r.path) else {
                continue;
            };
            // Seed at the root directory entry only when the head of the
            // range actually IS the root dir (its name matches): scan-built
            // indexes key children under it. Root-level entries with no root
            // entry at all (incremental/test inserts) key under u32::MAX, so
            // seed virtually and walk from there.
            let root_name = r.path.file_name().and_then(|n| n.to_str());
            let mut cur = match (self.entries.get(r.first as usize), root_name) {
                (Some(e), Some(name)) if e.parent == u32::MAX && self.name_of(r.first) == name => {
                    r.first
                }
                (Some(e), None) if e.parent == u32::MAX => r.first,
                _ => u32::MAX,
            };
            if rest.as_os_str().is_empty() {
                // The root path itself: a real entry when the scan created
                // one, `None` under a virtual seed (no root entry exists).
                return (cur != u32::MAX).then_some(cur);
            }
            for comp in rest.components() {
                let name = comp.as_os_str().to_str()?;
                cur = *map.get(&(cur, name.to_string()))?;
            }
            return Some(cur);
        }
        None
    }

    /// Transient `(parent, name) -> idx` map with owned keys.
    ///
    /// Build once per bulk pass (revalidation, imports) and resolve paths in
    /// O(1) with [`Index::lookup_in`]. Owned keys let the map outlive the
    /// index lock it was built under. O(n) memory, dropped after the pass.
    pub fn lookup_map_owned(&self) -> std::collections::HashMap<(u32, String), u32> {
        let mut map = std::collections::HashMap::with_capacity(self.entries.len());
        for &idx in &self.sorted {
            let e = &self.entries[idx as usize];
            let name = self.names.get(e.name_off, e.name_len);
            map.insert((e.parent, name.to_string()), idx);
        }
        map
    }

    /// Resolve an absolute path to an entry index.
    ///
    /// Spends one O(n) map build per call; callers resolving many paths in a
    /// row should build the map once with [`Index::lookup_map_owned`] and use
    /// [`Index::lookup_in`] (this is what revalidation does).
    pub fn lookup_path(&self, abs: &std::path::Path) -> Option<u32> {
        let map = self.lookup_map_owned();
        self.lookup_in(&map, abs)
    }

    /// Build the full path for an entry by walking parents to root.
    pub fn path_of(&self, idx: u32) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = idx;
        loop {
            let e = &self.entries[cur as usize];
            let name = self.names.get(e.name_off, e.name_len);
            parts.push(name.to_string());
            if e.parent == u32::MAX {
                let root_idx = self.find_root_for_entry(cur);
                if let Some(r) = self.roots.get(root_idx) {
                    parts.push(r.path.to_string_lossy().into_owned());
                }
                break;
            }
            cur = e.parent;
        }
        parts.reverse();
        std::path::Path::new("").join(parts.join(std::path::MAIN_SEPARATOR_STR))
    }

    fn find_root_for_entry(&self, idx: u32) -> usize {
        for (i, r) in self.roots.iter().enumerate() {
            if idx >= r.first && idx < r.first + r.count {
                return i;
            }
        }
        0
    }

    /// Search the index with a parsed query.
    pub fn search(&self, q: &crate::query::Query, _now_unix: u64) -> crate::query::SearchResult {
        use std::borrow::Cow;
        use std::time::Instant;
        let start = Instant::now();

        // Extract first literal term for simple search
        let folded_query = crate::fold::fold_vec(
            &q.terms
                .iter()
                .filter_map(|t| match t {
                    crate::query::Term::Lit(s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" "),
        );

        // Collect matching candidates as entry indices only (no name clones).
        let mut candidates: Vec<u32> = Vec::new();
        let mut buf = [0u8; 4096];

        for &idx in &self.sorted {
            if !self.is_live(idx) {
                continue;
            }
            let name = self.name_of(idx);
            // Fold into a reusable stack buffer; oversized names fall back to
            // a heap fold, so the folded bytes always equal `fold_vec(name)`.
            let folded: Cow<'_, [u8]> = match crate::fold::fold_try(name, &mut buf) {
                Some(n) => Cow::Borrowed(&buf[..n]),
                None => Cow::Owned(crate::fold::fold_vec(name)),
            };

            // Simple literal substring match on folded bytes
            if memchr::memmem::find(&folded, &folded_query).is_some() {
                candidates.push(idx);
            }
        }

        let total = candidates.len() as u64;

        // Pagination - keep results in sorted index order (folded name order)
        let start_idx = q.offset.min(candidates.len());
        let end_idx = (start_idx + q.limit).min(candidates.len());

        // Build Hit structs only for the paginated slice.
        let hits: Vec<crate::query::Hit> = candidates[start_idx..end_idx]
            .iter()
            .map(|&idx| {
                let name = self.name_of(idx);
                let e = &self.entries[idx as usize];
                crate::query::Hit {
                    idx,
                    score: crate::query::score::score_entry(e, name, &folded_query, e.flags & 3),
                    path: self.path_of(idx),
                    name: name.to_string(),
                    kind: e.flags & 3,
                    size: e.size as u64,
                    mtime: e.mtime,
                }
            })
            .collect();

        crate::query::SearchResult {
            hits,
            total,
            partial: false,
            took_us: start.elapsed().as_micros() as u64,
        }
    }

    /// Stat a path by absolute path.
    pub fn stat_path(&self, abs: &std::path::Path) -> crate::query::StatInfo {
        if let Some(idx) = self.lookup_path(abs) {
            let e = &self.entries[idx as usize];
            crate::query::StatInfo {
                path: abs.to_string_lossy().into_owned(),
                exists: true,
                kind: Some(e.flags & 3),
                size: Some(e.size as u64),
                mtime: Some(e.mtime),
                depth: Some(e.depth),
            }
        } else {
            crate::query::StatInfo {
                path: abs.to_string_lossy().into_owned(),
                exists: false,
                kind: None,
                size: None,
                mtime: None,
                depth: None,
            }
        }
    }
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_sorts_folded() {
        let mut idx = Index::new();
        idx.insert_batch(
            &[
                NewEntry {
                    name: "Zebra",
                    parent: None,
                    kind: 1,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "apple",
                    parent: None,
                    kind: 1,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "Banana",
                    parent: None,
                    kind: 1,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
            ],
            0,
        );
        idx.finalize();

        let names: Vec<_> = idx.sorted.iter().map(|&i| idx.name_of(i)).collect();
        assert_eq!(names, vec!["apple", "Banana", "Zebra"]);
    }

    #[test]
    fn prefix_search_window() {
        let mut idx = Index::new();
        idx.insert_batch(
            &[
                NewEntry {
                    name: "App.tsx",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "apple",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "application",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "Banana",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
            ],
            0,
        );
        idx.finalize();

        idx.roots.push(Root {
            path: PathBuf::from("/tmp"),
            first: 0,
            count: 4,
        });

        let q = crate::query::Query {
            raw: "app".into(),
            terms: vec![crate::query::Term::Lit("app".into())],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: false,
        };
        let res = idx.search(&q, 0);
        let names: Vec<_> = res.hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["App.tsx", "apple", "application"]);
    }

    #[test]
    fn substring_scan_finds_case_insensitive() {
        let mut idx = Index::new();
        idx.insert_batch(
            &[
                NewEntry {
                    name: "test_file.rs",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "TEST_OTHER.txt",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "other",
                    parent: None,
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
            ],
            0,
        );
        idx.finalize();

        idx.roots.push(Root {
            path: PathBuf::from("/tmp"),
            first: 0,
            count: 3,
        });

        let q = crate::query::Query {
            raw: "test".into(),
            terms: vec![crate::query::Term::Lit("test".into())],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: false,
        };
        let res = idx.search(&q, 0);
        assert_eq!(res.hits.len(), 2);
    }

    #[test]
    fn path_of_assembles() {
        let mut idx = Index::new();
        let indices = idx.insert_batch(
            &[
                NewEntry {
                    name: "src",
                    parent: None,
                    kind: 1,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                NewEntry {
                    name: "main.rs",
                    parent: Some(0),
                    kind: 0,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
            ],
            0,
        );
        idx.roots.push(Root {
            path: PathBuf::from("/home/user/project"),
            first: 0,
            count: 2,
        });
        idx.finalize();

        let path = idx.path_of(indices[1]);
        assert!(path.to_string_lossy().contains("src"));
        assert!(path.to_string_lossy().contains("main.rs"));
    }
}
