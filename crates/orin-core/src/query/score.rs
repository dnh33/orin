//! Deterministic scoring schedule for result ranking.

/// Score an entry against a folded query.
pub fn score_entry(entry: &crate::entry::Entry, name: &str, folded_query: &[u8], _kind: u8) -> f32 {
    let folded_name = crate::fold::fold_vec(name);
    let folded_name_bytes = folded_name.as_slice();

    // Tier scoring per frozen schedule
    if folded_name_bytes == folded_query {
        return 1000.0;
    }
    if folded_name_bytes.starts_with(folded_query) {
        return 900.0;
    }
    // Word-boundary substring (simplified)
    if folded_name_bytes
        .windows(folded_query.len())
        .any(|w| w == folded_query)
    {
        // Check if preceded by boundary char
        if let Some(pos) = folded_name_bytes
            .windows(folded_query.len())
            .position(|w| w == folded_query)
        {
            if pos == 0 || is_boundary_char(folded_name_bytes[pos - 1]) {
                return 700.0;
            }
        }
        return 500.0;
    }
    0.0
}

fn is_boundary_char(c: u8) -> bool {
    matches!(c, b'-' | b'_' | b'.' | b'/' | b'\\' | b' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_scores_highest() {
        let entry = crate::entry::Entry::default();
        let score = score_entry(&entry, "apple", b"apple", 0);
        assert_eq!(score, 1000.0);
    }

    #[test]
    fn prefix_match_scores_high() {
        let entry = crate::entry::Entry::default();
        let score = score_entry(&entry, "apple", b"app", 0);
        assert_eq!(score, 900.0);
    }

    #[test]
    fn word_boundary_scores_higher() {
        let entry = crate::entry::Entry::default();
        let score = score_entry(&entry, "test_file", b"file", 0);
        // "file" is preceded by '_' which is a boundary
        assert_eq!(score, 700.0);
    }
}
