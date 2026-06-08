//! Error and result types shared across the core engine.

use thiserror::Error;

/// Convenience alias for results produced by the core engine.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that the core engine can produce.
#[derive(Debug, Error)]
pub enum Error {
    /// An underlying I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A bridge line (or other input) could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),

    /// A network/source fetch failure.
    #[error("network error: {0}")]
    Network(String),

    /// JSON (de)serialization failure.
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// A persistence/database failure.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    /// Any other error with a human-readable message.
    #[error("{0}")]
    Other(String),
}
