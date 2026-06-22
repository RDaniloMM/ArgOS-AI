use std::path::{Path, PathBuf};
use std::sync::Arc;

use argos_agent::{Agent, AgentOutput, GenericAgent, ToolRegistry};
use argos_core::{
    ArgosError, Config, ConnMode, N8nConnection, N8nRunRef, OpenAiOAuthToken, ProviderAuthMethod,
    ProviderConfig,
};
use argos_n8n_connector::{N8nConnector, ReqwestN8nClient};
use argos_provider::aisdk_provider::AisdkProvider;
use argos_provider::Provider as ArgosProvider;
use argos_security::{KeyringVault, SecretVault};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::state::{ModelInfo, ModelPricing, StatusLevel, WorkflowItem};

const SHARED_KEYRING_SERVICE: &str = "argos-ui";
const OPENAI_OAUTH_ISSUER: &str = "https://auth.openai.com";
const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const DEFAULT_OPENAI_OAUTH_TOKEN_REF: &str = "provider/openai/oauth";
const OAUTH_REFRESH_SKEW_SECONDS: i64 = 300;
const OAUTH_POLL_TIMEOUT_SECONDS: u64 = 900;
const CODEX_REDIRECT_PORT: u16 = 1455;
const CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const CODEX_SCOPE: &str = "openid profile email offline_access";
const CODEX_ORIGINATOR: &str = "argos-ui";
const CODEX_TOKEN_REF: &str = "provider/codex/oauth";
pub const CODEX_API_KEY_REF: &str = "provider/codex/api_key";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiLoginStart {
    pub token_ref: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub interval_seconds: u64,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait AppServices: Send + Sync {
    async fn load_snapshot(&self) -> Result<Snapshot, String>;
    async fn submit_prompt(&self, prompt: String) -> Result<PromptResult, String>;
    async fn run_workflow(&self, workflow_id: String) -> Result<WorkflowRunResult, String>;
    async fn save_config(&self, config: &Config) -> Result<(), String>;
    async fn store_secret(&self, key_ref: &str, secret: &str) -> Result<(), String>;
    async fn delete_secret(&self, key_ref: &str) -> Result<(), String>;
    async fn start_openai_login(&self, token_ref: &str) -> Result<OpenAiLoginStart, String>;
    async fn complete_openai_login(&self, login: OpenAiLoginStart) -> Result<(), String>;
    async fn fetch_models(
        &self,
        backend: &str,
        endpoint: &str,
        api_key_ref: Option<&str>,
        auth_method: ProviderAuthMethod,
        oauth_token_ref: Option<&str>,
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
        validate_config_oauth_refs(config)?;
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

    async fn start_openai_login(&self, token_ref: &str) -> Result<OpenAiLoginStart, String> {
        start_openai_oauth_login(token_ref).await
    }

    async fn complete_openai_login(&self, login: OpenAiLoginStart) -> Result<(), String> {
        complete_openai_oauth_login(login).await
    }

    async fn fetch_models(
        &self,
        backend: &str,
        endpoint: &str,
        api_key_ref: Option<&str>,
        auth_method: ProviderAuthMethod,
        oauth_token_ref: Option<&str>,
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
            match auth_method {
                ProviderAuthMethod::ApiKey if backend == "openrouter" => {
                    if let Some(key_ref) = api_key_ref {
                        let api_key = retrieve_secret(Some(key_ref)).await?;
                        req = req.header("Authorization", format!("Bearer {api_key}"));
                    }
                }
                ProviderAuthMethod::ApiKey => {
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
                ProviderAuthMethod::OpenAiOAuth => {
                    if backend != "openai" {
                        return Err(format!(
                            "OAuth auth is only supported for OpenAI providers; `{backend}` must use API-key auth."
                        ));
                    }
                    let token_ref = oauth_token_ref.ok_or_else(|| {
                        "OpenAI OAuth provider is missing oauth_token_ref. Run `/openai-login` again.".to_string()
                    })?;
                    let access_token = retrieve_openai_oauth_bearer(token_ref).await?;
                    req = req.header("Authorization", format!("Bearer {access_token}"));
                }
                ProviderAuthMethod::Codex => {
                    if let Ok(api_key) = retrieve_secret(Some(CODEX_API_KEY_REF)).await {
                        if !api_key.trim().is_empty() {
                            req = req.header("Authorization", format!("Bearer {api_key}"));
                        }
                    } else {
                        let token_ref = oauth_token_ref.ok_or_else(|| {
                            "Codex provider is missing oauth_token_ref. Run `/codex-login` first.".to_string()
                        })?;
                        let access_token = retrieve_openai_oauth_bearer(token_ref).await?;
                        req = req.header("Authorization", format!("Bearer {access_token}"));
                    }
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
        return Ok(Arc::new(AisdkProvider::new_ollama(
            endpoint,
            config.model.clone(),
        )));
    }

    let api_key = provider_bearer_token(config, &backend).await?;
    Ok(Arc::new(AisdkProvider::new_openai(
        endpoint,
        api_key,
        config.model.clone(),
    )))
}

async fn provider_bearer_token(config: &ProviderConfig, backend: &str) -> Result<String, String> {
    match config.auth_method {
        ProviderAuthMethod::ApiKey => retrieve_secret(config.api_key_ref.as_deref()).await,
        ProviderAuthMethod::OpenAiOAuth => {
            if backend != "openai" {
                return Err(format!(
                    "OAuth auth is only supported for OpenAI providers; `{}` must use API-key auth.",
                    config.backend
                ));
            }
            let token_ref = config.oauth_token_ref.as_deref().ok_or_else(|| {
                "OpenAI OAuth provider is missing oauth_token_ref. Add it with `/provider-add-openai-oauth <model> [token-ref]` or run `/openai-login [token-ref]`.".to_string()
            })?;
            retrieve_openai_oauth_bearer(token_ref).await
        }
        ProviderAuthMethod::Codex => {
            // Prefer the token-exchanged API key stored by `start_codex_login`.
            // Raw ChatGPT OAuth access tokens are not a drop-in replacement for
            // OpenAI-compatible `/chat/completions`, which is what caused 404s.
            if let Ok(api_key) = retrieve_secret(Some(CODEX_API_KEY_REF)).await {
                if !api_key.trim().is_empty() {
                    return Ok(api_key);
                }
            }

            let token_ref = config.oauth_token_ref.as_deref().ok_or_else(|| {
                "Codex provider is missing oauth_token_ref. Run `/codex-login` first.".to_string()
            })?;

            retrieve_openai_oauth_bearer(token_ref).await
        }
    }
}

#[derive(Debug, Deserialize)]
struct DeviceUserCodeResponse {
    #[serde(rename = "device_auth_id", alias = "device_code")]
    device_code: String,
    user_code: String,
    #[serde(default, alias = "verification_url")]
    verification_uri: Option<String>,
    #[serde(default, alias = "verification_url_complete")]
    verification_uri_complete: Option<String>,
    #[serde(default = "default_device_interval_value")]
    interval: Value,
    #[serde(default)]
    expires_in: Option<u64>,
}

const OPENAI_OAUTH_SCOPE: &str = "model.request openid profile email";

#[derive(Debug, Serialize)]
struct DeviceUserCodeRequest<'a> {
    client_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct DeviceTokenRequest<'a> {
    device_auth_id: &'a str,
    user_code: &'a str,
}

#[derive(Debug, Deserialize)]
struct DeviceAuthorizationResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Serialize)]
struct AuthorizationCodeTokenRequest<'a> {
    grant_type: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
    client_id: &'a str,
    code_verifier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct RefreshTokenRequest<'a> {
    client_id: &'a str,
    refresh_token: &'a str,
    grant_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
}

#[derive(Debug)]
enum DevicePollResult {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Failed(String),
    Authorized(DeviceAuthorizationResponse),
}

#[async_trait]
trait OpenAiOAuthClient: Send + Sync {
    async fn request_device_code(&self) -> Result<DeviceUserCodeResponse, String>;
    async fn poll_device_authorization(
        &self,
        device_auth_id: &str,
        user_code: &str,
    ) -> Result<DevicePollResult, String>;
    async fn exchange_authorization_code(
        &self,
        auth: &DeviceAuthorizationResponse,
    ) -> Result<OpenAiOAuthToken, String>;
}

#[async_trait]
trait OAuthTokenVault: Send + Sync {
    async fn store_token_json(&self, token_ref: &str, token_json: &str) -> Result<(), String>;
}

struct ReqwestOpenAiOAuthClient {
    client: reqwest::Client,
}

impl ReqwestOpenAiOAuthClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

struct KeyringOAuthTokenVault;

#[async_trait]
impl OAuthTokenVault for KeyringOAuthTokenVault {
    async fn store_token_json(&self, token_ref: &str, token_json: &str) -> Result<(), String> {
        store_oauth_token_secret(token_ref, token_json).await
    }
}

#[async_trait]
impl OpenAiOAuthClient for ReqwestOpenAiOAuthClient {
    async fn request_device_code(&self) -> Result<DeviceUserCodeResponse, String> {
        let resp = self
            .client
            .post(format!(
                "{OPENAI_OAUTH_ISSUER}/api/accounts/deviceauth/usercode"
            ))
            .json(&DeviceUserCodeRequest {
                client_id: OPENAI_OAUTH_CLIENT_ID,
                scope: Some(OPENAI_OAUTH_SCOPE),
            })
            .send()
            .await
            .map_err(|err| format!("OpenAI OAuth login start failed: {err}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("OpenAI OAuth login start returned {status}."));
        }

        resp.json()
            .await
            .map_err(|err| format!("OpenAI OAuth login start returned unexpected JSON: {err}"))
    }

    async fn poll_device_authorization(
        &self,
        device_auth_id: &str,
        user_code: &str,
    ) -> Result<DevicePollResult, String> {
        let resp = self
            .client
            .post(format!(
                "{OPENAI_OAUTH_ISSUER}/api/accounts/deviceauth/token"
            ))
            .json(&DeviceTokenRequest {
                device_auth_id,
                user_code,
            })
            .send()
            .await
            .map_err(|err| format!("OpenAI OAuth polling failed: {err}"))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|err| format!("OpenAI OAuth polling response read failed: {err}"))?;

        if status.is_success() {
            return authorization_from_device_response(&text).map(DevicePollResult::Authorized);
        }

        if status.as_u16() == 403 || status.as_u16() == 404 {
            return Ok(DevicePollResult::Pending);
        }

        if let Ok(err) = serde_json::from_str::<OAuthErrorResponse>(&text) {
            return Ok(match err.error.as_str() {
                "authorization_pending" => DevicePollResult::Pending,
                "slow_down" => DevicePollResult::SlowDown,
                "access_denied" => DevicePollResult::Denied,
                "expired_token" => DevicePollResult::Expired,
                other => DevicePollResult::Failed(other.to_string()),
            });
        }

        Ok(DevicePollResult::Failed(format!(
            "polling returned {status}"
        )))
    }

    async fn exchange_authorization_code(
        &self,
        auth: &DeviceAuthorizationResponse,
    ) -> Result<OpenAiOAuthToken, String> {
        exchange_openai_authorization_code(&self.client, auth).await
    }
}

fn default_device_interval_value() -> Value {
    Value::from(5)
}

fn parse_device_interval(value: &Value) -> u64 {
    match value {
        Value::Number(number) => number.as_u64().unwrap_or(5).max(1),
        Value::String(text) => text.parse::<u64>().unwrap_or(5).max(1),
        _ => 5,
    }
}

async fn start_openai_oauth_login(token_ref: &str) -> Result<OpenAiLoginStart, String> {
    let token_ref = validate_openai_oauth_ref(&normalize_oauth_token_ref(Some(token_ref)))?;
    let client = ReqwestOpenAiOAuthClient::new();
    start_openai_oauth_login_with(&client, token_ref).await
}

async fn start_openai_oauth_login_with(
    client: &dyn OpenAiOAuthClient,
    token_ref: String,
) -> Result<OpenAiLoginStart, String> {
    let body = client.request_device_code().await?;

    Ok(OpenAiLoginStart {
        token_ref,
        device_code: body.device_code,
        user_code: body.user_code,
        verification_uri: body
            .verification_uri
            .unwrap_or_else(|| format!("{OPENAI_OAUTH_ISSUER}/codex/device")),
        verification_uri_complete: body.verification_uri_complete,
        interval_seconds: parse_device_interval(&body.interval),
        expires_at: Utc::now()
            + Duration::seconds(body.expires_in.unwrap_or(OAUTH_POLL_TIMEOUT_SECONDS) as i64),
    })
}

async fn complete_openai_oauth_login(login: OpenAiLoginStart) -> Result<(), String> {
    let client = ReqwestOpenAiOAuthClient::new();
    let vault = KeyringOAuthTokenVault;
    complete_openai_oauth_login_with(&client, &vault, login).await
}

async fn complete_openai_oauth_login_with(
    client: &dyn OpenAiOAuthClient,
    vault: &dyn OAuthTokenVault,
    login: OpenAiLoginStart,
) -> Result<(), String> {
    let deadline = login
        .expires_at
        .min(Utc::now() + Duration::seconds(OAUTH_POLL_TIMEOUT_SECONDS as i64));

    while Utc::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_secs(
            login.interval_seconds.max(1),
        ))
        .await;
        match client
            .poll_device_authorization(&login.device_code, &login.user_code)
            .await?
        {
            DevicePollResult::Authorized(auth) => {
                let token = client.exchange_authorization_code(&auth).await?;
                vault
                    .store_token_json(
                        &login.token_ref,
                        &serde_json::to_string(&token).map_err(|err| {
                            format!(
                                "failed to serialize OpenAI OAuth token for vault storage: {err}"
                            )
                        })?,
                    )
                    .await?;
                return Ok(());
            }
            DevicePollResult::Pending => continue,
            DevicePollResult::SlowDown => {
                tokio::time::sleep(std::time::Duration::from_secs(login.interval_seconds)).await;
                continue;
            }
            DevicePollResult::Denied => return Err(
                "OpenAI OAuth login was denied. Run `/openai-login` again if this was accidental."
                    .into(),
            ),
            DevicePollResult::Expired => {
                return Err("OpenAI OAuth login expired. Run `/openai-login` again.".into())
            }
            DevicePollResult::Failed(other) => {
                return Err(format!(
                    "OpenAI OAuth login failed with `{other}`. Run `/openai-login` again."
                ))
            }
        }
    }

    Err("OpenAI OAuth login timed out. Run `/openai-login` again.".into())
}

async fn exchange_openai_authorization_code(
    client: &reqwest::Client,
    auth: &DeviceAuthorizationResponse,
) -> Result<OpenAiOAuthToken, String> {
    let redirect_uri = format!("{OPENAI_OAUTH_ISSUER}/deviceauth/callback");
    let resp = client
        .post(format!("{OPENAI_OAUTH_ISSUER}/oauth/token"))
        .form(&AuthorizationCodeTokenRequest {
            grant_type: "authorization_code",
            code: &auth.authorization_code,
            redirect_uri: &redirect_uri,
            client_id: OPENAI_OAUTH_CLIENT_ID,
            code_verifier: &auth.code_verifier,
            scope: Some(OPENAI_OAUTH_SCOPE),
        })
        .send()
        .await
        .map_err(|err| format!("OpenAI OAuth token exchange failed: {err}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|err| format!("OpenAI OAuth token exchange response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "OpenAI OAuth token exchange returned {status}. Run `/openai-login` again."
        ));
    }

    token_from_response(&text, None)
}

fn authorization_from_device_response(text: &str) -> Result<DeviceAuthorizationResponse, String> {
    serde_json::from_str(text)
        .map_err(|err| format!("OpenAI OAuth device authorization returned unexpected JSON: {err}"))
}

async fn retrieve_openai_oauth_bearer(token_ref: &str) -> Result<String, String> {
    let token_json = retrieve_oauth_token_secret(token_ref).await?;
    let token: OpenAiOAuthToken = serde_json::from_str(&token_json).map_err(|err| {
        format!("OpenAI OAuth token `{token_ref}` is not valid token JSON: {err}")
    })?;

    validate_oauth_token(token_ref, &token)?;
    if !oauth_token_needs_refresh(&token, Utc::now()) {
        return Ok(token.access_token);
    }

    let refreshed = refresh_openai_oauth_token(token_ref, &token).await?;
    Ok(refreshed.access_token)
}

fn validate_oauth_token(token_ref: &str, token: &OpenAiOAuthToken) -> Result<(), String> {
    if token.access_token.trim().is_empty() {
        return Err(format!(
            "OpenAI OAuth token `{token_ref}` has an empty access_token. Run `/openai-login {token_ref}` again."
        ));
    }
    if token.refresh_token.trim().is_empty() {
        return Err(format!(
            "OpenAI OAuth token `{token_ref}` has an empty refresh_token. Run `/openai-login {token_ref}` again."
        ));
    }
    Ok(())
}

fn oauth_token_needs_refresh(token: &OpenAiOAuthToken, now: DateTime<Utc>) -> bool {
    token.expires_at <= now + Duration::seconds(OAUTH_REFRESH_SKEW_SECONDS)
}

async fn refresh_openai_oauth_token(
    token_ref: &str,
    token: &OpenAiOAuthToken,
) -> Result<OpenAiOAuthToken, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{OPENAI_OAUTH_ISSUER}/oauth/token"))
        .form(&RefreshTokenRequest {
            client_id: OPENAI_OAUTH_CLIENT_ID,
            refresh_token: &token.refresh_token,
            grant_type: "refresh_token",
            scope: Some(OPENAI_OAUTH_SCOPE),
        })
        .send()
        .await
        .map_err(|err| {
            format!(
                "OpenAI OAuth token refresh failed: {err}. Run `/openai-login {token_ref}` again."
            )
        })?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|err| format!("OpenAI OAuth refresh response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "OpenAI OAuth token refresh returned {status}. Run `/openai-login {token_ref}` again."
        ));
    }

    let refreshed = token_from_response(&text, Some(token.refresh_token.clone()))?;
    let serialized = serde_json::to_string(&refreshed)
        .map_err(|err| format!("failed to serialize refreshed OpenAI OAuth token: {err}"))?;
    store_oauth_token_secret(token_ref, &serialized).await?;
    Ok(refreshed)
}

fn token_from_response(
    text: &str,
    fallback_refresh_token: Option<String>,
) -> Result<OpenAiOAuthToken, String> {
    let response: OAuthTokenResponse = serde_json::from_str(text)
        .map_err(|err| format!("OpenAI OAuth token endpoint returned unexpected JSON: {err}"))?;
    let refresh_token = response
        .refresh_token
        .or(fallback_refresh_token)
        .ok_or_else(|| {
            "OpenAI OAuth token endpoint did not return a refresh token. Run `/openai-login` again."
                .to_string()
        })?;
    let expires_in = response.expires_in.unwrap_or(3600).max(1);
    Ok(OpenAiOAuthToken {
        access_token: response.access_token,
        refresh_token,
        expires_at: Utc::now() + Duration::seconds(expires_in),
        scope: response.scope,
        account_label: None,
    })
}

fn normalize_oauth_token_ref(token_ref: Option<&str>) -> String {
    token_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OPENAI_OAUTH_TOKEN_REF)
        .to_string()
}

pub fn is_valid_openai_oauth_ref(token_ref: &str) -> bool {
    let value = token_ref.trim();
    value == DEFAULT_OPENAI_OAUTH_TOKEN_REF
        || value
            .strip_prefix(DEFAULT_OPENAI_OAUTH_TOKEN_REF)
            .is_some_and(|suffix| suffix.starts_with('/') && suffix.len() > 1)
}

pub fn validate_openai_oauth_ref(token_ref: &str) -> Result<String, String> {
    let value = token_ref.trim();
    if is_valid_openai_oauth_ref(value) {
        Ok(value.to_string())
    } else {
        Err(format!(
            "Invalid OpenAI OAuth token ref `{value}`. Use `provider/openai/oauth` or `provider/openai/oauth/<name>`; API-key and other-provider refs are not allowed."
        ))
    }
}

fn validate_provider_oauth_ref(provider: &ProviderConfig) -> Result<(), String> {
    if provider.auth_method != ProviderAuthMethod::OpenAiOAuth {
        return Ok(());
    }
    if !provider.backend.eq_ignore_ascii_case("openai") {
        return Err(format!(
            "OpenAI OAuth auth is only valid for `openai`; provider `{}` must use API-key auth.",
            provider.backend
        ));
    }
    let token_ref = provider.oauth_token_ref.as_deref().ok_or_else(|| {
        "OpenAI OAuth provider is missing oauth_token_ref. Use `provider/openai/oauth` or `provider/openai/oauth/<name>`.".to_string()
    })?;
    validate_openai_oauth_ref(token_ref).map(|_| ())
}

fn validate_config_oauth_refs(config: &Config) -> Result<(), String> {
    validate_provider_oauth_ref(&config.provider)?;
    for provider in &config.providers {
        validate_provider_oauth_ref(provider)?;
    }
    Ok(())
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

async fn retrieve_oauth_token_secret(token_ref: &str) -> Result<String, String> {
    let vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
    vault
        .retrieve(token_ref)
        .await
        .map_err(|err| oauth_token_error(token_ref, err))
}

async fn store_oauth_token_secret(token_ref: &str, token_json: &str) -> Result<(), String> {
    let mut vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
    vault
        .store(token_ref, token_json)
        .await
        .map_err(|err| format!("failed to store OpenAI OAuth token `{token_ref}` in vault: {err}"))
}

fn oauth_token_error(token_ref: &str, err: ArgosError) -> String {
    let msg = err.to_string();
    if msg.contains("No matching entry") || msg.contains("NoEntry") || msg.contains("not found") {
        format!(
            "OpenAI OAuth token `{token_ref}` not found in Windows Credential Manager. Run `/openai-login {token_ref}`, then `/refresh`."
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

fn open_browser(url: &str) {
    let result = if cfg!(target_os = "windows") {
        // WARNING: cmd.exe interprets `&` as a command separator, which
        // destroys OAuth URLs with query parameters. Use rundll32 to
        // invoke the OS protocol handler directly, bypassing cmd.exe.
        std::process::Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        eprintln!("failed to open browser: {e}");
    }
}

pub async fn start_codex_login() -> Result<(), String> {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use sha2::{Digest, Sha256};

    // 1. Generate PKCE challenge + verifier
    let code_verifier: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(86)
        .map(char::from)
        .collect();

    let hash = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = data_encoding::BASE64URL_NOPAD.encode(&hash);

    let state: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    // 2. Build authorize URL (must match Codex CLI official implementation exactly)
    let auth_url = Url::parse_with_params(
        CODEX_AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ("redirect_uri", CODEX_REDIRECT_URI),
            ("scope", CODEX_SCOPE),
            ("code_challenge", &code_challenge[..]),
            ("code_challenge_method", "S256"),
            ("state", &state[..]),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", CODEX_ORIGINATOR),
        ],
    )
    .map_err(|e| format!("failed to build authorize URL: {e}"))?
    .to_string();

    // 3. Start TCP listener and open browser
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{CODEX_REDIRECT_PORT}"))
        .await
        .map_err(|e| format!("failed to bind to port {CODEX_REDIRECT_PORT}: {e}"))?;

    let _ = open_browser(&auth_url);

    // 4. Wait for OAuth callback (5-minute timeout)
    let (mut stream, _) =
        tokio::time::timeout(std::time::Duration::from_secs(300), listener.accept())
            .await
            .map_err(|_| "Codex login timed out waiting for browser callback (300s)".to_string())?
            .map_err(|e| format!("TCP accept failed: {e}"))?;

    // 5. Parse callback HTTP request
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| format!("failed to read callback request: {e}"))?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let path_with_query = parts
        .get(1)
        .ok_or_else(|| "invalid callback request line".to_string())?;
    let (_path, query) = path_with_query
        .split_once('?')
        .ok_or_else(|| "callback request has no query string".to_string())?;

    let callback_params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();

    let auth_code = callback_params
        .get("code")
        .ok_or_else(|| "callback missing code parameter".to_string())?;
    let received_state = callback_params
        .get("state")
        .ok_or_else(|| "callback missing state parameter".to_string())?;

    if *received_state != state {
        return Err("OAuth state parameter mismatch".to_string());
    }

    // 6. Send response to browser
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 93\r\n\r\n<html><body><h1>Authorization successful!</h1><p>You may close this tab.</p></body></html>";
    stream
        .write_all(response)
        .await
        .map_err(|e| format!("failed to write callback response: {e}"))?;

    // 7. Exchange auth code for tokens
    let client = reqwest::Client::new();
    let resp = client
        .post("https://auth.openai.com/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", OPENAI_OAUTH_CLIENT_ID),
            ("code", auth_code),
            ("code_verifier", &code_verifier),
            ("redirect_uri", CODEX_REDIRECT_URI),
        ])
        .send()
        .await
        .map_err(|e| format!("Codex token exchange failed: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Codex token exchange response read failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("Codex token exchange returned {status}: {text}"));
    }

    #[derive(serde::Deserialize)]
    struct CodexTokenResponse {
        access_token: String,
        #[serde(default)]
        id_token: Option<String>,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        expires_in: Option<i64>,
    }

    let parsed: CodexTokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("Codex token response unexpected JSON: {e}"))?;

    let refresh_token = parsed
        .refresh_token
        .ok_or_else(|| "Codex token response missing refresh_token".to_string())?;
    let expires_in = parsed.expires_in.unwrap_or(3600).max(1);

    let token = OpenAiOAuthToken {
        access_token: parsed.access_token,
        refresh_token,
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(expires_in),
        scope: None,
        account_label: None,
    };

    let token_json = serde_json::to_string(&token)
        .map_err(|e| format!("failed to serialize codex token: {e}"))?;

    // 8. Store OAuth token in vault (for later refresh)
    let mut vault = KeyringVault::new(SHARED_KEYRING_SERVICE);
    vault
        .store(CODEX_TOKEN_REF, &token_json)
        .await
        .map_err(|e| format!("failed to store codex token in vault: {e}"))?;

    // 9. Exchange id_token for an OpenAI API key (like official Codex CLI does).
    //     The resulting API key works with https://api.openai.com/v1 (Chat Completions).
    if let Some(ref id_token) = parsed.id_token {
        let api_key_resp = client
            .post("https://auth.openai.com/oauth/token")
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("client_id", OPENAI_OAUTH_CLIENT_ID),
                ("requested_token", "openai-api-key"),
                ("subject_token", id_token),
                (
                    "subject_token_type",
                    "urn:ietf:params:oauth:token-type:id_token",
                ),
                ("scope", "model.request"),
            ])
            .send()
            .await
            .map_err(|e| format!("Codex API key exchange failed: {e}"))?;

        if api_key_resp.status().is_success() {
            #[derive(serde::Deserialize)]
            struct ApiKeyResponse {
                access_token: String,
            }
            if let Ok(api_key_body) = api_key_resp.json::<ApiKeyResponse>().await {
                vault
                    .store(CODEX_API_KEY_REF, &api_key_body.access_token)
                    .await
                    .map_err(|e| format!("failed to store codex API key in vault: {e}"))?;
            }
        }
        // Non-fatal: if API key exchange fails, we still have the OAuth token
        // (some accounts don't support token-exchange, fall back to OAuth bearer)
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        complete_openai_oauth_login_with, is_valid_openai_oauth_ref, normalize_oauth_token_ref,
        oauth_token_needs_refresh, parse_model_list, provider_bearer_token, select_n8n_transport,
        start_openai_oauth_login_with, token_from_response, unsupported_mcp_message,
        validate_config_oauth_refs, validate_openai_oauth_ref, DeviceAuthorizationResponse,
        DevicePollResult, DeviceUserCodeResponse, N8nTransportPlan, OAuthTokenVault,
        OpenAiOAuthClient,
    };
    use argos_core::{
        Config, ConnMode, EmbedderConfig, N8nConnection, OpenAiOAuthToken, ProviderAuthMethod,
        ProviderConfig, StorageProfile,
    };
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use serde_json::json;
    use std::sync::Mutex;
    use url::Url;

    struct MockOAuthClient {
        device_response: DeviceUserCodeResponse,
        polls: Mutex<Vec<DevicePollResult>>,
        exchanged: Mutex<Vec<(String, String)>>,
        token: OpenAiOAuthToken,
    }

    #[async_trait]
    impl OpenAiOAuthClient for MockOAuthClient {
        async fn request_device_code(&self) -> Result<DeviceUserCodeResponse, String> {
            Ok(DeviceUserCodeResponse {
                device_code: self.device_response.device_code.clone(),
                user_code: self.device_response.user_code.clone(),
                verification_uri: self.device_response.verification_uri.clone(),
                verification_uri_complete: self.device_response.verification_uri_complete.clone(),
                interval: self.device_response.interval.clone(),
                expires_in: self.device_response.expires_in,
            })
        }

        async fn poll_device_authorization(
            &self,
            _device_auth_id: &str,
            _user_code: &str,
        ) -> Result<DevicePollResult, String> {
            Ok(self.polls.lock().unwrap().remove(0))
        }

        async fn exchange_authorization_code(
            &self,
            auth: &DeviceAuthorizationResponse,
        ) -> Result<OpenAiOAuthToken, String> {
            self.exchanged
                .lock()
                .unwrap()
                .push((auth.authorization_code.clone(), auth.code_verifier.clone()));
            Ok(self.token.clone())
        }
    }

    struct MockVault {
        writes: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl OAuthTokenVault for MockVault {
        async fn store_token_json(&self, token_ref: &str, token_json: &str) -> Result<(), String> {
            self.writes
                .lock()
                .unwrap()
                .push((token_ref.to_string(), token_json.to_string()));
            Ok(())
        }
    }

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

    #[tokio::test]
    async fn non_openai_provider_cannot_use_oauth_auth() {
        let provider = ProviderConfig {
            backend: "openrouter".into(),
            model: "openai/gpt-4.1".into(),
            endpoint: Some("https://openrouter.ai/api/v1".into()),
            api_key_ref: None,
            auth_method: ProviderAuthMethod::OpenAiOAuth,
            oauth_token_ref: Some("provider/openrouter/oauth".into()),
        };

        let err = provider_bearer_token(&provider, "openrouter")
            .await
            .unwrap_err();

        assert!(err.contains("only supported for OpenAI"));
        assert!(err.contains("API-key auth"));
    }

    #[tokio::test]
    async fn openai_oauth_missing_ref_returns_actionable_error() {
        let provider = ProviderConfig {
            backend: "openai".into(),
            model: "gpt-4.1".into(),
            endpoint: Some("https://api.openai.com/v1".into()),
            api_key_ref: None,
            auth_method: ProviderAuthMethod::OpenAiOAuth,
            oauth_token_ref: None,
        };

        let err = provider_bearer_token(&provider, "openai")
            .await
            .unwrap_err();

        assert!(err.contains("missing oauth_token_ref"));
        assert!(err.contains("/provider-add-openai-oauth"));
    }

    #[test]
    fn oauth_token_refreshes_when_expired_or_near_expiry() {
        let now = Utc::now();
        let near = OpenAiOAuthToken {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: now + Duration::seconds(120),
            scope: None,
            account_label: None,
        };
        let valid = OpenAiOAuthToken {
            expires_at: now + Duration::seconds(900),
            ..near.clone()
        };

        assert!(oauth_token_needs_refresh(&near, now));
        assert!(!oauth_token_needs_refresh(&valid, now));
    }

    #[test]
    fn token_response_preserves_refresh_token_without_leaking_values() {
        let token = token_from_response(
            r#"{"access_token":"new-access","expires_in":60,"scope":"openid"}"#,
            Some("old-refresh".into()),
        )
        .unwrap();

        assert_eq!(token.access_token, "new-access");
        assert_eq!(token.refresh_token, "old-refresh");
        assert_eq!(token.scope.as_deref(), Some("openid"));
    }

    #[test]
    fn default_oauth_token_ref_is_provider_scoped() {
        assert_eq!(normalize_oauth_token_ref(None), "provider/openai/oauth");
        assert_eq!(
            normalize_oauth_token_ref(Some(" provider/openai/oauth/custom ")),
            "provider/openai/oauth/custom"
        );
    }

    #[test]
    fn openai_oauth_ref_namespace_is_strict() {
        assert!(is_valid_openai_oauth_ref("provider/openai/oauth"));
        assert!(is_valid_openai_oauth_ref("provider/openai/oauth/work"));
        assert!(validate_openai_oauth_ref(" provider/openai/oauth/work ").is_ok());

        for invalid in [
            "provider/openai/api_key",
            "provider/openrouter/oauth",
            "provider/anthropic/oauth",
            "provider/openai/oauthish",
            "provider/openai/oauth/",
        ] {
            let err = validate_openai_oauth_ref(invalid).unwrap_err();
            assert!(err.contains("Invalid OpenAI OAuth token ref"));
        }
    }

    #[test]
    fn config_validation_rejects_oauth_refs_outside_openai_oauth_namespace() {
        let provider = ProviderConfig {
            backend: "openai".into(),
            model: "gpt-4.1".into(),
            endpoint: Some("https://api.openai.com/v1".into()),
            api_key_ref: None,
            auth_method: ProviderAuthMethod::OpenAiOAuth,
            oauth_token_ref: Some("provider/openai/api_key".into()),
        };
        let config = Config {
            provider: provider.clone(),
            providers: vec![provider],
            n8n: None,
            embedder: EmbedderConfig::default(),
            storage: StorageProfile::default(),
            reuse_threshold: 0.82,
        };

        let err = validate_config_oauth_refs(&config).unwrap_err();
        assert!(err.contains("Invalid OpenAI OAuth token ref"));
    }

    #[tokio::test]
    async fn openai_headless_flow_exchanges_authorization_code_and_writes_vault_only() {
        let token = OpenAiOAuthToken {
            access_token: "access-final".into(),
            refresh_token: "refresh-final".into(),
            expires_at: Utc::now() + Duration::seconds(3600),
            scope: Some("openid".into()),
            account_label: None,
        };
        let client = MockOAuthClient {
            device_response: DeviceUserCodeResponse {
                device_code: "device-auth-id".into(),
                user_code: "ABCD-EFGH".into(),
                verification_uri: None,
                verification_uri_complete: None,
                interval: json!("1"),
                expires_in: Some(30),
            },
            polls: Mutex::new(vec![DevicePollResult::Authorized(
                DeviceAuthorizationResponse {
                    authorization_code: "auth-code".into(),
                    code_verifier: "pkce-verifier".into(),
                },
            )]),
            exchanged: Mutex::new(Vec::new()),
            token: token.clone(),
        };
        let vault = MockVault {
            writes: Mutex::new(Vec::new()),
        };

        let login = start_openai_oauth_login_with(&client, "provider/openai/oauth/test".into())
            .await
            .unwrap();
        assert_eq!(login.device_code, "device-auth-id");
        assert_eq!(login.user_code, "ABCD-EFGH");
        assert_eq!(
            login.verification_uri,
            "https://auth.openai.com/codex/device"
        );

        complete_openai_oauth_login_with(&client, &vault, login)
            .await
            .unwrap();

        assert_eq!(
            client.exchanged.lock().unwrap().as_slice(),
            &[("auth-code".into(), "pkce-verifier".into())]
        );
        let writes = vault.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, "provider/openai/oauth/test");
        let stored: OpenAiOAuthToken = serde_json::from_str(&writes[0].1).unwrap();
        assert_eq!(stored.access_token, token.access_token);
        assert_eq!(stored.refresh_token, token.refresh_token);
    }
}
