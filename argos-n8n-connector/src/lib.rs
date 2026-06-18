//! ArgOS n8n connector.
//!
//! Bidirectional integration with n8n (the workflow engine). ArgOS discovers,
//! imports, exports, executes (by delegation), and monitors n8n workflows. MCP
//! is the preferred transport; REST is the fallback — both sit behind the
//! [`client::N8nClient`] trait so callers are transport-agnostic (ADR-011).
//! ArgOS never executes a workflow itself; n8n owns all execution and
//! durability, and ArgOS mirrors run status for audit only.

pub mod client;
pub mod connector;

pub use client::{N8nClient, StubN8nClient};
pub use connector::N8nConnector;
