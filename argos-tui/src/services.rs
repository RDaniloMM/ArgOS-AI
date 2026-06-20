use std::path::{Path, PathBuf};
use std::sync::Arc;

use argos_agent::{Agent, AgentOutput, GenericAgent, ToolRegistry};
use argos_core::{ArgosError, Config, ConnMode, N8nConnection, N8nRunRef, ProviderConfig};
use argos_n8n_connector::{N8nConnector, ReqwestN8nClient};
use argos_provider::ollama::{OllamaConfig, OllamaProvider, ReqwestHttpClient};
use argos_provider::{OpenAICompatibleConfig, OpenAICompatibleProvider, Provider as ArgosProvider};
use argos_security::{KeyringVault, SecretVault};
use async_trait::async_trait;
use url::Url;

use crate::state::{ModelInfo, ModelPricing, StatusLevel, WorkflowItem};

const SHARED_KEYRING_SERVICE: &str = "argos-ui";

#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub level: StatusLevel,
    pub title: String,
    pub detail: String,
    pub backend: Option<String>,
    pub model: Option<String>,
    pub vault_name: String,
}

#[derive(Debug, Clone)]
pub struct N8nSnapshot {
    pub level: StatusLevel,
    pub title: String,
    pub detail: String,
    pub workflows: Vec<WorkflowItem>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub provider: ProviderSnapshot,
    pub n8n: N8nSnapshot,
    pub config: Option<Config>,
}

#[derive(Debug)]
pub struct PromptResult {
    pub backend: String,
    pub model: String,
    pub output: AgentOutput,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug)]
pub struct WorkflowRunResult {
    pub mode_label: String,
    pub run: N8nRunRef,
}

#[async_trait]
pub trait AppServices: Send + Sync {
    async fn load_snapshot(&self) -> Result<Snapshot, String>;
    async fn submit_prompt(&self, prompt: String) -> Result<PromptResult, String>;
    async fn run_workflow(&self, workflow_id: String) -> Result<WorkflowRunResult, String>;
    async fn save_config(&self, config: &Config) -> Result<(), String>;
    async fn store_secret(&self, key_ref: &str, secret: &str) -> Result<(), String>;
    async fn delete_secret(&self, key_ref: &str) -> Result<(), String>;
    async fn fetch_models(
        &self,
        backend: &str,
        endpoint: &str,
        api_key_ref: Option<&str>,
    ) -> Result<Vec<crate::state::ModelInfo>, String>;
}

pub struct RealServices {
    config_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum N8nTransportPlan {
    Rest,
}

impl RealServices {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            config_dir: argos_dir()?,
        })
    }
}

#[async_trait]
impl AppServices for RealServices {
    async fn load_snapshot(&self) -> Result<Snapshot, String> {
        let config = load_config(&self.config_dir)?;
        let Some(ref config) = config else {
            return Ok(Snapshot {
                provider: ProviderSnapshot {
                    level: StatusLevel::Missing,
                    title: "Provider".into(),
                    detail: "No .argos/config.toml found yet.".into(),
                    backend: None,
                    model: None,
                    vault_name: SHARED_KEYRING_SERVICE.into(),
                },
                n8n: N8nSnapshot {
                    level: StatusLevel::Missing,
                    title: "Workflows".into(),
                    detail: "Optional. Configure n8n only if you want workflow actions.".into(),
                    workflows: Vec::new(),
                },
                config: None,
            });
        };

        let provider = provider_snapshot(config).await;
        let n8n = n8n_snapshot(config).await;

        Ok(Snapshot {
            provider,
            n8n,
            config: Some(config.clone()),
        })
    }

    async fn submit_prompt(&self, prompt: String) -> Result<PromptResult, String> {
        let config = load_config(&self.config_dir)?
            .ok_or_else(|| "No .argos/config.toml found yet.".to_string())?;

        let backend = config.provider.backend.clone();
        let model = config.provider.model.clone();
        let provider = build_provider_from_config(&config.provider).await?;
        let tools = Arc::new(ToolRegistry::new());
        let mut agent = GenericAgent::new("argos-tui-agent", provider, tools);
        let output = agent.run(&prompt).await.map_err(|err| err.to_string())?;

        Ok(PromptResult {
            backend,
            model,
            prompt_tokens: output.prompt_tokens,
            completion_tokens: output.completion_tokens,
            output,
        })
    }

    async fn run_workflow(&self, workflow_id: String) -> Result<WorkflowRunResult, String> {
        let config = load_config(&self.config_dir)?
            .ok_or_else(|| "No .argos/config.toml found yet.".to_string())?;
        let connection = config
            .n8n
            .ok_or_else(|| "n8n is not configured in .argos/config.toml.".to_string())?;
        let mode_label = mode_label(&connection).to_string();
        let connector = build_n8n_connector(connection).await?;
        let run = connector
            .run_workflow(&workflow_id, None)
            .await
            .map_err(|err| err.to_string())?;

        Ok(WorkflowRunResult { mode_label, run })
    }

    async fn save_config(&self, config: &Config) -> Result<(), String> {
        save_config(&self.config_dir, config)
    }

    async fn store_secret(&self, key_ref: &str, secret: &str) -> Result<(), String> {
        let mut vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
        vault
            .store(key_ref, secret)
            .await
            .map_err(|err| err.to_string())
    }

    async fn delete_secret(&self, key_ref: &str) -> Result<(), String> {
        let mut vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
        vault.delete(key_ref).await.map_err(|err| err.to_string())
    }

    async fn fetch_models(
        &self,
        backend: &str,
        endpoint: &str,
        api_key_ref: Option<&str>,
    ) -> Result<Vec<ModelInfo>, String> {
        let backend = backend.trim().to_lowercase();
        let url = if backend == "ollama" {
            format!("{}/api/tags", endpoint.trim_end_matches('/'))
        } else {
            format!("{}/models", endpoint.trim_end_matches('/'))
        };
        let client = reqwest::Client::new();
        let mut req = client.get(&url);

        if backend != "ollama" {
            if backend == "openrouter" {
                if let Some(key_ref) = api_key_ref {
                    let api_key = retrieve_secret(Some(key_ref)).await?;
                    req = req.header("Authorization", format!("Bearer {api_key}"));
                }
            } else {
                let key_ref = api_key_ref.ok_or_else(|| {
                    format!("provider `{backend}` needs an api_key_ref before fetching models")
                })?;
                let api_key = retrieve_secret(Some(key_ref)).await?;
                if backend == "anthropic" {
                    req = req
                        .header("x-api-key", api_key)
                        .header("anthropic-version", "2023-06-01");
                } else {
                    req = req.header("Authorization", format!("Bearer {api_key}"));
                }
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("fetch models failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("models endpoint returned {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("failed to read models response: {e}"))?;

        parse_model_list(&body)
    }
}

pub fn argos_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .ok_or_else(|| "could not determine home directory".to_string())
        .map(|home| home.join(".argos"))
}

fn config_path_from(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

fn load_config(config_dir: &Path) -> Result<Option<Config>, String> {
    let path = config_path_from(config_dir);
    if !path.exists() {
        return Ok(None);
    }

    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    toml::from_str(&text)
        .map(Some)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn save_config(config_dir: &Path, config: &Config) -> Result<(), String> {
    let dir = config_dir;
    std::fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create config dir {}: {err}", dir.display()))?;
    let path = config_path_from(config_dir);
    let text = toml::to_string_pretty(config)
        .map_err(|err| format!("failed to serialize config: {err}"))?;
    std::fs::write(&path, text).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

async fn provider_snapshot(config: &Config) -> ProviderSnapshot {
    let backend = Some(config.provider.backend.clone());
    let model = Some(config.provider.model.clone());
    match build_provider_from_config(&config.provider).await {
        Ok(provider) => match provider.health_check().await {
            Ok(()) => ProviderSnapshot {
                level: StatusLevel::Success,
                title: provider_title(&config.provider),
                detail: "Connected".into(),
                backend,
                model,
                vault_name: SHARED_KEYRING_SERVICE.into(),
            },
            Err(err) => ProviderSnapshot {
                level: StatusLevel::Error,
                title: provider_title(&config.provider),
                detail: err.to_string(),
                backend,
                model,
                vault_name: SHARED_KEYRING_SERVICE.into(),
            },
        },
        Err(err) => ProviderSnapshot {
            level: StatusLevel::Error,
            title: provider_title(&config.provider),
            detail: err,
            backend,
            model,
            vault_name: SHARED_KEYRING_SERVICE.into(),
        },
    }
}

async fn n8n_snapshot(config: &Config) -> N8nSnapshot {
    let Some(connection) = config.n8n.clone() else {
        return N8nSnapshot {
            level: StatusLevel::Missing,
            title: "Workflows".into(),
            detail: "Optional. Configure n8n only if you want workflow actions.".into(),
            workflows: Vec::new(),
        };
    };

    match build_n8n_connector(connection.clone()).await {
        Ok(connector) => match connector.list_workflows().await {
            Ok(workflows) => N8nSnapshot {
                level: StatusLevel::Success,
                title: format!("n8n ({})", mode_label(&connection)),
                detail: format!("{} workflows available.", workflows.len()),
                workflows: workflows
                    .into_iter()
                    .map(|workflow| WorkflowItem {
                        id: workflow.id,
                        name: workflow.name,
                    })
                    .collect(),
            },
            Err(err) => N8nSnapshot {
                level: StatusLevel::Error,
                title: format!("n8n ({})", mode_label(&connection)),
                detail: err.to_string(),
                workflows: Vec::new(),
            },
        },
        Err(err) => N8nSnapshot {
            level: StatusLevel::Error,
            title: format!("n8n ({})", mode_label(&connection)),
            detail: err,
            workflows: Vec::new(),
        },
    }
}

async fn build_provider_from_config(
    config: &ProviderConfig,
) -> Result<Arc<dyn ArgosProvider>, String> {
    let backend = config.backend.trim().to_lowercase();
    let endpoint = provider_endpoint(config, &backend)?;

    if backend == "ollama" {
        return Ok(Arc::new(OllamaProvider::new(
            OllamaConfig {
                endpoint,
                model: config.model.clone(),
                embed_model: None,
            },
            ReqwestHttpClient::default(),
        )));
    }

    let api_key = retrieve_secret(config.api_key_ref.as_deref()).await?;
    Ok(Arc::new(OpenAICompatibleProvider::new(
        OpenAICompatibleConfig {
            endpoint,
            api_key,
            model: config.model.clone(),
            embed_model: None,
        },
    )))
}

fn provider_endpoint(config: &ProviderConfig, backend: &str) -> Result<Url, String> {
    let fallback = crate::commands::known_provider(backend).and_then(|kp| kp.default_endpoint);

    let raw = config
        .endpoint
        .as_deref()
        .or(fallback)
        .ok_or_else(|| format!("provider `{}` is missing an endpoint.", config.backend))?;
    Url::parse(raw).map_err(|err| format!("invalid provider endpoint `{raw}`: {err}"))
}

async fn build_n8n_connector(connection: N8nConnection) -> Result<N8nConnector, String> {
    select_n8n_transport(&connection)?;
    let api_key = match connection.api_key_ref.as_deref() {
        Some(key_ref) => Some(retrieve_secret(Some(key_ref)).await?),
        None => None,
    };
    let client = ReqwestN8nClient::new(connection.endpoint.clone(), api_key);
    let connector = N8nConnector::new(Box::new(client), connection);
    connector.connect().await.map_err(|err| err.to_string())?;
    Ok(connector)
}

fn select_n8n_transport(connection: &N8nConnection) -> Result<N8nTransportPlan, String> {
    match connection.mode {
        ConnMode::Rest => Ok(N8nTransportPlan::Rest),
        ConnMode::Mcp => Err(unsupported_mcp_message(connection)),
    }
}

fn unsupported_mcp_message(connection: &N8nConnection) -> String {
    format!(
        "n8n MCP mode is configured for {} but argos-tui cannot compose the MCP transport in this slice. Set n8n.mode = \"rest\" to list or run workflows from the TUI.",
        connection.endpoint
    )
}

async fn retrieve_secret(secret_ref: Option<&str>) -> Result<String, String> {
    let key_ref = secret_ref.ok_or_else(|| "missing api_key_ref in config.".to_string())?;
    let vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
    vault
        .retrieve(key_ref)
        .await
        .map_err(|err| secret_error(key_ref, err))
}

fn secret_error(key_ref: &str, err: ArgosError) -> String {
    let msg = err.to_string();
    if msg.contains("No matching entry") || msg.contains("NoEntry") {
        format!(
            "API key `{key_ref}` not found in Windows Credential Manager. Store it with `/vault set {key_ref} <your-key>` in the composer, then `/refresh`."
        )
    } else {
        msg
    }
}

fn provider_title(config: &ProviderConfig) -> String {
    format!("{} / {}", config.backend, config.model)
}

fn mode_label(connection: &N8nConnection) -> &'static str {
    match connection.mode {
        ConnMode::Mcp => "MCP-configured",
        ConnMode::Rest => "REST",
    }
}

fn parse_model_list(body: &str) -> Result<Vec<ModelInfo>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("failed to parse models JSON: {e}"))?;

    let models = parsed["data"]
        .as_array()
        .or_else(|| parsed["models"].as_array())
        .ok_or_else(|| "unexpected models response format".to_string())?;

    let names: Vec<ModelInfo> = models
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str().or_else(|| m["name"].as_str())?;
            let pricing = parse_pricing(m);
            Some(ModelInfo {
                id: id.to_string(),
                pricing,
            })
        })
        .collect();

    if names.is_empty() {
        Err("no models found in response".to_string())
    } else {
        Ok(names)
    }
}

fn parse_pricing(m: &serde_json::Value) -> Option<ModelPricing> {
    let p = m.get("pricing")?;
    let input_str = p.get("prompt")?.as_str()?;
    let output_str = p.get("completion")?.as_str()?;
    Some(ModelPricing {
        input_per_mtok: input_str.parse::<f64>().ok()? * 1_000_000.0,
        output_per_mtok: output_str.parse::<f64>().ok()? * 1_000_000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_model_list, select_n8n_transport, unsupported_mcp_message, N8nTransportPlan,
    };
    use argos_core::{ConnMode, N8nConnection};
    use url::Url;

    fn connection(mode: ConnMode) -> N8nConnection {
        N8nConnection {
            endpoint: Url::parse("http://localhost:5678").unwrap(),
            mode,
            api_key_ref: Some("n8n_key".into()),
        }
    }

    #[test]
    fn rest_mode_selects_rest_transport() {
        assert_eq!(
            select_n8n_transport(&connection(ConnMode::Rest)).unwrap(),
            N8nTransportPlan::Rest
        );
    }

    #[test]
    fn mcp_mode_returns_explicit_unsupported_error() {
        let conn = connection(ConnMode::Mcp);
        let err = select_n8n_transport(&conn).unwrap_err();

        assert_eq!(err, unsupported_mcp_message(&conn));
        assert!(err.contains("cannot compose the MCP transport"));
        assert!(err.contains("n8n.mode = \"rest\""));
    }

    #[test]
    fn parse_model_list_preserves_native_slash_model_ids() {
        let models = parse_model_list(
            r#"{
                "models": [
                    {"name": "models/gemini-2.5-flash"},
                    {"id": "models/gemini-2.5-pro"}
                ]
            }"#,
        )
        .unwrap();

        let ids: Vec<String> = models.into_iter().map(|model| model.id).collect();
        assert_eq!(
            ids,
            vec!["models/gemini-2.5-flash", "models/gemini-2.5-pro"]
        );
    }
}
