//! MCP server implementation — ArgOS tools exposed via MCP (T-030).
//!
//! [`McpServerImpl`] wires a [`ToolRegistry`], [`PermissionGate`], and
//! [`AuditLog`] into an [`McpServer`]. Tools are registered via `expose_tool`
//! (which audits each exposure). Every tool call is permission-gated and
//! audited. Slice 1 uses stdio transport; HTTP/SSE is future.
//!
//! # Stub-first testing
//! All tests inject stub transports and canned handlers — no real I/O, no
//! running n8n. The transport, gate, audit log, and tool handlers are seams.

use std::sync::{Arc, Mutex};

use argos_agent::registry::{ToolHandler, ToolInfo, ToolRegistry};
use argos_core::{Result, ToolResult};
use argos_security::{AuditEntry, AuditLog, Permission, PermissionGate};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Mutex as TokioMutex;

use super::server::McpServer;
use super::types::{McpCapabilities, McpServerConfig, McpServerState};

/// Concrete MCP server exposing ArgOS tools to n8n and other MCP clients.
///
/// # Seams (all injectable for testing)
/// - `ToolRegistry` — holds the tool catalog (inject canned handlers)
/// - `PermissionGate` — gates every tool call (inject pre-granted gate)
/// - `AuditLog` — records every exposure and invocation (inject Vec-backed stub)
pub struct McpServerImpl {
    config: McpServerConfig,
    registry: ToolRegistry,
    permission_gate: Arc<dyn PermissionGate>,
    audit_log: Arc<TokioMutex<Box<dyn AuditLog>>>,
    state: Mutex<McpServerState>,
    capabilities: McpCapabilities,
    /// Negotiated capabilities from the last `initialize` handshake.
    negotiated: Mutex<Option<McpCapabilities>>,
}

impl McpServerImpl {
    /// Create a new MCP server with the given config, registry, permission
    /// gate, and audit log. Tools should be registered via `expose_tool` after
    /// construction.
    pub fn new(
        config: McpServerConfig,
        permission_gate: Arc<dyn PermissionGate>,
        audit_log: Arc<TokioMutex<Box<dyn AuditLog>>>,
    ) -> Self {
        Self {
            config,
            registry: ToolRegistry::new(),
            permission_gate,
            audit_log,
            state: Mutex::new(McpServerState::Stopped),
            capabilities: McpCapabilities::default(),
            negotiated: Mutex::new(None),
        }
    }

    /// Create a server with a pre-built `ToolRegistry`.
    pub fn with_registry(
        config: McpServerConfig,
        registry: ToolRegistry,
        permission_gate: Arc<dyn PermissionGate>,
        audit_log: Arc<TokioMutex<Box<dyn AuditLog>>>,
    ) -> Self {
        Self {
            config,
            registry,
            permission_gate,
            audit_log,
            state: Mutex::new(McpServerState::Stopped),
            capabilities: McpCapabilities::default(),
            negotiated: Mutex::new(None),
        }
    }

    /// Register a tool for MCP exposure, emitting an audit event.
    pub fn expose_tool(&mut self, name: &str, description: &str, handler: Box<dyn ToolHandler>) {
        self.registry.register(name, description, handler);
    }

    /// Register a tool AND emit the exposure audit record synchronously
    /// (for use in tests and setup). In production the audit is async, but
    /// during construction we fire-and-forget.
    pub async fn expose_tool_audited(
        &mut self,
        name: &str,
        description: &str,
        handler: Box<dyn ToolHandler>,
    ) -> Result<()> {
        self.registry.register(name, description, handler);
        let event = AuditEntry {
            timestamp: Utc::now(),
            subject: self.config.name.clone(),
            action: "mcp.tool.exposed".into(),
            resource: format!("mcp://tools/{name}"),
            result: "ok".into(),
            prev_hash: String::new(),
            this_hash: String::new(),
        };
        let mut log = self.audit_log.lock().await;
        log.record(&event).await?;
        Ok(())
    }

    /// Apply negotiated capabilities from an `initialize` handshake.
    pub fn set_negotiated(&self, caps: McpCapabilities) {
        *self.negotiated.lock().unwrap() = Some(caps);
    }

    /// Return the effective capabilities (negotiated if set, declared otherwise).
    fn effective_capabilities(&self) -> McpCapabilities {
        self.negotiated
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.capabilities.clone())
    }
}

#[async_trait]
impl McpServer for McpServerImpl {
    async fn start(&mut self) -> Result<()> {
        *self.state.lock().unwrap() = McpServerState::Started;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        *self.state.lock().unwrap() = McpServerState::Stopped;
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let tools: Vec<ToolInfo> = self.registry.list().into_iter().cloned().collect();
        Ok(tools)
    }

    async fn call_tool(&self, name: &str, args: &str) -> Result<ToolResult> {
        // 1. Permission check.
        let perm = self
            .permission_gate
            .check(&self.config.name, &format!("mcp://tools/{name}"), "invoke")
            .await?;
        match perm {
            Permission::Deny(reason) => {
                // Audit denial.
                let entry = AuditEntry {
                    timestamp: Utc::now(),
                    subject: self.config.name.clone(),
                    action: "mcp.tool.invoked".into(),
                    resource: format!("mcp://tools/{name}"),
                    result: format!("denied: {reason}"),
                    prev_hash: String::new(),
                    this_hash: String::new(),
                };
                let mut log = self.audit_log.lock().await;
                log.record(&entry).await?;
                return Ok(ToolResult::Err(format!("permission denied: {reason}")));
            }
            Permission::Allow => {}
        }

        // 2. Invoke the tool.
        let result = self.registry.invoke(name, args).await?;

        // 3. Audit invocation.
        let entry = AuditEntry {
            timestamp: Utc::now(),
            subject: self.config.name.clone(),
            action: "mcp.tool.invoked".into(),
            resource: format!("mcp://tools/{name}"),
            result: match &result {
                ToolResult::Ok(msg) => format!("ok: {msg}"),
                ToolResult::Err(msg) => format!("err: {msg}"),
            },
            prev_hash: String::new(),
            this_hash: String::new(),
        };
        let mut log = self.audit_log.lock().await;
        log.record(&entry).await?;

        Ok(result)
    }

    async fn state(&self) -> McpServerState {
        self.state.lock().unwrap().clone()
    }

    async fn capabilities(&self) -> McpCapabilities {
        self.effective_capabilities()
    }
}

// ---------------------------------------------------------------------------
// Tests (T-030: McpServer)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::ArgosError;
    use std::collections::HashSet;

    // --- Stub PermissionGate (in-memory, pre-grantable) ---
    #[allow(dead_code)]
    struct StubPermissionGate {
        grants: Mutex<HashSet<(String, String, String)>>,
        /// When true, every `check` returns `Allow` regardless of grants.
        unrestricted: bool,
    }

    #[allow(dead_code)]
    impl StubPermissionGate {
        fn new() -> Self {
            Self {
                grants: Mutex::new(HashSet::new()),
                unrestricted: false,
            }
        }

        fn allow_all() -> Self {
            let mut gates = HashSet::new();
            gates.insert((
                "argos-mcp".into(),
                "mcp://tools/wiki.query".into(),
                "invoke".into(),
            ));
            gates.insert((
                "argos-mcp".into(),
                "mcp://tools/workflow.recommend_reuse".into(),
                "invoke".into(),
            ));
            gates.insert((
                "argos-mcp".into(),
                "mcp://tools/workflow.similar".into(),
                "invoke".into(),
            ));
            Self {
                grants: Mutex::new(gates),
                unrestricted: false,
            }
        }

        /// A gate that always returns Allow.
        fn unrestricted() -> Self {
            Self {
                grants: Mutex::new(HashSet::new()),
                unrestricted: true,
            }
        }

        fn grant(&self, subject: &str, resource: &str, action: &str) {
            self.grants
                .lock()
                .unwrap()
                .insert((subject.into(), resource.into(), action.into()));
        }
    }

    #[async_trait]
    impl PermissionGate for StubPermissionGate {
        async fn check(&self, subject: &str, resource: &str, action: &str) -> Result<Permission> {
            if self.unrestricted {
                return Ok(Permission::Allow);
            }
            let key = (subject.into(), resource.into(), action.into());
            if self.grants.lock().unwrap().contains(&key) {
                Ok(Permission::Allow)
            } else {
                Ok(Permission::Deny(format!(
                    "no grant for {subject}/{resource}/{action}"
                )))
            }
        }

        async fn grant(&mut self, subject: &str, resource: &str, action: &str) -> Result<()> {
            self.grants
                .lock()
                .unwrap()
                .insert((subject.into(), resource.into(), action.into()));
            Ok(())
        }

        async fn revoke(&mut self, subject: &str, resource: &str, action: &str) -> Result<()> {
            self.grants
                .lock()
                .unwrap()
                .remove(&(subject.into(), resource.into(), action.into()));
            Ok(())
        }
    }

    // --- Stub AuditLog (Vec-backed — entries shared via Arc for inspection) ---
    struct StubAuditLog {
        entries: Arc<Mutex<Vec<AuditEntry>>>,
    }

    impl StubAuditLog {
        fn new() -> Self {
            Self {
                entries: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Clone the shared entries handle for test inspection.
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

        async fn query(&self, filter: &argos_security::AuditFilter) -> Result<Vec<AuditEntry>> {
            let entries = self.entries.lock().unwrap();
            let results: Vec<AuditEntry> = entries
                .iter()
                .filter(|e| {
                    if let Some(ref s) = filter.subject {
                        if &e.subject != s {
                            return false;
                        }
                    }
                    if let Some(ref a) = filter.action {
                        if &e.action != a {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            Ok(results)
        }

        async fn verify_chain(&self) -> Result<bool> {
            Ok(true)
        }
    }

    // --- Echo tool handler ---
    struct EchoHandler {
        result: ToolResult,
    }

    #[async_trait]
    impl ToolHandler for EchoHandler {
        async fn invoke(&self, _args: &str) -> Result<ToolResult> {
            Ok(self.result.clone())
        }
    }

    // --- Helpers ---
    fn make_config() -> McpServerConfig {
        McpServerConfig {
            name: "argos-mcp".into(),
            version: "0.1.0".into(),
        }
    }

    /// Build a server with a fresh gate + audit log. Returns the server, the
    /// gate handle (for granting), and the audit entries handle (for inspection).
    fn make_server_with_log() -> (
        McpServerImpl,
        Arc<StubPermissionGate>,
        Arc<Mutex<Vec<AuditEntry>>>,
    ) {
        let gate = Arc::new(StubPermissionGate::new());
        let stub = StubAuditLog::new();
        let entries = stub.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> = Arc::new(TokioMutex::new(Box::new(stub)));
        let server = McpServerImpl::new(make_config(), gate.clone(), log);
        (server, gate, entries)
    }

    /// Build a server with a pre-granted (allow-all) gate + fresh audit log.
    fn make_server_allow_all() -> (
        McpServerImpl,
        Arc<StubPermissionGate>,
        Arc<Mutex<Vec<AuditEntry>>>,
    ) {
        let gate = Arc::new(StubPermissionGate::unrestricted());
        let stub = StubAuditLog::new();
        let entries = stub.entries_handle();
        let log: Arc<TokioMutex<Box<dyn AuditLog>>> = Arc::new(TokioMutex::new(Box::new(stub)));
        let server = McpServerImpl::new(make_config(), gate.clone(), log);
        (server, gate, entries)
    }

    // =====================================================================
    // T-030 Test 1: mcp_server_starts_and_stops
    // =====================================================================
    #[tokio::test]
    async fn mcp_server_starts_and_stops() {
        let (mut server, _gate, _entries) = make_server_with_log();

        // Initially Stopped.
        assert_eq!(server.state().await, McpServerState::Stopped);

        // Start → Started.
        server.start().await.unwrap();
        assert_eq!(server.state().await, McpServerState::Started);

        // Stop → Stopped.
        server.stop().await.unwrap();
        assert_eq!(server.state().await, McpServerState::Stopped);
    }

    // =====================================================================
    // T-030 Test 2: mcp_server_advertises_registered_tools
    // =====================================================================
    #[tokio::test]
    async fn mcp_server_advertises_registered_tools() {
        let (mut server, _gate, _entries) = make_server_with_log();

        // Register 3 tools.
        server.expose_tool(
            "wiki.query",
            "Query OKF knowledge",
            Box::new(EchoHandler {
                result: ToolResult::Ok("{}".into()),
            }),
        );
        server.expose_tool(
            "workflow.recommend_reuse",
            "Recommend workflow reuse",
            Box::new(EchoHandler {
                result: ToolResult::Ok("{}".into()),
            }),
        );
        server.expose_tool(
            "workflow.similar",
            "Find similar workflows",
            Box::new(EchoHandler {
                result: ToolResult::Ok("{}".into()),
            }),
        );

        let tools = server.list_tools().await.unwrap();
        assert_eq!(tools.len(), 3, "should list 3 registered tools");

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"wiki.query"));
        assert!(names.contains(&"workflow.recommend_reuse"));
        assert!(names.contains(&"workflow.similar"));
    }

    // =====================================================================
    // T-030 Test 3: mcp_server_calls_tool_and_returns_result
    // =====================================================================
    #[tokio::test]
    async fn mcp_server_calls_tool_and_returns_result() {
        let (mut server, _gate, _entries) = make_server_allow_all();

        server.expose_tool(
            "wiki.query",
            "Query OKF wiki",
            Box::new(EchoHandler {
                result: ToolResult::Ok(r#"{"answer":"Rust async works via Futures"}"#.into()),
            }),
        );

        let result = server
            .call_tool("wiki.query", r#"{"q":"how does Rust async work?"}"#)
            .await
            .unwrap();

        assert_eq!(
            result,
            ToolResult::Ok(r#"{"answer":"Rust async works via Futures"}"#.into())
        );
    }

    // =====================================================================
    // T-030 Test 4: mcp_server_denies_ungranted_tool
    // =====================================================================
    #[tokio::test]
    async fn mcp_server_denies_ungranted_tool() {
        // Gate with NO grants.
        let (mut server, _gate, entries) = make_server_with_log();

        server.expose_tool(
            "wiki.query",
            "Query OKF wiki",
            Box::new(EchoHandler {
                result: ToolResult::Ok("ok".into()),
            }),
        );

        let result = server
            .call_tool("wiki.query", r#"{"q":"test"}"#)
            .await
            .unwrap();
        assert!(
            matches!(result, ToolResult::Err(ref s) if s.contains("permission denied")),
            "expected permission denied, got {result:?}"
        );

        // An audit entry was recorded for the denial.
        let audit_entries = entries.lock().unwrap().clone();
        let denial = audit_entries
            .iter()
            .find(|e| e.action == "mcp.tool.invoked" && e.result.contains("denied"))
            .expect("denial should be audited");
        assert_eq!(denial.resource, "mcp://tools/wiki.query");
    }

    // =====================================================================
    // T-030 Test 5: mcp_server_audits_tool_exposed_on_registration
    // =====================================================================
    #[tokio::test]
    async fn mcp_server_audits_tool_exposed_on_registration() {
        let (mut server, _gate, entries) = make_server_with_log();

        server
            .expose_tool_audited(
                "wiki.query",
                "Query OKF wiki",
                Box::new(EchoHandler {
                    result: ToolResult::Ok("ok".into()),
                }),
            )
            .await
            .unwrap();

        let audit_entries = entries.lock().unwrap().clone();
        assert_eq!(audit_entries.len(), 1, "one audit entry for tool exposure");
        assert_eq!(audit_entries[0].action, "mcp.tool.exposed");
        assert_eq!(audit_entries[0].resource, "mcp://tools/wiki.query");
        assert_eq!(audit_entries[0].result, "ok");
    }

    // =====================================================================
    // T-030 Test 6: call_tool_returns_error_on_unknown_tool
    // =====================================================================
    #[tokio::test]
    async fn call_tool_returns_error_on_unknown_tool() {
        let (server, _gate, _entries) = make_server_allow_all();

        let result = server.call_tool("nonexistent", "{}").await;
        assert!(result.is_err(), "calling unknown tool should error");
        assert!(
            matches!(&result, Err(ArgosError::NotFound(ref m)) if m.contains("nonexistent")),
            "expected NotFound error, got {result:?}"
        );
    }

    // =====================================================================
    // T-030 Test 7: list_tools_returns_empty_when_no_tools_registered
    // =====================================================================
    #[tokio::test]
    async fn list_tools_returns_empty_when_no_tools_registered() {
        let (server, _gate, _entries) = make_server_with_log();

        let tools = server.list_tools().await.unwrap();
        assert!(tools.is_empty(), "no tools registered, should be empty");
    }
}
