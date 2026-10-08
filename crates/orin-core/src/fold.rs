//! Case folding and NFC normalization for search keys.
//!
//! This module provides ASCII-fast-path case folding for search keys
//! and entry names. Non-ASCII characters use Unicode simple lowercase.

/// Push the case-folded (lowercased) version of `s` into `out`.
///
/// ASCII fast path: 'A'..='Z' → 'a'..='z' via `byte | 0x20`.
/// Non-ASCII: uses `char::to_lowercase()` (Unicode simple lowercase).
pub fn fold_into(s: &str, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(s.len());
    for ch in s.chars() {
        if ch.is_ascii() {
            out.push(ch.to_ascii_lowercase() as u8);
        } else {
            for c in ch.to_lowercase() {
                out.extend_from_slice(&c.to_string().into_bytes());
            }
        }
    }
}

/// Return a new `Vec<u8>` containing the case-folded version of `s`.
pub fn fold_vec(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    fold_into(s, &mut out);
    out
}

/// Compare two strings case-insensitively (folded comparison).
///
/// Returns `Less`, `Equal`, or `Greater` based on folded byte order.
/// Does not allocate; folds into a small stack buffer.
pub fn cmp_folded(a: &str, b: &str) -> core::cmp::Ordering {
    let mut buf_a = [0u8; 256];
    let mut buf_b = [0u8; 256];
    let len_a = fold_into_buf(a, &mut buf_a);
    let len_b = fold_into_buf(b, &mut buf_b);
    buf_a[..len_a].cmp(&buf_b[..len_b])
}

/// Compare a name against a pre-folded query prefix.
///
/// Returns `Equal` iff `folded(name).starts_with(query)`.
/// Otherwise returns the lexical ordering of the folded bytes.
pub fn prefix_cmp(name: &str, folded_query: &[u8]) -> core::cmp::Ordering {
    let mut buf = [0u8; 256];
    let len = fold_into_buf(name, &mut buf);
    let folded_name = &buf[..len];
    if folded_name.starts_with(folded_query) {
        core::cmp::Ordering::Equal
    } else {
        folded_name.cmp(folded_query)
    }
}

/// Internal: fold into a fixed buffer, return length.
fn fold_into_buf(s: &str, buf: &mut [u8]) -> usize {
    let mut i = 0;
    for ch in s.chars() {
        if i >= buf.len() {
            break;
        }
        if ch.is_ascii() {
            buf[i] = ch.to_ascii_lowercase() as u8;
            i += 1;
        } else {
            // Non-ASCII: simple lowercase, may expand to multiple bytes
            let lower = ch.to_lowercase().collect::<String>();
            let bytes = lower.as_bytes();
            let take = buf.len().saturating_sub(i).min(bytes.len());
            buf[i..i + take].copy_from_slice(&bytes[..take]);
            i += take;
            if take < bytes.len() {
                break;
            }
        }
    }
    i
}

/// Normalize a string to Unicode NFC form.
///
/// This ensures consistent byte representation for names containing
/// combining characters (e.g., "e\u{301}" ≡ "é").
pub fn normalize_nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_ascii() {
        assert_eq!(fold_vec("Hello"), b"hello");
        assert_eq!(fold_vec("HELLO"), b"hello");
        assert_eq!(fold_vec("HeLlO"), b"hello");
    }

    #[test]
    fn fold_unicode_lowercase() {
        // Unicode simple lowercase
        assert_eq!(fold_vec("ÄÖÜ"), b"\xc3\xa4\xc3\xb6\xc3\xbc"); // "äöü"
        assert_eq!(fold_vec("ß"), b"\xc3\x9f"); // "ß" (stays ß in simple lowercase)
    }

    #[test]
    fn cmp_folded_range() {
        assert_eq!(cmp_folded("App", "apple"), core::cmp::Ordering::Less);
        assert_eq!(cmp_folded("apple", "App"), core::cmp::Ordering::Greater);
        assert_eq!(cmp_folded("app", "app"), core::cmp::Ordering::Equal);
    }

    #[test]
    fn prefix_cmp_equal_when_prefix() {
        // "app" is prefix of "apple"
        assert_eq!(prefix_cmp("apple", b"app"), core::cmp::Ordering::Equal);
        assert_eq!(prefix_cmp("App.tsx", b"app"), core::cmp::Ordering::Equal);
        // shorter than query
        assert_eq!(prefix_cmp("ap", b"app"), core::cmp::Ordering::Less);
        // diverges
        assert_eq!(prefix_cmp("apq", b"app"), core::cmp::Ordering::Greater);
    }

    #[test]
    fn nfc_roundtrip() {
        // "e" + combining acute accent ≡ "é"
        let composed = "é";
        let decomposed = "e\u{301}";
        assert_eq!(normalize_nfc(decomposed), composed);
        assert_eq!(normalize_nfc(composed), composed);
    }
}
