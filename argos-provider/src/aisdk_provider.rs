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
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInputMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<usize>,
    temperature: f64,
    stream: bool,
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
        }
    }

    /// Determine which wire API to use (Responses vs Chat Completions).
    fn api_style(&self) -> ApiStyle {
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
    async fn complete_responses(
        &self,
        prompt: &str,
        options: &CompletionOptions,
    ) -> Result<Completion> {
        let mut input = Vec::new();

        if let Some(sys) = &options.system_prompt {
            input.push(ResponsesInputMessage {
                role: "system".into(),
                content: sys.clone(),
            });
        }

        input.push(ResponsesInputMessage {
            role: "user".into(),
            content: prompt.to_string(),
        });

        let req = ResponsesRequest {
            model: self.model.clone(),
            input,
            max_output_tokens: options.max_tokens,
            temperature: options.temperature,
            stream: false,
        };

        let body = serde_json::to_string(&req).map_err(|e| {
            ArgosError::Provider(format!("failed to serialize responses request: {e}"))
        })?;

        let url = responses_url(&self.endpoint);
        let resp = self
            .reqwest
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", &self.inner.settings.api_key),
            )
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("request to {url} failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| {
            ArgosError::Provider(format!("failed to read response from {url}: {e}"))
        })?;

        if !status.is_success() {
            return Err(ArgosError::Provider(format!(
                "API POST {url} returned {status}: {text}"
            )));
        }

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
