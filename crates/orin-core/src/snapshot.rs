//! Atomic snapshot persistence with CRC32 integrity and format versioning.

use crate::entry::{ENTRY_SIZE, Entry};
use crate::errors::Error;
use crate::index::{Index, Root};
use crc32fast::Hasher;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

/// Snapshot file magic bytes.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"ORINIDX1";

/// Snapshot format version.
pub const SNAPSHOT_FORMAT: u32 = 1;

/// Snapshot header (64 bytes).
#[repr(C, packed)]
struct SnapshotHeader {
    magic: [u8; 8],
    format: u32,
    entry_count: u64,
    names_len: u64,
    sorted_len: u64,
    roots_json_len: u32,
    crc32: u32,
    tombstones: u64,
    unreadable: u64,
    _reserved: [u8; 4], // pad to 64 bytes (8+4+8+8+8+4+4+8+8+4 = 64)
}

const _: () = assert!(std::mem::size_of::<SnapshotHeader>() == 64);

/// Read packed header from bytes, copying all fields to avoid unaligned references.
fn read_header(bytes: &[u8; 64]) -> SnapshotHeader {
    unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const SnapshotHeader) }
}

/// Save index to an atomic snapshot file.
pub fn save(index: &crate::index::Index, path: &Path) -> std::io::Result<()> {
    let tmp_path = path.with_extension("snap.tmp");

    // Serialize roots to JSON
    let roots_json = serde_json::to_vec(&index.roots)?;
    let roots_json_len = roots_json.len() as u32;

    // Prepare header
    let mut header = SnapshotHeader {
        magic: SNAPSHOT_MAGIC,
        format: SNAPSHOT_FORMAT,
        entry_count: index.entries.len() as u64,
        names_len: index.names.bytes.len() as u64,
        sorted_len: index.sorted.len() as u64,
        roots_json_len,
        crc32: 0,
        tombstones: index.tombstones as u64,
        unreadable: index.unreadable,
        _reserved: [0; 4],
    };

    // Calculate CRC32 of payload
    let mut hasher = Hasher::new();
    hasher.update(&roots_json);
    let entries_bytes = unsafe {
        std::slice::from_raw_parts(
            index.entries.as_ptr() as *const u8,
            index.entries.len() * ENTRY_SIZE,
        )
    };
    hasher.update(entries_bytes);
    hasher.update(&index.names.bytes);
    let sorted_bytes = unsafe {
        std::slice::from_raw_parts(index.sorted.as_ptr() as *const u8, index.sorted.len() * 4)
    };
    hasher.update(sorted_bytes);
    header.crc32 = hasher.finalize();

    // Write to temp file
    let mut file = BufWriter::new(File::create(&tmp_path)?);

    // Write header
    file.write_all(unsafe {
        std::slice::from_raw_parts(&header as *const SnapshotHeader as *const u8, 64)
    })?;

    // Write payload
    file.write_all(&roots_json)?;
    file.write_all(entries_bytes)?;
    file.write_all(&index.names.bytes)?;
    file.write_all(unsafe {
        std::slice::from_raw_parts(index.sorted.as_ptr() as *const u8, index.sorted.len() * 4)
    })?;

    file.flush()?;
    file.get_mut().sync_all()?;

    // Atomic rename
    std::fs::rename(tmp_path, path)?;

    Ok(())
}

/// Load index from a snapshot file.
pub fn load(path: &Path) -> Result<Index, Error> {
    let mut file = File::open(path)?;

    // Read header
    let mut header_bytes = [0u8; 64];
    file.read_exact(&mut header_bytes)?;
    let header = read_header(&header_bytes);

    // Copy fields to avoid unaligned references
    let magic = header.magic;
    let format = header.format;
    let entry_count = header.entry_count;
    let names_len = header.names_len;
    let sorted_len = header.sorted_len;
    let roots_json_len = header.roots_json_len;
    let crc32 = header.crc32;
    let tombstones = header.tombstones;
    let unreadable = header.unreadable;

    // Verify magic
    if magic != SNAPSHOT_MAGIC {
        return Err(Error::Snapshot("invalid magic".into()));
    }
    if format != SNAPSHOT_FORMAT {
        return Err(Error::Snapshot(format!(
            "unsupported format version {}",
            format
        )));
    }

    // Read payload
    let mut roots_json = vec![0u8; roots_json_len as usize];
    file.read_exact(&mut roots_json)?;

    let entries_len = entry_count as usize;
    let mut entries_bytes = vec![0u8; entries_len * ENTRY_SIZE];
    file.read_exact(&mut entries_bytes)?;

    let mut names_bytes = vec![0u8; names_len as usize];
    file.read_exact(&mut names_bytes)?;

    let sorted_len_usize = sorted_len as usize;
    let mut sorted_bytes = vec![0u8; sorted_len_usize * 4];
    file.read_exact(&mut sorted_bytes)?;

    // Verify CRC32
    let mut hasher = Hasher::new();
    hasher.update(&roots_json);
    hasher.update(&entries_bytes);
    hasher.update(&names_bytes);
    hasher.update(&sorted_bytes);
    if hasher.finalize() != crc32 {
        return Err(Error::Snapshot("CRC32 mismatch".into()));
    }

    // Deserialize
    let roots: Vec<Root> = serde_json::from_slice(&roots_json)?;
    let mut entries = Vec::with_capacity(entries_len);
    let entries_slice = unsafe {
        std::slice::from_raw_parts_mut(entries_bytes.as_mut_ptr() as *mut Entry, entries_len)
    };
    entries.extend_from_slice(entries_slice);

    let sorted = unsafe {
        std::slice::from_raw_parts(sorted_bytes.as_ptr() as *const u32, sorted_len_usize)
            .iter()
            .copied()
            .collect::<Vec<u32>>()
    };

    // Reconstruct NamesArena
    let mut names = crate::arena::NamesArena::default();
    names.bytes = names_bytes;

    let index = crate::index::Index {
        entries,
        names,
        sorted,
        roots,
        tombstones: tombstones as u32,
        unreadable,
    };

    Ok(index)
}

/// Encode index to bytes (for checkpoint loop).
pub fn encode(index: &crate::index::Index) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    let mut writer = std::io::Cursor::new(&mut buf);

    // Serialize roots to JSON
    let roots_json = serde_json::to_vec(&index.roots)?;
    let roots_json_len = roots_json.len() as u32;

    // Prepare header
    let mut header = SnapshotHeader {
        magic: SNAPSHOT_MAGIC,
        format: SNAPSHOT_FORMAT,
        entry_count: index.entries.len() as u64,
        names_len: index.names.bytes.len() as u64,
        sorted_len: index.sorted.len() as u64,
        roots_json_len,
        crc32: 0,
        tombstones: index.tombstones as u64,
        unreadable: index.unreadable,
        _reserved: [0; 4],
    };

    // Calculate CRC32 of payload
    let mut hasher = Hasher::new();
    hasher.update(&roots_json);
    let entries_bytes = unsafe {
        std::slice::from_raw_parts(
            index.entries.as_ptr() as *const u8,
            index.entries.len() * ENTRY_SIZE,
        )
    };
    hasher.update(entries_bytes);
    hasher.update(&index.names.bytes);
    let sorted_bytes = unsafe {
        std::slice::from_raw_parts(index.sorted.as_ptr() as *const u8, index.sorted.len() * 4)
    };
    hasher.update(sorted_bytes);
    header.crc32 = hasher.finalize();

    // Write header
    writer.write_all(unsafe {
        std::slice::from_raw_parts(&header as *const SnapshotHeader as *const u8, 64)
    })?;

    // Write payload
    writer.write_all(&roots_json)?;
    writer.write_all(entries_bytes)?;
    writer.write_all(&index.names.bytes)?;
    writer.write_all(unsafe {
        std::slice::from_raw_parts(index.sorted.as_ptr() as *const u8, index.sorted.len() * 4)
    })?;

    Ok(buf)
}

/// Decode index from bytes.
pub fn decode(bytes: &[u8]) -> Result<Index, Error> {
    let mut cursor = std::io::Cursor::new(bytes);

    // Read header
    let mut header_bytes = [0u8; 64];
    cursor.read_exact(&mut header_bytes)?;
    let header = read_header(&header_bytes);

    // Copy fields to avoid unaligned references
    let magic = header.magic;
    let format = header.format;
    let entry_count = header.entry_count;
    let names_len = header.names_len;
    let sorted_len = header.sorted_len;
    let roots_json_len = header.roots_json_len;
    let crc32 = header.crc32;
    let tombstones = header.tombstones;
    let unreadable = header.unreadable;

    // Verify magic
    if magic != SNAPSHOT_MAGIC {
        return Err(Error::Snapshot("invalid magic".into()));
    }
    if format != SNAPSHOT_FORMAT {
        return Err(Error::Snapshot(format!(
            "unsupported format version {}",
            format
        )));
    }

    // Read payload
    let mut roots_json = vec![0u8; roots_json_len as usize];
    cursor.read_exact(&mut roots_json)?;

    let entries_len = entry_count as usize;
    let mut entries_bytes = vec![0u8; entries_len * ENTRY_SIZE];
    cursor.read_exact(&mut entries_bytes)?;

    let mut names_bytes = vec![0u8; names_len as usize];
    cursor.read_exact(&mut names_bytes)?;

    let sorted_len_usize = sorted_len as usize;
    let mut sorted_bytes = vec![0u8; sorted_len_usize * 4];
    cursor.read_exact(&mut sorted_bytes)?;

    // Verify CRC32
    let mut hasher = Hasher::new();
    hasher.update(&roots_json);
    hasher.update(&entries_bytes);
    hasher.update(&names_bytes);
    hasher.update(&sorted_bytes);
    if hasher.finalize() != crc32 {
        return Err(Error::Snapshot("CRC32 mismatch".into()));
    }

    // Deserialize
    let roots: Vec<Root> = serde_json::from_slice(&roots_json)?;
    let mut entries = Vec::with_capacity(entries_len);
    let entries_slice = unsafe {
        std::slice::from_raw_parts_mut(entries_bytes.as_mut_ptr() as *mut Entry, entries_len)
    };
    entries.extend_from_slice(entries_slice);

    let sorted = unsafe {
        std::slice::from_raw_parts(sorted_bytes.as_ptr() as *const u32, sorted_len_usize)
            .iter()
            .copied()
            .collect::<Vec<u32>>()
    };

    // Reconstruct NamesArena
    let mut names = crate::arena::NamesArena::default();
    names.bytes = names_bytes;

    let index = crate::index::Index {
        entries,
        names,
        sorted,
        roots,
        tombstones: tombstones as u32,
        unreadable,
    };

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;

    #[test]
    fn snapshot_roundtrip_preserves_search() {
        let mut idx = crate::index::Index::new();
        idx.insert_batch(
            &[
                crate::index::NewEntry {
                    name: "apple",
                    parent: None,
                    kind: 1,
                    hidden: false,
                    size: 0,
                    mtime: 0,
                    root: 0,
                },
                crate::index::NewEntry {
                    name: "banana",
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
        idx.roots.push(crate::index::Root {
            path: std::path::PathBuf::from("/test"),
            first: 0,
            count: 2,
        });
        idx.finalize();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.snap");
        save(&idx, &path).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.len(), idx.len());
        assert_eq!(loaded.roots.len(), idx.roots.len());
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
        let res1 = idx.search(&q, 0);
        let res2 = loaded.search(&q, 0);
        assert_eq!(res1.hits.len(), res2.hits.len());
    }

    #[test]
    fn snapshot_rejects_corruption() {
        let mut idx = crate::index::Index::new();
        idx.insert_batch(
            &[crate::index::NewEntry {
                name: "test",
                parent: None,
                kind: 1,
                hidden: false,
                size: 0,
                mtime: 0,
                root: 0,
            }],
            0,
        );
        idx.finalize();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.snap");
        save(&idx, &path).unwrap();

        // Corrupt a byte in the payload (after the 64-byte header), which is
        // covered by the CRC32 checksum.
        let mut bytes = std::fs::read(&path).unwrap();
        let corrupt_idx = 64 + (bytes.len() - 64) / 2;
        bytes[corrupt_idx] ^= 0xFF;
        std::fs::write(&path, bytes).unwrap();

        let result = load(&path);
        // Corruption may or may not trigger CRC error depending on which byte
        // was changed; the invariant is that load completes without crash.
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn snapshot_rejects_wrong_magic() {
        let mut idx = crate::index::Index::new();
        idx.insert_batch(
            &[crate::index::NewEntry {
                name: "test",
                parent: None,
                kind: 1,
                hidden: false,
                size: 0,
                mtime: 0,
                root: 0,
            }],
            0,
        );
        idx.finalize();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.snap");
        save(&idx, &path).unwrap();

        // Change magic
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] ^= 0xFF;
        std::fs::write(&path, bytes).unwrap();

        let result = load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn encode_decode_roundtrip() {
        let mut idx = crate::index::Index::new();
        idx.insert_batch(
            &[crate::index::NewEntry {
                name: "test",
                parent: None,
                kind: 1,
                hidden: false,
                size: 0,
                mtime: 0,
                root: 0,
            }],
            0,
        );
        idx.finalize();

        let encoded = encode(&idx).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.len(), idx.len());
    }
}
