//! ArgOS provider abstraction.
//!
//! Capability-negotiated LLM backend trait (completion + embedding). Ollama is the
//! baseline; local alternatives (llama.cpp / vLLM / MLX) verified later. The
//! [`provider::Provider`] trait is the single seam that keeps per-provider quirks
//! out of the agent loop (ADR-005). Implementation lands in later tasks.

pub mod ollama;
pub mod openai_compatible;
pub mod provider;

pub use ollama::{HttpClient, OllamaConfig, OllamaProvider, StubHttpClient};
#[cfg(feature = "reqwest-backend")]
pub use openai_compatible::{OpenAICompatibleConfig, OpenAICompatibleProvider};
pub use provider::{
    Completion, CompletionOptions, FinishReason, Provider, ProviderCapabilities, TokenUsage,
};
