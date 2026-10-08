//! Error types shared across orin crates.

use thiserror::Error;

/// Core error type.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Snapshot error: {0}")]
    Snapshot(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Interprocess error: {0}")]
    Interprocess(#[from] interprocess::Error),

    #[error("Other: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = Error::Other("test".into());
        assert_eq!(format!("{}", e), "Other: test");
    }
}
