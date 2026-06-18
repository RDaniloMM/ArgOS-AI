//! Test-only helpers shared across the n8n connector unit tests.
//!
//! [`FailingN8nClient`] simulates an unreachable n8n instance: every operation
//! returns [`ArgosError::N8nConnection`]. It is the fixture for the
//! graceful-disconnect tests (spec scenario `n8n-disconnects-gracefully`) and
//! never touches the network.

#![cfg(test)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use argos_core::{ArgosError, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};
use async_trait::async_trait;

use crate::client::N8nClient;

/// A stub [`N8nClient`] whose every operation fails with `N8nConnection`.
///
/// A shared [`AtomicU32`] counter records how many times any method was
/// invoked, so tests can assert retry behaviour and that failing operations
/// return immediately rather than hanging. The counter is returned from
/// [`new`](Self::new) so a test can keep observing it after the client is
/// boxed into the connector.
pub struct FailingN8nClient {
    call_count: Arc<AtomicU32>,
}

impl FailingN8nClient {
    /// Create a failing client and a shared handle to its call counter.
    pub fn new() -> (Self, Arc<AtomicU32>) {
        let counter = Arc::new(AtomicU32::new(0));
        (
            Self {
                call_count: counter.clone(),
            },
            counter,
        )
    }

    fn fail<T>(&self, fallback: T) -> Result<T> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Err(ArgosError::N8nConnection(
            "n8n unreachable: connection refused".into(),
        ))
        .map(|_: ()| fallback)
    }
}

#[async_trait]
impl N8nClient for FailingN8nClient {
    async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
        self.fail(Vec::new())
    }

    async fn get_workflow(&self, _id: &str) -> Result<N8nWorkflowRef> {
        self.fail(empty_workflow())
    }

    async fn create_workflow(&self, _name: &str, _definition: &str) -> Result<N8nWorkflowRef> {
        self.fail(empty_workflow())
    }

    async fn update_workflow(
        &self,
        _id: &str,
        _name: &str,
        _definition: &str,
    ) -> Result<N8nWorkflowRef> {
        self.fail(empty_workflow())
    }

    async fn run_workflow(&self, _id: &str, _data: Option<&str>) -> Result<N8nRunRef> {
        self.fail(empty_run())
    }

    async fn get_run_status(&self, _run_id: &str) -> Result<N8nRunStatus> {
        self.fail(N8nRunStatus::Failed)
    }

    async fn health_check(&self) -> Result<()> {
        self.fail(())
    }
}

fn empty_workflow() -> N8nWorkflowRef {
    N8nWorkflowRef {
        id: String::new(),
        name: String::new(),
        url: None,
    }
}

fn empty_run() -> N8nRunRef {
    N8nRunRef {
        id: String::new(),
        workflow_id: String::new(),
        status: N8nRunStatus::Failed,
    }
}
