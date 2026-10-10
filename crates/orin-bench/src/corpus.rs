//! Deterministic synthetic corpus generation.
//!
//! Same (corpus name, seed) yields the same tree: every name comes from a
//! fixed table plus the seeded RNG - never wall-clock time, hostname, PID or
//! environment. Seed 0xC0FFEE is the default in main.rs.
//!
//! # Plant map (mirrors crates/orin-bench/queries.json)
//!
//! Synthetic machinery classes (unchanged counts, index-arithmetic only):
//! - exact:     Cargo.toml, README.md, main.rs, LICENSE, .gitignore at the
//!   root + the first 2 leaf dirs (3 hits per name)
//! - prefix:    test_/app_/config_/src_ x50, pkg_ via node_modules x50
//! - substring: test/util/mod/fn/struct x20
//! - case:      App.tsx x3, APP x3 (fd smart-case and rg globs are
//!   case-sensitive on the uppercase needles `App`/`APP`)
//!
//! Realistic classes - every needle gets >= 3 deterministic hits per corpus
//! size (the bench preflight refuses empty searches, so this is mandatory):
//!
//! | class          | plants per needle                                        |
//! |----------------|----------------------------------------------------------|
//! | multiword      | budget 2026.xlsx, report final.docx, project plan.docx x6|
//! | datever        | invoice-2026.xlsx x6, parser-v2.py x6, notes_final_2 x6  |
//! | camera         | IMG_20261008_*.jpg x6 + 3 other dates + Photos/ tree     |
//! | ext_scope      | annual-report.pdf, budget-summary.pdf, board-minutes.pdf |
//! |                | x6 each (pdf is plant-only: no generated pdfs)           |
//! | ext_filter     | orin-only `ext:` arm; jpg/png/pdf all exist in-corpus    |
//! | deep_path      | 6 dirs named with the needle (2 in the 10-level chain,   |
//! |                | 4 at natural depth) + 3 `-summary-` basename files so    |
//! |                | rg's basename-only `-g` arm stays non-empty              |
//! | case_mixed     | Main.rs x6 leaf dirs (never root: the exact-class        |
//! |                | `main.rs` plant lives there and Windows files are        |
//! |                | case-insensitive), Makefile/Dockerfile root + 5 dirs     |
//! | realistic_miss | bank-statement-2019.pdf x3 in EVERY size (preflight      |
//! |                | refuses empty searches); `large` only adds the           |
//! |                | Documents/Bank/2019 neighborhood (sparsity realism)      |
//!
//! Realism beyond queries: kebab/snake/date/version/camera/space/Title name
//! shapes in the main loop, bounded noise (nested node_modules, deep .git,
//! __pycache__, AppData/Local/Temp-like, target/), a fixed 10-level chain so
//! some files sit 8-12 levels deep, and 3 non-ASCII names (caf, ligatures).

use anyhow::Context;
use rand::Rng;
use rand::RngCore;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::fs;
use std::path::{Path, PathBuf};

pub struct CorpusConfig {
    pub files: usize,
    pub depth: usize,
    pub extensions: &'static [&'static str],
    pub prefix: &'static str,
}

/// Name-shape tables for realistic corpus naming. Slugs stay lowercase and
/// away from every query needle (see the plant map) so match counts stay
/// interpretable; nothing here is derived from the clock or the hostname.
const SLUGS: &[&str] = &["report", "draft", "notes", "asset", "ledger", "summary"];
const SPACE_SLUGS: &[&str] = &["quarterly", "planning", "agenda", "meeting"];
const SPACE_TAILS: &[&str] = &["draft", "notes", "final", "copy"];
const TITLE_SLUGS: &[&str] = &["Report", "Draft", "Notes", "Asset", "Summary"];
const PHOTO_EXTS: &[&str] = &["jpg", "png"];
const PY_MODULES: &[&str] = &["utils", "helpers", "models", "views"];
/// Fixed 10-level chain: files land 8-12 levels deep, and the deep-path
/// query class plants two of its directories inside it.
const CHAIN: &[&str] = &[
    "projects",
    "client-portal",
    "src",
    "features",
    "billing",
    "handlers",
    "tests",
    "fixtures",
    "2026",
    "qa",
];

fn pick<'a>(rng: &mut StdRng, table: &[&'a str]) -> &'a str {
    table[rng.random_range(0..table.len())]
}

/// Deterministic realistic filename for file `i`: fixed shape tables plus the
/// seeded RNG. Every template embeds `i`, so names never collide (corpus file
/// counts stay exact) even on case-insensitive filesystems.
fn realistic_name(rng: &mut StdRng, cfg: &CorpusConfig, i: usize) -> String {
    let exts = cfg.extensions;
    match rng.random_range(0..100) {
        // Synthetic baseline - keeps the original micro-classes' look.
        0..=49 => format!("{}_{}.{}", cfg.prefix, i, pick(rng, exts)),
        // kebab-case: report-42.md
        50..=61 => format!("{}-{}.{}", pick(rng, SLUGS), i, pick(rng, exts)),
        // snake_case: tiny_notes_42.txt
        62..=70 => format!(
            "{}_{}_{}.{}",
            cfg.prefix,
            pick(rng, SLUGS),
            i,
            pick(rng, exts)
        ),
        // dated: 2026-03-15-notes-42.md
        71..=78 => {
            let m = rng.random_range(1..13);
            let d = rng.random_range(1..29);
            let stem = format!("2026-{:02}-{:02}-{}-{}", m, d, pick(rng, SLUGS), i);
            format!("{stem}.{}", pick(rng, exts))
        }
        // version suffixes: report-42-v2.md, draft-42_final.md, report-42 (1).md
        79..=85 => {
            let slug = pick(rng, SLUGS);
            let stem = match rng.random_range(0..3) {
                0 => format!("{slug}-{i}-v2"),
                1 => format!("{slug}-{i}_final"),
                _ => format!("{slug}-{i} (1)"),
            };
            format!("{stem}.{}", pick(rng, exts))
        }
        // camera/media: IMG_20261008_143222.jpg - photo extensions; the tail
        // is `i` so 2M-file corpora stay collision-free
        86..=90 => {
            let m = rng.random_range(1..13);
            let d = rng.random_range(1..29);
            format!(
                "IMG_2026{:02}{:02}_{:06}.{}",
                m,
                d,
                i,
                pick(rng, PHOTO_EXTS)
            )
        }
        // names with spaces: quarterly notes 42.docx
        91..=96 => {
            let slug = pick(rng, SPACE_SLUGS);
            let tail = pick(rng, SPACE_TAILS);
            format!("{slug} {tail} {i}.{}", pick(rng, exts))
        }
        // mixed case: Report-42.md
        _ => format!("{}-{}.{}", pick(rng, TITLE_SLUGS), i, pick(rng, exts)),
    }
}

/// Bounded realistic noise: nested node_modules, deep .git objects/refs,
/// __pycache__, a Temp-like tree, and target/. Counts are fixed (no RNG), so
/// totals stay predictable even for the 2M-file corpora.
fn add_noise(out: &Path, dirs: &[PathBuf]) -> anyhow::Result<()> {
    for (n, d) in dirs.iter().take(100).enumerate() {
        // node_modules, with a nested dependency tree for the first dirs
        let nm = d.join("node_modules");
        fs::create_dir_all(&nm)?;
        for j in 0..50 {
            fs::write(nm.join(format!("pkg_{}.js", j)), b"module.exports = {};")?;
        }
        if n < 20 {
            let inner = nm.join("dep_pkg").join("node_modules").join("inner");
            fs::create_dir_all(&inner)?;
            for j in 0..10 {
                fs::write(inner.join(format!("inner_{j}.js")), b"module.exports = {};")?;
            }
        }
        // .git with realistic ref/object depth
        let git = d.join(".git");
        fs::create_dir_all(git.join("objects"))?;
        fs::create_dir_all(git.join("refs").join("heads"))?;
        fs::write(git.join("HEAD"), b"ref: refs/heads/main\n")?;
        fs::write(git.join("config"), b"[core]\nrepositoryformatversion = 0\n")?;
        let head = git.join("refs").join("heads").join("main");
        fs::write(head, b"0000000000000000000000000000000000000000")?;
        for j in 0..8 {
            let sub = git.join("objects").join(format!("{j:02x}"));
            fs::create_dir_all(&sub)?;
            fs::write(sub.join(format!("{j:040x}")), b"\0")?;
        }
        // __pycache__ with compiled module names
        if n < 30 {
            let py = d.join("__pycache__");
            fs::create_dir_all(&py)?;
            for j in 0..10 {
                let m = PY_MODULES[j % PY_MODULES.len()];
                fs::write(py.join(format!("{m}_{j}.cpython-312.pyc")), b"pyc")?;
            }
        }
    }
    // AppData/Local/Temp-like tree (fixed user name; nothing env-derived)
    let temp = out
        .join("Users")
        .join("Admin")
        .join("AppData")
        .join("Local");
    fs::create_dir_all(temp.join("Temp"))?;
    for j in 0..200 {
        fs::write(temp.join("Temp").join(format!("~DF{j:04X}.tmp")), b"tmp")?;
    }
    // target/ build output
    let deps = out.join("target").join("debug").join("deps");
    fs::create_dir_all(&deps)?;
    for j in 0..20 {
        fs::write(deps.join(format!("orin_bench-{j:04}.rlib")), b"rlib")?;
    }
    Ok(())
}

/// Fixed 10-level chain: files land 8-12 levels deep (a corpus requirement)
/// and the deep-path query class plants two directories inside it. Returns
/// every level's path so callers can hang subdirectories off any depth.
fn add_deep_chain(out: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut levels = Vec::with_capacity(CHAIN.len());
    let mut cur = out.to_path_buf();
    for seg in CHAIN {
        cur = cur.join(seg);
        fs::create_dir_all(&cur)?;
        levels.push(cur.clone());
    }
    let bottom_files = [
        "coverage.xml",
        "snapshot.json",
        "notes.md",
        "gate.log",
        "registry.toml",
        "manifest.json",
    ];
    let bottom = &levels[levels.len() - 1];
    for name in bottom_files {
        fs::write(bottom.join(name), b"planted\n")?;
    }
    Ok(levels)
}

/// Plant `fname` (identical content) into `count` deterministic leaf
/// directories, starting at `start` and wrapping around the tree.
fn leaf_plant(dirs: &[PathBuf], fname: &str, start: usize, count: usize) -> anyhow::Result<()> {
    for k in 0..count {
        let d = &dirs[(start + k) % dirs.len()];
        fs::write(d.join(fname), b"planted\n")?;
    }
    Ok(())
}

/// Deterministic plants for the shared machinery classes (exact / prefix /
/// substring) plus the `case` class floor. Index-arithmetic only, no RNG, so
/// existing rows keep their match counts. (`pkg_` is covered by add_noise.)
fn plant_shared(out: &Path, dirs: &[PathBuf], extensions: &[&str]) -> anyhow::Result<()> {
    for name in [
        "Cargo.toml",
        "README.md",
        "main.rs",
        "LICENSE",
        ".gitignore",
    ] {
        fs::write(out.join(name), b"planted\n")?;
        for d in dirs.iter().take(2) {
            fs::write(d.join(name), b"planted\n")?;
        }
    }
    for (pi, prefix) in ["test_", "app_", "config_", "src_"].iter().enumerate() {
        for j in 0..50 {
            let d = &dirs[(pi * 7 + j) % dirs.len()];
            let ext = extensions[j % extensions.len()];
            fs::write(d.join(format!("{prefix}{j}.{ext}")), b"planted\n")?;
        }
    }
    for (si, s) in ["test", "util", "mod", "fn", "struct"].iter().enumerate() {
        for j in 0..20 {
            let d = &dirs[(si * 11 + j) % dirs.len()];
            fs::write(d.join(format!("{s}_hit_{j}.txt")), b"planted\n")?;
        }
    }
    // `case` floor: fd's smart-case and rg's globs are case-sensitive on the
    // uppercase needles `App`/`APP`, so they need exact-case hits of their
    // own or those arms would preflight-fail.
    for d in dirs.iter().take(3) {
        fs::write(d.join("App.tsx"), b"planted\n")?;
        fs::write(d.join("APP"), b"planted\n")?;
    }
    Ok(())
}

/// Plants for the realistic query classes. Every needle in queries.json gets
/// at least 3 deterministic hits per corpus size - the bench preflight
/// refuses empty searches, so this is mandatory. See the module doc plant map.
fn plant_realistic(
    out: &Path,
    dirs: &[PathBuf],
    corpus: &str,
    levels: &[PathBuf],
) -> anyhow::Result<()> {
    // multiword (spaces in names)
    leaf_plant(dirs, "budget 2026.xlsx", 0, 6)?;
    leaf_plant(dirs, "report final.docx", 1, 6)?;
    leaf_plant(dirs, "project plan.docx", 2, 6)?;
    // date/version suffix names (-v2 also fans out over generated names)
    leaf_plant(dirs, "invoice-2026.xlsx", 3, 6)?;
    leaf_plant(dirs, "parser-v2.py", 4, 6)?;
    leaf_plant(dirs, "notes_final_2.txt", 5, 6)?;
    // camera/media names (the 20261008 files hit BOTH camera needles)
    for k in 0..6 {
        let d = &dirs[(6 + k) % dirs.len()];
        fs::write(d.join(format!("IMG_20261008_{k:06}.jpg")), b"planted\n")?;
    }
    leaf_plant(dirs, "IMG_20260101_000000.jpg", 7, 3)?;
    let photos = out.join("Photos").join("2026").join("10");
    fs::create_dir_all(&photos)?;
    for k in 0..20 {
        fs::write(
            photos.join(format!("IMG_20261008_{k:06}.jpg")),
            b"planted\n",
        )?;
    }
    // extension-scoped cross-tool arm (pdf is plant-only: no generated pdfs)
    leaf_plant(dirs, "annual-report.pdf", 8, 6)?;
    leaf_plant(dirs, "budget-summary.pdf", 9, 6)?;
    leaf_plant(dirs, "board-minutes.pdf", 10, 6)?;
    // case-mixed fold behavior; Main.rs never goes at the root because the
    // exact-class `main.rs` plant already lives there (same file on Windows)
    leaf_plant(dirs, "Main.rs", 2, 6)?;
    fs::write(out.join("Makefile"), b"planted\n")?;
    fs::write(out.join("Dockerfile"), b"planted\n")?;
    fs::write(out.join("README"), b"planted\n")?;
    for k in 0..5 {
        let d = &dirs[(2 + k) % dirs.len()];
        fs::write(d.join("Makefile"), b"planted\n")?;
        fs::write(d.join("Dockerfile"), b"planted\n")?;
    }
    // deep-path: the needle lives in DIRECTORY names. The basename files are
    // a preflight floor for rg, whose `-g '*needle*'` matches file basenames
    // only (files under an invoices-2026/ dir do not match it).
    for (ni, needle) in ["invoices-2026", "client-export"].iter().enumerate() {
        for li in [4, 7] {
            let sub = levels[li].join(needle);
            fs::create_dir_all(&sub)?;
            for k in 0..3 {
                fs::write(sub.join(format!("entry-{k:03}.txt")), b"planted\n")?;
            }
        }
        for k in 0..4 {
            let sub = dirs[(ni * 4 + k) % dirs.len()].join(needle);
            fs::create_dir_all(&sub)?;
            fs::write(sub.join("entry-000.txt"), b"planted\n")?;
        }
        for k in 0..3 {
            let d = &dirs[(12 + ni * 3 + k) % dirs.len()];
            fs::write(d.join(format!("{needle}-summary-{k}.txt")), b"planted\n")?;
        }
    }
    // realistic miss: 3-hit floor in EVERY corpus (preflight refuses empty
    // searches, so a large-only needle would kill tiny/medium runs); the
    // realistic neighborhood exists only in `large` (sparsity realism)
    leaf_plant(dirs, "bank-statement-2019.pdf", 15, 3)?;
    if corpus == "large" {
        let bank = out.join("Documents").join("Bank").join("2019");
        fs::create_dir_all(&bank)?;
        for n in [
            "bank-statement-2019.pdf",
            "bank-statement-2019-scan.jpg",
            "bank-statement-2019 (1).pdf",
        ] {
            fs::write(bank.join(n), b"planted\n")?;
        }
    }
    // unicode names (non-ASCII realism; no query targets them)
    fs::write(out.join("café-menu.md"), b"planted\n")?;
    fs::write(dirs[0].join("rapport æøå.txt"), b"planted\n")?;
    fs::write(dirs[1].join("prøve-fil.rs"), b"planted\n")?;
    Ok(())
}

fn config_for(name: &str) -> CorpusConfig {
    match name {
        "tiny" => CorpusConfig {
            files: 10_000,
            depth: 3,
            extensions: &["txt", "rs", "md", "toml", "json"],
            prefix: "tiny",
        },
        "medium" => CorpusConfig {
            files: 500_000,
            depth: 8,
            extensions: &["txt", "rs", "md", "toml", "json", "lock", "py", "js", "ts"],
            prefix: "medium",
        },
        "large" => CorpusConfig {
            files: 2_000_000,
            depth: 15,
            extensions: &[
                "txt", "rs", "md", "toml", "json", "lock", "py", "js", "ts", "c", "h",
            ],
            prefix: "large",
        },
        "real-home" => CorpusConfig {
            files: 1_000_000,
            depth: 6,
            extensions: &[
                "txt", "rs", "md", "toml", "json", "lock", "py", "js", "ts", "c", "h", "cfg",
            ],
            prefix: "home",
        },
        _ => CorpusConfig {
            files: 10_000,
            depth: 3,
            extensions: &["txt", "rs", "md", "toml", "json"],
            prefix: "tiny",
        },
    }
}

pub fn generate(name: &str, out: &Path, seed: u64) -> anyhow::Result<()> {
    let cfg = config_for(name);

    if out.exists() {
        fs::remove_dir_all(out).with_context(|| format!("remove {}", out.display()))?;
    }
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    // Expand 8-byte seed to 32-byte array for StdRng
    let mut seed_bytes = [0u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let mut rng = StdRng::from_seed(seed_bytes);

    // Build directory tree
    let mut dirs: Vec<PathBuf> = vec![out.to_path_buf()];
    for _ in 0..cfg.depth {
        let mut new_dirs = Vec::new();
        for d in &dirs {
            let n = rng.random_range(2..=4);
            for i in 0..n {
                let sub = d.join(format!("{}_{}_{}", cfg.prefix, d.iter().count(), i));
                fs::create_dir_all(&sub)?;
                new_dirs.push(sub);
            }
        }
        dirs = new_dirs;
        if dirs.len() > cfg.files / 10 {
            break;
        }
    }

    // Write files
    let mut written = 0usize;
    let mut i = 0;
    while written < cfg.files {
        let dir = &dirs[i % dirs.len()];
        let fname = realistic_name(&mut rng, &cfg, i);
        let path = dir.join(&fname);

        // Realistic small-file-dominant distribution (bounded totals so the
        // medium/large corpora fit on a runner disk): ~5.4KB average.
        let size = match rng.random_range(0..1000) {
            0..=699 => rng.random_range(1..500),            // tiny (70%)
            700..=949 => rng.random_range(500..10_000),     // small (25%)
            950..=994 => rng.random_range(10_000..100_000), // medium (4.5%)
            _ => rng.random_range(100_000..500_000),        // large (0.5%)
        };

        let mut content = vec![0u8; size];
        rng.fill_bytes(&mut content);
        fs::write(&path, &content)?;
        written += 1;
        i += 1;
    }

    add_noise(out, &dirs)?;
    let levels = add_deep_chain(out)?;

    // Planted coverage for every query class; see the module doc plant map.
    plant_shared(out, &dirs, cfg.extensions)?;
    plant_realistic(out, &dirs, name, &levels)?;

    Ok(())
}
