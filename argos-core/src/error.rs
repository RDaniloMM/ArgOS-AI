//! Error types for the ArgOS platform.
//!
//! A single error enum covers all subsystems. Each variant carries enough
//! context to diagnose the failure without leaking secrets.

use thiserror::Error;

/// Top-level error type returned by every ArgOS subsystem.
#[derive(Debug, Error)]
pub enum ArgosError {
    /// A storage-layer failure (SQLite, file I/O, blob store, vector store).
    #[error("storage error: {0}")]
    Storage(String),

    /// An LLM provider error (network, API key, rate limit, model timeout).
    #[error("provider error: {0}")]
    Provider(String),

    /// An n8n connection error (MCP handshake, REST timeout, unreachable host).
    #[error("n8n connection error: {0}")]
    N8nConnection(String),

    /// A knowledge/Wiki operation error (parse failure, missing bundle, invalid frontmatter).
    #[error("knowledge/wiki error: {0}")]
    Knowledge(String),

    /// A security violation (permission denied, audit hash mismatch, vault access failure).
    #[error("security error: {0}")]
    Security(String),

    /// A configuration error (missing required key, invalid profile, broken toolchain).
    #[error("configuration error: {0}")]
    Config(String),

    /// A resource was not found (concept, workflow, tool, blob).
    #[error("resource not found: {0}")]
    NotFound(String),

    /// A permission-gated operation was attempted without sufficient rights.
    #[error("permission denied: {0}")]
    Permission(String),

    /// A filesystem or network I/O error.
    #[error("I/O error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ArgosError {
    fn from(err: std::io::Error) -> Self {
        ArgosError::Io(err.to_string())
    }
}

/// Convenience type alias used across all ArgOS crates.
pub type Result<T> = std::result::Result<T, ArgosError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_variant_displays() {
        let err = ArgosError::Storage("disk full".into());
        assert_eq!(err.to_string(), "storage error: disk full");
    }

    #[test]
    fn provider_variant_displays() {
        let err = ArgosError::Provider("timeout".into());
        assert_eq!(err.to_string(), "provider error: timeout");
    }

    #[test]
    fn n8n_connection_variant_displays() {
        let err = ArgosError::N8nConnection("refused".into());
        assert_eq!(err.to_string(), "n8n connection error: refused");
    }

    #[test]
    fn knowledge_variant_displays() {
        let err = ArgosError::Knowledge("concept missing".into());
        assert_eq!(err.to_string(), "knowledge/wiki error: concept missing");
    }

    #[test]
    fn security_variant_displays() {
        let err = ArgosError::Security("sandbox escape".into());
        assert_eq!(err.to_string(), "security error: sandbox escape");
    }

    #[test]
    fn config_variant_displays() {
        let err = ArgosError::Config("missing field".into());
        assert_eq!(err.to_string(), "configuration error: missing field");
    }

    #[test]
    fn not_found_variant_displays() {
        let err = ArgosError::NotFound("workflow 42".into());
        assert_eq!(err.to_string(), "resource not found: workflow 42");
    }

    #[test]
    fn permission_variant_displays() {
        let err = ArgosError::Permission("write denied".into());
        assert_eq!(err.to_string(), "permission denied: write denied");
    }

    #[test]
    fn io_variant_displays() {
        let err = ArgosError::Io("file locked".into());
        assert_eq!(err.to_string(), "I/O error: file locked");
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let argos_err: ArgosError = io_err.into();
        assert!(matches!(argos_err, ArgosError::Io(_)));
    }
}
