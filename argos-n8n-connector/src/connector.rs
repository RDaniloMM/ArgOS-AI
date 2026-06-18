//! High-level n8n connector.
//!
//! [`N8nConnector`] wraps an [`N8nClient`](crate::client::N8nClient) transport
//! and an [`N8nConnection`] (endpoint + mode + api-key reference). It is the
//! public API other ArgOS crates use to talk to n8n: `connect` verifies
//! reachability, and every operation delegates to the injected transport so
//! callers stay transport-agnostic (ADR-011).

use argos_core::{N8nConnection, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};

use crate::client::N8nClient;

/// The public ArgOS → n8n facade.
///
/// Holds a boxed [`N8nClient`] (MCP or REST, chosen at construction) plus the
/// connection metadata. Operations are thin delegations; the value of this
/// type is the single, transport-agnostic entry point and the `connect`
/// health gate.
pub struct N8nConnector {
    client: Box<dyn N8nClient>,
    connection: N8nConnection,
}

impl N8nConnector {
    /// Create a connector over `client` with connection metadata `connection`.
    pub fn new(client: Box<dyn N8nClient>, connection: N8nConnection) -> Self {
        Self { client, connection }
    }

    /// Borrow the connection metadata.
    pub fn connection(&self) -> &N8nConnection {
        &self.connection
    }

    /// Verify the n8n instance is reachable (delegates to `health_check`).
    pub async fn connect(&self) -> Result<()> {
        self.client.health_check().await
    }

    /// List every workflow visible in n8n.
    pub async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
        self.client.list_workflows().await
    }

    /// Fetch a single workflow by id.
    pub async fn get_workflow(&self, id: &str) -> Result<N8nWorkflowRef> {
        self.client.get_workflow(id).await
    }

    /// Create a new workflow in n8n.
    pub async fn create_workflow(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef> {
        self.client.create_workflow(name, definition).await
    }

    /// Update an existing workflow's name and definition.
    pub async fn update_workflow(
        &self,
        id: &str,
        name: &str,
        definition: &str,
    ) -> Result<N8nWorkflowRef> {
        self.client.update_workflow(id, name, definition).await
    }

    /// Execute a workflow by id (n8n owns the execution).
    pub async fn run_workflow(&self, id: &str, data: Option<&str>) -> Result<N8nRunRef> {
        self.client.run_workflow(id, data).await
    }

    /// Poll the status of a run.
    pub async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        self.client.get_run_status(run_id).await
    }

    /// Verify reachability directly.
    pub async fn health_check(&self) -> Result<()> {
        self.client.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StubN8nClient;
    use argos_core::ConnMode;
    use url::Url;

    fn connection() -> N8nConnection {
        N8nConnection {
            endpoint: Url::parse("http://localhost:5678").unwrap(),
            mode: ConnMode::Mcp,
            api_key_ref: Some("n8n_key".into()),
        }
    }

    #[test]
    fn connector_constructs_with_client_and_connection() {
        let connector = N8nConnector::new(Box::new(StubN8nClient::new()), connection());
        assert_eq!(connector.connection().mode, ConnMode::Mcp);
        assert_eq!(
            connector.connection().endpoint.as_str(),
            "http://localhost:5678/"
        );
    }

    #[tokio::test]
    async fn connector_connect_calls_health_check() {
        // A healthy stub -> connect must succeed.
        let connector = N8nConnector::new(Box::new(StubN8nClient::new()), connection());
        assert!(connector.connect().await.is_ok());
    }
}
