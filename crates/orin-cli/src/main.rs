//! orin — CLI client for the orin search daemon.
//!
//! Speaks the orin-core wire protocol (4-byte little-endian length prefix +
//! JSON frames) to `orin daemon` over its local socket / named pipe,
//! spawning the daemon in the background when it is not already running.
//!
//! One shipped binary, two names: `on.exe` is a byte-identical copy of
//! `orin.exe`, and argv[0] picks the default subcommand (see `parse_cli`).

mod client;
mod tui;

use clap::CommandFactory;
use clap::Parser;
use clap::Subcommand;
use orin_core::protocol::HitWire;
use orin_core::protocol::StatusData;
use orin_daemon::DaemonArgs;
use orin_daemon::run_daemon;
use std::ffi::OsString;
use std::process::ExitCode;

/// rg-like exit code: the command worked but found nothing.
const NO_RESULTS: u8 = 1;
/// Exit code for runtime failures (daemon unreachable, protocol errors).
const ERROR: u8 = 2;
/// Root flags the `on` alias must not rewrite into `orin query <flag>`.
const ROOT_FLAGS: [&str; 4] = ["-h", "--help", "-V", "--version"];
/// `--help` epilog: what the byte-identical `on` copy does.
const ON_EPILOG: &str = "\
`on` is a byte-identical copy of `orin`, so argv[0] picks the subcommand:
  on <terms>  ==  orin query <terms>   (`on foo` == `orin query foo`)
  on status   ==  orin status          (explicit subcommands always win)";

#[derive(Debug, Parser)]
#[command(name = "orin", version, about = "instant whole-disk file search")]
#[command(after_help = ON_EPILOG)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Search the index and print matching paths
    Query {
        /// Search terms (query language: ext:rs, size:>10M, path:src, ...)
        #[arg(required = true)]
        terms: Vec<String>,
        /// Maximum number of results
        #[arg(long, default_value_t = 10_000)]
        limit: u32,
        /// Print one JSON object per result (JSON Lines)
        #[arg(long)]
        json: bool,
    },
    /// Show daemon status (version, entries, roots, memory)
    Status {
        /// Print status as pretty-printed JSON
        #[arg(long)]
        json: bool,
    },
    /// Run the orin MCP server on stdio for AI agents
    Mcp,
    /// Interactive fuzzy picker over the daemon index (TUI)
    Tui {
        /// Open the selected path instead of printing it to stdout
        #[arg(long)]
        open: bool,
    },
    /// Run the search daemon in the foreground (index + serve until stopped)
    Daemon {
        #[command(flatten)]
        args: DaemonArgs,
    },
}

fn main() -> ExitCode {
    let cli = parse_cli();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("orin: {err:#}");
            ExitCode::from(ERROR)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Commands::Query { terms, limit, json } => query_cmd(&terms, limit, json),
        Commands::Status { json } => status_cmd(json),
        Commands::Mcp => mcp_cmd(),
        Commands::Tui { open } => tui::run(open),
        Commands::Daemon { args } => {
            run_daemon(args)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Parse this process's arguments, honoring the `on` argv[0] alias.
///
/// `on` is a byte-identical copy of `orin`, so argv[0] picks the default
/// subcommand: `on foo` parses as `orin query foo`. An explicit subcommand
/// typed after `on` (`on status`) still wins, as do the root flags
/// (`on --version`). Any other argv[0] parses as plain `orin`.
fn parse_cli() -> Cli {
    let argv: Vec<OsString> = std::env::args_os().collect();
    if on_alias(&argv) {
        let mut aliased = Vec::with_capacity(argv.len() + 2);
        aliased.push(OsString::from("orin"));
        aliased.push(OsString::from("query"));
        aliased.extend(argv.into_iter().skip(1));
        Cli::parse_from(aliased)
    } else {
        Cli::parse_from(argv)
    }
}

/// True when argv[0] is the `on` copy and no explicit subcommand follows it.
fn on_alias(argv: &[OsString]) -> bool {
    let Some(argv0) = argv.first() else {
        return false;
    };
    let stem = std::path::Path::new(argv0).file_stem().unwrap_or(argv0);
    if !stem
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("on"))
    {
        return false;
    }
    let Some(first) = argv.get(1) else {
        // Bare `on`: show the product's usage rather than an empty query.
        return false;
    };
    let first = first.to_string_lossy().into_owned();
    let mut command = Cli::command();
    command.build();
    let explicit = command
        .get_subcommands()
        .any(|sub| sub.get_name() == first.as_str());
    !explicit && !ROOT_FLAGS.contains(&first.as_str())
}

fn query_cmd(terms: &[String], limit: u32, json: bool) -> anyhow::Result<ExitCode> {
    let hits = client::query(&terms.join(" "), limit)?;
    if hits.is_empty() {
        return Ok(ExitCode::from(NO_RESULTS));
    }
    if json {
        for hit in &hits {
            let line = serde_json::to_string(&hit_json(hit))?;
            println!("{line}");
        }
    } else {
        for hit in &hits {
            println!("{}  {:.2}  {}", hit.p, hit.score, human_size(hit.s));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn status_cmd(json: bool) -> anyhow::Result<ExitCode> {
    let data = client::status()?;
    if json {
        let text = serde_json::to_string_pretty(&data)?;
        println!("{text}");
    } else {
        print_status(&data);
    }
    Ok(ExitCode::SUCCESS)
}

fn mcp_cmd() -> anyhow::Result<ExitCode> {
    orin_mcp::run_stdio()?;
    Ok(ExitCode::SUCCESS)
}

fn print_status(data: &StatusData) {
    println!("orin daemon {} (protocol {})", data.version, data.protocol);
    println!("state: {}", data.state);
    println!("entries: {}", data.entries);
    println!("memory: {}", human_size(data.mem_bytes));
    println!("unreadable: {}", data.unreadable);
    if let Some(p) = &data.progress {
        println!("scanning: {} entries ({} ms)", p.entries, p.since_ms);
    }
    println!("roots ({}):", data.roots.len());
    for root in &data.roots {
        let (path, entries, watch) = (&root.path, root.entries, &root.watch);
        println!("  {path} ({entries} entries, watch={watch})");
    }
}

fn hit_json(hit: &HitWire) -> serde_json::Value {
    let kind = match hit.t {
        0 => "file",
        1 => "dir",
        2 => "link",
        _ => "other",
    };
    serde_json::json!({
        "path": &hit.p,
        "name": &hit.n,
        "type": kind,
        "size": hit.s,
        "mtime": hit.m,
        "score": hit.score,
    })
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_flags_parse() {
        let args = ["orin", "query", "foo", "--json"];
        let cli = Cli::try_parse_from(args).expect("parses");
        let Commands::Query { terms, json, .. } = cli.command else {
            panic!("expected the query subcommand");
        };
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0], "foo");
        assert!(json);
    }

    #[test]
    fn query_limit_parses() {
        let args = ["orin", "query", "foo", "--limit", "5"];
        let cli = Cli::try_parse_from(args).expect("parses");
        let Commands::Query { limit, .. } = cli.command else {
            panic!("expected the query subcommand");
        };
        assert_eq!(limit, 5);
    }

    #[test]
    fn query_requires_terms() {
        assert!(Cli::try_parse_from(["orin", "query"]).is_err());
    }

    #[test]
    fn on_alias_rewrites_query_but_not_subcommands() {
        let argv = |argv0: &str, arg: &str| [OsString::from(argv0), OsString::from(arg)];
        assert!(on_alias(&argv("on", "foo")));
        assert!(on_alias(&argv("on.exe", "--json")));
        assert!(on_alias(&argv("ON.EXE", "foo")));
        assert!(!on_alias(&argv("on.exe", "status")));
        assert!(!on_alias(&argv("on.exe", "--version")));
        assert!(!on_alias(&argv("orin.exe", "foo")));
    }

    #[test]
    fn status_json_parses() {
        let args = ["orin", "status", "--json"];
        let cli = Cli::try_parse_from(args).expect("parses");
        let Commands::Status { json } = cli.command else {
            panic!("expected the status subcommand");
        };
        assert!(json);
    }

    #[test]
    fn hit_json_has_expected_fields() {
        let hit = HitWire {
            p: "a.txt".into(),
            n: "a.txt".into(),
            t: 0,
            s: 7,
            m: 1,
            score: 500.0,
        };
        let value = hit_json(&hit);
        assert_eq!(value["path"], "a.txt");
        assert_eq!(value["type"], "file");
        assert_eq!(value["size"].as_u64(), Some(7));
    }

    #[test]
    fn human_size_formats_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(1024 * 1024), "1.0 MiB");
    }
}
