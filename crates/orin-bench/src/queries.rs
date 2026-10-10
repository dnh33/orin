//! Canonical query matrix for benchmark runs.
//!
//! Two arm kinds (benchmark-harness rule: tool-specific syntax is a SEPARATE
//! arm, never mixed into the shared table):
//! - **shared** plain-name classes run against every tool: exact, prefix,
//!   substring, case, multiword, datever, camera, ext_scope, deep_path,
//!   case_mixed, realistic_miss.
//! - **orin-only** classes use orin's filter/glob/regex syntax: ext,
//!   ext_filter, type_q, size, depth, combined, glob, regex.
//!
//! Needles are stored BARE (no quote-wrapping): the runner passes each query
//! as one argv entry (no shell involved), and orin parses a quote-wrapped
//! phrase into a Phrase term that its search path does not substring-match.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    /// Regex queries (orin syntax)
    pub regex: Vec<String>,
    /// Realistic multi-word queries with spaces, e.g. `budget 2026` (shared)
    #[serde(default)]
    pub multiword: Vec<String>,
    /// Realistic date/version suffix names, e.g. `invoice-2026` (shared)
    #[serde(default)]
    pub datever: Vec<String>,
    /// Realistic camera/media names, e.g. `IMG_2026` (shared)
    #[serde(default)]
    pub camera: Vec<String>,
    /// Extension-scoped cross-tool arm: a basename with a known extension
    #[serde(default)]
    pub ext_scope: Vec<String>,
    /// Extension-scoped orin-only arm: `ext:` filter syntax (separate arm)
    #[serde(default)]
    pub ext_filter: Vec<String>,
    /// Deep-path needles planted in directory names, not filenames (shared)
    #[serde(default)]
    pub deep_path: Vec<String>,
    /// Case-mixed needles probing fold behavior, e.g. `Main.rs` (shared)
    #[serde(default)]
    pub case_mixed: Vec<String>,
    /// Rare-but-plausible name for sparsity realism (shared)
    #[serde(default)]
    pub realistic_miss: Vec<String>,
}

impl QueryMatrix {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let matrix: QueryMatrix = serde_json::from_str(&data).context("parse queries.json")?;
        matrix.validate()?;
        Ok(matrix)
    }

    /// Refuse an empty needle: the bench preflight refuses empty searches,
    /// and an empty string would silently match everything instead.
    fn validate(&self) -> anyhow::Result<()> {
        let shared = self.shared_classes();
        let orin_only = self.orin_only_classes();
        for (label, needles) in shared.into_iter().chain(orin_only) {
            for (i, needle) in needles.iter().enumerate() {
                if needle.trim().is_empty() {
                    anyhow::bail!("queries.json: {label}[{i}] is empty - needle needs plants");
                }
            }
        }
        Ok(())
    }

    /// Plain-name classes shared by every tool arm, in run order:
    /// (class label, needles).
    pub fn shared_classes(&self) -> Vec<(&'static str, &[String])> {
        vec![
            ("exact", &self.exact),
            ("prefix", &self.prefix),
            ("substring", &self.substring),
            ("case", &self.case),
            ("multiword", &self.multiword),
            ("datever", &self.datever),
            ("camera", &self.camera),
            ("ext_scope", &self.ext_scope),
            ("deep_path", &self.deep_path),
            ("case_mixed", &self.case_mixed),
            ("realistic_miss", &self.realistic_miss),
        ]
    }

    /// orin-only classes: tool-specific filter/glob/regex syntax runs as its
    /// own arm(s), never in the shared cross-tool table.
    pub fn orin_only_classes(&self) -> Vec<(&'static str, &[String])> {
        vec![
            ("ext", &self.ext),
            ("ext_filter", &self.ext_filter),
            ("type_q", &self.type_q),
            ("size", &self.size),
            ("depth", &self.depth),
            ("combined", &self.combined),
            ("glob", &self.glob),
            ("regex", &self.regex),
        ]
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(&self)?)
            .with_context(|| format!("write {}", path.display()))
    }

    #[allow(dead_code)]
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
            + self.multiword.len()
            + self.datever.len()
            + self.camera.len()
            + self.ext_scope.len()
            + self.ext_filter.len()
            + self.deep_path.len()
            + self.case_mixed.len()
            + self.realistic_miss.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Queries {
    pub corpus_size: usize,
    pub queries: Vec<String>,
}
