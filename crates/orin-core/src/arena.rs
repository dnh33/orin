//! Names arena: contiguous byte storage for file names.
//!
//! All entry names are stored in a single `Vec<u8>` to avoid per-entry
//! allocations and reduce memory overhead. Names are stored as-is from
//! `OsStr` via `to_string_lossy()` (NFC-normalized elsewhere).

/// Contiguous arena for file name bytes.
#[derive(Debug, Default, Clone)]
pub struct NamesArena {
    pub bytes: Vec<u8>,
}

impl NamesArena {
    /// Push a name into the arena, returning `(offset, stored_length)`.
    ///
    /// If the name exceeds `u16::MAX` bytes, it is truncated and the
    /// returned length is `u16::MAX` (the caller must set `FLAG_NAME_TRUNCATED`).
    pub fn push(&mut self, s: &str) -> (u32, u16) {
        let off = self.bytes.len() as u32;
        let b = s.as_bytes();
        let len = b.len().min(u16::MAX as usize) as u16;
        self.bytes.extend_from_slice(&b[..len as usize]);
        (off, len)
    }

    /// Get a name from the arena by offset and length.
    pub fn get(&self, off: u32, len: u16) -> &str {
        let start = off as usize;
        let end = start + len as usize;
        core::str::from_utf8(&self.bytes[start..end]).unwrap_or("<invalid utf8>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_push_get_roundtrip() {
        let mut a = NamesArena::default();
        let (off, len) = a.push("hello");
        assert_eq!(len, 5);
        assert_eq!(a.get(off, len), "hello");
    }

    #[test]
    fn arena_multiple_names() {
        let mut a = NamesArena::default();
        let (off1, len1) = a.push("foo");
        let (off2, len2) = a.push("barbaz");
        assert_eq!(a.get(off1, len1), "foo");
        assert_eq!(a.get(off2, len2), "barbaz");
        // Offsets are sequential
        assert_eq!(off2, off1 + len1 as u32);
    }

    #[test]
    fn arena_truncates_long_names() {
        let mut a = NamesArena::default();
        let long = "x".repeat(70_000); // exceeds u16::MAX (65535)
        let (off, len) = a.push(&long);
        assert_eq!(len, u16::MAX);
        assert_eq!(a.get(off, len).len(), u16::MAX as usize);
    }
}
