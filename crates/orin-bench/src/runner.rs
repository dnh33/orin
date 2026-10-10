//! Tool runner + timing.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[allow(dead_code)]
pub struct RunResult {
    #[allow(dead_code)]
    pub tool: String,
    #[allow(dead_code)]
    pub corpus_dir: String,
    #[allow(dead_code)]
    pub query: String,
    #[allow(dead_code)]
    pub latency_us: u64,
    #[allow(dead_code)]
    pub exit_code: i32,
    #[allow(dead_code)]
    pub output_bytes: usize,
}

/// Directory containing the built release binaries (sibling of this exe).
fn bin_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("target/release"))
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Unique socket name for the benchmark daemon per runner process.
fn bench_socket() -> String {
    if cfg!(windows) {
        format!("orin-bench-{}", std::process::id())
    } else {
        std::env::temp_dir()
            .join(format!("orin-bench-{}.sock", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }
}

/// Spawn `orind` indexing the corpus and wait until it serves results.
/// Returns the daemon child handle (kept alive for the whole run).
fn ensure_daemon(corpus_dir: &Path) -> anyhow::Result<std::process::Child> {
    let orind = bin_dir().join(exe_name("orind"));
    let orin = bin_dir().join(exe_name("orin"));
    let data_dir = std::env::temp_dir().join(format!("orin-bench-data-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir).with_context(|| format!("create {}", data_dir.display()))?;
    let socket = bench_socket();

    let mut cmd = Command::new(&orind);
    cmd.env("ORIN_SOCKET", &socket)
        .env("ORIN_DATA_DIR", &data_dir)
        .env("ORIN_ROOTS", corpus_dir)
        .env("RUST_LOG", "warn")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", orind.display()))?;

    // Poll `orin status --json` until the index reports entries (scan done).
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let out = Command::new(&orin)
            .arg("status")
            .arg("--json")
            .env("ORIN_SOCKET", &socket)
            .env("ORIN_NO_SPAWN", "1")
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                let v: Option<serde_json::Value> = serde_json::from_slice(&out.stdout).ok();
                let entries = v
                    .as_ref()
                    .and_then(|v| v.get("entries"))
                    .and_then(|e| e.as_u64())
                    .unwrap_or(0);
                if entries > 0 || v.is_none() {
                    return Ok(child);
                }
            }
        }
        if Instant::now() > deadline {
            anyhow::bail!("orind did not become ready within 30s");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
        if result.status.code() != Some(0) {
            eprintln!("Warning: {} exited non-zero for query '{}'", tool, query);
        }
    }

    Ok(latencies)
}

fn run_single(
    tool: &str,
    corpus_dir: &PathBuf,
    query: &str,
) -> anyhow::Result<std::process::Output> {
    let mut tool_cmd = match tool {
        "orin" => {
            let mut cmd = Command::new(bin_dir().join(exe_name("orin")));
            cmd.arg("query")
                .arg(query)
                .arg("--limit")
                .arg("1000")
                .env("ORIN_SOCKET", bench_socket())
                .env("ORIN_NO_SPAWN", "1");
            cmd
        }
        "fd" => {
            // apt ships `fdfind`, brew/choco ship `fd`.
            let mut cmd = Command::new(if cfg!(target_os = "linux") {
                "fdfind"
            } else {
                "fd"
            });
            cmd.arg(query);
            cmd
        }
        "find" => {
            let mut cmd = Command::new("find");
            cmd.arg(".").arg("-iname").arg(format!("*{query}*"));
            cmd
        }
        "rg" | "ripgrep" => {
            // Filename-mode ripgrep: glob filter over --files.
            let mut cmd = Command::new("rg");
            cmd.arg("--files").arg("-g").arg(format!("*{query}*"));
            cmd
        }
        _ => anyhow::bail!("unknown tool: {tool}"),
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

    // Start the benchmark daemon once for the orin arm.
    let _daemon = if tool == "orin" {
        Some(ensure_daemon(corpus_dir)?)
    } else {
        None
    };

    // First 10 queries for a quick benchmark run (plain-name classes only;
    // ext/type/size queries use orin syntax and are not portable to other tools).
    let queries_to_run: Vec<String> = qmatrix
        .exact
        .iter()
        .cloned()
        .chain(qmatrix.prefix.iter().cloned())
        .chain(qmatrix.substring.iter().cloned())
        .collect();

    let mut results = Vec::new();
    for q in queries_to_run.iter().take(10) {
        let latencies = run_tool(tool, corpus_dir, q, iterations)?;
        let stat = crate::stats::from_durations(&latencies);
        results.push(format!(
            "{{\"query\":{},\"tool\":\"{}\",\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"qps\":{:.2}}}",
            serde_json::to_string(&q)?,
            tool,
            stat.latency_p50_us,
            stat.latency_p95_us,
            stat.latency_p99_us,
            stat.qps
        ));
    }

    std::fs::write(out_path, format!("[{}]", results.join(",")))?;
    println!(
        "bench: {} queries x {} iterations = {} results -> {}",
        results.len(),
        iterations,
        results.len(),
        out_path.display()
    );
    Ok(())
}

#[allow(dead_code)]
pub fn summarize(latencies: &[Duration]) -> (f64, f64, f64) {
    if latencies.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted: Vec<f64> = latencies.iter().map(|d| d.as_micros() as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = sorted[sorted.len() * 50 / 100];
    let p95 = sorted[sorted.len() * 95 / 100];
    let p99 = sorted[sorted.len() * 99 / 100];

    let total: f64 = sorted.iter().sum();
    let _qps = (latencies.len() as f64 / total) * 1_000_000.0;

    (p50, p95, p99)
}
