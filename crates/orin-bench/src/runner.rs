//! Tool runner + timing.

use anyhow::Context;
use std::os::windows::process::CommandExt;
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
    format!("{base}.exe")
}

/// Unique pipe name for the benchmark daemon per runner process.
fn bench_socket() -> String {
    format!("orin-bench-{}", std::process::id())
}

/// Resolved binary for a comparison tool. The workflow exports BENCH_TOOL_* with
/// the Windows path verified in the SAME shell context the bench runs under -
/// a bare name lets CreateProcess resolve `find` to System32 find.exe (a string
/// filter whose instant error exits get timed as if they were fast searches).
fn tool_bin(tool: &str) -> String {
    let key = match tool {
        "fd" => "BENCH_TOOL_FD",
        "find" => "BENCH_TOOL_FIND",
        "rg" | "ripgrep" => "BENCH_TOOL_RG",
        _ => "",
    };
    if key.is_empty() {
        return tool.to_string();
    }
    std::env::var(key).unwrap_or_else(|_| tool.to_string())
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
    let log_path = data_dir.join("orind-stderr.log");
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("create {}", log_path.display()))?;
    let log_err = log.try_clone().context("clone orind stderr log")?;
    cmd.env("ORIN_SOCKET", &socket)
        .env("ORIN_DATA_DIR", &data_dir)
        .env("ORIN_ROOTS", corpus_dir)
        .env("RUST_LOG", "warn")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log_err);
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    cmd.creation_flags(DETACHED_PROCESS);
    let child = cmd
        .spawn()
        .with_context(|| format!("spawn {}", orind.display()))?;

    // Poll `orin status --json` until the daemon answers with indexed entries.
    // Parse stdout regardless of exit code: a status call may exit non-zero
    // on "no results" semantics even while the daemon is answering.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = String::new();
    loop {
        if let Ok(out) = Command::new(&orin)
            .arg("status")
            .arg("--json")
            .env("ORIN_SOCKET", &socket)
            .env("ORIN_NO_SPAWN", "1")
            .output()
        {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            last = format!(
                "exit={:?} stdout={} stderr={}",
                out.status.code(),
                stdout.trim(),
                stderr.trim()
            );
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
                let entries = v.get("entries").and_then(|e| e.as_u64()).unwrap_or(0);
                if entries > 0 || std::env::var("ORIN_BENCH_ALLOW_EMPTY").is_ok() {
                    return Ok(child);
                }
                // JSON answer but empty index: scan still in progress, keep polling.
            }
        }
        if Instant::now() > deadline {
            let log_tail = std::fs::read_to_string(&log_path).unwrap_or_default();
            let tail: String = log_tail
                .chars()
                .rev()
                .take(2000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            anyhow::bail!(
                "orind did not become ready within 30s\nlast status: {last}\n\
                 orind stderr tail:\n{tail}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn run_tool(
    tool: &str,
    corpus_dir: &Path,
    query: &str,
    iterations: usize,
) -> anyhow::Result<(Vec<Duration>, usize, usize)> {
    let mut latencies = Vec::with_capacity(iterations);
    let mut matches = 0usize;
    let mut nonzero = 0usize;

    // Preflight (also warmup): refuse to time an empty search - a query that
    // matches nothing is not a measurement ("a tool can win by returning nothing").
    let warm = run_single(tool, corpus_dir, query)?;
    if String::from_utf8_lossy(&warm.stdout).lines().count() == 0 {
        anyhow::bail!(
            "query '{}' matched nothing for {} (exit={:?}) - corpus/query \
             misalignment, refusing to time empty searches",
            query,
            tool,
            warm.status.code()
        );
    }

    for _ in 0..iterations {
        let start = Instant::now();
        let result = run_single(tool, corpus_dir, query)?;
        let dur = start.elapsed();
        latencies.push(dur);
        matches = String::from_utf8_lossy(&result.stdout).lines().count();
        if result.status.code() != Some(0) {
            nonzero += 1;
            eprintln!("Warning: {} exited non-zero for query '{}'", tool, query);
        }
    }

    Ok((latencies, matches, nonzero))
}

fn run_single(tool: &str, corpus_dir: &Path, query: &str) -> anyhow::Result<std::process::Output> {
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
            // Whole-tree scope parity: hidden + .gitignore'd files are in scope
            // for find/orin, so fd must not silently skip them.
            let mut cmd = Command::new(tool_bin(tool));
            cmd.arg("--hidden").arg("--no-ignore").arg(query);
            cmd
        }
        "find" => {
            let mut cmd = Command::new(tool_bin(tool));
            cmd.arg(".").arg("-iname").arg(format!("*{query}*"));
            cmd
        }
        "rg" | "ripgrep" => {
            // Filename-mode ripgrep: glob filter over --files.
            let mut cmd = Command::new(tool_bin(tool));
            cmd.arg("--files")
                .arg("--hidden")
                .arg("--no-ignore")
                .arg("-g")
                .arg(format!("*{query}*"));
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
    corpus_dir: &Path,
    queries_path: &Path,
    iterations: usize,
    out_path: &Path,
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
    // Evidence of identity: the resolved binary (env from the workflow's
    // identity gate), not just the command name.
    let tool_path = if tool == "orin" {
        bin_dir().join(exe_name("orin")).display().to_string()
    } else {
        tool_bin(tool)
    };
    for q in queries_to_run.iter().take(10) {
        let (latencies, matches, nonzero) = run_tool(tool, corpus_dir, q, iterations)?;
        let stat = crate::stats::from_durations(&latencies);
        results.push(format!(
            "{{\"query\":{},\"tool\":\"{}\",\"tool_path\":{},\"matches\":{},\
             \"nonzero_exits\":{},\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"qps\":{:.2}}}",
            serde_json::to_string(&q)?,
            tool,
            serde_json::to_string(&tool_path)?,
            matches,
            nonzero,
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
