//! Platform paths: data dir, config dir, socket, log file, lock file — with env overrides.

use interprocess::local_socket::{GenericNamespaced, Name, ToNsName};
use std::path::PathBuf;

/// Get the data directory (snapshots, logs, lock).
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ORIN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from(r"C:\orin"))
        .join("orin")
}

/// Get the config directory.
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ORIN_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(r"C:\orin"))
        .join("orin")
}

/// Alias for config.rs compatibility.
pub fn default_data_dir() -> PathBuf {
    data_dir()
}

/// Alias for config.rs compatibility.
pub fn default_config_dir() -> PathBuf {
    config_dir()
}

/// Get the socket/pipe name.
pub fn socket_name() -> std::io::Result<Name<'static>> {
    if let Ok(name) = std::env::var("ORIN_SOCKET") {
        return name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
    }
    let user = whoami::username();
    let hash = std::process::id() % 10000;
    let pipe_name = format!(r"\\.\pipe\orin-{}-{:04}", user, hash);
    pipe_name
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
}

/// Get the log file path.
pub fn log_file() -> PathBuf {
    data_dir().join("orind.log")
}

/// Get the lock file path.
pub fn lock_file() -> PathBuf {
    data_dir().join("orind.lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_env_override() {
        unsafe { std::env::set_var("ORIN_DATA_DIR", "/custom/path") };
        assert_eq!(data_dir(), std::path::PathBuf::from("/custom/path"));
    }
}
