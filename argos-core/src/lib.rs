//! ArgOS core domain types.
//!
//! Shared domain model for the ArgOS AI Operating System: OKF knowledge concepts,
//! n8n connector references, agent lifecycle, configuration, and errors. Every type
//! here is serde-serializable and carries no I/O — this crate is the vocabulary that
//! every other ArgOS crate speaks.

pub mod agent;
pub mod common;
pub mod config;
pub mod error;
pub mod n8n;
pub mod okf;

pub use agent::{AgentId, AgentState, Hand, Tool, ToolInvocation, ToolResult};
pub use common::{Embedding, SimilarityHit, Timestamp};
pub use config::{
    Config, EmbedderConfig, OpenAiOAuthToken, ProviderAuthMethod, ProviderConfig, StorageProfile,
};
pub use error::{ArgosError, Result};
pub use n8n::{ConnMode, N8nConnection, N8nRunRef, N8nRunStatus, N8nWorkflowRef};
pub use okf::{
    Bundle, Concept, ConceptPath, ConceptType, CrossLink, Frontmatter, RawSource, RelationKind,
    TypedRelation,
};
