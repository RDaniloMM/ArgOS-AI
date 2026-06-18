//! MCP client trait — ArgOS consuming n8n MCP tools (T-031).
//!
//! The [`McpClient`] trait abstracts MCP server discovery and tool invocation.
//! The concrete [`McpClientImpl`](super::client_impl::McpClientImpl) handles
//! the JSON-RPC handshake, capability negotiation, tool discovery, and
//! permission-gated invocation.

use argos_agent::registry::ToolInfo;
use argos_core::{Result, ToolResult};
use async_trait::async_trait;

use super::types::{McpCapabilities, McpServerInfo};

/// MCP client abstraction — ArgOS as MCP client.
///
/// Discovers and invokes tools on a remote MCP server (n8n or other).
/// Every tool call is permission-gated and audited.
#[async_trait]
pub trait McpClient: Send + Sync {
    /// Connect to the MCP server described by `server_info`.
    /// Performs the `initialize` handshake, negotiates capabilities, and
    /// discovers available tools via `tools/list`.
    async fn connect(&mut self, server_info: McpServerInfo) -> Result<()>;
    /// Disconnect from the server and mark tools unavailable.
    async fn disconnect(&mut self) -> Result<()>;
    /// List tools discovered during the `initialize` handshake.
    async fn list_tools(&self) -> Result<Vec<ToolInfo>>;
    /// Invoke a remote tool by name with JSON `args`.
    async fn call_tool(&self, name: &str, args: &str) -> Result<ToolResult>;
    /// Return the negotiated capabilities from the last handshake.
    async fn capabilities(&self) -> McpCapabilities;
}
