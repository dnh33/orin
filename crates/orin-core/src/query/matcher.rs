//! Name matching modes: literal, glob, regex, fuzzy.

use crate::query::MatchMode;

/// Check if a name matches the query terms according to the match mode.
pub fn matches_name(name: &str, folded_name: &[u8], query: &crate::query::Query) -> bool {
    match query.mode {
        MatchMode::Literal => matches_literal(name, folded_name, query),
        MatchMode::Glob => matches_glob(name, query),
        MatchMode::Regex => matches_regex(name, query),
        MatchMode::Fuzzy => matches_fuzzy(name, query),
    }
}

fn matches_literal(name: &str, folded_name: &[u8], query: &crate::query::Query) -> bool {
    for term in &query.terms {
        match term {
            crate::query::Term::Lit(s) => {
                let folded = crate::fold::fold_vec(s);
                if !folded_name
                    .windows(folded.len())
                    .any(|w| w == folded.as_slice())
                {
                    return false;
                }
            }
            crate::query::Term::Phrase(s) => {
                let folded = crate::fold::fold_vec(s);
                if !folded_name
                    .windows(folded.len())
                    .any(|w| w == folded.as_slice())
                {
                    return false;
                }
            }
            crate::query::Term::Negate(inner) => {
                if matches_literal(
                    name,
                    folded_name,
                    &crate::query::Query {
                        raw: String::new(),
                        terms: vec![*inner.clone()],
                        mode: crate::query::MatchMode::Literal,
                        sort: crate::query::SortKey::Score,
                        limit: 10,
                        offset: 0,
                        root: None,
                        path_scope: None,
                        escalate: false,
                    },
                ) {
                    return false;
                }
            }
            _ => {} // other terms handled by filter
        }
    }
    true
}

fn matches_glob(_name: &str, _query: &crate::query::Query) -> bool {
    // TODO: implement glob matching
    true
}

fn matches_regex(_name: &str, _query: &crate::query::Query) -> bool {
    // TODO: implement regex matching
    true
}

fn matches_fuzzy(_name: &str, _query: &crate::query::Query) -> bool {
    // TODO: implement fuzzy matching
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_match_ascii() {
        let query = crate::query::Query {
            raw: "app".into(),
            terms: vec![crate::query::Term::Lit("app".into())],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: true,
        };
        assert!(matches_name("apple", b"apple", &query));
        assert!(matches_name("App.tsx", b"app.tsx", &query));
        assert!(!matches_name("banana", b"banana", &query));
    }

    #[test]
    fn literal_match_case_insensitive() {
        let query = crate::query::Query {
            raw: "app".into(),
            terms: vec![crate::query::Term::Lit("app".into())],
            mode: crate::query::MatchMode::Literal,
            sort: crate::query::SortKey::Score,
            limit: 10,
            offset: 0,
            root: None,
            path_scope: None,
            escalate: true,
        };
        assert!(matches_name("APPLE", b"apple", &query));
        assert!(matches_name("App", b"app", &query));
    }
}
