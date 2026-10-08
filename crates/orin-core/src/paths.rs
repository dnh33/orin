//! Platform paths: data dir, config dir, socket, log file, lock file — with env overrides.

use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, ToFsName, ToNsName};
use std::path::PathBuf;

#[cfg(not(windows))]
use nix::unistd::Uid;

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
        #[cfg(windows)]
        {
            return name.to_ns_name::<GenericNamespaced>().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
        }
        #[cfg(not(windows))]
        {
            return name.to_fs_name::<GenericFilePath>().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
        }
    }
    if cfg!(windows) {
        let user = whoami::username();
        let hash = std::process::id() % 10000;
        let pipe_name = format!(r"\\.\pipe\orin-{}-{:04}", user, hash);
        pipe_name.to_ns_name::<GenericNamespaced>().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    } else {
        #[cfg(not(windows))]
        {
            let uid = Uid::current().as_raw();
            let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
            let sock_path = format!("{}/orin-{}.sock", runtime, uid);
            sock_path.to_fs_name::<GenericFilePath>().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        }
        #[cfg(windows)]
        {
            // Windows branch already handled above, unreachable
            Err(std::io::Error::new(std::io::ErrorKind::Other, "unreachable"))
        }
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