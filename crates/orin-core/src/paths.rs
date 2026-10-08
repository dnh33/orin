//! Platform paths: data dir, config dir, socket, log file, lock file — with env overrides.

use interprocess::local_socket::Name;
use std::path::PathBuf;

/// Get the data directory (snapshots, logs, lock).
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ORIN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\orin"))
            .join("orin")
    } else if cfg!(target_os = "macos") {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("orin")
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("orin")
    }
}

/// Get the config directory.
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ORIN_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\orin"))
            .join("orin")
    } else if cfg!(target_os = "macos") {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("orin")
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("orin")
    }
}

/// Get the socket/pipe name.
pub fn socket_name() -> std::io::Result<interprocess::local_socket::Name<'static>> {
    if let Ok(name) = std::env::var("ORIN_SOCKET") {
        return Ok(Name::new(name)?);
    }
    if cfg!(windows) {
        let user = whoami::username();
        let hash = std::process::id() % 10000;
        Ok(Name::new(format!(r"\\.\pipe\orin-{}-{:04}", user, hash))?)
    } else {
        let uid = nix::unistd::Uid::current().as_raw();
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        Ok(Name::new(format!("{}/orin-{}.sock", runtime, uid))?)
    }
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
        std::env::set_var("ORIN_DATA_DIR", "/custom/path");
        assert_eq!(data_dir(), std::path::PathBuf::from("/custom/path"));
    }
}
