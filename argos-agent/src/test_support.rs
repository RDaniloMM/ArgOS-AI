//! Shared test utilities for the agent crate.
//!
//! [`StubProvider`] mirrors `argos_knowledge::test_support::StubProvider`: a
//! canned-completion `Provider` with a deterministic embedding, so the agent
//! loop and Tier-1 tool tests stay network-free. [`ScriptedProvider`] returns a
//! sequence of canned completions (for multi-step tool-call loop tests).
//! [`EchoHandler`] is a canned-result [`ToolHandler`] for registry tests.
//!
//! All items here are `#[cfg(test)]` — they never ship in a release build.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use argos_core::{Embedding, Result, ToolResult};
use argos_provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};
use async_trait::async_trait;

use crate::registry::ToolHandler;

/// In-process `Provider` stub returning a single canned completion and a
/// deterministic embedding (one f32 per byte). No network, no running Ollama.
pub struct StubProvider {
    /// Text returned by every `complete` call.
    pub completion_text: String,
}

impl StubProvider {
    /// Create a stub whose `complete` always returns `completion_text`.
    pub fn new(completion_text: impl Into<String>) -> Self {
        Self {
            completion_text: completion_text.into(),
        }
    }
}

#[async_trait]
impl Provider for StubProvider {
    async fn complete(&self, _prompt: &str, _options: &CompletionOptions) -> Result<Completion> {
        Ok(Completion {
            text: self.completion_text.clone(),
            usage: TokenUsage {
                prompt_tokens: 0,
                completion_tokens: self.completion_text.len() as u32,
            },
            finish_reason: FinishReason::Stop,
        })
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
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

/// `Provider` stub that returns a scripted sequence of completions, one per
/// `complete` call. Panics if the script runs out of responses — tests should
/// size the script to match the expected number of loop iterations.
pub struct ScriptedProvider {
    responses: Mutex<Vec<String>>,
}

impl ScriptedProvider {
    /// Create a stub that returns `responses` in order, one per `complete` call.
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn complete(&self, _prompt: &str, _options: &CompletionOptions) -> Result<Completion> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(argos_core::ArgosError::Provider(
                "ScriptedProvider ran out of responses".to_string(),
            ));
        }
        let text = responses.remove(0);
        Ok(Completion {
            text,
            usage: TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
            },
            finish_reason: FinishReason::Stop,
        })
    }

    async fn embed(&self, text: &str) -> Result<Embedding> {
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

/// Convert an owned provider into an `Arc<dyn Provider>` (the shape the Tier-1
/// tool factories expect). Centralises the coercion so tests stay readable.
pub fn arc_provider<P: Provider + 'static>(provider: P) -> Arc<dyn Provider> {
    Arc::new(provider)
}

/// `ToolHandler` stub returning a canned [`ToolResult`] regardless of args.
/// Used by the registry unit tests to verify dispatch without pulling in real
/// services.
pub struct EchoHandler {
    pub result: ToolResult,
}

#[async_trait]
impl ToolHandler for EchoHandler {
    async fn invoke(&self, _args: &str) -> Result<ToolResult> {
        Ok(self.result.clone())
    }
}
