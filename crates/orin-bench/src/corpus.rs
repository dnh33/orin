//! Deterministic synthetic corpus generation.

use anyhow::Context;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::fs;
use std::path::PathBuf;

pub struct CorpusConfig {
    pub files: usize,
    pub depth: usize,
    pub extensions: &'static [&'static str],
    pub prefix: &'static str,
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

pub fn generate(name: &str, out: &PathBuf, seed: u64) -> anyhow::Result<()> {
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
    let mut dirs: Vec<PathBuf> = vec![out.clone()];
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
        let ext = cfg.extensions[rng.random_range(0..cfg.extensions.len())];
        let fname = format!("{}_{}_{}.{}", cfg.prefix, i, ext);
        let path = dir.join(&fname);

        let size = match rng.random_range(0..10) {
            0 => rng.random_range(1..100),                // tiny
            1 => rng.random_range(100..10_000),           // small
            2 => rng.random_range(10_000..1_000_000),     // medium
            3 => rng.random_range(1_000_000..50_000_000), // large
            _ => rng.random_range(1..1_000),              // default
        };

        let content: Vec<u8> = (0..size).map(|_| rng.random_range(0..256u8)).collect();
        fs::write(&path, &content)?;
        written += 1;
        i += 1;
    }

    // Create some node_modules-like dirs and .git-like dirs
    for d in &dirs {
        let nm = d.join("node_modules");
        fs::create_dir_all(&nm)?;
        for j in 0..50 {
            fs::write(nm.join(format!("pkg_{}.js", j)), b"module.exports = {};")?;
        }
        let git = d.join(".git");
        fs::create_dir_all(git.join("objects"))?;
        fs::write(git.join("config"), b"[core]\nrepositoryformatversion = 0\n")?;
    }

    Ok(())
}
