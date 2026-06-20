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

use crate::state::{StatusLevel, WorkflowItem};

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
}

#[derive(Debug)]
pub struct PromptResult {
    pub backend: String,
    pub model: String,
    pub output: AgentOutput,
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
        let Some(config) = load_config(&self.config_dir)? else {
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
                    title: "n8n".into(),
                    detail: "Add an [n8n] section to .argos/config.toml to list and run workflows."
                        .into(),
                    workflows: Vec::new(),
                },
            });
        };

        let provider = provider_snapshot(&config).await;
        let n8n = n8n_snapshot(&config).await;

        Ok(Snapshot { provider, n8n })
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
            title: "n8n".into(),
            detail: "Add an [n8n] section to .argos/config.toml to list and run workflows.".into(),
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
    let fallback = if backend == "ollama" {
        Some("http://localhost:11434")
    } else {
        None
    };

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
    vault.retrieve(key_ref).await.map_err(secret_error)
}

fn secret_error(err: ArgosError) -> String {
    match err {
        ArgosError::NotFound(_) => format!(
            "Secret is missing from the shared keyring service `{}`.",
            SHARED_KEYRING_SERVICE
        ),
        other => other.to_string(),
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

#[cfg(test)]
mod tests {
    use super::{select_n8n_transport, unsupported_mcp_message, N8nTransportPlan};
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
}
