//! Backend logic for ArgOS desktop UI.
//!
//! Provider presets, config persistence, vault integration, assistant
//! execution, and n8n connectivity for the native desktop workspace.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use argos_agent::{Agent, GenericAgent, ToolRegistry};
use argos_core::{ArgosError, Config, N8nConnection, N8nRunRef, N8nWorkflowRef, ProviderConfig};
use argos_n8n_connector::{N8nConnector, ReqwestN8nClient};
use argos_provider::ollama::{OllamaConfig, OllamaProvider, ReqwestHttpClient};
use argos_provider::{OpenAICompatibleConfig, OpenAICompatibleProvider, Provider as ArgosProvider};
use argos_security::{MemoryVault, SecretVault};
use async_trait::async_trait;

const DEFAULT_REUSE_THRESHOLD: f64 = 0.82;
const DESKTOP_KEYRING_SERVICE: &str = "argos-ui";

/// A provider preset displayed in the desktop UI.
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

/// User-editable provider state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub preset_id: String,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

/// Result of a provider connectivity test.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub connected: bool,
    pub message: String,
}

/// UI-friendly tool invocation detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantToolEvent {
    pub name: String,
    pub args: String,
    pub result: String,
}

/// UI-friendly assistant response payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponse {
    pub text: String,
    pub final_state: String,
    pub provider_backend: String,
    pub model: String,
    pub tool_invocations: Vec<AssistantToolEvent>,
}

/// Desktop vault selection.
pub enum DesktopVault {
    Keyring(argos_security::KeyringVault),
    Memory(MemoryVault),
}

#[async_trait]
impl SecretVault for DesktopVault {
    async fn store(&mut self, key: &str, secret: &str) -> argos_core::Result<()> {
        match self {
            DesktopVault::Keyring(vault) => vault.store(key, secret).await,
            DesktopVault::Memory(vault) => vault.store(key, secret).await,
        }
    }

    async fn retrieve(&self, key: &str) -> argos_core::Result<String> {
        match self {
            DesktopVault::Keyring(vault) => vault.retrieve(key).await,
            DesktopVault::Memory(vault) => vault.retrieve(key).await,
        }
    }

    async fn delete(&mut self, key: &str) -> argos_core::Result<()> {
        match self {
            DesktopVault::Keyring(vault) => vault.delete(key).await,
            DesktopVault::Memory(vault) => vault.delete(key).await,
        }
    }

    async fn list(&self) -> argos_core::Result<Vec<String>> {
        match self {
            DesktopVault::Keyring(vault) => vault.list().await,
            DesktopVault::Memory(vault) => vault.list().await,
        }
    }
}

pub fn desktop_vault() -> DesktopVault {
    {
        return DesktopVault::Keyring(argos_security::KeyringVault::new(DESKTOP_KEYRING_SERVICE));
    }

    #[allow(unreachable_code)]
    DesktopVault::Memory(MemoryVault::new())
}

pub fn desktop_vault_name() -> &'static str {
    {
        return "KeyringVault";
    }

    #[allow(unreachable_code)]
    "MemoryVault"
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

pub fn load_config(config_dir: &Path) -> Result<Option<Config>, String> {
    let path = config_path_from(config_dir);
    if !path.exists() {
        return Ok(None);
    }

    read_config(&path).map(Some)
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
    let provider = ProviderConfig {
        backend: input.preset_id.clone(),
        model: input.model.clone(),
        endpoint: Some(normalize_endpoint(&input.endpoint)).filter(|s| !s.is_empty()),
        api_key_ref: Some(api_key_ref(&input.preset_id)),
    };

    Config {
        n8n: None,
        provider: provider.clone(),
        providers: vec![provider],
        embedder: Default::default(),
        storage: Default::default(),
        reuse_threshold: DEFAULT_REUSE_THRESHOLD,
    }
}

fn provider_config_from_input(input: &ProviderInput) -> ProviderConfig {
    ProviderConfig {
        backend: input.preset_id.clone(),
        model: input.model.clone(),
        endpoint: Some(normalize_endpoint(&input.endpoint)).filter(|s| !s.is_empty()),
        api_key_ref: Some(api_key_ref(&input.preset_id)),
    }
}

/// Return the built-in provider presets.
pub fn provider_presets() -> Vec<ProviderPreset> {
    vec![
        ProviderPreset {
            id: "openai".into(),
            name: "OpenAI".into(),
            description: "Official OpenAI API (GPT-4o, GPT-4.1, and related models).".into(),
            default_endpoint: "https://api.openai.com/v1".into(),
            default_model: "".into(),
            icon: "openai".into(),
        },
        ProviderPreset {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            description: "Claude models via the Anthropic Messages API.".into(),
            default_endpoint: "https://api.anthropic.com/v1".into(),
            default_model: "".into(),
            icon: "anthropic".into(),
        },
        ProviderPreset {
            id: "google".into(),
            name: "Google Gemini".into(),
            description: "Gemini family through Google AI Studio.".into(),
            default_endpoint: "https://generativelanguage.googleapis.com/v1beta".into(),
            default_model: "".into(),
            icon: "google".into(),
        },
        ProviderPreset {
            id: "ollama".into(),
            name: "Ollama".into(),
            description: "Self-hosted models running on your machine.".into(),
            default_endpoint: "http://localhost:11434".into(),
            default_model: "".into(),
            icon: "ollama".into(),
        },
        ProviderPreset {
            id: "opencode".into(),
            name: "OpenCode Go".into(),
            description: "OpenCode Go subscription with OpenAI-compatible endpoints.".into(),
            default_endpoint: "https://opencode.ai/zen/go/v1".into(),
            default_model: "".into(),
            icon: "opencode".into(),
        },
        ProviderPreset {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            description: "DeepSeek Chat and reasoner models.".into(),
            default_endpoint: "https://api.deepseek.com/v1".into(),
            default_model: "".into(),
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
            Err(ArgosError::Security(_)) => return Ok(None),
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

pub async fn load_current_provider(config_dir: &Path) -> Result<Option<ProviderInput>, String> {
    let vault = desktop_vault();
    get_current_provider(config_dir, &vault).await
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

    let path = config_path_from(config_dir);
    let mut config = if path.exists() {
        read_config(&path)?
    } else {
        default_config(input)
    };
    config.provider = provider_config_from_input(input);
    if let Some(existing) = config
        .providers
        .iter_mut()
        .find(|provider| provider.backend == config.provider.backend)
    {
        *existing = config.provider.clone();
    } else {
        config.providers.push(config.provider.clone());
    }

    write_config(&path, &config)
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

fn provider_from_input(input: &ProviderInput) -> Result<Arc<dyn ArgosProvider>, String> {
    let endpoint = url::Url::parse(&normalize_endpoint(&input.endpoint))
        .map_err(|e| format!("invalid provider endpoint: {e}"))?;

    if input.preset_id == "ollama" {
        Ok(Arc::new(OllamaProvider::new(
            OllamaConfig {
                endpoint,
                model: input.model.clone(),
                embed_model: None,
            },
            ReqwestHttpClient::default(),
        )))
    } else {
        Ok(Arc::new(OpenAICompatibleProvider::new(
            OpenAICompatibleConfig {
                endpoint,
                api_key: input.api_key.clone(),
                model: input.model.clone(),
                embed_model: None,
            },
        )))
    }
}

pub async fn run_assistant(config_dir: &Path, prompt: &str) -> Result<AssistantResponse, String> {
    let provider_input = load_current_provider(config_dir).await?.ok_or_else(|| {
        "No provider is configured yet. Open Provider and save a model first.".to_string()
    })?;

    let provider_backend = provider_input.preset_id.clone();
    let model = provider_input.model.clone();
    let provider = provider_from_input(&provider_input)?;
    let tools = Arc::new(ToolRegistry::new());
    let mut agent = GenericAgent::new("argos-ui-assistant", provider, tools);
    let output = agent.run(prompt).await.map_err(|e| e.to_string())?;

    let tool_invocations = output
        .tool_invocations
        .into_iter()
        .map(|invocation| AssistantToolEvent {
            name: invocation.tool.name,
            args: invocation.args,
            result: match invocation.result {
                argos_core::ToolResult::Ok(value) => value,
                argos_core::ToolResult::Err(error) => format!("Error: {error}"),
            },
        })
        .collect();

    Ok(AssistantResponse {
        text: output.text,
        final_state: format!("{:?}", output.final_state),
        provider_backend,
        model,
        tool_invocations,
    })
}

async fn build_n8n_connector(
    config_dir: &Path,
) -> Result<Option<(N8nConnection, N8nConnector)>, String> {
    let Some(config) = load_config(config_dir)? else {
        return Ok(None);
    };
    let Some(connection) = config.n8n else {
        return Ok(None);
    };

    let api_key = if let Some(ref key_ref) = connection.api_key_ref {
        let vault = desktop_vault();
        match vault.retrieve(key_ref).await {
            Ok(secret) => Some(secret),
            Err(ArgosError::NotFound(_)) | Err(ArgosError::Security(_)) => None,
            Err(err) => return Err(err.to_string()),
        }
    } else {
        None
    };

    let client = ReqwestN8nClient::new(connection.endpoint.clone(), api_key);
    let connector = N8nConnector::new(Box::new(client), connection.clone());
    connector.connect().await.map_err(|e| e.to_string())?;

    Ok(Some((connection, connector)))
}

pub async fn list_n8n_workflows(
    config_dir: &Path,
) -> Result<Option<(N8nConnection, Vec<N8nWorkflowRef>)>, String> {
    let Some((connection, connector)) = build_n8n_connector(config_dir).await? else {
        return Ok(None);
    };

    let workflows = connector
        .list_workflows()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some((connection, workflows)))
}

pub async fn run_n8n_workflow(
    config_dir: &Path,
    workflow_id: &str,
    data: Option<&str>,
) -> Result<(N8nConnection, N8nRunRef), String> {
    let Some((connection, connector)) = build_n8n_connector(config_dir).await? else {
        return Err("n8n is not configured in .argos/config.toml.".to_string());
    };

    let run = connector
        .run_workflow(workflow_id, data)
        .await
        .map_err(|e| e.to_string())?;

    Ok((connection, run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{ConnMode, N8nConnection};
    use url::Url;

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
    fn provider_presets_do_not_surface_static_default_models() {
        let presets = provider_presets();

        assert!(presets.iter().all(|preset| preset.default_model.is_empty()));
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
    async fn save_provider_preserves_existing_n8n_settings() {
        let (dir, _tmp) = temp_argos_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let existing = Config {
            n8n: Some(N8nConnection {
                endpoint: Url::parse("http://localhost:5678").unwrap(),
                mode: ConnMode::Rest,
                api_key_ref: Some("n8n/api_key".into()),
            }),
            provider: ProviderConfig {
                backend: "openai".into(),
                model: "old".into(),
                endpoint: Some("https://old.test/v1".into()),
                api_key_ref: Some("provider/openai/api_key".into()),
            },
            providers: Vec::new(),
            embedder: Default::default(),
            storage: Default::default(),
            reuse_threshold: 0.91,
        };
        write_config(&config_path_from(&dir), &existing).unwrap();

        let mut vault = MemoryVault::new();
        let input = sample_input("anthropic");
        save_provider(&dir, &mut vault, &input).await.unwrap();

        let config = read_config(&config_path_from(&dir)).unwrap();
        let n8n = config.n8n.expect("n8n config should be preserved");
        assert_eq!(n8n.endpoint, Url::parse("http://localhost:5678").unwrap());
        assert_eq!(n8n.api_key_ref.as_deref(), Some("n8n/api_key"));
        assert_eq!(config.reuse_threshold, 0.91);
        assert_eq!(config.provider.backend, "anthropic");
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

    #[test]
    fn load_config_returns_none_without_file() {
        let (dir, _tmp) = temp_argos_dir();
        let config = load_config(&dir).unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn desktop_vault_reports_expected_backend() {
        assert_eq!(desktop_vault_name(), "KeyringVault");
    }
}
