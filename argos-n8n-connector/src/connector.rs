//! High-level n8n connector.
//!
//! [`N8nConnector`] wraps an [`N8nClient`](crate::client::N8nClient) transport
//! and an [`N8nConnection`] (endpoint + mode + api-key reference). It is the
//! public API other ArgOS crates use to talk to n8n: `connect` verifies
//! reachability, and every operation delegates to the injected transport so
//! callers stay transport-agnostic (ADR-011).

use argos_core::{ArgosError, N8nConnection, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};

use crate::client::N8nClient;

/// The public ArgOS → n8n facade.
///
/// Holds a boxed [`N8nClient`] (MCP or REST, chosen at construction) plus the
/// connection metadata. Operations are thin delegations; the value of this
/// type is the single, transport-agnostic entry point and the `connect`
/// health gate. When n8n is unreachable, every operation returns a
/// recoverable [`ArgosError::N8nConnection`] — the connector never panics or
/// hangs, so the wiki and workflow intelligence keep working offline
/// (spec scenario `n8n-disconnects-gracefully`).
pub struct N8nConnector {
    client: Box<dyn N8nClient>,
    connection: N8nConnection,
    max_retries: u32,
}

impl N8nConnector {
    /// Create a connector over `client` with connection metadata `connection`.
    /// No retry: `connect` probes n8n once.
    pub fn new(client: Box<dyn N8nClient>, connection: N8nConnection) -> Self {
        Self {
            client,
            connection,
            max_retries: 0,
        }
    }

    /// Create a connector that retries the health probe up to `max_retries`
    /// additional times before giving up. Data operations (`list_workflows`,
    /// `run_workflow`, ...) are never retried in slice 1 — they fail fast so
    /// the caller can degrade gracefully.
    pub fn with_retry(
        client: Box<dyn N8nClient>,
        connection: N8nConnection,
        max_retries: u32,
    ) -> Self {
        Self {
            client,
            connection,
            max_retries,
        }
    }

    /// Borrow the connection metadata.
    pub fn connection(&self) -> &N8nConnection {
        &self.connection
    }

    /// Verify the n8n instance is reachable, retrying the health probe up to
    /// `max_retries` extra times when configured via [`with_retry`]. Returns
    /// the last error (mapped to [`ArgosError::N8nConnection`]) on persistent
    /// failure.
    pub async fn connect(&self) -> Result<()> {
        let mut last_err: Option<ArgosError> = None;
        for attempt in 0..=self.max_retries {
            match self.client.health_check().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = Some(wrap_connection_error(e));
                    if attempt < self.max_retries {
                        continue;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ArgosError::N8nConnection("n8n unreachable".into())))
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

    /// Activate a workflow so its triggers become live.
    pub async fn activate_workflow(&self, id: &str) -> Result<()> {
        self.client.activate_workflow(id).await
    }

    /// Deactivate a workflow, stopping its triggers.
    pub async fn deactivate_workflow(&self, id: &str) -> Result<()> {
        self.client.deactivate_workflow(id).await
    }

    /// Poll the status of a run.
    pub async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        self.client.get_run_status(run_id).await
    }

    /// Verify reachability directly (single probe, no retry).
    pub async fn health_check(&self) -> Result<()> {
        self.client.health_check().await
    }
}

/// Normalise a connection-probe failure into [`ArgosError::N8nConnection`].
///
/// `NotFound` and other non-transport errors are passed through unchanged so
/// callers can distinguish "n8n unreachable" from "workflow does not exist".
fn wrap_connection_error(e: ArgosError) -> ArgosError {
    match e {
        ArgosError::N8nConnection(_) => e,
        other => ArgosError::N8nConnection(format!("n8n unreachable: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StubN8nClient;
    use crate::test_support::FailingN8nClient;
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

    // --- Graceful disconnect (spec scenario `n8n-disconnects-gracefully`) ----

    #[tokio::test]
    async fn connect_returns_error_on_health_check_failure() {
        let (failing, _counter) = FailingN8nClient::new();
        let connector = N8nConnector::new(Box::new(failing), connection());
        let err = connector.connect().await.unwrap_err();
        assert!(
            matches!(err, ArgosError::N8nConnection(_)),
            "connect failure must surface as N8nConnection, got {err:?}"
        );
    }

    #[tokio::test]
    async fn list_workflows_on_failed_client_returns_error_not_panic() {
        let (failing, _counter) = FailingN8nClient::new();
        let connector = N8nConnector::new(Box::new(failing), connection());
        let res = connector.list_workflows().await;
        assert!(
            res.is_err(),
            "list_workflows must error when n8n is unreachable"
        );
        assert!(matches!(res.unwrap_err(), ArgosError::N8nConnection(_)));
    }

    #[tokio::test]
    async fn run_workflow_on_failed_client_returns_error() {
        let (failing, _counter) = FailingN8nClient::new();
        let connector = N8nConnector::new(Box::new(failing), connection());
        let res = connector.run_workflow("42", None).await;
        assert!(
            res.is_err(),
            "run_workflow must error when n8n is unreachable"
        );
        assert!(matches!(res.unwrap_err(), ArgosError::N8nConnection(_)));
    }

    #[tokio::test]
    async fn get_workflow_on_failed_client_returns_connection_error() {
        let (failing, _counter) = FailingN8nClient::new();
        let connector = N8nConnector::new(Box::new(failing), connection());
        let err = connector.get_workflow("42").await.unwrap_err();
        assert!(matches!(err, ArgosError::N8nConnection(_)));
    }

    #[tokio::test]
    async fn failing_operations_return_immediately_without_hang() {
        // Every operation on a failing client must return Err at once — the
        // connector never blocks waiting on a dead n8n. We assert the shared
        // call counter advances by exactly one per operation (no retry loop on
        // data ops) and that all complete without timing out.
        let (failing, counter) = FailingN8nClient::new();
        let connector = N8nConnector::new(Box::new(failing), connection());
        let _ = connector.health_check().await;
        let _ = connector.list_workflows().await;
        let _ = connector.run_workflow("42", None).await;
        // Three distinct operations => three client calls, no retries.
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "data ops must not retry; each failed op calls the client once"
        );
    }

    #[tokio::test]
    async fn with_retry_retries_health_probe_then_fails() {
        let (failing, counter) = FailingN8nClient::new();
        // 2 extra retries => 3 probes total.
        let connector = N8nConnector::with_retry(Box::new(failing), connection(), 2);
        let err = connector.connect().await.unwrap_err();
        assert!(matches!(err, ArgosError::N8nConnection(_)));
        // 1 initial + 2 retries = 3 health_check calls.
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn with_retry_succeeds_when_probe_evently_succeeds() {
        // A healthy stub succeeds on the first probe even with retry budget.
        let connector = N8nConnector::with_retry(Box::new(StubN8nClient::new()), connection(), 3);
        assert!(connector.connect().await.is_ok());
    }
}
