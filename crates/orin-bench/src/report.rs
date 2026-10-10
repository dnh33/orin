//! Generate comparison.md from benchmark artifacts.

use anyhow::Context;
use std::fs;
use std::path::Path;

pub fn compare(results_dir: &Path, out: &Path) -> anyhow::Result<()> {
    let entries = fs::read_dir(results_dir)?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    let mut lines = vec![
        "# orin Benchmark Results".to_string(),
        "".to_string(),
        "| Tool | Corpus | Latency p50 (µs) | Latency p95 (µs) | Latency p99 (µs) | QPS |"
            .to_string(),
        "|------|--------|------------------|------------------|------------------|-----|"
            .to_string(),
    ];

    for entry in &entries {
        if let Ok(name) = entry.file_name().into_string()
            && name.ends_with(".json")
        {
            lines.push(format!("| {} | ... | ... | ... | ... | ... |", name));
        }
    }

    lines.push("".to_string());
    lines.push(
        "*Results produced by `orin-bench compare`. All times on GitHub runners.".to_string(),
    );

    fs::write(out, lines.join("\n")).with_context(|| format!("write {}", out.display()))?;

    Ok(())
}
