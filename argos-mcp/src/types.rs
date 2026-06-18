//! MCP domain types — bidirectional Model Context Protocol.
//!
//! Server-exposed types (McpToolProxy, McpServerConfig, McpServerState) and
//! client-side types (McpServerInfo, McpTransportType). Capability negotiation
//! objects flow in both directions. These are MCP-specific — they live here,
//! not in argos-core.

use serde::{Deserialize, Serialize};

use argos_agent::registry::ToolHandler;

/// Capabilities negotiated during the MCP `initialize` handshake.
///
/// Both sides declare what they support; the intersection becomes the
/// negotiated set exposed to each peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
    pub sampling: bool,
}

impl Default for McpCapabilities {
    fn default() -> Self {
        Self {
            tools: true,
            resources: false,
            prompts: false,
            sampling: false,
        }
    }
}

/// Server identity sent during `initialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub version: String,
}

/// Lifecycle state of the MCP server or client connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerState {
    Started,
    Stopped,
    Degraded(String),
}

/// How an MCP peer connects (transport layer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportType {
    Stdio,
    Http,
}

/// Information needed to discover and connect to an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    /// e.g. `"http://localhost:5678/mcp"` or a stdio command
    pub endpoint: String,
    pub transport: McpTransportType,
}

/// A tool exposed by the ArgOS MCP server.
///
/// Maps a tool name to its async handler. Registered during server setup;
/// every exposure emits an `McpToolExposed` audit event.
pub struct McpToolProxy {
    pub name: String,
    pub handler: Box<dyn ToolHandler>,
}

/// Audit event recorded when a tool is exposed to MCP clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolExposedEvent {
    pub tool_name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_have_only_tools_enabled() {
        let caps = McpCapabilities::default();
        assert!(caps.tools);
        assert!(!caps.resources);
        assert!(!caps.prompts);
        assert!(!caps.sampling);
    }

    #[test]
    fn capabilities_roundtrip_json() {
        let caps = McpCapabilities {
            tools: true,
            resources: true,
            prompts: false,
            sampling: false,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: McpCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
        assert!(json.contains("tools"));
        assert!(json.contains("resources"));
    }

    #[test]
    fn server_config_constructs() {
        let cfg = McpServerConfig {
            name: "argos-mcp".into(),
            version: "0.1.0".into(),
        };
        assert_eq!(cfg.name, "argos-mcp");
        assert_eq!(cfg.version, "0.1.0");
    }

    #[test]
    fn server_config_roundtrip_json() {
        let cfg = McpServerConfig {
            name: "argos-mcp".into(),
            version: "0.1.0".into(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn server_state_variants() {
        assert_eq!(McpServerState::Started, McpServerState::Started);
        assert_eq!(McpServerState::Stopped, McpServerState::Stopped);
        assert_eq!(
            McpServerState::Degraded("unreachable".into()),
            McpServerState::Degraded("unreachable".into())
        );
    }

    #[test]
    fn transport_type_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&McpTransportType::Stdio).unwrap(),
            "\"stdio\""
        );
        assert_eq!(
            serde_json::to_string(&McpTransportType::Http).unwrap(),
            "\"http\""
        );
    }

    #[test]
    fn server_info_constructs_and_roundtrips() {
        let info = McpServerInfo {
            name: "n8n-mcp".into(),
            endpoint: "http://localhost:5678/mcp".into(),
            transport: McpTransportType::Http,
        };
        assert_eq!(info.name, "n8n-mcp");
        assert_eq!(info.transport, McpTransportType::Http);
        let json = serde_json::to_string(&info).unwrap();
        let back: McpServerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn mcp_tool_exposed_event_serializes() {
        let event = McpToolExposedEvent {
            tool_name: "wiki.query".into(),
            description: "Query OKF wiki".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("wiki.query"));
        assert!(json.contains("Query OKF wiki"));
        let back: McpToolExposedEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }
}
