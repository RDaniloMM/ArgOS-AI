//! MCP client implementation — ArgOS discovering n8n MCP tools (T-031).
//!
//! [`McpClientImpl`] connects to a remote MCP server (n8n) via a
//! [`McpTransport`], negotiates capabilities, discovers tools, and gates
//! every invocation through [`PermissionGate`] and [`AuditLog`].
//!
//! # Stub-first testing
//! All tests use [`StubTransport`] with canned JSON-RPC exchanges — no real
//! n8n, no real I/O. The transport, gate, audit log, and tool responses are
//! fully deterministic.

use std::sync::{Arc, Mutex};

use argos_agent::registry::ToolInfo;
use argos_core::{Result, ToolResult};
use argos_security::{AuditEntry, AuditLog, Permission, PermissionGate};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Mutex as TokioMutex;

use super::client::McpClient;
use super::transport::McpTransport;
use super::types::{McpCapabilities, McpServerInfo, McpServerState};

/// Concrete MCP client connecting to a remote MCP server.
///
/// # Seams (all injectable for testing)
/// - `McpTransport` — message exchange (inject StubTransport)
/// - `PermissionGate` — gates every tool call (inject pre-granted gate)
/// - `AuditLog` — records discovery and invocation (inject Vec-backed stub)
pub struct McpClientImpl {
    transport: Option<Box<dyn McpTransport>>,
    permission_gate: Arc<dyn PermissionGate>,
    audit_log: Arc<TokioMutex<Box<dyn AuditLog>>>,
    tools: Mutex<Vec<ToolInfo>>,
    capabilities: Mutex<McpCapabilities>,
    state: Mutex<McpServerState>,
    /// Whether the user has approved first-time discovery.
    approved: Mutex<bool>,
}

impl McpClientImpl {
    /// Create a new unconnected client.
    pub fn new(
        permission_gate: Arc<dyn PermissionGate>,
        audit_log: Arc<TokioMutex<Box<dyn AuditLog>>>,
    ) -> Self {
        Self {
            transport: None,
            permission_gate,
            audit_log,
            tools: Mutex::new(Vec::new()),
            capabilities: Mutex::new(McpCapabilities::default()),
            state: Mutex::new(McpServerState::Stopped),
            approved: Mutex::new(false),
        }
    }

    /// Inject a transport for the client to use.
    pub fn with_transport(mut self, transport: Box<dyn McpTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Mark the client as approved for first-time discovery (user consent).
    pub fn approve(&self) {
        *self.approved.lock().unwrap() = true;
    }
}

#[async_trait]
impl McpClient for McpClientImpl {
    async fn connect(&mut self, server_info: McpServerInfo) -> Result<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| argos_core::ArgosError::Config("no transport configured".into()))?;

        // 1. Send initialize request.
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": McpCapabilities::default(),
                "clientInfo": {
                    "name": server_info.name,
                    "version": "0.1.0"
                }
            }
        })
        .to_string();
        transport.write_message(&init_request).await?;

        // 2. Receive initialize response.
        let init_resp = transport
            .read_message()
            .await?
            .ok_or_else(|| argos_core::ArgosError::N8nConnection("server disconnected".into()))?;
        let parsed: serde_json::Value = serde_json::from_str(&init_resp).map_err(|e| {
            argos_core::ArgosError::N8nConnection(format!("invalid init response: {e}"))
        })?;

        // Extract server capabilities.
        if let Some(caps) = parsed["result"]["capabilities"].as_object() {
            let negotiated = McpCapabilities {
                tools: caps.get("tools").and_then(|v| v.as_bool()).unwrap_or(false),
                resources: caps
                    .get("resources")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                prompts: caps
                    .get("prompts")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                sampling: caps
                    .get("sampling")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };
            *self.capabilities.lock().unwrap() = negotiated;
        }

        // 3. Send tools/list request to discover tools.
        let list_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        })
        .to_string();
        transport.write_message(&list_request).await?;

        let list_resp = transport
            .read_message()
            .await?
            .ok_or_else(|| argos_core::ArgosError::N8nConnection("server disconnected".into()))?;
        let parsed_list: serde_json::Value = serde_json::from_str(&list_resp).map_err(|e| {
            argos_core::ArgosError::N8nConnection(format!("invalid tools/list response: {e}"))
        })?;

        // Parse discovered tools.
        let mut discovered = Vec::new();
        if let Some(tools) = parsed_list["result"]["tools"].as_array() {
            for tool in tools {
                if let (Some(name), Some(desc)) =
                    (tool["name"].as_str(), tool["description"].as_str())
                {
                    discovered.push(ToolInfo {
                        name: name.to_string(),
                        description: desc.to_string(),
                    });
                }
            }
        }
        *self.tools.lock().unwrap() = discovered.clone();

        // 4. Record user-approval audit (first discovery).
        if !*self.approved.lock().unwrap() {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                subject: server_info.name.clone(),
                action: "mcp.client.discovered".into(),
                resource: format!("mcp://{}", server_info.endpoint),
                result: "approval_required".into(),
                prev_hash: String::new(),
                this_hash: String::new(),
            };
            let mut log = self.audit_log.lock().await;
            log.record(&entry).await?;
        }

        *self.state.lock().unwrap() = McpServerState::Started;
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        *self.state.lock().unwrap() = McpServerState::Stopped;
        // Clear discovered tools on disconnect.
        self.tools.lock().unwrap().clear();
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let state = self.state.lock().unwrap().clone();
        if state != McpServerState::Started {
            return Err(argos_core::ArgosError::N8nConnection(
                "client not connected".into(),
            ));
        }
        Ok(self.tools.lock().unwrap().clone())
    }

    async fn call_tool(&self, name: &str, args: &str) -> Result<ToolResult> {
        let state = self.state.lock().unwrap().clone();
        if state != McpServerState::Started {
            return Err(argos_core::ArgosError::N8nConnection(
                "client not connected — cannot call tool".into(),
            ));
        }

        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| argos_core::ArgosError::Config("no transport configured".into()))?;

        // 1. Permission check.
        let perm = self
            .permission_gate
            .check("argos-client", &format!("mcp://tools/{name}"), "invoke")
            .await?;
        if let Permission::Deny(reason) = perm {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                subject: "argos-client".into(),
                action: "mcp.client.invoked".into(),
                resource: format!("mcp://tools/{name}"),
                result: format!("denied: {reason}"),
                prev_hash: String::new(),
                this_hash: String::new(),
            };
            let mut log = self.audit_log.lock().await;
            log.record(&entry).await?;
            return Ok(ToolResult::Err(format!("permission denied: {reason}")));
        }

        // 2. Send tools/call request.
        let call_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": serde_json::from_str::<serde_json::Value>(args).unwrap_or(serde_json::Value::String(args.into()))
            }
        })
        .to_string();
        transport.write_message(&call_request).await?;

        // 3. Receive response.
        let call_resp = transport
            .read_message()
            .await?
            .ok_or_else(|| argos_core::ArgosError::N8nConnection("server disconnected".into()))?;
        let parsed: serde_json::Value = serde_json::from_str(&call_resp).map_err(|e| {
            argos_core::ArgosError::N8nConnection(format!("invalid tools/call response: {e}"))
        })?;

        // 4. Extract content.
        let content = parsed["result"]["content"].as_array();
        let is_error = parsed["result"]["isError"].as_bool().unwrap_or(false);
        let text = content
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("(empty response)")
            .to_string();

        let result = if is_error {
            ToolResult::Err(text.clone())
        } else {
            ToolResult::Ok(text.clone())
        };

        // 5. Audit invocation.
        let entry = AuditEntry {
            timestamp: Utc::now(),
            subject: "argos-client".into(),
            action: "mcp.client.invoked".into(),
            resource: format!("mcp://tools/{name}"),
            result: format!("ok: {text}"),
            prev_hash: String::new(),
            this_hash: String::new(),
        };
        let mut log = self.audit_log.lock().await;
        log.record(&entry).await?;

        Ok(result)
    }

    async fn capabilities(&self) -> McpCapabilities {
        self.capabilities.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Tests (T-031: McpClient)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::StubTransport;
    use std::collections::HashSet;

    // --- Stub PermissionGate (reused from server_impl tests pattern) ---
    #[allow(dead_code)]
    struct StubPermissionGate {
        #[allow(dead_code)]
        grants: Mutex<HashSet<(String, String, String)>>,
        unrestricted: bool,
    }

    impl StubPermissionGate {
        fn unrestricted() -> Self {
            Self {
                grants: Mutex::new(HashSet::new()),
                unrestricted: true,
            }
        }
        fn new() -> Self {
            Self {
                grants: Mutex::new(HashSet::new()),
                unrestricted: false,
            }
        }
    }

    #[async_trait]
    impl PermissionGate for StubPermissionGate {
        async fn check(
            &self,
            _subject: &str,
            _resource: &str,
            _action: &str,
        ) -> Result<Permission> {
            if self.unrestricted {
                Ok(Permission::Allow)
            } else {
                Ok(Permission::Deny("no grant".into()))
            }
        }
        async fn grant(&mut self, _s: &str, _r: &str, _a: &str) -> Result<()> {
            Ok(())
        }
        async fn revoke(&mut self, _s: &str, _r: &str, _a: &str) -> Result<()> {
            Ok(())
        }
    }

    // --- Stub AuditLog ---
    struct StubAuditLog {
        entries: Arc<Mutex<Vec<AuditEntry>>>,
    }

    impl StubAuditLog {
        fn new() -> Self {
            Self {
                entries: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn entries_handle(&self) -> Arc<Mutex<Vec<AuditEntry>>> {
            self.entries.clone()
        }
    }

    #[async_trait]
    impl AuditLog for StubAuditLog {
        async fn record(&mut self, entry: &AuditEntry) -> Result<()> {
            let mut stored = entry.clone();
            stored.prev_hash = "0".repeat(64);
            stored.this_hash = "f".repeat(64);
            self.entries.lock().unwrap().push(stored);
            Ok(())
        }
        async fn query(&self, _filter: &argos_security::AuditFilter) -> Result<Vec<AuditEntry>> {
            Ok(self.entries.lock().unwrap().clone())
        }
        async fn verify_chain(&self) -> Result<bool> {
            Ok(true)
        }
    }

    /// Build an initialize response (server advertises tools-only capability).
    fn init_response(id: u64) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": true, "resources": false, "prompts": false, "sampling": false },
                "serverInfo": { "name": "n8n-mcp", "version": "1.0.0" }
            }
        })
        .to_string()
    }

    /// Build a tools/list response with two tools.
    fn tools_list_response(id: u64) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    { "name": "list_workflows", "description": "List all workflows", "inputSchema": {} },
                    { "name": "run_workflow", "description": "Run a workflow", "inputSchema": {} }
                ]
            }
        })
        .to_string()
    }

    /// Build a tools/call success response.
    fn tools_call_response(id: u64, text: &str) -> String {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "text", "text": text}],
                "isError": false
            }
        })
        .to_string()
    }

    fn server_info() -> McpServerInfo {
        McpServerInfo {
            name: "n8n-mcp".into(),
            endpoint: "localhost:5678".into(),
            transport: crate::types::McpTransportType::Stdio,
        }
    }

    /// Create a client with canned transport messages for connect + tools/list.
    fn make_connected_client(
        gate: Arc<dyn PermissionGate>,
    ) -> (McpClientImpl, StubTransport, Arc<Mutex<Vec<AuditEntry>>>) {
        let stub_audit = StubAuditLog::new();
        let entries = stub_audit.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        let transport =
            StubTransport::with_messages(vec![init_response(1), tools_list_response(2)]);

        let client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));
        (client, StubTransport::with_messages(vec![]), entries)
    }

    // =====================================================================
    // T-031 Test 1: mcp_client_connects_and_negotiates_capabilities
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_connects_and_negotiates_capabilities() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let (mut client, _transport, _entries) = make_connected_client(gate);

        client.connect(server_info()).await.unwrap();
        let caps = client.capabilities().await;
        assert!(caps.tools);
        assert!(!caps.resources);
    }

    // =====================================================================
    // T-031 Test 2: mcp_client_discovers_tools
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_discovers_tools() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let (mut client, _transport, _entries) = make_connected_client(gate);

        client.connect(server_info()).await.unwrap();

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "list_workflows");
        assert_eq!(tools[1].name, "run_workflow");
    }

    // =====================================================================
    // T-031 Test 3: mcp_client_calls_remote_tool
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_calls_remote_tool() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let stub_audit = StubAuditLog::new();
        let entries = stub_audit.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        // Transport: initialize, tools/list, then tools/call response.
        let transport = StubTransport::with_messages(vec![
            init_response(1),
            tools_list_response(2),
            tools_call_response(3, r#"[{"id":"wf-1","name":"Daily Report"}]"#),
        ]);

        let mut client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));

        client.connect(server_info()).await.unwrap();

        let result = client.call_tool("list_workflows", "{}").await.unwrap();
        assert!(
            matches!(&result, ToolResult::Ok(ref s) if s.contains("Daily Report")),
            "expected workflow list, got {result:?}"
        );

        // Audit entry was recorded.
        let audit_entries = entries.lock().unwrap().clone();
        let invoked = audit_entries
            .iter()
            .find(|e| e.action == "mcp.client.invoked" && e.result.starts_with("ok:"))
            .expect("invocation should be audited");
        assert_eq!(invoked.resource, "mcp://tools/list_workflows");
    }

    // =====================================================================
    // T-031 Test 4: mcp_client_handles_connection_error
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_handles_connection_error() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let stub_audit = StubAuditLog::new();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        // Transport with NO messages — will return None (EOF) immediately.
        let transport = StubTransport::with_messages(vec![]);

        let mut client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));

        let result = client.connect(server_info()).await;
        assert!(
            result.is_err(),
            "connection to unavailable server should error"
        );
    }

    // =====================================================================
    // T-031 Test 5: mcp_client_handles_disconnect
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_handles_disconnect() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let (mut client, _transport, _entries) = make_connected_client(gate);

        client.connect(server_info()).await.unwrap();
        client.disconnect().await.unwrap();

        // Tools should be cleared after disconnect.
        let result = client.list_tools().await;
        assert!(result.is_err(), "list_tools after disconnect should error");
    }

    // =====================================================================
    // T-031 Test 6: mcp_client_permission_denies_remote_tool
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_permission_denies_remote_tool() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::new()); // NO grants
        let stub_audit = StubAuditLog::new();
        let entries = stub_audit.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        let transport =
            StubTransport::with_messages(vec![init_response(1), tools_list_response(2)]);

        let mut client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));

        client.connect(server_info()).await.unwrap();

        let result = client.call_tool("list_workflows", "{}").await.unwrap();
        assert!(
            matches!(result, ToolResult::Err(ref s) if s.contains("permission denied")),
            "expected permission denied, got {result:?}"
        );

        // Denial is audited.
        let audit_entries = entries.lock().unwrap().clone();
        let denied = audit_entries
            .iter()
            .find(|e| e.result.contains("denied"))
            .expect("denial should be audited");
        assert_eq!(denied.resource, "mcp://tools/list_workflows");
    }

    // =====================================================================
    // T-031 Test 7: mcp_client_audits_tool_invocation
    // =====================================================================
    #[tokio::test]
    async fn mcp_client_audits_tool_invocation() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let stub_audit = StubAuditLog::new();
        let entries = stub_audit.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        let transport = StubTransport::with_messages(vec![
            init_response(1),
            tools_list_response(2),
            tools_call_response(3, "result-ok"),
        ]);

        let mut client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));

        client.connect(server_info()).await.unwrap();
        client.call_tool("list_workflows", "{}").await.unwrap();

        let audit_entries = entries.lock().unwrap().clone();
        let invoked = audit_entries
            .iter()
            .find(|e| e.action == "mcp.client.invoked" && e.result.starts_with("ok:"))
            .expect("invocation should be audited");
        assert!(invoked.result.contains("result-ok"));
    }

    // =====================================================================
    // T-031 Test 8: client_records_discovery_audit_on_first_connect
    // =====================================================================
    #[tokio::test]
    async fn client_records_discovery_audit_on_first_connect() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let stub_audit = StubAuditLog::new();
        let entries = stub_audit.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        let transport =
            StubTransport::with_messages(vec![init_response(1), tools_list_response(2)]);

        let mut client = McpClientImpl::new(gate, log).with_transport(Box::new(transport));

        // First connect — should record approval_required audit.
        client.connect(server_info()).await.unwrap();

        let audit_entries = entries.lock().unwrap().clone();
        let discovered = audit_entries
            .iter()
            .find(|e| e.action == "mcp.client.discovered")
            .expect("discovery should be audited on first connect");
        assert_eq!(discovered.result, "approval_required");
    }

    // =====================================================================
    // T-031 Test 9: list_tools_errors_when_not_connected
    // =====================================================================
    #[tokio::test]
    async fn list_tools_errors_when_not_connected() {
        let gate: Arc<dyn PermissionGate> = Arc::new(StubPermissionGate::unrestricted());
        let stub_audit = StubAuditLog::new();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> =
            Arc::new(TokioMutex::new(Box::new(stub_audit)));

        let client = McpClientImpl::new(gate, log)
            .with_transport(Box::new(StubTransport::with_messages(vec![])));

        let result = client.list_tools().await;
        assert!(result.is_err(), "list_tools before connect should error");
    }
}
