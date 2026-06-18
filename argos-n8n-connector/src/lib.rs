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
pub mod exporter;
pub mod importer;
pub mod rest;
pub mod runner;

#[cfg(feature = "reqwest-backend")]
pub use client::ReqwestN8nClient;
pub use client::{N8nClient, StubN8nClient};
pub use connector::N8nConnector;
pub use exporter::WorkflowExporter;
pub use importer::{slugify, workflow_resource, ImportResult, WorkflowImporter};
pub use rest::map_status;
pub use runner::{RunMirror, WorkflowRunner};

// Shared test utilities for the n8n connector unit tests (compiled only under
// test).
#[cfg(test)]
mod test_support;
