//! Shared test utilities for the LLM-Wiki operations.
//!
//! [`StubProvider`] implements [`Provider`](argos_provider::Provider) with a
//! configurable canned completion, recording the last prompt it received so
//! tests can assert on WHICH context the operation selected (not just the
//! text it returned). No network, no running Ollama — the whole knowledge
//! crate stays network-free under test.

#![allow(dead_code)]

use std::sync::Mutex;

use argos_core::{Embedding, Result};
use argos_provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};
use async_trait::async_trait;

/// In-process `Provider` stub returning a canned completion and recording the
/// last prompt seen.
pub struct StubProvider {
    /// Text returned by every `complete` call.
    pub completion_text: String,
    last_prompt: Mutex<Option<String>>,
}

impl StubProvider {
    /// Create a stub whose `complete` returns `completion_text`.
    pub fn new(completion_text: impl Into<String>) -> Self {
        Self {
            completion_text: completion_text.into(),
            last_prompt: Mutex::new(None),
        }
    }

    /// The most recent prompt handed to `complete` (for selection assertions).
    pub fn last_prompt(&self) -> Option<String> {
        self.last_prompt.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for StubProvider {
    async fn complete(&self, prompt: &str, _options: &CompletionOptions) -> Result<Completion> {
        *self.last_prompt.lock().unwrap() = Some(prompt.to_string());
        Ok(Completion {
            text: self.completion_text.clone(),
            usage: TokenUsage {
                prompt_tokens: prompt.len() as u32,
                completion_tokens: self.completion_text.len() as u32,
            },
            finish_reason: FinishReason::Stop,
        })
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
        // Deterministic stub embedding: one dimension per byte value.
        Ok(text.bytes().map(|b| b as f32).collect())
    }

    async fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_streaming: false,
            supports_function_calling: false,
            supports_vision: false,
            supports_embeddings: true,
            max_context: 8192,
        }
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}
