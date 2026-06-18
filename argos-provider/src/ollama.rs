//! Ollama provider implementation.
//!
//! Connects to a local Ollama instance over its HTTP API (`/api/chat`,
//! `/api/embed`, `/api/tags`). The HTTP transport is abstracted behind the
//! [`HttpClient`] trait so the provider is unit-testable without a running
//! Ollama server: tests inject [`StubHttpClient`], production enables the
//! `reqwest-backend` feature and injects [`ReqwestHttpClient`]. Per-provider
//! quirks stay here and never leak through the [`Provider`] seam (ADR-005).

use argos_core::{ArgosError, Embedding, Result};
use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use crate::provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};

/// HTTP transport seam used by [`OllamaProvider`].
///
/// Abstracting the client keeps the provider network-free under test
/// ([`StubHttpClient`]) and lets the production backend
/// (`reqwest-backend` feature → [`ReqwestHttpClient`]) be swapped without
/// touching provider logic.
#[async_trait]
pub trait HttpClient: Send + Sync {
    /// POST `body` (JSON) to `url` and return the response body as a string.
    async fn post_json(&self, url: &str, body: String) -> Result<String>;
    /// GET `url` and return the response body as a string.
    async fn get(&self, url: &str) -> Result<String>;
}

/// Configuration for an [`OllamaProvider`].
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    /// Base endpoint of the Ollama server (e.g. `http://localhost:11434`).
    pub endpoint: Url,
    /// Model used for completions.
    pub model: String,
    /// Model used for embeddings; defaults to `nomic-embed-text` when `None`.
    pub embed_model: Option<String>,
}

/// Conservative capability defaults advertised by [`OllamaProvider`].
///
/// Ollama's real capabilities depend on the loaded model; these are safe
/// baseline values the agent loop can rely on.
const OLLAMA_MAX_CONTEXT: usize = 8192;

/// Ollama provider generic over the HTTP transport `C`.
///
/// Construct with [`OllamaProvider::new`], passing a config and any
/// [`HttpClient`]. Tests pass [`StubHttpClient`]; production (with the
/// `reqwest-backend` feature) passes [`ReqwestHttpClient`].
pub struct OllamaProvider<C: HttpClient> {
    config: OllamaConfig,
    client: C,
}

impl<C: HttpClient> OllamaProvider<C> {
    /// Create a new provider backed by `client`.
    pub fn new(config: OllamaConfig, client: C) -> Self {
        Self { config, client }
    }

    /// Borrow the configured model name.
    pub fn config(&self) -> &OllamaConfig {
        &self.config
    }

    /// Resolve the embedding model, defaulting to `nomic-embed-text`.
    pub fn embed_model(&self) -> String {
        self.config
            .embed_model
            .clone()
            .unwrap_or_else(|| "nomic-embed-text".to_string())
    }

    /// Join a path onto the configured endpoint (no double slashes).
    fn url(&self, path: &str) -> String {
        let base = self.config.endpoint.as_str().trim_end_matches('/');
        format!("{base}{path}")
    }
}

#[async_trait]
impl<C: HttpClient> Provider for OllamaProvider<C> {
    async fn complete(&self, prompt: &str, options: &CompletionOptions) -> Result<Completion> {
        let url = self.url("/api/chat");
        let mut req = serde_json::json!({
            "model": self.config.model,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": options.stream,
            "options": { "temperature": options.temperature },
        });
        if let Some(max) = options.max_tokens {
            req["options"]["num_predict"] = serde_json::json!(max);
        }
        if let Some(sys) = &options.system_prompt {
            req["messages"]
                .as_array_mut()
                .unwrap()
                .insert(0, serde_json::json!({ "role": "system", "content": sys }));
        }
        let body = self.client.post_json(&url, req.to_string()).await?;
        parse_chat(&body)
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
        let url = self.url("/api/embed");
        let req = serde_json::json!({ "model": self.embed_model(), "input": text });
        let body = self.client.post_json(&url, req.to_string()).await?;
        parse_embedding(&body)
    }

    async fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: true,
            supports_function_calling: false,
            supports_vision: false,
            supports_embeddings: true,
            max_context: OLLAMA_MAX_CONTEXT,
        }
    }

    async fn health_check(&self) -> Result<()> {
        let url = self.url("/api/tags");
        self.client.get(&url).await.map(|_| ())
    }
}

/// Subset of Ollama's `/api/chat` response that we parse.
#[derive(Deserialize)]
struct ChatResponse {
    message: Option<ChatMessage>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default = "default_true")]
    done: bool,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

fn default_true() -> bool {
    true
}

/// Parse a `/api/chat` response body into a [`Completion`].
///
/// `prompt_eval_count` maps to prompt tokens, `eval_count` to completion
/// tokens (Ollama may omit them when counting is disabled; treated as 0).
fn parse_chat(body: &str) -> Result<Completion> {
    let resp: ChatResponse = serde_json::from_str(body)
        .map_err(|e| ArgosError::Provider(format!("invalid chat response: {e}")))?;
    let text = resp
        .message
        .ok_or_else(|| ArgosError::Provider("chat response missing message".into()))?
        .content;
    let finish_reason = if resp.done {
        FinishReason::Stop
    } else {
        FinishReason::Length
    };
    Ok(Completion {
        text,
        usage: TokenUsage {
            prompt_tokens: resp.prompt_eval_count.unwrap_or(0),
            completion_tokens: resp.eval_count.unwrap_or(0),
        },
        finish_reason,
    })
}

/// Parse a `/api/embed` (or legacy `/api/embeddings`) response body.
///
/// The current `/api/embed` returns `{"embeddings": [[...]]}` (array of
/// arrays); the deprecated `/api/embeddings` returns `{"embedding": [...]}`
/// (flat). Both are accepted — the flat form is taken directly, the nested
/// form takes index 0.
fn parse_embedding(body: &str) -> Result<Embedding> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| ArgosError::Provider(format!("invalid embed response: {e}")))?;
    if let Some(arr) = value.get("embeddings").and_then(|v| v.as_array()) {
        let first = arr
            .first()
            .ok_or_else(|| ArgosError::Provider("embeddings array is empty".into()))?;
        return json_to_embedding(first);
    }
    if let Some(flat) = value.get("embedding") {
        return json_to_embedding(flat);
    }
    Err(ArgosError::Provider(
        "embed response missing `embeddings`/`embedding`".into(),
    ))
}

/// Convert a JSON array of numbers into an [`Embedding`].
fn json_to_embedding(value: &serde_json::Value) -> Result<Embedding> {
    let arr = value
        .as_array()
        .ok_or_else(|| ArgosError::Provider("embedding is not an array".into()))?;
    arr.iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| ArgosError::Provider("embedding element is not a number".into()))
        })
        .collect()
}

/// In-process stub HTTP client returning canned Ollama-format responses.
///
/// Used by unit tests so the provider never touches the network. Fields are
/// public so each test can shape the canned responses it needs.
pub struct StubHttpClient {
    /// Body returned for `POST /api/chat`.
    pub chat_body: String,
    /// Body returned for `POST /api/embed`.
    pub embed_body: String,
    /// Whether `GET /api/tags` reports success.
    pub tags_ok: bool,
}

#[async_trait]
impl HttpClient for StubHttpClient {
    async fn post_json(&self, url: &str, _body: String) -> Result<String> {
        if url.ends_with("/api/chat") {
            Ok(self.chat_body.clone())
        } else if url.ends_with("/api/embed") {
            Ok(self.embed_body.clone())
        } else {
            Err(ArgosError::Provider(format!(
                "stub: no canned response for {url}"
            )))
        }
    }

    async fn get(&self, url: &str) -> Result<String> {
        if url.ends_with("/api/tags") {
            if self.tags_ok {
                Ok(r#"{"models":[]}"#.to_string())
            } else {
                Err(ArgosError::Provider("stub: ollama unavailable".into()))
            }
        } else {
            Err(ArgosError::Provider(format!(
                "stub: no canned response for {url}"
            )))
        }
    }
}

/// Production HTTP backend backed by `reqwest` (feature `reqwest-backend`).
///
/// Enabled with `--features argos-security/...`-style `argos-provider/reqwest-backend`.
/// Uses rustls so it links no native TLS library (Windows GNU safe).
#[cfg(feature = "reqwest-backend")]
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

#[cfg(feature = "reqwest-backend")]
impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(feature = "reqwest-backend")]
#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn post_json(&self, url: &str, body: String) -> Result<String> {
        let resp = self
            .client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("http post failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::Provider(format!("http read failed: {e}")))?;
        if !status.is_success() {
            return Err(ArgosError::Provider(format!(
                "ollama returned {status}: {text}"
            )));
        }
        Ok(text)
    }

    async fn get(&self, url: &str) -> Result<String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("http get failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::Provider(format!("http read failed: {e}")))?;
        if !status.is_success() {
            return Err(ArgosError::Provider(format!(
                "ollama returned {status}: {text}"
            )));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::ollama::{OllamaConfig, OllamaProvider, StubHttpClient};
    use crate::{CompletionOptions, FinishReason, Provider};
    use url::Url;

    /// Build a stub client returning canned Ollama-format JSON.
    fn stub() -> StubHttpClient {
        StubHttpClient {
            chat_body: r#"{"message":{"role":"assistant","content":"hello there"},
                           "prompt_eval_count":12,"eval_count":8,"done":true}"#
                .to_string(),
            embed_body: r#"{"embeddings":[[0.1,0.2,0.3]]}"#.to_string(),
            tags_ok: true,
        }
    }

    fn config() -> OllamaConfig {
        OllamaConfig {
            endpoint: Url::parse("http://localhost:11434").unwrap(),
            model: "llama3".into(),
            embed_model: None,
        }
    }

    #[test]
    fn stub_http_client_constructs() {
        let s = stub();
        assert!(!s.chat_body.is_empty());
        assert!(s.tags_ok);
    }

    #[test]
    fn ollama_provider_constructs_with_config() {
        let provider = OllamaProvider::new(config(), stub());
        assert_eq!(provider.config().model, "llama3");
        // embed_model defaults to nomic-embed-text when None.
        assert_eq!(provider.embed_model(), "nomic-embed-text");
    }

    #[tokio::test]
    async fn ollama_complete_with_stub_returns_completion() {
        let provider = OllamaProvider::new(config(), stub());
        let completion = provider
            .complete("hi", &CompletionOptions::default())
            .await
            .unwrap();
        assert_eq!(completion.text, "hello there");
        assert_eq!(completion.finish_reason, FinishReason::Stop);
    }

    #[tokio::test]
    async fn ollama_complete_parses_token_usage() {
        let provider = OllamaProvider::new(config(), stub());
        let completion = provider
            .complete("hi", &CompletionOptions::default())
            .await
            .unwrap();
        // prompt_eval_count -> prompt_tokens, eval_count -> completion_tokens.
        assert_eq!(completion.usage.prompt_tokens, 12);
        assert_eq!(completion.usage.completion_tokens, 8);
    }

    #[tokio::test]
    async fn ollama_embed_with_stub_returns_embedding() {
        let provider = OllamaProvider::new(config(), stub());
        let emb = provider.embed("some text").await.unwrap();
        assert_eq!(emb, vec![0.1_f32, 0.2, 0.3]);
    }

    #[tokio::test]
    async fn ollama_capabilities_returns_conservative_defaults() {
        let provider = OllamaProvider::new(config(), stub());
        let caps = provider.capabilities().await;
        // Conservative defaults; Ollama's real caps depend on the model.
        assert!(caps.supports_streaming);
        assert!(!caps.supports_function_calling);
        assert!(!caps.supports_vision);
        assert!(caps.supports_embeddings);
        assert_eq!(caps.max_context, 8192);
    }

    #[tokio::test]
    async fn ollama_health_check_ok_when_tags_returned() {
        let provider = OllamaProvider::new(config(), stub());
        assert!(provider.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn ollama_health_check_err_when_stub_errors() {
        let mut s = stub();
        s.tags_ok = false;
        let provider = OllamaProvider::new(config(), s);
        assert!(provider.health_check().await.is_err());
    }
}
