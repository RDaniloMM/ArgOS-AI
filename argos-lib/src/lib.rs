//! ArgOS — AI Operating System (unified library facade).
//!
//! This crate re-exports the public API of all ArgOS bounded contexts.
//! Use it when you need the full ArgOS surface without depending on
//! individual crates.
//!
//! # Bounded Contexts
//! - **core**: Domain types (AgentState, Tool, Error, etc.)
//! - **storage**: Storage traits + implementations (VectorStore, BlobStore, SqliteRelationalStore, etc.)
//! - **provider**: LLM provider abstraction (Provider trait, OllamaProvider, OpenAICompatibleProvider)
//! - **security**: Permission, secrets, audit (PermissionGate, SecretVault, AuditLog)
//! - **knowledge**: OKF wiki (BundleStore, IngestOp, QueryOp, LintReport)
//! - **n8n**: n8n connector (N8nClient, WorkflowImporter, WorkflowRunner)
//! - **intelligence**: Workflow reuse (ReuseRecommender)
//! - **agent**: Agent runtime (GenericAgent, ToolRegistry, ToolHandler)
//! - **mcp**: MCP bidirectional server/client (McpServer, McpClient)
//! - **wasm**: WASM extension stub (WasmRuntime)

#![warn(missing_docs)]

// Core domain re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_core::*;

// Storage re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_storage::*;

// Provider re-exports
pub use argos_provider::*;

// Security re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_security::*;

// Knowledge re-exports
pub use argos_knowledge::*;

// n8n connector re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_n8n_connector::*;

// Workflow intelligence re-exports
pub use argos_workflow_intelligence::*;

// Agent re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_agent::*;

// MCP re-exports
#[allow(ambiguous_glob_reexports)]
pub use argos_mcp::*;

// WASM re-exports
pub use argos_wasm::*;

#[cfg(test)]
mod tests {
    #[test]
    fn re_exports_core_types() {
        use crate as argos_lib;
        let _state: argos_lib::AgentState = argos_lib::AgentState::Idle;
        let _tool = argos_lib::Tool {
            name: "test".into(),
            description: "test tool".into(),
        };
    }

    #[test]
    fn re_exports_agent_types() {
        use crate as argos_lib;
        let _registry = argos_lib::ToolRegistry::new();
    }

    #[test]
    fn re_exports_storage_types() {
        use crate::VectorStore;
        let _ = std::any::type_name::<dyn VectorStore>();
    }

    #[test]
    fn re_exports_security_types() {
        use crate::PermissionGate;
        let _ = std::any::type_name::<dyn PermissionGate>();
    }

    #[test]
    fn re_exports_provider_types() {
        use crate::Provider;
        let _ = std::any::type_name::<dyn Provider>();
    }

    #[test]
    fn re_exports_knowledge_types() {
        use crate::BundleStore;
        let _ = std::any::type_name::<BundleStore>();
    }

    #[test]
    fn re_exports_n8n_types() {
        use crate::N8nClient;
        let _ = std::any::type_name::<dyn N8nClient>();
    }

    #[test]
    fn re_exports_mcp_types() {
        use crate::McpServer;
        let _ = std::any::type_name::<dyn McpServer>();
    }

    #[test]
    fn re_exports_wasm_types() {
        use crate::WasmRuntime;
        let _ = std::any::type_name::<dyn WasmRuntime>();
    }

    #[test]
    fn re_exports_error_type() {
        use crate as argos_lib;
        let err = argos_lib::ArgosError::Config("test".into());
        assert!(matches!(err, argos_lib::ArgosError::Config(_)));
    }
}
