//! Tool registry — maps tool names to async handlers (T-028, ADR-003 Tier-1).
//!
//! The registry is the agent's catalog of compiled Tier-1 tools. Each tool is a
//! name + description + [`ToolHandler`] (an async closure-like object). The
//! agent loop consults [`ToolRegistry::list`] to advertise available tools to
//! the LLM and [`ToolRegistry::invoke`] to dispatch a tool call returned by the
//! LLM. Concrete Tier-1 tools (wiki ingest/query/lint, n8n list/run, workflow
//! reuse) live in [`crate::tools`].
//!
//! Slice 1 keeps the registry in-process and ungated — permission gating wraps
//! `invoke` in a later task (spec: "Permission Gating").

use argos_core::{Result, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Metadata describing a registered tool (name + description). The agent loop
/// serialises this list into the LLM system prompt so the model knows which
/// tools it can call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    /// The tool name (e.g. `wiki.ingest`, `n8n.run`).
    pub name: String,
    /// Human/LLM-readable description of what the tool does.
    pub description: String,
}

/// Async handler for a single tool. Implementations capture shared services
/// (via `Arc`) and execute in-process when the agent loop dispatches a tool
/// call. The `args` string is JSON; each tool parses what it needs.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Execute the tool with JSON `args`, returning a [`ToolResult`].
    async fn invoke(&self, args: &str) -> Result<ToolResult>;
}

/// A registry of available Tier-1 tools.
///
/// Tools are stored in insertion order so [`ToolRegistry::list`] is stable
/// (deterministic system prompts). Lookup by name is linear — the catalog is
/// small (single-digit tools in slice 1) and a `HashMap` would not pay for
/// itself.
pub struct ToolRegistry {
    tools: Vec<(ToolInfo, Box<dyn ToolHandler>)>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool under `name` with `description` and `handler`. Order is
    /// preserved for deterministic [`Self::list`] output.
    pub fn register(&mut self, name: &str, description: &str, handler: Box<dyn ToolHandler>) {
        self.tools.push((
            ToolInfo {
                name: name.to_string(),
                description: description.to_string(),
            },
            handler,
        ));
    }

    /// Look up a tool's metadata by name.
    pub fn get(&self, name: &str) -> Option<&ToolInfo> {
        self.tools
            .iter()
            .find(|(info, _)| info.name == name)
            .map(|(info, _)| info)
    }

    /// List every registered tool's metadata, in insertion order.
    pub fn list(&self) -> Vec<&ToolInfo> {
        self.tools.iter().map(|(info, _)| info).collect()
    }

    /// Invoke the tool named `name` with JSON `args`. Returns
    /// [`ArgosError::NotFound`] when the tool is not registered.
    pub async fn invoke(&self, name: &str, args: &str) -> Result<ToolResult> {
        let handler = self
            .tools
            .iter()
            .find(|(info, _)| info.name == name)
            .map(|(_, h)| h.as_ref())
            .ok_or_else(|| argos_core::ArgosError::NotFound(format!("tool not found: {name}")))?;
        handler.invoke(args).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EchoHandler;

    #[test]
    fn registry_constructs_empty() {
        let reg = ToolRegistry::new();
        assert!(reg.list().is_empty(), "fresh registry should have no tools");
    }

    #[test]
    fn register_adds_tool_and_get_returns_it() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "echo",
            "Echoes back a canned result",
            Box::new(EchoHandler {
                result: ToolResult::Ok("ok".into()),
            }),
        );
        let info = reg.get("echo").expect("echo should be registered");
        assert_eq!(info.name, "echo");
        assert_eq!(info.description, "Echoes back a canned result");
    }

    #[test]
    fn get_on_unknown_tool_returns_none() {
        let reg = ToolRegistry::new();
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn list_returns_all_registered_tools_in_order() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "a",
            "first",
            Box::new(EchoHandler {
                result: ToolResult::Ok("a".into()),
            }),
        );
        reg.register(
            "b",
            "second",
            Box::new(EchoHandler {
                result: ToolResult::Ok("b".into()),
            }),
        );
        let names: Vec<&str> = reg.list().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn invoke_calls_handler_and_returns_ok_result() {
        let mut reg = ToolRegistry::new();
        reg.register(
            "echo",
            "echo",
            Box::new(EchoHandler {
                result: ToolResult::Ok("canned payload".into()),
            }),
        );
        let result = reg.invoke("echo", r#"{"x":1}"#).await.unwrap();
        assert_eq!(result, ToolResult::Ok("canned payload".into()));
    }

    #[tokio::test]
    async fn invoke_propagates_handler_err_result() {
        // The handler runs successfully but produces a ToolResult::Err — the
        // registry must surface it as Ok(Err) (tool ran, tool-level failure),
        // not as a registry-level Err.
        let mut reg = ToolRegistry::new();
        reg.register(
            "failing-tool",
            "always fails at the tool level",
            Box::new(EchoHandler {
                result: ToolResult::Err("bad args".into()),
            }),
        );
        let result = reg.invoke("failing-tool", "{}").await.unwrap();
        assert_eq!(result, ToolResult::Err("bad args".into()));
    }

    #[tokio::test]
    async fn invoke_on_unknown_tool_returns_error() {
        let reg = ToolRegistry::new();
        let err = reg.invoke("missing", "{}").await.unwrap_err();
        assert!(
            matches!(err, argos_core::ArgosError::NotFound(ref m) if m.contains("missing")),
            "expected NotFound error, got {err:?}"
        );
    }

    #[test]
    fn tool_info_constructs_and_serializes() {
        let info = ToolInfo {
            name: "wiki.query".into(),
            description: "Query the OKF wiki".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("wiki.query"));
        let back: ToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn tool_result_ok_and_err_variants() {
        let ok = ToolResult::Ok("payload".into());
        let err = ToolResult::Err("reason".into());
        assert!(matches!(ok, ToolResult::Ok(ref s) if s == "payload"));
        assert!(matches!(err, ToolResult::Err(ref s) if s == "reason"));
        assert_ne!(ok, err);
    }
}
