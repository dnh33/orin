//! Canonical query matrix for benchmark runs.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryMatrix {
    /// Name-exact queries
    pub exact: Vec<String>,
    /// Prefix queries
    pub prefix: Vec<String>,
    /// Substring queries
    pub substring: Vec<String>,
    /// Extension queries
    pub ext: Vec<String>,
    /// Type queries
    pub type_q: Vec<String>,
    /// Size queries
    pub size: Vec<String>,
    /// Depth queries
    pub depth: Vec<String>,
    /// Combined queries
    pub combined: Vec<String>,
    /// Case sensitivity variants
    pub case: Vec<String>,
    /// Glob queries
    pub glob: Vec<String>,
    /// Regex queries
    pub regex: Vec<String>,
}

impl QueryMatrix {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let data =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).context("parse queries.json")
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(&self)?)
            .with_context(|| format!("write {}", path.display()))
    }

    pub fn len(&self) -> usize {
        self.exact.len()
            + self.prefix.len()
            + self.substring.len()
            + self.ext.len()
            + self.type_q.len()
            + self.size.len()
            + self.depth.len()
            + self.combined.len()
            + self.case.len()
            + self.glob.len()
            + self.regex.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Queries {
    pub corpus_size: usize,
    pub queries: Vec<String>,
}
