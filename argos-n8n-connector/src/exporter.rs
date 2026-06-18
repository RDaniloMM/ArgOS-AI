//! Export / generate new workflows to n8n (spec: Export / Generate Workflows
//! to n8n; ADR-011).
//!
//! [`WorkflowExporter`] creates a new n8n workflow from a JSON definition the
//! workflow-intelligence agent produces. It delegates to
//! [`N8nClient::create_workflow`] and returns the resulting
//! [`N8nWorkflowRef`]; the caller then links the originating OKF
//! workflow-concept's `resource` frontmatter to `n8n://workflows/<new_id>`.

use argos_core::{N8nWorkflowRef, Result};

use crate::client::N8nClient;

/// The export operation: turn a generated workflow definition into a real n8n
/// workflow.
pub struct WorkflowExporter<'a, C: N8nClient> {
    client: &'a C,
}

impl<'a, C: N8nClient> WorkflowExporter<'a, C> {
    /// Create an exporter backed by `client`.
    pub fn new(client: &'a C) -> Self {
        Self { client }
    }

    /// Create a new workflow in n8n named `name` from `definition` (n8n JSON)
    /// and return the reference to the newly created workflow.
    pub async fn export(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef> {
        self.client.create_workflow(name, definition).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StubN8nClient;

    #[test]
    fn workflow_exporter_constructs() {
        let client = StubN8nClient::new();
        let exporter = WorkflowExporter::new(&client);
        let _ = exporter;
    }

    #[tokio::test]
    async fn export_creates_workflow_and_returns_ref() {
        let client = StubN8nClient::new();
        let exporter = WorkflowExporter::new(&client);
        let wf = exporter
            .export("Daily Report", r#"{"nodes":[]}"#)
            .await
            .unwrap();
        assert_eq!(wf.name, "Daily Report");
        assert!(!wf.id.is_empty(), "exported workflow must get an n8n id");
        // The workflow is now retrievable from the client.
        let fetched = client.get_workflow(&wf.id).await.unwrap();
        assert_eq!(fetched.id, wf.id);
    }

    #[tokio::test]
    async fn export_with_definition_stores_the_definition() {
        let client = StubN8nClient::new();
        let exporter = WorkflowExporter::new(&client);
        let definition = r#"{"nodes":[{"type":"start","name":"Start"}],"connections":{}}"#;
        let wf = exporter.export("Weekly Report", definition).await.unwrap();
        // The definition passed to export round-tripped through create_workflow.
        assert_eq!(client.definition_of(&wf.id).as_deref(), Some(definition));
    }

    #[tokio::test]
    async fn export_distinct_names_get_distinct_ids() {
        let client = StubN8nClient::new();
        let exporter = WorkflowExporter::new(&client);
        let a = exporter.export("A", r#"{"nodes":[]}"#).await.unwrap();
        let b = exporter.export("B", r#"{"nodes":[]}"#).await.unwrap();
        assert_ne!(a.id, b.id, "each export must create a distinct workflow");
    }
}
