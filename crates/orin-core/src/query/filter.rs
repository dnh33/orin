//! Filter evaluation for search candidates.

use crate::query::Query;

/// Evaluate all filters on a candidate entry.
pub fn evaluate_filters(
    query: &Query,
    entry: &crate::entry::Entry,
    name: &str,
    folded_name: &[u8],
) -> bool {
    for term in &query.terms {
        if !evaluate_term(term, entry, name, folded_name) {
            return false;
        }
    }
    true
}

fn evaluate_term(
    term: &crate::query::Term,
    entry: &crate::entry::Entry,
    name: &str,
    _folded_name: &[u8],
) -> bool {
    use crate::query::Term;
    match term {
        Term::Lit(_) | Term::Phrase(_) | Term::Globs(_) | Term::Regex(_) => {
            // These are handled by the matcher, not the filter
            true
        }
        Term::Negate(inner) => !evaluate_term(inner, entry, name, folded_name),
        Term::Ext(exts, negate) => {
            let name_lower = name.to_lowercase();
            let has_ext = exts
                .iter()
                .any(|ext| name_lower.ends_with(&format!(".{}", ext.to_lowercase())));
            if *negate { !has_ext } else { has_ext }
        }
        Term::SizeMin(min) => min.is_none_or(|min| entry.size as u64 >= min),
        Term::SizeMax(max) => max.is_none_or(|max| entry.size as u64 <= max),
        Term::Kind(kind) => (entry.flags & 3) == *kind,
        Term::MaxDepth(d) => entry.depth <= *d,
        Term::MinDepth(d) => entry.depth >= *d,
        Term::MtimeAfter(_) => true,  // placeholder
        Term::MtimeBefore(_) => true, // placeholder
        Term::PathSeg(_seg) => {
            // Would need path reconstruction - placeholder
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_filter_positive() {
        let query = crate::query::Query {
            raw: "ext:rs".into(),
            terms: vec![crate::query::Term::Ext(vec!["rs".into()], false)],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: true,
        };
        let entry = crate::entry::Entry { flags: 0, ..Default::default() };
        assert!(evaluate_filters(&query, &entry, "main.rs", b"main.rs"));
        assert!(!evaluate_filters(&query, &entry, "main.txt", b"main.txt"));
    }

    #[test]
    fn ext_filter_negation() {
        let query = crate::query::Query {
            raw: "ext:!json".into(),
            terms: vec![crate::query::Term::Ext(vec!["json".into()], true)],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: true,
        };
        let entry = crate::entry::Entry { flags: 0, ..Default::default() };
        assert!(evaluate_filters(&query, &entry, "main.rs", b"main.rs"));
        assert!(!evaluate_filters(
            &query,
            &entry,
            "config.json",
            b"config.json"
        ));
    }

    #[test]
    fn kind_filter() {
        let query = crate::query::Query {
            raw: "type:d".into(),
            terms: vec![crate::query::Term::Kind(1)],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: true,
        };
        let file_entry = crate::entry::Entry { flags: 0, ..Default::default() };
        let dir_entry = crate::entry::Entry { flags: 1, ..Default::default() };
        assert!(!evaluate_filters(&query, &file_entry, "file", b"file"));
        assert!(evaluate_filters(&query, &dir_entry, "dir", b"dir"));
    }
}
