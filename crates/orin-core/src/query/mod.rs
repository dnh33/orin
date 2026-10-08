//! Query parsing, filtering, matching, and scoring.

pub mod filter;
pub mod matcher;
pub mod parse;
pub mod score;

use serde::{Deserialize, Serialize};
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    crate::query::parse::parse_query(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_reexports() {
        let q = parse_query("hello").unwrap();
        assert_eq!(q.terms.len(), 1);
    }
}
