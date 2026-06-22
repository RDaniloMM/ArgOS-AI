//! Configuration domain types.
//!
//! The top-level `Config` struct is deserialised from `.argos/config.toml`.
//! It carries the n8n connection, provider settings, configured provider list,
//! storage profile, and
//! tunable parameters like the reuse similarity threshold.

use crate::n8n::N8nConnection;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The deployment/storage profile.
///
/// Solo = embedded SQLite + sqlite-vec + filesystem (zero daemons).
/// Team = server backends (Postgres + Qdrant + Neo4j) — future.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageProfile {
    /// Local-first embedded storage (SQLite + sqlite-vec + FS).
    #[default]
    Solo,
    /// Server-based storage for teams (Postgres + Qdrant + Neo4j).
    Team,
}

/// LLM provider configuration.
///
/// ArgOS is provider-agnostic: switching providers must not require code
/// changes. This struct holds the connection details for one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// The backend type (e.g. `ollama`, `openai`, `anthropic`).
    pub backend: String,
    /// The model name (e.g. `llama3.2`, `gpt-4o`, `claude-sonnet-4-20250514`).
    pub model: String,
    /// The API endpoint URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Reference to the API key in the SecretVault (never the raw key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
    /// Authentication mode for the provider. Missing legacy values default to API key auth.
    #[serde(default, skip_serializing_if = "ProviderAuthMethod::is_api_key")]
    pub auth_method: ProviderAuthMethod,
    /// Reference to OpenAI OAuth token JSON in the SecretVault (never raw tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_token_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthMethod {
    #[default]
    ApiKey,
    #[serde(rename = "openai_oauth")]
    OpenAiOAuth,
    #[serde(rename = "codex")]
    Codex,
}

impl ProviderAuthMethod {
    /// Returns true when the auth method is the default (ApiKey), so serde can
    /// skip serializing it for backward-compatible TOML output.
    ///
    /// NOTE: Codex is deliberately excluded — it is NOT the default and must be
    /// serialized so it survives a save/load round-trip. Without this, a Codex
    /// provider would be reloaded as ApiKey with no api_key_ref, causing
    /// "missing api_key_ref in config".
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::ApiKey)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiOAuthToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
}

/// Embedding model configuration for workflow intelligence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedderConfig {
    /// The embedding model name (default: `nomic-embed-text`).
    pub model: String,
    /// The vector dimension (locked at `argos init`, changing requires reindex).
    pub dimension: usize,
    /// The provider backend for embeddings (usually same as ProviderConfig).
    pub backend: String,
    /// Optional endpoint override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            model: "nomic-embed-text".into(),
            dimension: 768,
            backend: "ollama".into(),
            endpoint: None,
        }
    }
}

/// Top-level ArgOS configuration, deserialised from `config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Connection to the n8n instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n8n: Option<N8nConnection>,
    /// The active LLM provider for completions.
    pub provider: ProviderConfig,
    /// Providers explicitly configured by the user for TUI selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderConfig>,
    /// The embedding model for workflow intelligence.
    #[serde(default)]
    pub embedder: EmbedderConfig,
    /// The storage profile (Solo or Team).
    #[serde(default)]
    pub storage: StorageProfile,
    /// Similarity threshold for reuse recommendation (0.0–1.0, default 0.82).
    #[serde(default = "default_reuse_threshold")]
    pub reuse_threshold: f64,
}

fn default_reuse_threshold() -> f64 {
    0.82
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::n8n::ConnMode;
    use url::Url;

    #[test]
    fn config_deserializes_from_toml() {
        let toml_str = r#"
reuse_threshold = 0.85

[provider]
backend = "ollama"
model = "llama3.2"

[embedder]
model = "nomic-embed-text"
dimension = 768
backend = "ollama"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.provider.backend, "ollama");
        assert_eq!(config.provider.model, "llama3.2");
        assert!(config.providers.is_empty());
        assert_eq!(config.embedder.model, "nomic-embed-text");
        assert_eq!(config.embedder.dimension, 768);
        assert_eq!(config.storage, StorageProfile::Solo);
        assert!((config.reuse_threshold - 0.85).abs() < 1e-9);
    }

    #[test]
    fn config_with_n8n_deserializes_from_toml() {
        let toml_str = r#"
[n8n]
endpoint = "http://localhost:5678"
mode = "mcp"
api_key_ref = "n8n_key"

[provider]
backend = "ollama"
model = "llama3.2"

reuse_threshold = 0.82
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let n8n = config.n8n.unwrap();
        assert_eq!(n8n.mode, ConnMode::Mcp);
        assert_eq!(n8n.endpoint, Url::parse("http://localhost:5678").unwrap());
    }

    #[test]
    fn config_defaults_reuse_threshold_to_082() {
        let toml_str = r#"
[provider]
backend = "ollama"
model = "llama3.2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.reuse_threshold - 0.82).abs() < 1e-9);
    }

    #[test]
    fn config_defaults_storage_to_solo() {
        let toml_str = r#"
[provider]
backend = "ollama"
model = "llama3.2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage, StorageProfile::Solo);
    }

    #[test]
    fn config_defaults_embedder_to_nomic() {
        let toml_str = r#"
[provider]
backend = "ollama"
model = "llama3.2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedder.model, "nomic-embed-text");
        assert_eq!(config.embedder.dimension, 768);
    }

    #[test]
    fn config_deserializes_explicit_provider_entries() {
        let toml_str = r#"
[provider]
backend = "openrouter"
model = "openai/gpt-oss-20b:free"
endpoint = "https://openrouter.ai/api/v1"
api_key_ref = "provider/openrouter/api_key"

[[providers]]
backend = "openrouter"
model = "openai/gpt-oss-20b:free"
endpoint = "https://openrouter.ai/api/v1"
api_key_ref = "provider/openrouter/api_key"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0], config.provider);
    }

    #[test]
    fn legacy_api_key_provider_defaults_to_api_key_auth() {
        let toml_str = r#"
[provider]
backend = "openai"
model = "gpt-4.1"
api_key_ref = "provider/openai/api_key"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.provider.auth_method, ProviderAuthMethod::ApiKey);
        assert_eq!(
            config.provider.api_key_ref.as_deref(),
            Some("provider/openai/api_key")
        );
        assert!(config.provider.oauth_token_ref.is_none());
    }

    #[test]
    fn openai_oauth_provider_uses_token_ref_without_api_key_ref() {
        let toml_str = r#"
[provider]
backend = "openai"
model = "gpt-4.1"
auth_method = "openai_oauth"
oauth_token_ref = "provider/openai/oauth"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.provider.auth_method, ProviderAuthMethod::OpenAiOAuth);
        assert_eq!(
            config.provider.oauth_token_ref.as_deref(),
            Some("provider/openai/oauth")
        );
        assert!(config.provider.api_key_ref.is_none());
    }

    #[test]
    fn storage_profile_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&StorageProfile::Solo).unwrap(),
            "\"solo\""
        );
        assert_eq!(
            serde_json::to_string(&StorageProfile::Team).unwrap(),
            "\"team\""
        );
    }
}
