//! Execute workflows by delegation + mirror run status (spec: Execute by
//! Delegation, Run Status Mirroring; ADR-011).
//!
//! [`WorkflowRunner`] delegates execution to n8n via [`N8nClient::run_workflow`]
//! — ArgOS never executes a workflow itself — and records each returned
//! [`N8nRunRef`] in a [`RunMirror`] for audit. [`RunMirror`] is an in-memory
//! store of run references (slice 1); a SQLite-backed mirror via
//! `RelationalStore` is a future enhancement. Status polling delegates to
//! [`N8nClient::get_run_status`]; the mirror is audit-only and does not own
//! execution state.

use std::collections::HashMap;
use std::sync::Mutex;

use argos_core::{N8nRunRef, N8nRunStatus, Result};

use crate::client::N8nClient;

/// In-memory audit mirror of n8n run references.
///
/// Stores run references keyed by run id so ArgOS can answer "which runs did
/// we observe for this workflow?" without owning execution state. Slice 1 is
/// in-memory; a SQLite-backed mirror is future work.
pub struct RunMirror {
    runs: HashMap<String, N8nRunRef>,
}

impl RunMirror {
    /// Create an empty mirror.
    pub fn new() -> Self {
        Self {
            runs: HashMap::new(),
        }
    }

    /// Record a run reference (cloned into the mirror, keyed by run id).
    pub fn record(&mut self, run: &N8nRunRef) {
        self.runs.insert(run.id.clone(), run.clone());
    }

    /// Retrieve a recorded run reference by run id.
    pub fn get(&self, run_id: &str) -> Option<&N8nRunRef> {
        self.runs.get(run_id)
    }

    /// List every recorded run reference for a workflow id.
    pub fn list_for_workflow(&self, workflow_id: &str) -> Vec<&N8nRunRef> {
        self.runs
            .values()
            .filter(|r| r.workflow_id == workflow_id)
            .collect()
    }

    /// Number of recorded runs.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Whether no runs have been recorded.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

impl Default for RunMirror {
    fn default() -> Self {
        Self::new()
    }
}

/// The execute-by-delegation operation: run a workflow on n8n and mirror the
/// run reference for audit.
pub struct WorkflowRunner<'a, C: N8nClient> {
    client: &'a C,
    mirror: Mutex<RunMirror>,
}

impl<'a, C: N8nClient> WorkflowRunner<'a, C> {
    /// Create a runner backed by `client` with an empty run mirror.
    pub fn new(client: &'a C) -> Self {
        Self {
            client,
            mirror: Mutex::new(RunMirror::new()),
        }
    }

    /// Execute `workflow_id` on n8n (passing optional input `data`), record the
    /// returned run reference in the mirror, and return it. n8n owns the
    /// execution; ArgOS only mirrors the reference.
    pub async fn run(&self, workflow_id: &str, data: Option<&str>) -> Result<N8nRunRef> {
        let run = self.client.run_workflow(workflow_id, data).await?;
        self.mirror.lock().unwrap().record(&run);
        Ok(run)
    }

    /// Poll the status of a run by id (delegates to the n8n client).
    pub async fn check_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        self.client.get_run_status(run_id).await
    }

    /// Borrow the run mirror under a lock for audit inspection.
    pub fn mirror(&self) -> std::sync::MutexGuard<'_, RunMirror> {
        self.mirror.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StubN8nClient;
    use argos_core::N8nRunStatus;

    fn run(id: &str, wf: &str, status: N8nRunStatus) -> N8nRunRef {
        N8nRunRef {
            id: id.into(),
            workflow_id: wf.into(),
            status,
        }
    }

    #[test]
    fn run_mirror_constructs_empty() {
        let mirror = RunMirror::new();
        assert!(mirror.is_empty());
        assert_eq!(mirror.len(), 0);
    }

    #[test]
    fn run_mirror_records_and_retrieves_runs() {
        let mut mirror = RunMirror::new();
        mirror.record(&run("r1", "wf-1", N8nRunStatus::Success));
        assert_eq!(mirror.len(), 1);
        let got = mirror.get("r1").unwrap();
        assert_eq!(got.id, "r1");
        assert_eq!(got.workflow_id, "wf-1");
        assert_eq!(got.status, N8nRunStatus::Success);
    }

    #[test]
    fn run_mirror_returns_none_for_unknown_run_id() {
        let mirror = RunMirror::new();
        assert!(mirror.get("nope").is_none());
    }

    #[test]
    fn run_mirror_lists_runs_for_a_specific_workflow() {
        let mut mirror = RunMirror::new();
        mirror.record(&run("r1", "wf-1", N8nRunStatus::Success));
        mirror.record(&run("r2", "wf-1", N8nRunStatus::Failed));
        mirror.record(&run("r3", "wf-2", N8nRunStatus::Running));
        let wf1 = mirror.list_for_workflow("wf-1");
        assert_eq!(wf1.len(), 2, "wf-1 must have two recorded runs");
        assert!(wf1.iter().any(|r| r.id == "r1"));
        assert!(wf1.iter().any(|r| r.id == "r2"));
        let wf2 = mirror.list_for_workflow("wf-2");
        assert_eq!(wf2.len(), 1);
        assert!(mirror.list_for_workflow("wf-99").is_empty());
    }

    #[test]
    fn run_mirror_record_overwrites_same_run_id() {
        let mut mirror = RunMirror::new();
        mirror.record(&run("r1", "wf-1", N8nRunStatus::Running));
        mirror.record(&run("r1", "wf-1", N8nRunStatus::Success));
        // Same id -> single entry, latest status wins.
        assert_eq!(mirror.len(), 1);
        assert_eq!(mirror.get("r1").unwrap().status, N8nRunStatus::Success);
    }

    #[test]
    fn workflow_runner_constructs() {
        let client = StubN8nClient::new();
        let runner = WorkflowRunner::new(&client);
        assert!(runner.mirror().is_empty());
    }

    #[tokio::test]
    async fn runner_run_executes_via_client_and_returns_run_ref() {
        let client = StubN8nClient::with_workflows(vec![argos_core::N8nWorkflowRef {
            id: "wf-1".into(),
            name: "Daily".into(),
            url: None,
        }]);
        let runner = WorkflowRunner::new(&client);
        let run = runner.run("wf-1", None).await.unwrap();
        assert_eq!(run.workflow_id, "wf-1");
        assert_eq!(run.status, N8nRunStatus::Success);
        assert!(!run.id.is_empty());
    }

    #[tokio::test]
    async fn runner_check_status_returns_current_status() {
        let client = StubN8nClient::with_workflows(vec![argos_core::N8nWorkflowRef {
            id: "wf-1".into(),
            name: "Daily".into(),
            url: None,
        }]);
        let runner = WorkflowRunner::new(&client);
        let run = runner.run("wf-1", None).await.unwrap();
        let status = runner.check_status(&run.id).await.unwrap();
        assert_eq!(status, N8nRunStatus::Success);
    }

    #[tokio::test]
    async fn runner_run_then_check_status_returns_stored_status() {
        // The end-to-end execute→poll path: run returns a ref, and polling the
        // same run id yields the status n8n (here the stub) recorded for it.
        let client = StubN8nClient::with_workflows(vec![argos_core::N8nWorkflowRef {
            id: "wf-1".into(),
            name: "Daily".into(),
            url: None,
        }]);
        let runner = WorkflowRunner::new(&client);
        let run = runner.run("wf-1", Some(r#"{"k":"v"}"#)).await.unwrap();
        assert_eq!(runner.check_status(&run.id).await.unwrap(), run.status);
    }

    #[tokio::test]
    async fn runner_run_records_run_in_mirror() {
        let client = StubN8nClient::with_workflows(vec![argos_core::N8nWorkflowRef {
            id: "wf-1".into(),
            name: "Daily".into(),
            url: None,
        }]);
        let runner = WorkflowRunner::new(&client);
        let run = runner.run("wf-1", None).await.unwrap();
        // The runner mirrored the run for audit.
        let guard = runner.mirror();
        let mirrored = guard.get(&run.id).unwrap();
        assert_eq!(mirrored.workflow_id, "wf-1");
        assert_eq!(guard.list_for_workflow("wf-1").len(), 1);
    }

    #[tokio::test]
    async fn runner_run_on_missing_workflow_propagates_success_from_stub() {
        // The stub's run_workflow does not require the workflow to pre-exist;
        // it always returns a successful run. This documents that the runner
        // delegates execution (and existence checks) to n8n, not ArgOS.
        let client = StubN8nClient::new();
        let runner = WorkflowRunner::new(&client);
        let run = runner.run("absent", None).await.unwrap();
        assert_eq!(run.workflow_id, "absent");
    }
}
