//! Tool runner + timing.

use anyhow::Context;
use serde_json;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub struct RunResult {
    pub tool: String,
    pub corpus_dir: String,
    pub query: String,
    pub latency_us: u64,
    pub exit_code: i32,
    pub output_bytes: usize,
}

pub fn run_tool(
    tool: &str,
    corpus_dir: &PathBuf,
    query: &str,
    iterations: usize,
) -> anyhow::Result<Vec<Duration>> {
    let mut latencies = Vec::with_capacity(iterations);

    // Warmup
    _ = run_single(tool, corpus_dir, query)?;

    for _ in 0..iterations {
        let start = Instant::now();
        let result = run_single(tool, corpus_dir, query)?;
        let dur = start.elapsed();
        latencies.push(dur);
        if result.exit_code != 0 {
            eprintln!("Warning: {} exited {} for query '{}'", tool, result.exit_code, query);
        }
    }

    Ok(latencies)
}

fn run_single(tool: &str, corpus_dir: &PathBuf, query: &str) -> anyhow::Result<std::process::Output> {
    let tool_cmd = match tool {
        "orin" => {
            // orin query mode
            let mut cmd = Command::new("cargo");
            cmd.arg("run");
            cmd.arg("--release");
            cmd.arg("-p");
            cmd.arg("orin-core");
            cmd.arg("--");
            cmd.arg("query");
            cmd.arg(query);
            cmd.arg("--data-dir");
            cmd.arg(""); // placeholder
            cmd
        }
        "fd" => Command::new("fd-find"),
        "find" => Command::new("find"),
        "rg" | "ripgrep" => Command::new("rg"),
        _ => anyhow::bail!("unknown tool: {}", tool),
    };

    let output = tool_cmd
        .current_dir(corpus_dir)
        .output()
        .with_context(|| format!("run {} on {}", tool, corpus_dir.display()))?;

    Ok(output)
}

pub fn run(
    tool: &str,
    corpus_dir: &PathBuf,
    queries_path: &PathBuf,
    iterations: usize,
    out_path: &PathBuf,
) -> anyhow::Result<()> {
    use crate::queries;
    let qmatrix = queries::QueryMatrix::load(queries_path)?;
    let total_queries = qmatrix.len();
    let mut results = Vec::with_capacity(total_queries);

    // Just first 10 queries for quick benchmark run
    let queries_to_run: Vec<String> = qmatrix.exact.iter()
        .cloned().chain(qmatrix.prefix.iter().cloned())
        .chain(qmatrix.substring.iter().cloned()).collect();

    for q in queries_to_run.iter().take(10) {
        let latencies = run_tool(tool, corpus_dir, q, iterations)?;
        let stat = crate::stats::from_durations(&latencies);
        results.push(format!(
            "{{\"query\":{}\",\"tool\":\"{}\",\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"qps\":{:.2}}}",
            serde_json::to_string(&q)?,
            tool,
            stat.latency_p50_us,
            stat.latency_p95_us,
            stat.latency_p99_us,
            stat.qps
        ));
    }

    std::fs::write(
        out_path,
        format!("[{}]", results.join(",")),
    )?;
    println!("bench: {} queries x {} iterations = {} results -> {}", results.len(), iterations, results.len(), out_path.display());
    Ok(())
}

pub fn summarize(latencies: &[Duration]) -> (f64, f64, f64) {
    if latencies.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let sorted: Vec<f64> = latencies.iter().map(|d| d.as_micros() as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = sorted[sorted.len() * 50 / 100];
    let p95 = sorted[sorted.len() * 95 / 100];
    let p99 = sorted[sorted.len() * 99 / 100];

    let total: f64 = sorted.iter().sum();
    let qps = (latencies.len() as f64 / total) * 1_000_000.0;

    (p50, p95, p99)
}