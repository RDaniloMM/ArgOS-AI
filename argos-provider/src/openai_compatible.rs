//! OpenAI-compatible provider (OpenCode Go, OpenAI, OpenRouter, etc.).
//!
//! Any LLM backend that speaks the OpenAI Chat Completions API
//! (`POST /v1/chat/completions`, `POST /v1/embeddings`) works through this
//! provider. This includes OpenCode Go subscriptions, OpenAI directly,
//! OpenRouter, vLLM with the OpenAI server, and more.

use argos_core::{ArgosError, Embedding, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};

/// Configuration for an [`OpenAICompatibleProvider`].
#[derive(Debug, Clone)]
pub struct OpenAICompatibleConfig {
    /// Base URL of the API (e.g. `https://opencode.ai/zen/go/v1`).
    pub endpoint: Url,
    /// API key (e.g. `sk-...`).
    pub api_key: String,
    /// Model name for completions (e.g. `deepseek-v4-flash`).
    pub model: String,
    /// Model name for embeddings (optional — not all providers support this).
    pub embed_model: Option<String>,
}

/// Provider that speaks the OpenAI Chat Completions + Embeddings API.
///
/// Works with OpenCode Go, OpenAI, OpenRouter, vLLM, and any other backend
/// that implements the OpenAI-compatible API surface.
#[cfg(feature = "reqwest-backend")]
pub struct OpenAICompatibleProvider {
    http: reqwest::Client,
    config: OpenAICompatibleConfig,
}

#[cfg(feature = "reqwest-backend")]
impl OpenAICompatibleProvider {
    pub fn new(config: OpenAICompatibleConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    fn url(&self, path: &str) -> String {
        let base = self.config.endpoint.as_str().trim_end_matches('/');
        format!("{base}{path}")
    }
}

/// Request body for `POST /chat/completions`.
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<usize>,
    temperature: f64,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Response body for `POST /chat/completions`.
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ResponseUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ResponseUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// Request body for `POST /embeddings`.
#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    input: String,
}

/// Response body for `POST /embeddings`.
#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

#[cfg(feature = "reqwest-backend")]
#[async_trait]
impl Provider for OpenAICompatibleProvider {
    async fn complete(&self, prompt: &str, options: &CompletionOptions) -> Result<Completion> {
        let mut messages = Vec::new();

        if let Some(sys) = &options.system_prompt {
            messages.push(ChatMessage {
                role: "system".into(),
                content: sys.clone(),
            });
        }

        messages.push(ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
        });

        let req = ChatRequest {
            model: self.config.model.clone(),
            messages,
            max_tokens: options.max_tokens,
            temperature: options.temperature,
            stream: false,
        };

        let body = serde_json::to_string(&req)
            .map_err(|e| ArgosError::Provider(format!("failed to serialize request: {e}")))?;

        let resp = self
            .http
            .post(self.url("/chat/completions"))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::Provider(format!("failed to read response: {e}")))?;

        if !status.is_success() {
            return Err(ArgosError::Provider(format!(
                "API returned {status}: {text}"
            )));
        }

        let chat: ChatResponse = serde_json::from_str(&text)
            .map_err(|e| ArgosError::Provider(format!("failed to parse response: {e}")))?;

        let choice = chat
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ArgosError::Provider("no choices in response".into()))?;

        let content = choice.message.content.unwrap_or_default();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolCall,
            _ => FinishReason::Stop,
        };

        let usage = chat.usage.unwrap_or(ResponseUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
        });

        Ok(Completion {
            text: content,
            usage: TokenUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
            },
            finish_reason,
        })
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
        let model = self
            .config
            .embed_model
            .as_deref()
            .unwrap_or(&self.config.model);

        let req = EmbedRequest {
            model: model.to_string(),
            input: text.to_string(),
        };

        let body = serde_json::to_string(&req)
            .map_err(|e| ArgosError::Provider(format!("failed to serialize embed request: {e}")))?;

        let resp = self
            .http
            .post(self.url("/embeddings"))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("embed request failed: {e}")))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| ArgosError::Provider(format!("failed to read embed response: {e}")))?;

        if !status.is_success() {
            return Err(ArgosError::Provider(format!(
                "embeddings API returned {status}: {resp_text}"
            )));
        }

        let embed: EmbedResponse = serde_json::from_str(&resp_text)
            .map_err(|e| ArgosError::Provider(format!("failed to parse embed response: {e}")))?;

        let embedding = embed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| ArgosError::Provider("no embedding in response".into()))?;

        Ok(embedding)
    }

    async fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: true,
            supports_function_calling: true,
            supports_vision: false,
            supports_embeddings: self.config.embed_model.is_some(),
            max_context: 65536,
        }
    }

    async fn health_check(&self) -> Result<()> {
        let resp = self
            .http
            .get(self.url("/models"))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await
            .map_err(|e| ArgosError::Provider(format!("health check failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ArgosError::Provider(format!(
                "health check returned {}",
                resp.status()
            )));
        }

        Ok(())
    }
}
