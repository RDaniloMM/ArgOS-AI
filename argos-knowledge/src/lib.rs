//! ArgOS knowledge system.
//!
//! OKF bundle management (concept CRUD, frontmatter parsing, cross-links, typed
//! relations) and the LLM-Wiki operations: ingest, query, lint. The OKF markdown
//! bundle under `.argos/wiki/` is the source of truth; SQLite/sqlite-vec are
//! derived indexes only (ADR-010).
//!
//! `argos-knowledge` performs synchronous filesystem I/O (`std::fs`) for the
//! bundle primitives. The LLM-Wiki `ingest`/`query` operations are async because
//! they drive a [`Provider`](argos_provider::Provider) for LLM synthesis; the
//! CLI/agent layer runs them on the async runtime and wraps the sync bundle
//! primitives in `spawn_blocking` when integrating. `lint` is purely structural
//! (graph traversal + existence checks) and stays synchronous — no LLM, fully
//! deterministic.

pub mod bundle;
pub mod ingest;
pub mod links;
pub mod lint;
pub mod parser;
pub mod query;
pub mod raw;

pub use bundle::BundleStore;
pub use ingest::{IngestOperation, IngestResult};
pub use links::{CrossLinkParser, LinkGraph, RelationManager};
pub use lint::{LintOperation, LintReport};
pub use parser::{OkfParser, OkfWriter};
pub use query::{QueryOperation, QueryResult};
pub use raw::RawSourceStore;

// Shared test utilities for the LLM-Wiki operations (compiled only under test).
#[cfg(test)]
mod test_support;
