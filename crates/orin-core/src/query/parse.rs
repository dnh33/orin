//! Query parsing, filtering, matching, and scoring.

pub mod filter;
pub mod matcher;
pub mod parse;
pub mod score;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A parsed search query.
#[derive(Clone, Debug)]
pub struct Query {
    pub raw: String,
    pub terms: Vec<Term>,
    pub mode: MatchMode,
    pub sort: SortKey,
    pub limit: usize,
    pub offset: usize,
    pub root: Option<String>,
    pub path_scope: Option<String>,
    pub escalate: bool,
}

/// Matching mode for name search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    Literal,
    Glob,
    Regex,
    Fuzzy,
}

/// Sort key for results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Score,
    Name,
    Size,
    Mtime,
    Depth,
}

/// A single query term.
#[derive(Clone, Debug)]
pub enum Term {
    Lit(String),
    Phrase(String),
    Globs(Vec<String>),
    Regex(String),
    Negate(Box<Term>),
    Ext(Vec<String>, bool),
    SizeMin(Option<u64>),
    SizeMax(Option<u64>),
    Kind(u8),
    MaxDepth(u8),
    MinDepth(u8),
    MtimeAfter(u64),
    MtimeBefore(u64),
    PathSeg(String),
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
    pub path: std::path::PathBuf,
    pub name: String,
    pub kind: u8,
    pub size: u64,
    pub mtime: u32,
}

/// File metadata for stat operations.
#[derive(Clone, Debug, Serialize)]
pub struct StatInfo {
    pub path: String,
    pub exists: bool,
    pub kind: Option<u8>,
    pub size: Option<u64>,
    pub mtime: Option<u32>,
    pub depth: Option<u8>,
}

/// Parse error.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("unclosed quote")]
    UnclosedQuote,
    #[error("empty term")]
    EmptyTerm,
}

/// Parse a query string into a `Query`.
pub fn parse_query(input: &str) -> Result<Query, QueryError> {
    let raw = input.to_string();
    let terms = parse_terms(input)?;
    Ok(Query {
        raw,
        terms,
        mode: MatchMode::Literal,
        sort: SortKey::Score,
        limit: 10000,
        offset: 0,
        root: None,
        path_scope: None,
        escalate: true,
    })
}

fn parse_terms(input: &str) -> Result<Vec<Term>, QueryError> {
    let mut terms = Vec::new();
    let mut chars = input.chars().peekable();
    let mut current = String::new();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if !current.is_empty() {
                    terms.push(parse_atom(&current));
                    current.clear();
                }
                // Parse quoted phrase
                let mut phrase = String::new();
                for ch in chars.by_ref() {
                    if ch == '"' {
                        break;
                    }
                    phrase.push(ch);
                }
                if phrase.is_empty() {
                    return Err(QueryError::EmptyTerm);
                }
                terms.push(Term::Phrase(phrase));
            }
            ' ' | '\t' | '\n' => {
                if !current.is_empty() {
                    terms.push(parse_atom(&current));
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        terms.push(parse_atom(&current));
    }

    Ok(terms)
}

fn parse_atom(s: &str) -> Term {
    // Check for known prefixes
    if let Some(rest) = s.strip_prefix("ext:") {
        let neg = rest.starts_with('!');
        let exts = rest.trim_start_matches('!').split(',').map(|s| s.to_string()).collect();
        return Term::Ext(exts, neg);
    }
    if let Some(rest) = s.strip_prefix("size:") {
        if let Some(rest) = rest.strip_prefix('>') {
            return Term::SizeMin(parse_size(rest));
        }
        if let Some(rest) = rest.strip_prefix('<') {
            return Term::SizeMax(parse_size(rest));
        }
        if let Some((min, max)) = rest.split_once("..") {
            return Term::SizeMin(parse_size(min)).combine_with_max(parse_size(max));
        }
    }
    if let Some(rest) = s.strip_prefix("type:") {
        let kind = match rest {
            "f" => 0,
            "d" => 1,
            "l" => 2,
            _ => 3,
        };
        return Term::Kind(kind);
    }
    if let Some(rest) = s.strip_prefix("depth:") {
        if let Some(rest) = rest.strip_prefix('<') {
            return Term::MaxDepth(rest.parse().unwrap_or(0));
        }
        if let Some(rest) = rest.strip_prefix('>') {
            return Term::MinDepth(rest.parse().unwrap_or(0));
        }
    }
    if let Some(rest) = s.strip_prefix("mtime:") {
        if let Some(rest) = rest.strip_prefix('<') {
            return Term::MtimeBefore(parse_time(rest));
        }
        if let Some(rest) = rest.strip_prefix('>') {
            return Term::MtimeAfter(parse_time(rest));
        }
    }
    if let Some(rest) = s.strip_prefix("path:") {
        return Term::PathSeg(rest.to_string());
    }
    if let Some(rest) = s.strip_prefix("re:") {
        return Term::Regex(rest.to_string());
    }
    if s.contains('*') || s.contains('?') {
        return Term::Globs(vec![s.to_string()]);
    }
    if s.starts_with('!') {
        return Term::Negate(Box::new(parse_atom(&s[1..])));
    }
    Term::Lit(s.to_string())
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    let (num, unit) = if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len()-1], 1024)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len()-1], 1024*1024)
    } else if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len()-1], 1024*1024*1024)
    } else if s.ends_with('T') || s.ends_with('t') {
        (&s[..s.len()-1], 1024*1024*1024*1024)
    } else {
        (s, 1)
    };
    num.parse().ok().map(|n: u64| n * unit)
}

fn parse_time(s: &str) -> u64 {
    // Simplified: treat as days if ends with 'd', else as unix timestamp
    if s.ends_with('d') {
        let days: u64 = s[..s.len()-1].parse().unwrap_or(0);
        0 // placeholder
    } else {
        s.parse().unwrap_or(0)
    }
}

trait CombineWithMax {
    fn combine_with_max(self, max: Option<u64>) -> Term;
}

impl CombineWithMax for Term {
    fn combine_with_max(self, max: Option<u64>) -> Term {
        match self {
            Term::SizeMin(min) => Term::SizeMin(min), // simplified
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literal() {
        let q = parse_query("hello").unwrap();
        assert_eq!(q.terms.len(), 1);
        assert!(matches!(q.terms[0], Term::Lit(ref s) if s == "hello"));
    }

    #[test]
    fn parse_ext_filter() {
        let q = parse_query("ext:rs,toml").unwrap();
        assert!(matches!(q.terms[0], Term::Ext(ref exts, false) if exts == &["rs", "toml"]));
    }

    #[test]
    fn parse_ext_negation() {
        let q = parse_query("ext:!json").unwrap();
        assert!(matches!(q.terms[0], Term::Ext(ref exts, true) if exts == &["json"]));
    }

    #[test]
    fn parse_size() {
        let q = parse_query("size:>10M").unwrap();
        assert!(matches!(q.terms[0], Term::SizeMin(Some(n)) if n == 10_485_760));
    }

    #[test]
    fn parse_type() {
        let q = parse_query("type:f").unwrap();
        assert!(matches!(q.terms[0], Term::Kind(0)));
    }

    #[test]
    fn parse_smart_case() {
        let q = parse_query("App").unwrap();
        assert!(matches!(q.terms[0], Term::Lit(ref s) if s == "App"));
        let q = parse_query("app").unwrap();
        assert!(matches!(q.terms[0], Term::Lit(ref s) if s == "app"));
    }
}
