//! Configuration model (TOML) and platform defaults.

use crate::paths::default_config_dir;

/// Daemon configuration.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct DaemonConfig {
    pub idle_exit_secs: u64,
    pub checkpoint_secs: u64,
    pub log_level: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_exit_secs: 0,
            checkpoint_secs: 60,
            log_level: "info".into(),
        }
    }
}

/// Index configuration.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct IndexConfig {
    pub roots: Vec<String>,
    pub watch: String,
    pub poll_secs: u64,
    pub max_scan_threads: usize,
    pub exclude: Vec<String>,
    pub respect_gitignore: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            watch: "auto".into(),
            poll_secs: 30,
            max_scan_threads: 0,
            exclude: Vec::new(),
            respect_gitignore: false,
        }
    }
}

/// Query configuration.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct QueryConfig {
    pub sort: String,
    pub limit: usize,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            sort: "score".into(),
            limit: 10000,
        }
    }
}

/// UI configuration.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct UiConfig {
    pub theme: String,
    pub preview: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "auto".into(),
            preview: true,
        }
    }
}

/// Full configuration.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub daemon: DaemonConfig,
    pub index: IndexConfig,
    pub query: QueryConfig,
    pub ui: UiConfig,
}

/// Get default roots for the current platform.
pub fn default_roots() -> Vec<String> {
    // Env override: ORIN_ROOTS (`;`-separated).
    if let Ok(env_roots) = std::env::var("ORIN_ROOTS") {
        let roots: Vec<String> = env_roots
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    let mut roots = Vec::new();
    for c in b'C'..=b'Z' {
        let drive = format!("{}:\\", c as char);
        if std::path::Path::new(&drive).exists() {
            roots.push(drive);
        }
    }
    if roots.is_empty() {
        roots.push("C:\\".into());
    }
    roots
}

/// Load configuration from disk, returning config and any warnings.
pub fn load_config() -> (Config, Vec<String>) {
    let config_path = default_config_dir().join("config.toml");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).unwrap_or_default();
        match toml::from_str::<Config>(&content) {
            Ok(cfg) => (cfg, Vec::new()),
            Err(e) => {
                let warnings = vec![format!("config parse error: {}", e)];
                let mut cfg = Config::default();
                cfg.index.roots = default_roots();
                (cfg, warnings)
            }
        }
    } else {
        let mut cfg = Config::default();
        cfg.index.roots = default_roots();
        (cfg, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roots_nonempty() {
        let roots = default_roots();
        assert!(!roots.is_empty());
    }
}
