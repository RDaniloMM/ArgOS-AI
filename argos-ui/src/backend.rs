//! Backend logic for ArgOS desktop UI.
//!
//! Provider preset definitions, config persistence, vault integration, and
//! connectivity testing — all pure Rust, no Tauri dependency.
//!
//! Ported from the original Tauri backend (argos-ui/src-tauri/src/lib.rs).

use std::path::{Path, PathBuf};

use argos_core::{ArgosError, Config, ProviderConfig};
use argos_provider::ollama::{OllamaConfig, OllamaProvider, ReqwestHttpClient};
use argos_provider::{OpenAICompatibleConfig, OpenAICompatibleProvider, Provider as ArgosProvider};
use argos_security::SecretVault;

const DEFAULT_REUSE_THRESHOLD: f64 = 0.82;

/// A provider preset displayed as a card in the UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_endpoint: String,
    pub default_model: String,
    pub icon: String,
}

/// User-editable provider state sent from the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub preset_id: String,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

/// Result of a connectivity test.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub connected: bool,
    pub message: String,
}

pub fn argos_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .ok_or_else(|| "could not determine home directory".to_string())
        .map(|h| h.join(".argos"))
}

fn config_path_from(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

fn read_config(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("failed to read config: {e}"))?;
    toml::from_str(&text).map_err(|e| format!("failed to parse config: {e}"))
}

fn write_config(path: &Path, config: &Config) -> Result<(), String> {
    let text =
        toml::to_string_pretty(config).map_err(|e| format!("failed to serialize config: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("failed to write config: {e}"))
}

fn api_key_ref(preset_id: &str) -> String {
    format!("provider/{preset_id}/api_key")
}

fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/chat/completions")
        .unwrap_or(trimmed)
        .to_string()
}

fn default_config(input: &ProviderInput) -> Config {
    Config {
        n8n: None,
        provider: ProviderConfig {
            backend: input.preset_id.clone(),
            model: input.model.clone(),
            endpoint: Some(normalize_endpoint(&input.endpoint)).filter(|s| !s.is_empty()),
            api_key_ref: Some(api_key_ref(&input.preset_id)),
        },
        embedder: Default::default(),
        storage: Default::default(),
        reuse_threshold: DEFAULT_REUSE_THRESHOLD,
    }
}

/// Return the seven built-in provider presets.
pub fn provider_presets() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            id: "openai".into(),
            name: "OpenAI".into(),
            description: "Official OpenAI API (GPT-4o, GPT-4o-mini, etc.).".into(),
            default_endpoint: "https://api.openai.com/v1".into(),
            default_model: "gpt-4o".into(),
            icon: "openai".into(),
        },
        ProviderPreset {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            description: "Claude models via the Anthropic Messages API.".into(),
            default_endpoint: "https://api.anthropic.com/v1".into(),
            default_model: "claude-sonnet-4-20250514".into(),
            icon: "anthropic".into(),
        },
        ProviderPreset {
            id: "google".into(),
            name: "Google Gemini".into(),
            description: "Gemini family through Google AI Studio.".into(),
            default_endpoint: "https://generativelanguage.googleapis.com/v1beta".into(),
            default_model: "gemini-1.5-flash".into(),
            icon: "google".into(),
        },
        ProviderPreset {
            id: "ollama".into(),
            name: "Ollama".into(),
            description: "Self-hosted models running on your machine.".into(),
            default_endpoint: "http://localhost:11434".into(),
            default_model: "llama3.2".into(),
            icon: "ollama".into(),
        },
        ProviderPreset {
            id: "opencode".into(),
            name: "OpenCode Go".into(),
            description: "OpenCode Go subscription with OpenAI-compatible endpoints.".into(),
            default_endpoint: "https://opencode.ai/zen/go/v1".into(),
            default_model: "deepseek-v4-flash".into(),
            icon: "opencode".into(),
        },
        ProviderPreset {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            description: "DeepSeek Chat and reasoner models.".into(),
            default_endpoint: "https://api.deepseek.com/v1".into(),
            default_model: "deepseek-chat".into(),
            icon: "deepseek".into(),
        },
        ProviderPreset {
            id: "custom".into(),
            name: "Custom".into(),
            description: "Any OpenAI-compatible endpoint you configure manually.".into(),
            default_endpoint: "".into(),
            default_model: "".into(),
            icon: "custom".into(),
        },
    ]
}

/// Read the currently saved provider, resolving the API key from the vault.
pub async fn get_current_provider(
    config_dir: &Path,
    vault: &dyn SecretVault,
) -> Result<Option<ProviderInput>, String> {
    let path = config_path_from(config_dir);
    if !path.exists() {
        return Ok(None);
    }
    let config = read_config(&path)?;
    let api_key = match config.provider.api_key_ref {
        Some(ref key_ref) => match vault.retrieve(key_ref).await {
            Ok(key) => key,
            Err(ArgosError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        },
        None => String::new(),
    };
    Ok(Some(ProviderInput {
        preset_id: config.provider.backend,
        api_key,
        endpoint: config.provider.endpoint.unwrap_or_default(),
        model: config.provider.model,
    }))
}

/// Save provider config to `.argos/config.toml` and store the API key in the vault.
pub async fn save_provider(
    config_dir: &Path,
    vault: &mut dyn SecretVault,
    input: &ProviderInput,
) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    let key_ref = api_key_ref(&input.preset_id);
    vault
        .store(&key_ref, &input.api_key)
        .await
        .map_err(|e| e.to_string())?;
    let config = default_config(input);
    write_config(&config_path_from(config_dir), &config)
}

/// Test connectivity to the configured provider.
pub async fn test_provider(input: &ProviderInput) -> Result<ProviderStatus, String> {
    let endpoint = url::Url::parse(&normalize_endpoint(&input.endpoint))
        .map_err(|e| format!("invalid endpoint: {e}"))?;

    if input.preset_id == "ollama" {
        let config = OllamaConfig {
            endpoint,
            model: input.model.clone(),
            embed_model: None,
        };
        let provider = OllamaProvider::new(config, ReqwestHttpClient::default());
        match provider.health_check().await {
            Ok(()) => Ok(ProviderStatus {
                connected: true,
                message: "Ollama is reachable".into(),
            }),
            Err(e) => Ok(ProviderStatus {
                connected: false,
                message: e.to_string(),
            }),
        }
    } else {
        let config = OpenAICompatibleConfig {
            endpoint,
            api_key: input.api_key.clone(),
            model: input.model.clone(),
            embed_model: None,
        };
        let provider = OpenAICompatibleProvider::new(config);
        match provider.health_check().await {
            Ok(()) => Ok(ProviderStatus {
                connected: true,
                message: "Provider is reachable".into(),
            }),
            Err(e) => Ok(ProviderStatus {
                connected: false,
                message: e.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (ported from the original Tauri backend)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use argos_security::MemoryVault;

    fn temp_argos_dir() -> (PathBuf, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".argos");
        (dir, tmp)
    }

    fn sample_input(preset_id: &str) -> ProviderInput {
        ProviderInput {
            preset_id: preset_id.into(),
            api_key: format!("{preset_id}-key"),
            endpoint: format!("https://api.{preset_id}.test/v1"),
            model: "test-model".into(),
        }
    }

    #[test]
    fn get_provider_presets_returns_seven() {
        let presets = provider_presets();
        assert_eq!(presets.len(), 7);
        let ids: Vec<String> = presets.iter().map(|p| p.id.clone()).collect();
        for expected in [
            "openai",
            "anthropic",
            "google",
            "ollama",
            "opencode",
            "deepseek",
            "custom",
        ] {
            assert!(
                ids.contains(&expected.to_string()),
                "missing preset {expected}"
            );
        }
    }

    #[test]
    fn provider_input_deserializes_camel_case() {
        let json = serde_json::json!({
            "presetId": "opencode",
            "apiKey": "sk-test",
            "endpoint": "https://opencode.ai/zen/go/v1",
            "model": "deepseek-v4-flash"
        });

        let input: ProviderInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.preset_id, "opencode");
        assert_eq!(input.api_key, "sk-test");
    }

    #[test]
    fn provider_preset_serializes_camel_case() {
        let preset = provider_presets()
            .into_iter()
            .find(|p| p.id == "opencode")
            .unwrap();
        let json = serde_json::to_value(preset).unwrap();

        assert!(json.get("defaultEndpoint").is_some());
        assert!(json.get("defaultModel").is_some());
        assert!(json.get("default_endpoint").is_none());
    }

    #[test]
    fn normalize_endpoint_accepts_full_chat_completions_url() {
        assert_eq!(
            normalize_endpoint("https://opencode.ai/zen/go/v1/chat/completions"),
            "https://opencode.ai/zen/go/v1"
        );
        assert_eq!(
            normalize_endpoint("https://opencode.ai/zen/go/v1/chat/completions/"),
            "https://opencode.ai/zen/go/v1"
        );
    }

    #[tokio::test]
    async fn save_provider_writes_config() {
        let (dir, _tmp) = temp_argos_dir();
        let mut vault = MemoryVault::new();
        let input = sample_input("openai");
        save_provider(&dir, &mut vault, &input).await.unwrap();

        let config = read_config(&config_path_from(&dir)).unwrap();
        assert_eq!(config.provider.backend, "openai");
        assert_eq!(config.provider.model, "test-model");
        assert_eq!(
            config.provider.endpoint,
            Some("https://api.openai.test/v1".into())
        );
        assert_eq!(
            config.provider.api_key_ref,
            Some("provider/openai/api_key".into())
        );
    }

    #[tokio::test]
    async fn save_provider_stores_secret() {
        let (dir, _tmp) = temp_argos_dir();
        let mut vault = MemoryVault::new();
        let input = sample_input("anthropic");
        save_provider(&dir, &mut vault, &input).await.unwrap();

        let secret = vault.retrieve("provider/anthropic/api_key").await.unwrap();
        assert_eq!(secret, "anthropic-key");
    }

    #[tokio::test]
    async fn get_current_provider_reads_config() {
        let (dir, _tmp) = temp_argos_dir();
        let mut vault = MemoryVault::new();
        let input = sample_input("deepseek");
        save_provider(&dir, &mut vault, &input).await.unwrap();

        let current = get_current_provider(&dir, &vault).await.unwrap();
        assert!(current.is_some());
        let current = current.unwrap();
        assert_eq!(current.preset_id, "deepseek");
        assert_eq!(current.api_key, "deepseek-key");
        assert_eq!(current.model, "test-model");
    }

    #[tokio::test]
    #[ignore = "requires a live OpenCode Go API key"]
    async fn test_provider_with_opencode() {
        let input = ProviderInput {
            preset_id: "opencode".into(),
            api_key: std::env::var("OPENCODE_API_KEY").unwrap_or_default(),
            endpoint: "https://opencode.ai/zen/go/v1".into(),
            model: "deepseek-v4-flash".into(),
        };
        if input.api_key.is_empty() {
            panic!("set OPENCODE_API_KEY to run this integration test");
        }
        let status = test_provider(&input).await.unwrap();
        assert!(status.connected, "{}", status.message);
    }
}
