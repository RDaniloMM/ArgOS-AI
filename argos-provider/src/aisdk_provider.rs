//! Aisdk-based provider wrapping [`aisdk::providers::OpenAICompatible<DynamicModel>`].
//!
//! Both OpenAI-compatible and Ollama endpoints share the same builder, differing
//! only in `base_url` and `api_key` (empty for Ollama). The Responses API uses a
//! manual reqwest code path because aisdk's body shape does not match.

use aisdk::core::language_model::request::LanguageModelRequest;
use aisdk::core::DynamicModel;
use aisdk::core::EmbeddingModelRequest;
use aisdk::providers::OpenAICompatible;
use argos_core::{ArgosError, Embedding, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

/// Endpoint for Codex OAuth (ChatGPT Pro/Plus) — the only API endpoint that
/// accepts OpenAI OAuth tokens. Standard `https://api.openai.com/v1` rejects
/// OAuth JWTs from auth.openai.com.
const CODEX_API_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";

/// Extract the `chatgpt_account_id` from an OpenAI OAuth JWT.
///
/// The JWT payload contains a custom claim at `https://api.openai.com/auth`
/// with a `chatgpt_account_id` field. Returns `None` if the token is not a
/// valid JWT or the claim is absent.
fn extract_chatgpt_account_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    // Decode the base64url-encoded payload (no signature verification needed).
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let auth = payload.pointer("/https://api.openai.com/auth")?;
    Some(auth.get("chatgpt_account_id")?.as_str()?.to_string())
}

use crate::provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};

/// Distinguishes supported provider backends for capability advertisement
/// and configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderBackend {
    /// Local Ollama instance.
    Ollama,
    /// OpenAI-compatible API (OpenAI, OpenRouter, etc.).
    OpenAI,
}

/// Adapter wrapping [`OpenAICompatible<DynamicModel>`] behind the [`Provider`] trait.
///
/// Construction
/// ------------
/// - [`AisdkProvider::new_openai`] — configure with an API key (OpenAI, OpenRouter, etc.)
/// - [`AisdkProvider::new_ollama`] — configure without an API key (local Ollama)
pub struct AisdkProvider {
    /// The underlying aisdk provider constructed via [`OpenAICompatible::builder`].
    inner: OpenAICompatible<DynamicModel>,
    /// Which backend this instance targets.
    backend: ProviderBackend,
    /// Reusable reqwest client for health check and Responses API calls.
    reqwest: reqwest::Client,
    /// Base URL of the API endpoint.
    endpoint: Url,
    /// Model name used for completions and embeddings.
    model: String,
    /// When true, force Responses API via Codex endpoint with OAuth-specific
    /// headers. Set by [`Self::new_openai_codex`].
    is_codex_oauth: bool,
}

// ---------------------------------------------------------------------------
// Heuristic helpers (copied from openai_compatible.rs patterns)
// ---------------------------------------------------------------------------

/// Determine whether a model name targets the Responses API.
fn is_openai_gpt_responses_model(model: &str) -> bool {
    let model = model.trim();
    model.starts_with("gpt-5")
        || model.starts_with("gpt-4.1")
        || model.starts_with("o3")
        || model.starts_with("o4")
}

/// Which wire API to use for completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiStyle {
    ChatCompletions,
    Responses,
}

/// Determine the API style based on endpoint URL and model name.
fn api_style(endpoint: &Url, model: &str) -> ApiStyle {
    let base = endpoint.as_str().trim_end_matches('/');

    if base.ends_with("/responses") {
        return ApiStyle::Responses;
    }
    if base.ends_with("/chat/completions") {
        return ApiStyle::ChatCompletions;
    }
    // OpenCode Zen base endpoint + GPT model should use Responses.
    if base.contains("opencode.ai/zen/v1") && !base.contains("/zen/go/") {
        if is_openai_gpt_responses_model(model) {
            return ApiStyle::Responses;
        }
    }
    ApiStyle::ChatCompletions
}

fn base_str(endpoint: &Url) -> String {
    endpoint.as_str().trim_end_matches('/').to_string()
}

fn models_url(endpoint: &Url) -> String {
    let b = base_str(endpoint);
    if b.ends_with("/chat/completions") {
        b.trim_end_matches("/chat/completions").to_string() + "/models"
    } else if b.ends_with("/responses") {
        b.trim_end_matches("/responses").to_string() + "/models"
    } else {
        format!("{b}/models")
    }
}

fn responses_url(endpoint: &Url) -> String {
    let b = base_str(endpoint);
    if b.ends_with("/responses") {
        b
    } else if b.ends_with("/chat/completions") {
        b.trim_end_matches("/chat/completions").to_string() + "/responses"
    } else {
        format!("{b}/responses")
    }
}

// ---------------------------------------------------------------------------
// Responses API request/response types (manual reqwest code path)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(untagged)]
enum InputValue {
    Messages(Vec<ResponsesInputMessage>),
}

#[derive(Serialize)]
struct ResponsesRequest {
    model: String,
    input: InputValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    /// Codex endpoint requires `stream: true`; standard Responses API omits it.
    #[serde(skip_serializing_if = "is_false")]
    stream: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Serialize)]
struct ResponsesInputMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
struct ResponsesOutputItem {
    #[serde(default)]
    content: Vec<ResponsesContentItem>,
}

#[derive(Deserialize)]
struct ResponsesContentItem {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

impl AisdkProvider {
    /// Create a new provider for an OpenAI-compatible endpoint.
    ///
    /// `endpoint` should be the base URL (e.g. `https://api.openai.com/v1`).
    /// `api_key` is the bearer token for Authorization.
    /// `model` is the model name (e.g. `gpt-4o`).
    pub fn new_openai(endpoint: Url, api_key: String, model: String) -> Self {
        let inner = OpenAICompatible::<DynamicModel>::builder()
            .base_url(endpoint.as_str())
            .api_key(&api_key)
            .model_name(&model)
            .build()
            .expect("valid OpenAICompatible builder configuration");
        Self {
            inner,
            backend: ProviderBackend::OpenAI,
            reqwest: reqwest::Client::new(),
            endpoint,
            model,
            is_codex_oauth: false,
        }
    }

    /// Create a new provider for a local Ollama instance.
    ///
    /// `endpoint` should be the Ollama server URL (e.g. `http://localhost:11434/v1`).
    /// `model` is the model name (e.g. `llama3`).
    ///
    /// Note: aisdk's builder requires a non-empty api_key, so we use a known
    /// placeholder. Ollama ignores unexpected auth headers for local requests.
    pub fn new_ollama(endpoint: Url, model: String) -> Self {
        let inner = OpenAICompatible::<DynamicModel>::builder()
            .base_url(endpoint.as_str())
            .api_key("ollama-placeholder")
            .model_name(&model)
            .build()
            .expect("valid OpenAICompatible builder configuration");
        Self {
            inner,
            backend: ProviderBackend::Ollama,
            reqwest: reqwest::Client::new(),
            endpoint,
            model,
            is_codex_oauth: false,
        }
    }

    /// Create a new provider for the Codex OAuth endpoint (ChatGPT Pro/Plus).
    ///
    /// This is identical to [`Self::new_openai`] except `is_codex_oauth` is set
    /// to `true`, which forces the Responses API code path with Codex-specific
    /// headers and endpoint override.
    pub fn new_openai_codex(endpoint: Url, api_key: String, model: String) -> Self {
        let mut p = Self::new_openai(endpoint, api_key, model);
        p.is_codex_oauth = true;
        p
    }

    /// Determine which wire API to use (Responses vs Chat Completions).
    fn api_style(&self) -> ApiStyle {
        if self.is_codex_oauth {
            return ApiStyle::Responses;
        }
        api_style(&self.endpoint, &self.model)
    }

    /// Chat Completions path using aisdk's `generate_text()`.
    async fn complete_chat_completions(
        &self,
        prompt: &str,
        options: &CompletionOptions,
    ) -> Result<Completion> {
        // OpenCode-style: pass system prompt through aisdk's native .system()
        // mechanism. DynamicModel implements TextInputSupport (auto-generated
        // by model_capabilities! macro), so ConversationStage::prompt() works.
        let mut builder = LanguageModelRequest::builder()
            .model(self.inner.clone())
            .system(options.system_prompt.clone().unwrap_or_default())
            .prompt(prompt)
            .temperature((options.temperature * 100.0) as u32);
        builder.max_output_tokens = options.max_tokens.map(|t| t as u32);

        let mut request = builder.build();
        let response = request
            .generate_text()
            .await
            .map_err(|e| ArgosError::Provider(format!("generate_text failed: {e}")))?;

        let text = response.text().unwrap_or_default();
        let usage = response.usage();
        let finish_reason = match response.stop_reason() {
            Some(aisdk::core::language_model::StopReason::Finish) => FinishReason::Stop,
            Some(aisdk::core::language_model::StopReason::Error(_)) => FinishReason::Error,
            _ => FinishReason::Stop,
        };

        Ok(Completion {
            text,
            usage: TokenUsage {
                prompt_tokens: usage.input_tokens.unwrap_or(0) as u32,
                completion_tokens: usage.output_tokens.unwrap_or(0) as u32,
            },
            finish_reason,
        })
    }

    /// Responses API path using manual reqwest POST (body shape mismatch with aisdk).
    ///
    /// For the Codex OAuth path (`is_codex_oauth`), this handles SSE streaming
    /// because the Codex endpoint requires `stream: true`.
    async fn complete_responses(
        &self,
        prompt: &str,
        options: &CompletionOptions,
    ) -> Result<Completion> {
        // Both Codex and standard Responses use array input format.
        let mut messages = Vec::new();
        if !self.is_codex_oauth {
            if let Some(sys) = &options.system_prompt {
                messages.push(ResponsesInputMessage {
                    role: "system".into(),
                    content: sys.clone(),
                });
            }
        }
        messages.push(ResponsesInputMessage {
            role: "user".into(),
            content: prompt.to_string(),
        });

        let instructions = if self.is_codex_oauth {
            // Codex endpoint requires instructions to be present and non-empty.
            Some(
                options
                    .system_prompt
                    .clone()
                    .unwrap_or_else(|| "You are a helpful assistant.".into()),
            )
        } else {
            None
        };

        let req = ResponsesRequest {
            model: self.model.clone(),
            input: InputValue::Messages(messages),
            instructions,
            store: if self.is_codex_oauth { Some(false) } else { None },
            // Codex gpt-5.5 rejects max_output_tokens as unsupported.
            max_output_tokens: if self.is_codex_oauth {
                None
            } else {
                options.max_tokens
            },
            temperature: if self.is_codex_oauth {
                None
            } else {
                Some(options.temperature)
            },
            stream: self.is_codex_oauth,
        };

        let body = serde_json::to_string(&req).map_err(|e| {
            ArgosError::Provider(format!("failed to serialize responses request: {e}"))
        })?;

        let url = if self.is_codex_oauth {
            CODEX_API_ENDPOINT.to_string()
        } else {
            responses_url(&self.endpoint)
        };

        let mut req_builder = self
            .reqwest
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", &self.inner.settings.api_key),
            )
            .header("Content-Type", "application/json");

        if self.is_codex_oauth {
            req_builder = req_builder
                .header("OpenAI-Beta", "responses=experimental")
                .header("originator", "codex_cli_rs")
                .header("session_id", uuid::Uuid::new_v4().to_string());

            // Extract chatgpt-account-id from the JWT payload if possible.
            if let Some(account_id) = extract_chatgpt_account_id(&self.inner.settings.api_key) {
                req_builder = req_builder.header("ChatGPT-Account-Id", account_id);
            }
        }

        let resp = req_builder
            .body(body)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("request to {url} failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "(no body)".into());
            return Err(ArgosError::Provider(format!(
                "API POST {url} returned {status}: {text}"
            )));
        }

        if self.is_codex_oauth {
            // Codex returns SSE streaming — read and accumulate text deltas.
            Self::read_codex_stream(resp).await
        } else {
            // Standard Responses API returns a single JSON body.
            let text = resp.text().await.map_err(|e| {
                ArgosError::Provider(format!("failed to read response from {url}: {e}"))
            })?;

            let response: ResponsesResponse = serde_json::from_str(&text).map_err(|e| {
                ArgosError::Provider(format!(
                    "failed to parse responses response: {e}; body: {text}"
                ))
            })?;

            let content = response.output_text.unwrap_or_else(|| {
                response
                    .output
                    .iter()
                    .flat_map(|item| item.content.iter())
                    .filter_map(|content| content.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("")
            });

            let usage = response.usage.unwrap_or(ResponsesUsage {
                input_tokens: 0,
                output_tokens: 0,
            });

            Ok(Completion {
                text: content,
                usage: TokenUsage {
                    prompt_tokens: usage.input_tokens,
                    completion_tokens: usage.output_tokens,
                },
                finish_reason: FinishReason::Stop,
            })
        }
    }

    /// Read a Codex SSE streaming response and accumulate the output text.
    ///
    /// The Codex endpoint sends [Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html)
    /// with `data: {...}` lines. We parse each event to extract delta/text fields
    /// until the terminal `data: [DONE]` event.
    async fn read_codex_stream(mut resp: reqwest::Response) -> Result<Completion> {
        let mut output = String::new();
        let mut buffer: Vec<u8> = Vec::new();

        loop {
            let chunk = match resp.chunk().await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => break, // stream closed without [DONE] — still valid
                Err(e) => {
                    return Err(ArgosError::Provider(format!(
                        "failed to read codex stream chunk: {e}"
                    )));
                }
            };

            buffer.extend_from_slice(&chunk);

            // Process complete SSE events (separated by \n\n)
            loop {
                // Find next \n\n boundary
                let event_end = match buffer
                    .windows(2)
                    .position(|w| w == b"\n\n")
                {
                    Some(pos) => pos,
                    None => break, // wait for more data
                };

                let event_bytes = buffer[..event_end].to_vec();
                buffer = buffer[event_end + 2..].to_vec();

                let event_str = String::from_utf8_lossy(&event_bytes);

                // Parse each "data: ..." line in the event
                for line in event_str.lines() {
                    let line = line.trim();
                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            return Ok(Completion {
                                text: output,
                                usage: TokenUsage {
                                    prompt_tokens: 0,
                                    completion_tokens: 0,
                                },
                                finish_reason: FinishReason::Stop,
                            });
                        }

                        // Try to extract text from the SSE JSON payload
                        if let Ok(json) =
                            serde_json::from_str::<serde_json::Value>(data)
                        {
                            // "delta" — streaming text delta events
                            if let Some(delta) =
                                json.get("delta").and_then(|v| v.as_str())
                            {
                                output.push_str(delta);
                            }
                            // "output_text" — done/final events
                            if let Some(text) =
                                json.get("output_text").and_then(|v| v.as_str())
                            {
                                output.push_str(text);
                            }
                            // "content" array (from response.done events)
                            if let Some(content) =
                                json.get("content").and_then(|v| v.as_array())
                            {
                                for item in content {
                                    if let Some(text) =
                                        item.get("text").and_then(|v| v.as_str())
                                    {
                                        output.push_str(text);
                                    }
                                }
                            }
                            // "output"[].content[].text (from response.done events)
                            if let Some(output_arr) =
                                json.get("output").and_then(|v| v.as_array())
                            {
                                for item in output_arr {
                                    if let Some(content) = item
                                        .get("content")
                                        .and_then(|v| v.as_array())
                                    {
                                        for c in content {
                                            if let Some(text) =
                                                c.get("text").and_then(|v| v.as_str())
                                            {
                                                output.push_str(text);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Completion {
            text: output,
            usage: TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
            },
            finish_reason: FinishReason::Stop,
        })
    }
}

// ---------------------------------------------------------------------------
// Provider trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for AisdkProvider {
    async fn complete(&self, prompt: &str, options: &CompletionOptions) -> Result<Completion> {
        match self.api_style() {
            ApiStyle::Responses => self.complete_responses(prompt, options).await,
            ApiStyle::ChatCompletions => self.complete_chat_completions(prompt, options).await,
        }
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
        let request = EmbeddingModelRequest::builder()
            .model(self.inner.clone())
            .input(vec![text.to_string()])
            .build();
        let mut response = request
            .embed()
            .await
            .map_err(|e| ArgosError::Provider(format!("embed failed: {e}")))?;
        // EmbeddingModelResponse is Vec<Vec<f32>>; take first result.
        response
            .pop()
            .ok_or_else(|| ArgosError::Provider("empty embedding response".into()))
    }

    async fn capabilities(&self) -> ProviderCapabilities {
        match self.backend {
            ProviderBackend::Ollama => ProviderCapabilities {
                supports_streaming: true,
                supports_function_calling: false,
                supports_vision: false,
                supports_embeddings: true,
                max_context: 8192,
            },
            ProviderBackend::OpenAI => ProviderCapabilities {
                supports_streaming: true,
                supports_function_calling: true,
                supports_vision: false,
                supports_embeddings: true,
                max_context: 65536,
            },
        }
    }

    async fn health_check(&self) -> Result<()> {
        let url = models_url(&self.endpoint);
        let resp = self
            .reqwest
            .get(&url)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("health check to {url} failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ArgosError::Provider(format!(
                "health check GET {url} returned {}",
                resp.status()
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn test_endpoint() -> Url {
        Url::parse("https://api.openai.com/v1").unwrap()
    }

    fn ollama_endpoint() -> Url {
        Url::parse("http://localhost:11434/v1").unwrap()
    }

    // -----------------------------------------------------------------------
    // ProviderBackend
    // -----------------------------------------------------------------------

    #[test]
    fn provider_backend_has_both_variants() {
        assert_ne!(ProviderBackend::Ollama as u8, ProviderBackend::OpenAI as u8);
    }

    #[test]
    fn provider_backend_debug_and_clone() {
        let backends = [ProviderBackend::Ollama, ProviderBackend::OpenAI];
        for b in &backends {
            let _formatted = format!("{b:?}");
        }
    }

    #[test]
    fn backend_not_equal() {
        assert_ne!(ProviderBackend::Ollama, ProviderBackend::OpenAI);
        assert_eq!(ProviderBackend::Ollama, ProviderBackend::Ollama);
    }

    // -----------------------------------------------------------------------
    // Constructor tests
    // -----------------------------------------------------------------------

    #[test]
    fn new_openai_constructs_with_correct_backend() {
        let p = AisdkProvider::new_openai(test_endpoint(), "sk-test".into(), "gpt-4o".into());
        assert_eq!(p.backend, ProviderBackend::OpenAI);
    }

    #[test]
    fn new_ollama_constructs_with_correct_backend() {
        let p = AisdkProvider::new_ollama(ollama_endpoint(), "llama3".into());
        assert_eq!(p.backend, ProviderBackend::Ollama);
    }

    #[test]
    fn new_openai_stores_model_and_endpoint() {
        let p = AisdkProvider::new_openai(test_endpoint(), "sk-test".into(), "gpt-4o".into());
        assert_eq!(p.model, "gpt-4o");
        assert_eq!(p.endpoint.as_str(), "https://api.openai.com/v1");
    }

    #[test]
    fn new_ollama_stores_model_and_endpoint() {
        let p = AisdkProvider::new_ollama(ollama_endpoint(), "llama3".into());
        assert_eq!(p.model, "llama3");
        assert_eq!(p.endpoint.as_str(), "http://localhost:11434/v1");
    }

    #[test]
    fn new_openai_with_custom_endpoint_and_model() {
        let p = AisdkProvider::new_openai(
            Url::parse("https://openrouter.ai/api/v1").unwrap(),
            "sk-or-v1-abc".into(),
            "anthropic/claude-3.5-sonnet".into(),
        );
        assert_eq!(p.model, "anthropic/claude-3.5-sonnet");
        assert_eq!(p.endpoint.as_str(), "https://openrouter.ai/api/v1");
    }

    #[test]
    fn new_ollama_with_different_model() {
        let p = AisdkProvider::new_ollama(
            Url::parse("http://localhost:11434/v1").unwrap(),
            "mistral".into(),
        );
        assert_eq!(p.model, "mistral");
    }

    // -----------------------------------------------------------------------
    // Heuristic pure-function tests (no I/O needed)
    // -----------------------------------------------------------------------

    #[test]
    fn is_openai_gpt_responses_model_identifies_gpt5() {
        assert!(is_openai_gpt_responses_model("gpt-5.4-mini"));
    }

    #[test]
    fn is_openai_gpt_responses_model_identifies_gpt41() {
        assert!(is_openai_gpt_responses_model("gpt-4.1-nano"));
    }

    #[test]
    fn is_openai_gpt_responses_model_rejects_chat_models() {
        assert!(!is_openai_gpt_responses_model("gpt-4o"));
        assert!(!is_openai_gpt_responses_model("gpt-4-turbo"));
    }

    #[test]
    fn is_openai_gpt_responses_model_identifies_o3_and_o4() {
        assert!(is_openai_gpt_responses_model("o3-mini"));
        assert!(is_openai_gpt_responses_model("o4-2"));
    }

    #[test]
    fn is_openai_gpt_responses_model_handles_whitespace() {
        assert!(is_openai_gpt_responses_model("  gpt-5.4-mini  "));
        assert!(!is_openai_gpt_responses_model("  gpt-4o  "));
    }

    #[test]
    fn api_style_standard_chat_completions_endpoint() {
        let url = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
        assert_eq!(api_style(&url, "gpt-4o"), ApiStyle::ChatCompletions);
    }

    #[test]
    fn api_style_explicit_responses_endpoint() {
        let url = Url::parse("https://api.openai.com/v1/responses").unwrap();
        assert_eq!(api_style(&url, "any-model"), ApiStyle::Responses);
    }

    #[test]
    fn api_style_opencode_zen_with_gpt_model_uses_responses() {
        let url = Url::parse("https://opencode.ai/zen/v1").unwrap();
        assert_eq!(api_style(&url, "gpt-5.4-mini"), ApiStyle::Responses);
    }

    #[test]
    fn api_style_opencode_zen_with_non_gpt_uses_chat() {
        let url = Url::parse("https://opencode.ai/zen/v1").unwrap();
        assert_eq!(api_style(&url, "deepseek-chat"), ApiStyle::ChatCompletions);
    }

    #[test]
    fn api_style_opencode_zen_go_always_chat() {
        let url = Url::parse("https://opencode.ai/zen/go/v1").unwrap();
        assert_eq!(api_style(&url, "gpt-5.4-mini"), ApiStyle::ChatCompletions);
    }

    #[test]
    fn api_style_plain_endpoint_defaults_to_chat() {
        let url = Url::parse("https://api.openai.com/v1").unwrap();
        assert_eq!(api_style(&url, "gpt-4o"), ApiStyle::ChatCompletions);
    }

    // -----------------------------------------------------------------------
    // URL helper tests
    // -----------------------------------------------------------------------

    #[test]
    fn models_url_chat_completions_endpoint() {
        let url = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
        assert_eq!(models_url(&url), "https://api.openai.com/v1/models");
    }

    #[test]
    fn models_url_responses_endpoint() {
        let url = Url::parse("https://api.openai.com/v1/responses").unwrap();
        assert_eq!(models_url(&url), "https://api.openai.com/v1/models");
    }

    #[test]
    fn models_url_plain_endpoint() {
        let url = Url::parse("https://api.openai.com/v1").unwrap();
        assert_eq!(models_url(&url), "https://api.openai.com/v1/models");
    }

    #[test]
    fn responses_url_plain_endpoint() {
        let url = Url::parse("https://api.openai.com/v1").unwrap();
        assert_eq!(responses_url(&url), "https://api.openai.com/v1/responses");
    }

    #[test]
    fn responses_url_from_chat_endpoint() {
        let url = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
        assert_eq!(responses_url(&url), "https://api.openai.com/v1/responses");
    }

    // -----------------------------------------------------------------------
    // capabilities() tests (no I/O needed)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn capabilities_openai_returns_expected_values() {
        let p = AisdkProvider::new_openai(test_endpoint(), "sk-test".into(), "gpt-4o".into());
        let caps = p.capabilities().await;
        assert!(caps.supports_streaming);
        assert!(caps.supports_function_calling);
        assert!(!caps.supports_vision);
        assert!(caps.supports_embeddings);
        assert_eq!(caps.max_context, 65536);
    }

    #[tokio::test]
    async fn capabilities_ollama_returns_expected_values() {
        let p = AisdkProvider::new_ollama(ollama_endpoint(), "llama3".into());
        let caps = p.capabilities().await;
        assert!(caps.supports_streaming);
        assert!(!caps.supports_function_calling);
        assert!(!caps.supports_vision);
        assert!(caps.supports_embeddings);
        assert_eq!(caps.max_context, 8192);
    }

    // -----------------------------------------------------------------------
    // ApiStyle enum
    // -----------------------------------------------------------------------

    #[test]
    fn api_style_variants_are_distinct() {
        assert_ne!(ApiStyle::ChatCompletions as u8, ApiStyle::Responses as u8);
    }
}
