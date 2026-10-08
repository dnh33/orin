//! Entry record: fixed 20-byte packed layout for the name index.
//!
//! This module defines the core `Entry` struct and related constants.
//! The layout is frozen (see spec §5.1) and must remain exactly 20 bytes.

#![allow(dead_code)] // placeholder fields until full impl lands

use bytemuck::{Pod, Zeroable};

/// Size of a single Entry in bytes (must equal 20).
pub const ENTRY_SIZE: usize = 20;

/// Bitmask for the type field in `flags` (bits 0-1).
pub const FLAG_TYPE_MASK: u8 = 0b0000_0011;
/// File type.
pub const TYPE_FILE: u8 = 0;
/// Directory type.
pub const TYPE_DIR: u8 = 1;
/// Symlink type.
pub const TYPE_SYMLINK: u8 = 2;
/// Other/unknown type.
pub const TYPE_OTHER: u8 = 3;

/// File is hidden (dot-prefix on Unix, hidden attribute on Windows).
pub const FLAG_HIDDEN: u8 = 0b0000_0100;
/// Name was truncated (longer than u16::MAX bytes).
pub const FLAG_NAME_TRUNCATED: u8 = 0b0000_1000;
/// This entry is a root directory marker.
pub const FLAG_ROOT: u8 = 0b0001_0000;

/// Fixed-size, packed entry record (20 bytes).
///
/// Fields:
/// - `name_off`: byte offset into the names arena
/// - `name_len`: byte length of the name (u16::MAX ⇒ truncated)
/// - `flags`: type (2 bits) + hidden + truncated + root
/// - `depth`: tree depth (0 = root, saturates at 255)
/// - `parent`: parent entry index (u32::MAX for roots)
/// - `size`: file size in bytes (saturated at u32::MAX; >4 GiB noted in overflow table v0.2)
/// - `mtime`: modification time as Unix seconds (0 if unavailable)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Entry {
    pub name_off: u32,
    pub name_len: u16,
    pub flags: u8,
    pub depth: u8,
    pub parent: u32,
    pub size: u32,
    pub mtime: u32,
}

// Compile-time assertion that Entry is exactly 20 bytes.
const _: () = assert!(core::mem::size_of::<Entry>() == ENTRY_SIZE);

impl Default for Entry {
    fn default() -> Self {
        Self {
            name_off: 0,
            name_len: 0,
            flags: 0,
            depth: 0,
            parent: u32::MAX, // roots have no parent
            size: 0,
            mtime: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_size_is_20() {
        assert_eq!(core::mem::size_of::<Entry>(), ENTRY_SIZE);
        assert_eq!(ENTRY_SIZE, 20);
    }

    #[test]
    fn entry_is_pod_and_zeroable() {
        // bytemuck derive ensures this; this test just confirms the bounds.
        let e = Entry::default();
        assert_eq!(e.name_off, 0);
        assert_eq!(e.name_len, 0);
        assert_eq!(e.flags, 0);
        assert_eq!(e.depth, 0);
        assert_eq!(e.parent, u32::MAX);
        assert_eq!(e.size, 0);
        assert_eq!(e.mtime, 0);
    }

    #[test]
    fn flag_constants_match_spec() {
        assert_eq!(FLAG_TYPE_MASK, 0b0000_0011);
        assert_eq!(TYPE_FILE, 0);
        assert_eq!(TYPE_DIR, 1);
        assert_eq!(TYPE_SYMLINK, 2);
        assert_eq!(TYPE_OTHER, 3);
        assert_eq!(FLAG_HIDDEN, 0b0000_0100);
        assert_eq!(FLAG_NAME_TRUNCATED, 0b0000_1000);
        assert_eq!(FLAG_ROOT, 0b0001_0000);
    }
}
