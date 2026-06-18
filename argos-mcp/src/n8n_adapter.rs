//! MCP-to-N8n adapter — implements [`N8nClient`] via MCP transport (T-031).
//!
//! [`McpN8nAdapter`] wraps an [`McpClient`] and delegates n8n operations to
//! MCP tools (`list_workflows`, `run_workflow`) when available. For methods
//! without a corresponding MCP tool, it falls back to the injected REST
//! [`N8nClient`] (if configured) or returns an error.

use std::sync::{Arc, Mutex};

use argos_core::{ArgosError, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};
use argos_n8n_connector::N8nClient;
use async_trait::async_trait;

use super::client::McpClient;
use super::types::McpServerInfo;

/// Adapter that implements the [`N8nClient`] trait using MCP transport.
///
/// # Fallback strategy
/// When an MCP tool is not available for a given operation (or the MCP
/// client is not connected), the adapter delegates to the optional REST
/// fallback. If neither is available, an error is returned.
pub struct McpN8nAdapter {
    mcp_client: Arc<dyn McpClient>,
    /// Optional REST fallback for operations not available via MCP.
    rest_fallback: Option<Box<dyn N8nClient>>,
    /// Connected server info (set during connect).
    server_info: Mutex<Option<McpServerInfo>>,
}

impl McpN8nAdapter {
    /// Create an adapter that uses only MCP transport (no REST fallback).
    pub fn new(mcp_client: Arc<dyn McpClient>) -> Self {
        Self {
            mcp_client,
            rest_fallback: None,
            server_info: Mutex::new(None),
        }
    }

    /// Create an adapter with a REST fallback for operations not covered by MCP.
    pub fn with_rest_fallback(mcp_client: Arc<dyn McpClient>, rest: Box<dyn N8nClient>) -> Self {
        Self {
            mcp_client,
            rest_fallback: Some(rest),
            server_info: Mutex::new(None),
        }
    }

    /// Connect the MCP client to an n8n MCP server.
    pub async fn connect_mcp(&self, info: McpServerInfo) -> Result<()> {
        // We can't call &mut self on an Arc-based adapter. The connect method
        // on McpClient takes &mut self. For simplicity in slice 1, we store
        // the server info and the caller handles the MCP client lifecycle.
        *self.server_info.lock().unwrap() = Some(info);
        Ok(())
    }
}

#[async_trait]
impl N8nClient for McpN8nAdapter {
    async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
        // Try MCP first.
        let result = self.mcp_client.call_tool("list_workflows", "{}").await;
        match result {
            Ok(tool_result) => match tool_result {
                argos_core::ToolResult::Ok(payload) => {
                    // Parse the payload as a JSON array of workflow refs.
                    let parsed: serde_json::Value =
                        serde_json::from_str(&payload).map_err(|e| {
                            ArgosError::N8nConnection(format!(
                                "invalid list_workflows response: {e}"
                            ))
                        })?;
                    let workflows: Vec<N8nWorkflowRef> = parsed
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|w| {
                            Some(N8nWorkflowRef {
                                id: w.get("id")?.as_str()?.to_string(),
                                name: w.get("name")?.as_str()?.to_string(),
                                url: w
                                    .get("url")
                                    .and_then(|u| u.as_str())
                                    .and_then(|s| s.parse().ok()),
                            })
                        })
                        .collect();
                    Ok(workflows)
                }
                argos_core::ToolResult::Err(msg) => {
                    // MCP tool error — try fallback.
                    if let Some(ref rest) = self.rest_fallback {
                        rest.list_workflows().await
                    } else {
                        Err(ArgosError::N8nConnection(format!(
                            "MCP list_workflows failed: {msg}"
                        )))
                    }
                }
            },
            Err(_) => {
                // MCP client error — try fallback.
                if let Some(ref rest) = self.rest_fallback {
                    rest.list_workflows().await
                } else {
                    Err(ArgosError::N8nConnection(
                        "MCP list_workflows unavailable".into(),
                    ))
                }
            }
        }
    }

    async fn get_workflow(&self, id: &str) -> Result<N8nWorkflowRef> {
        if let Some(ref rest) = self.rest_fallback {
            rest.get_workflow(id).await
        } else {
            Err(ArgosError::N8nConnection(
                "get_workflow not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn create_workflow(&self, name: &str, definition: &str) -> Result<N8nWorkflowRef> {
        if let Some(ref rest) = self.rest_fallback {
            rest.create_workflow(name, definition).await
        } else {
            Err(ArgosError::N8nConnection(
                "create_workflow not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn update_workflow(
        &self,
        id: &str,
        name: &str,
        definition: &str,
    ) -> Result<N8nWorkflowRef> {
        if let Some(ref rest) = self.rest_fallback {
            rest.update_workflow(id, name, definition).await
        } else {
            Err(ArgosError::N8nConnection(
                "update_workflow not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn activate_workflow(&self, id: &str) -> Result<()> {
        if let Some(ref rest) = self.rest_fallback {
            rest.activate_workflow(id).await
        } else {
            Err(ArgosError::N8nConnection(
                "activate_workflow not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn deactivate_workflow(&self, id: &str) -> Result<()> {
        if let Some(ref rest) = self.rest_fallback {
            rest.deactivate_workflow(id).await
        } else {
            Err(ArgosError::N8nConnection(
                "deactivate_workflow not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn run_workflow(&self, id: &str, data: Option<&str>) -> Result<N8nRunRef> {
        let args = serde_json::json!({
            "workflow_id": id,
            "data": data.unwrap_or("{}")
        })
        .to_string();

        let result = self.mcp_client.call_tool("run_workflow", &args).await;
        match result {
            Ok(tool_result) => match tool_result {
                argos_core::ToolResult::Ok(payload) => {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&payload).map_err(|e| {
                            ArgosError::N8nConnection(format!("invalid run_workflow response: {e}"))
                        })?;
                    Ok(N8nRunRef {
                        id: parsed["id"].as_str().unwrap_or("unknown").to_string(),
                        workflow_id: id.to_string(),
                        status: N8nRunStatus::Success,
                    })
                }
                argos_core::ToolResult::Err(msg) => {
                    if let Some(ref rest) = self.rest_fallback {
                        rest.run_workflow(id, data).await
                    } else {
                        Err(ArgosError::N8nConnection(format!(
                            "MCP run_workflow failed: {msg}"
                        )))
                    }
                }
            },
            Err(_) => {
                if let Some(ref rest) = self.rest_fallback {
                    rest.run_workflow(id, data).await
                } else {
                    Err(ArgosError::N8nConnection(
                        "MCP run_workflow unavailable".into(),
                    ))
                }
            }
        }
    }

    async fn get_run_status(&self, run_id: &str) -> Result<N8nRunStatus> {
        if let Some(ref rest) = self.rest_fallback {
            rest.get_run_status(run_id).await
        } else {
            Err(ArgosError::N8nConnection(
                "get_run_status not available via MCP in slice 1".into(),
            ))
        }
    }

    async fn health_check(&self) -> Result<()> {
        // Try MCP by calling a simple operation. If MCP is connected,
        // the client is healthy. Otherwise try REST fallback.
        if let Some(ref rest) = self.rest_fallback {
            rest.health_check().await
        } else {
            // With MCP-only, just check if we can list tools.
            self.mcp_client
                .list_tools()
                .await
                .map(|_| ())
                .map_err(|_| ArgosError::N8nConnection("MCP health check failed".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_agent::registry::ToolInfo;
    use argos_core::ToolResult;
    use async_trait::async_trait;

    use crate::types::McpCapabilities;

    // --- Stub McpClient for adapter tests ---
    struct StubMcpClient {
        tools: Mutex<Vec<ToolInfo>>,
        call_responses: Mutex<Vec<ToolResult>>,
        connected: Mutex<bool>,
    }

    impl StubMcpClient {
        fn new() -> Self {
            Self {
                tools: Mutex::new(vec![
                    ToolInfo {
                        name: "list_workflows".into(),
                        description: "List workflows".into(),
                    },
                    ToolInfo {
                        name: "run_workflow".into(),
                        description: "Run workflow".into(),
                    },
                ]),
                call_responses: Mutex::new(Vec::new()),
                connected: Mutex::new(false),
            }
        }

        fn with_call_responses(responses: Vec<ToolResult>) -> Self {
            Self {
                tools: Mutex::new(vec![
                    ToolInfo {
                        name: "list_workflows".into(),
                        description: "List workflows".into(),
                    },
                    ToolInfo {
                        name: "run_workflow".into(),
                        description: "Run workflow".into(),
                    },
                ]),
                call_responses: Mutex::new(responses),
                connected: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl McpClient for StubMcpClient {
        async fn connect(&mut self, _info: McpServerInfo) -> Result<()> {
            *self.connected.lock().unwrap() = true;
            Ok(())
        }

        async fn disconnect(&mut self) -> Result<()> {
            *self.connected.lock().unwrap() = false;
            Ok(())
        }

        async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
            Ok(self.tools.lock().unwrap().clone())
        }

        async fn call_tool(&self, _name: &str, _args: &str) -> Result<ToolResult> {
            let mut responses = self.call_responses.lock().unwrap();
            if responses.is_empty() {
                Ok(ToolResult::Ok("[]".into()))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn capabilities(&self) -> McpCapabilities {
            McpCapabilities::default()
        }
    }

    // --- Stub N8nClient for fallback tests ---
    struct StubRestClient;

    #[async_trait]
    impl N8nClient for StubRestClient {
        async fn list_workflows(&self) -> Result<Vec<N8nWorkflowRef>> {
            Ok(vec![N8nWorkflowRef {
                id: "rest-1".into(),
                name: "REST Workflow".into(),
                url: None,
            }])
        }

        async fn get_workflow(&self, _id: &str) -> Result<N8nWorkflowRef> {
            Err(ArgosError::NotFound("not found".into()))
        }

        async fn create_workflow(&self, _name: &str, _def: &str) -> Result<N8nWorkflowRef> {
            Err(ArgosError::N8nConnection("not implemented".into()))
        }

        async fn update_workflow(
            &self,
            _id: &str,
            _name: &str,
            _def: &str,
        ) -> Result<N8nWorkflowRef> {
            Err(ArgosError::N8nConnection("not implemented".into()))
        }

        async fn activate_workflow(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn deactivate_workflow(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn run_workflow(&self, _id: &str, _data: Option<&str>) -> Result<N8nRunRef> {
            Ok(N8nRunRef {
                id: "rest-run-1".into(),
                workflow_id: _id.into(),
                status: N8nRunStatus::Success,
            })
        }

        async fn get_run_status(&self, _run_id: &str) -> Result<N8nRunStatus> {
            Ok(N8nRunStatus::Success)
        }

        async fn health_check(&self) -> Result<()> {
            Ok(())
        }
    }

    // =====================================================================
    // T-031 Test 10: n8n_adapter_lists_workflows_via_mcp
    // =====================================================================
    #[tokio::test]
    async fn n8n_adapter_lists_workflows_via_mcp() {
        let mcp = Arc::new(StubMcpClient::with_call_responses(vec![ToolResult::Ok(
            r#"[
                    {"id":"wf-1","name":"Daily Report","url":"http://n8n/workflow/1"},
                    {"id":"wf-2","name":"Slack Alert","url":null}
                ]"#
            .into(),
        )]));
        let adapter = McpN8nAdapter::new(mcp);

        let workflows = adapter.list_workflows().await.unwrap();
        assert_eq!(workflows.len(), 2);
        assert_eq!(workflows[0].id, "wf-1");
        assert_eq!(workflows[0].name, "Daily Report");
        assert_eq!(workflows[1].id, "wf-2");
        assert_eq!(workflows[1].name, "Slack Alert");
    }

    // =====================================================================
    // T-031 Test 11: n8n_adapter_runs_workflow_via_mcp
    // =====================================================================
    #[tokio::test]
    async fn n8n_adapter_runs_workflow_via_mcp() {
        let mcp = Arc::new(StubMcpClient::with_call_responses(vec![ToolResult::Ok(
            r#"{"id":"exec-99","status":"success"}"#.into(),
        )]));
        let adapter = McpN8nAdapter::new(mcp);

        let run = adapter
            .run_workflow("wf-1", Some(r#"{"input":"test"}"#))
            .await
            .unwrap();
        assert_eq!(run.id, "exec-99");
        assert_eq!(run.workflow_id, "wf-1");
        assert_eq!(run.status, N8nRunStatus::Success);
    }

    // =====================================================================
    // T-031 Test 12: n8n_adapter_graceful_degradation_no_fallback
    // =====================================================================
    #[tokio::test]
    async fn n8n_adapter_graceful_degradation_no_fallback() {
        // MCP call fails — no REST fallback configured → error.
        let mcp = Arc::new(StubMcpClient::with_call_responses(vec![ToolResult::Err(
            "MCP tool error".into(),
        )]));
        let adapter = McpN8nAdapter::new(mcp);

        let result = adapter.list_workflows().await;
        assert!(
            result.is_err(),
            "should error when MCP fails and no REST fallback"
        );
    }

    // =====================================================================
    // T-031 Test 13: n8n_adapter_falls_back_to_rest
    // =====================================================================
    #[tokio::test]
    async fn n8n_adapter_falls_back_to_rest() {
        // MCP call fails, REST fallback configured → succeeds via REST.
        let mcp = Arc::new(StubMcpClient::with_call_responses(vec![ToolResult::Err(
            "MCP tool error".into(),
        )]));
        let rest = Box::new(StubRestClient);
        let adapter = McpN8nAdapter::with_rest_fallback(mcp, rest);

        let workflows = adapter.list_workflows().await.unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].id, "rest-1");
        assert_eq!(workflows[0].name, "REST Workflow");
    }

    // =====================================================================
    // T-031 Test 14: adapter_constructor_works
    // =====================================================================
    #[test]
    fn adapter_constructor_works() {
        let mcp = Arc::new(StubMcpClient::new());
        let adapter = McpN8nAdapter::new(mcp);
        let _boxed: Box<dyn N8nClient> = Box::new(adapter);
    }
}
