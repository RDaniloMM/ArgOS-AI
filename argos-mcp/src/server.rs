//! MCP server trait — ArgOS exposing tools to n8n and other MCP clients (T-030).
//!
//! The [`McpServer`] trait abstracts the server lifecycle: start/stop, tool
//! listing, tool invocation, capability negotiation, and state tracking. The
//! concrete [`McpServerImpl`](super::server_impl::McpServerImpl) wires a
//! [`ToolRegistry`], [`PermissionGate`], and [`AuditLog`] together.

use argos_agent::registry::ToolInfo;
use argos_core::{Result, ToolResult};
use async_trait::async_trait;

use super::types::{McpCapabilities, McpServerState};

/// MCP server abstraction — ArgOS as MCP server.
///
/// Exposes ArgOS tools (wiki.query, workflow.recommend_reuse,
/// workflow.similar) to MCP clients (n8n and others). Every tool call is
/// permission-gated and audited.
#[async_trait]
pub trait McpServer: Send + Sync {
    /// Begin listening for MCP client connections.
    async fn start(&mut self) -> Result<()>;
    /// Stop listening and close active connections.
    async fn stop(&mut self) -> Result<()>;
    /// List every tool currently exposed via MCP.
    async fn list_tools(&self) -> Result<Vec<ToolInfo>>;
    /// Invoke an exposed tool by name with JSON `args`.
    async fn call_tool(&self, name: &str, args: &str) -> Result<ToolResult>;
    /// Current lifecycle state.
    async fn state(&self) -> McpServerState;
    /// The server's declared capabilities.
    async fn capabilities(&self) -> McpCapabilities;
}
