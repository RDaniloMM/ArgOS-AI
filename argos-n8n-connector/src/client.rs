//! n8n HTTP transport seam + in-memory stub.
//!
//! [`N8nClient`] abstracts the transport to an n8n instance (MCP preferred,
//! REST fallback) so the connector is unit-testable without a running n8n
//! server: tests inject [`StubN8nClient`], production (with the
//! `reqwest-backend` feature) injects [`ReqwestN8nClient`]. Per the
//! OllamaProvider pattern (T-010), transport quirks stay behind this seam and
//! never leak through the [`N8nConnector`](crate::connector::N8nConnector) API.

use std::collections::HashMap;
use std::sync::Mutex;

use argos_core::{ArgosError, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};
use async_trait::async_trait;
use uuid::Uuid;

// `Url` is only needed by the feature-gated REST backend.
#[cfg(feature = "reqwest-backend")]
use url::Url;

/// Transport seam to an n8n instance.
///
/// All methods are async and return [`argos_core::Result`]. MCP is the
/// preferred transport; a REST implementation sits behind the same trait as a
/// fallback. Callers (the connector, importer, exporter, runner) are unaware
/// of which transport is in use.
#[async_trait]
pub trait N8nClient: Send + Sync {
    /// List every workflow visible to the connector in n8n.
    async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>>;
    /// Fetch a single workflow by its n8n id.
    async fn get_workflow(&self, id: &str) -> Result<N8nWorkflowRef>;
    /// Create a new workflow in n8n from a JSON `definition`.
    async fn create_workflow(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef>;
    /// Update an existing workflow's name and JSON `definition`.
    async fn update_workflow(
        &self,
        id: &str,
        name: &str,
        definition: &str,
    ) -> Result<N8nWorkflowRef>;
    /// Activate a workflow so its triggers (webhook, schedule) become live.
    /// Required before `run_workflow` can trigger a webhook-based workflow.
    async fn activate_workflow(&self, id: &str) -> Result<()>;
    /// Deactivate a workflow, stopping its triggers.
    async fn deactivate_workflow(&self, id: &str) -> Result<()>;
    /// Execute a workflow by id, optionally passing input `data` (JSON).
    ///
    /// For webhook-based workflows, this triggers the webhook endpoint and
    /// polls executions for the result. The workflow must be activated first.
    async fn run_workflow(&self, id: &str, data: Option<&str>) -> Result<N8nRunRef>;
    /// Poll the status of a run by its run id.
    async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus>;
    /// Verify the n8n instance is reachable.
    async fn health_check(&self) -> Result<()>;
}

/// In-memory [`N8nClient`] for tests — no network, no running n8n.
///
/// Workflows, definitions, and runs are stored in `Mutex<HashMap>` so the
/// `&self` trait methods can mutate state. A run always completes with
/// [`N8nRunStatus::Success`] immediately (the stub owns no execution engine).
pub struct StubN8nClient {
    workflows: Mutex<HashMap<String, N8nWorkflowRef>>,
    definitions: Mutex<HashMap<String, String>>,
    runs: Mutex<HashMap<String, N8nRunRef>>,
    active: Mutex<HashMap<String, bool>>,
}

impl StubN8nClient {
    /// Create an empty stub client.
    pub fn new() -> Self {
        Self {
            workflows: Mutex::new(HashMap::new()),
            definitions: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Create a stub client pre-populated with `workflows` (definitions left
    /// empty — useful when tests only need the refs).
    pub fn with_workflows(workflows: Vec<N8nWorkflowRef>) -> Self {
        let map = workflows.into_iter().map(|w| (w.id.clone(), w)).collect();
        Self {
            workflows: Mutex::new(map),
            definitions: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Look up the stored definition for a workflow id (test inspection helper
    /// used by the exporter tests to verify a definition round-tripped through
    /// `create_workflow`). Returns `None` when no definition was stored.
    pub fn definition_of(&self, id: &str) -> Option<String> {
        self.definitions.lock().unwrap().get(id).cloned()
    }
}

impl Default for StubN8nClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl N8nClient for StubN8nClient {
    async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
        let workflows = self.workflows.lock().unwrap();
        Ok(workflows.values().cloned().collect())
    }

    async fn get_workflow(&self, id: &str) -> Result<N8nWorkflowRef> {
        let workflows = self.workflows.lock().unwrap();
        workflows
            .get(id)
            .cloned()
            .ok_or_else(|| ArgosError::NotFound(format!("n8n workflow not found: {id}")))
    }

    async fn create_workflow(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef> {
        let id = Uuid::new_v4().to_string();
        let workflow = N8nWorkflowRef {
            id: id.clone(),
            name: name.to_string(),
            url: None,
        };
        self.workflows
            .lock()
            .unwrap()
            .insert(id.clone(), workflow.clone());
        self.definitions
            .lock()
            .unwrap()
            .insert(id, definition.to_string());
        Ok(workflow)
    }

    async fn update_workflow(
        &self,
        id: &str,
        name: &str,
        definition: &str,
    ) -> Result<N8nWorkflowRef> {
        let mut workflows = self.workflows.lock().unwrap();
        let workflow = workflows
            .get_mut(id)
            .ok_or_else(|| ArgosError::NotFound(format!("n8n workflow not found: {id}")))?;
        workflow.name = name.to_string();
        let updated = workflow.clone();
        drop(workflows);
        self.definitions
            .lock()
            .unwrap()
            .insert(id.to_string(), definition.to_string());
        Ok(updated)
    }

    async fn run_workflow(&self, id: &str, _data: Option<&str>) -> Result<N8nRunRef> {
        // The stub owns no execution engine: every run completes with Success
        // immediately. n8n owns real execution; this only models the contract.
        let run_id = Uuid::new_v4().to_string();
        let run = N8nRunRef {
            id: run_id.clone(),
            workflow_id: id.to_string(),
            status: N8nRunStatus::Success,
        };
        self.runs.lock().unwrap().insert(run_id, run.clone());
        Ok(run)
    }

    async fn activate_workflow(&self, id: &str) -> Result<()> {
        let workflows = self.workflows.lock().unwrap();
        if !workflows.contains_key(id) {
            return Err(ArgosError::NotFound(format!(
                "n8n workflow not found: {id}"
            )));
        }
        drop(workflows);
        self.active.lock().unwrap().insert(id.to_string(), true);
        Ok(())
    }

    async fn deactivate_workflow(&self, id: &str) -> Result<()> {
        let workflows = self.workflows.lock().unwrap();
        if !workflows.contains_key(id) {
            return Err(ArgosError::NotFound(format!(
                "n8n workflow not found: {id}"
            )));
        }
        drop(workflows);
        self.active.lock().unwrap().insert(id.to_string(), false);
        Ok(())
    }

    async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        let runs = self.runs.lock().unwrap();
        runs.get(run_id)
            .map(|r| r.status.clone())
            .ok_or_else(|| ArgosError::NotFound(format!("n8n run not found: {run_id}")))
    }

    async fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

/// Production REST backend for [`N8nClient`] (feature `reqwest-backend`).
///
/// Talks to n8n's Public REST API (`/api/v1/workflows`, `/api/v1/executions`,
/// `/healthz`) using `reqwest` with rustls — no native TLS, so it links no
/// `link.exe` on Windows GNU. Authentication is the `X-N8N-API-KEY` header;
/// the key itself is resolved by the caller from the SecretVault and passed
/// here, never stored in config. Transport failures and non-2xx responses are
/// mapped to [`ArgosError::N8nConnection`] so the connector degrades
/// gracefully when n8n is unreachable.
#[cfg(feature = "reqwest-backend")]
pub struct ReqwestN8nClient {
    http: reqwest::Client,
    endpoint: Url,
    api_key: Option<String>,
}

#[cfg(feature = "reqwest-backend")]
impl ReqwestN8nClient {
    /// Create a REST client targeting `endpoint` with an optional API key.
    pub fn new(endpoint: Url, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            endpoint,
            api_key,
        }
    }

    /// Join `path` onto the configured endpoint (no double slashes).
    fn url(&self, path: &str) -> String {
        let base = self.endpoint.as_str().trim_end_matches('/');
        format!("{base}{path}")
    }

    /// Attach the API key header to a request builder when configured.
    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => req.header("X-N8N-API-KEY", key),
            None => req,
        }
    }

    /// Run a GET, returning the body text or an `N8nConnection` error.
    async fn get_text(&self, path: &str) -> Result<String> {
        let resp = self
            .authed(self.http.get(self.url(path)))
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n unreachable: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n read failed: {e}")))?;
        if !status.is_success() {
            return Err(ArgosError::N8nConnection(format!(
                "n8n returned {status}: {text}"
            )));
        }
        Ok(text)
    }

    /// Run a POST with a JSON body, returning the body text.
    async fn post_text(&self, path: &str, body: String) -> Result<String> {
        let resp = self
            .authed(
                self.http
                    .post(self.url(path))
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body),
            )
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n unreachable: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n read failed: {e}")))?;
        if !status.is_success() {
            return Err(ArgosError::N8nConnection(format!(
                "n8n returned {status}: {text}"
            )));
        }
        Ok(text)
    }

    /// Run a PUT with a JSON body, returning the body text.
    async fn put_text(&self, path: &str, body: String) -> Result<String> {
        let resp = self
            .authed(
                self.http
                    .put(self.url(path))
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body),
            )
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n unreachable: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("n8n read failed: {e}")))?;
        if !status.is_success() {
            return Err(ArgosError::N8nConnection(format!(
                "n8n returned {status}: {text}"
            )));
        }
        Ok(text)
    }
}

#[cfg(feature = "reqwest-backend")]
#[async_trait]
impl N8nClient for ReqwestN8nClient {
    async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
        let body = self.get_text("/api/v1/workflows").await?;
        crate::rest::parse_workflow_list(&body)
    }

    async fn get_workflow(&self, id: &str) -> Result<N8nWorkflowRef> {
        let body = self.get_text(&format!("/api/v1/workflows/{id}")).await?;
        crate::rest::parse_workflow(&body)
    }

    async fn create_workflow(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef> {
        let req = build_workflow_payload(name, definition);
        let body = self.post_text("/api/v1/workflows", req).await?;
        crate::rest::parse_workflow(&body)
    }

    async fn update_workflow(
        &self,
        id: &str,
        name: &str,
        definition: &str,
    ) -> Result<N8nWorkflowRef> {
        let req = build_workflow_payload(name, definition);
        let body = self
            .put_text(&format!("/api/v1/workflows/{id}"), req)
            .await?;
        crate::rest::parse_workflow(&body)
    }

    async fn run_workflow(&self, id: &str, data: Option<&str>) -> Result<N8nRunRef> {
        // n8n's Public API does NOT support POST /workflows/{id}/run (returns
        // 405). Real execution works via webhook triggers: the workflow must
        // have a webhook node, be activated, and then we POST to the webhook
        // URL. This implementation follows that flow.

        // 1. Get the workflow definition to find the webhook node.
        let wf_body = self.get_text(&format!("/api/v1/workflows/{id}")).await?;
        let wf_json: serde_json::Value = serde_json::from_str(&wf_body)
            .map_err(|e| ArgosError::N8nConnection(format!("invalid workflow response: {e}")))?;

        // 2. Find the webhook path.
        let webhook_path = crate::rest::extract_webhook_path(&wf_json).ok_or_else(|| {
            ArgosError::N8nConnection(
                "workflow has no webhook trigger — cannot execute externally. \
                 Add a webhook node to the workflow, or use a schedule trigger."
                    .to_string(),
            )
        })?;

        // 3. POST to the webhook URL to trigger execution.
        //
        // n8n webhooks support two response modes:
        // - "onReceived": responds immediately with {"message":"Workflow was started"}
        //   — the workflow runs async and we'd need to poll for the result.
        // - "lastNode": BLOCKS until the workflow completes, then returns the
        //   output data from the last node. This is real async/await —
        //   reqwest's `.send().await` naturally waits for n8n to finish.
        //
        // ArgOS-created workflows default to "lastNode" so run_workflow is a
        // clean await with no polling. If a user's workflow uses "onReceived",
        // we detect the immediate "started" response and fall back to polling.
        let webhook_url = format!(
            "{}/webhook/{}",
            self.endpoint.as_str().trim_end_matches('/'),
            webhook_path
        );
        let body = data.unwrap_or("{}");
        let resp = self
            .http
            .post(&webhook_url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("webhook trigger failed: {e}")))?;

        let status_code = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("webhook response read failed: {e}")))?;

        if !status_code.is_success() {
            if status_code.as_u16() == 404 {
                return Err(ArgosError::N8nConnection(
                    "webhook not found — the workflow may not be active. \
                     Activate the workflow first with activate_workflow()."
                        .to_string(),
                ));
            }
            return Err(ArgosError::N8nConnection(format!(
                "webhook returned {status_code}: {resp_text}"
            )));
        }

        // 4. Determine whether the workflow completed synchronously.
        //
        // With "lastNode" mode, the response contains the workflow output
        // (not a "started" message) — the execution is already done.
        // With "onReceived" mode, the response is {"message":"Workflow was
        // started"} — we need to poll until the execution registers.
        let started_async = resp_text.contains("Workflow was started");

        if !started_async {
            // lastNode mode — workflow already completed. Query executions
            // ONCE to get the execution ID (the webhook response itself
            // doesn't carry it in a standard field).
            let exec_body = self
                .get_text(&format!("/api/v1/executions?workflowId={id}"))
                .await?;
            return crate::rest::parse_latest_execution(&exec_body, id);
        }

        // 5. onReceived mode — poll with backoff until the execution registers.
        //
        // This only happens for workflows the user configured with
        // "onReceived" mode. ArgOS-created workflows use "lastNode" and
        // never reach this branch.
        let mut delay = std::time::Duration::from_millis(50);
        let max_retries = 10u32;

        for attempt in 0..max_retries {
            let exec_body = self
                .get_text(&format!("/api/v1/executions?workflowId={id}"))
                .await;

            if let Ok(body) = exec_body {
                if let Ok(run) = crate::rest::parse_latest_execution(&body, id) {
                    return Ok(run);
                }
            }

            if attempt + 1 < max_retries {
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
        }

        Err(ArgosError::N8nConnection(format!(
            "webhook triggered but no execution registered for workflow {id} \
             after {max_retries} retries — the workflow may use 'onReceived' \
             mode and failed to start, or has no active nodes"
        )))
    }

    async fn activate_workflow(&self, id: &str) -> Result<()> {
        let resp = self
            .authed(
                self.http
                    .post(self.url(&format!("/api/v1/workflows/{id}/activate")))
                    .header(reqwest::header::CONTENT_TYPE, "application/json"),
            )
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("activate failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ArgosError::N8nConnection(format!(
                "activate returned {status}: {text}"
            )));
        }
        Ok(())
    }

    async fn deactivate_workflow(&self, id: &str) -> Result<()> {
        let resp = self
            .authed(
                self.http
                    .post(self.url(&format!("/api/v1/workflows/{id}/deactivate")))
                    .header(reqwest::header::CONTENT_TYPE, "application/json"),
            )
            .send()
            .await
            .map_err(|e| ArgosError::N8nConnection(format!("deactivate failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ArgosError::N8nConnection(format!(
                "deactivate failed: {text}"
            )));
        }
        Ok(())
    }

    async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        let body = self
            .get_text(&format!("/api/v1/executions/{run_id}"))
            .await?;
        crate::rest::parse_status(&body)
    }

    async fn health_check(&self) -> Result<()> {
        self.get_text("/healthz").await.map(|_| ())
    }
}

/// Build a `POST/PUT /api/v1/workflows` JSON payload from a name and a raw
/// n8n workflow `definition`. The definition is embedded under `nodes` when it
/// parses as an object carrying `nodes`; otherwise it is sent as the request
/// body verbatim wrapped in `{name, ...}`.
#[cfg(feature = "reqwest-backend")]
fn build_workflow_payload(name: &str, definition: &str) -> String {
    if let Ok(mut obj) = serde_json::from_str::<serde_json::Value>(definition) {
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            return serde_json::to_string(&obj).unwrap_or_else(|_| definition.to_string());
        }
    }
    format!(r#"{{"name":"{name}","definition":{definition}}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn wf(id: &str, name: &str) -> N8nWorkflowRef {
        N8nWorkflowRef {
            id: id.into(),
            name: name.into(),
            url: Some(Url::parse("http://localhost:5678/workflow/42").unwrap()),
        }
    }

    #[test]
    fn stub_n8n_client_constructs_empty() {
        let client = StubN8nClient::new();
        // An empty stub lists no workflows.
        let workflows = client.workflows.lock().unwrap();
        assert!(workflows.is_empty(), "new stub must hold no workflows");
    }

    #[test]
    fn stub_n8n_client_can_be_pre_populated_with_workflows() {
        let client = StubN8nClient::with_workflows(vec![wf("1", "One"), wf("2", "Two")]);
        let workflows = client.workflows.lock().unwrap();
        assert_eq!(
            workflows.len(),
            2,
            "pre-populated stub must hold both workflows"
        );
        assert!(workflows.contains_key("1"));
        assert!(workflows.contains_key("2"));
    }

    #[tokio::test]
    async fn list_workflows_returns_all_stored_workflows() {
        let client =
            StubN8nClient::with_workflows(vec![wf("1", "One"), wf("2", "Two"), wf("3", "Three")]);
        let list = client.list_workflows().await.unwrap();
        assert_eq!(
            list.len(),
            3,
            "list_workflows must return every stored workflow"
        );
        let ids: Vec<&str> = list.iter().map(|w| w.id.as_str()).collect();
        assert!(ids.contains(&"1"));
        assert!(ids.contains(&"2"));
        assert!(ids.contains(&"3"));
    }

    #[tokio::test]
    async fn create_workflow_generates_id_and_stores_workflow() {
        let client = StubN8nClient::new();
        let created = client
            .create_workflow("Daily Report", r#"{"nodes":[]}"#)
            .await
            .unwrap();
        assert_eq!(created.name, "Daily Report");
        assert!(
            !created.id.is_empty(),
            "create_workflow must assign a non-empty id"
        );
        // The workflow is now retrievable.
        let fetched = client.get_workflow(&created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        // And the definition is stored.
        let defs = client.definitions.lock().unwrap();
        assert_eq!(
            defs.get(&created.id).map(String::as_str),
            Some(r#"{"nodes":[]}"#)
        );
    }

    #[tokio::test]
    async fn get_workflow_returns_existing_workflow() {
        let client = StubN8nClient::with_workflows(vec![wf("42", "Daily Report")]);
        let got = client.get_workflow("42").await.unwrap();
        assert_eq!(got.id, "42");
        assert_eq!(got.name, "Daily Report");
    }

    #[tokio::test]
    async fn get_workflow_on_missing_id_returns_error() {
        let client = StubN8nClient::new();
        let res = client.get_workflow("nope").await;
        assert!(res.is_err(), "get_workflow on a missing id must error");
    }

    #[tokio::test]
    async fn update_workflow_modifies_name_and_definition() {
        let client = StubN8nClient::new();
        let created = client
            .create_workflow("Old", r#"{"nodes":[]}"#)
            .await
            .unwrap();
        let updated = client
            .update_workflow(&created.id, "New Name", r#"{"nodes":["x"]}"#)
            .await
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "New Name");
        // The stored definition reflects the update.
        let defs = client.definitions.lock().unwrap();
        assert_eq!(
            defs.get(&created.id).map(String::as_str),
            Some(r#"{"nodes":["x"]}"#)
        );
        // And the stored workflow name reflects the update.
        let wfs = client.workflows.lock().unwrap();
        assert_eq!(wfs.get(&created.id).unwrap().name, "New Name");
    }

    #[tokio::test]
    async fn run_workflow_creates_run_with_success_status() {
        let client = StubN8nClient::with_workflows(vec![wf("42", "Daily Report")]);
        let run = client.run_workflow("42", None).await.unwrap();
        assert_eq!(run.workflow_id, "42");
        assert_eq!(
            run.status,
            N8nRunStatus::Success,
            "stub runs complete immediately"
        );
        assert!(!run.id.is_empty(), "run must have a non-empty id");
    }

    #[tokio::test]
    async fn get_run_status_returns_stored_status() {
        let client = StubN8nClient::with_workflows(vec![wf("42", "Daily Report")]);
        let run = client.run_workflow("42", None).await.unwrap();
        let status = client.get_run_status(&run.id).await.unwrap();
        assert_eq!(status, N8nRunStatus::Success);
    }

    #[tokio::test]
    async fn health_check_returns_ok() {
        let client = StubN8nClient::new();
        assert!(client.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn run_workflow_accepts_optional_input_data() {
        let client = StubN8nClient::with_workflows(vec![wf("42", "Daily Report")]);
        // Passing Some(data) must still succeed (the stub ignores the payload).
        let run = client
            .run_workflow("42", Some(r#"{"foo":"bar"}"#))
            .await
            .unwrap();
        assert_eq!(run.status, N8nRunStatus::Success);
    }

    // --- ReqwestN8nClient (feature-gated compile check) ---------------------
    // The production REST backend is only compiled under `reqwest-backend`.
    // This test merely proves the type exists and is constructible; it makes
    // no HTTP call. Run with: cargo test -p argos-n8n-connector --features reqwest-backend
    #[cfg(feature = "reqwest-backend")]
    #[test]
    fn reqwest_n8n_client_type_exists() {
        use url::Url;
        let endpoint = Url::parse("http://localhost:5678").unwrap();
        let _client = super::ReqwestN8nClient::new(endpoint, None);
    }

    #[cfg(feature = "reqwest-backend")]
    #[tokio::test]
    async fn reqwest_n8n_client_implements_n8n_client_trait() {
        use url::Url;
        let endpoint = Url::parse("http://localhost:5678").unwrap();
        let client = super::ReqwestN8nClient::new(endpoint, Some("key".into()));
        // The trait is implemented: we can box it as `dyn N8nClient`.
        let _boxed: Box<dyn N8nClient> = Box::new(client);
    }
}
