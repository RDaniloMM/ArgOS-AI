//! n8n connector domain types.
//!
//! ArgOS does not execute workflows — n8n does. These types model the
//! connection to an n8n instance and the references ArgOS holds to workflows
//! and runs that live on the n8n side.

use serde::{Deserialize, Serialize};
use url::Url;

/// How ArgOS connects to an n8n instance.
///
/// MCP is the preferred transport (n8n has native MCP server support).
/// REST is a fallback when MCP is unavailable or for operations not yet
/// exposed via MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnMode {
    /// Connect via n8n's MCP server (preferred).
    Mcp,
    /// Connect via n8n's REST API (fallback).
    Rest,
}

/// Configuration for connecting to an n8n instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct N8nConnection {
    /// The n8n base URL (e.g. `http://localhost:5678`).
    pub endpoint: Url,
    /// The transport mode (MCP preferred, REST fallback).
    pub mode: ConnMode,
    /// Reference to the API key or token stored in the SecretVault.
    /// Never the raw secret — always a lookup key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
}

/// A reference to a workflow that lives in n8n.
///
/// ArgOS stores these in OKF concepts (`resource: n8n://workflows/<id>`)
/// and in the SQLite `n8n_workflow_refs` table. The workflow definition
/// itself is never copied into ArgOS — n8n is the source of truth for
/// workflow execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct N8nWorkflowRef {
    /// The n8n workflow ID.
    pub id: String,
    /// The workflow name as shown in n8n.
    pub name: String,
    /// Direct URL to the workflow in the n8n editor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
}

/// A reference to a workflow run that lives in n8n.
///
/// ArgOS mirrors run status for audit purposes but does not own the
/// execution state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct N8nRunRef {
    /// The n8n run/execution ID.
    pub id: String,
    /// The workflow this run belongs to.
    pub workflow_id: String,
    /// The current status of the run.
    pub status: N8nRunStatus,
}

/// The status of an n8n workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum N8nRunStatus {
    /// The run is currently executing.
    Running,
    /// The run completed successfully.
    Success,
    /// The run failed.
    Failed,
    /// The run was cancelled by the user or system.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n8n_connection_mcp_constructs() {
        let conn = N8nConnection {
            endpoint: Url::parse("http://localhost:5678").unwrap(),
            mode: ConnMode::Mcp,
            api_key_ref: Some("n8n_key".into()),
        };
        assert_eq!(conn.mode, ConnMode::Mcp);
        assert_eq!(conn.endpoint.as_str(), "http://localhost:5678/");
    }

    #[test]
    fn n8n_connection_rest_constructs() {
        let conn = N8nConnection {
            endpoint: Url::parse("http://localhost:5678").unwrap(),
            mode: ConnMode::Rest,
            api_key_ref: None,
        };
        assert_eq!(conn.mode, ConnMode::Rest);
        assert!(conn.api_key_ref.is_none());
    }

    #[test]
    fn conn_mode_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&ConnMode::Mcp).unwrap(), "\"mcp\"");
        assert_eq!(serde_json::to_string(&ConnMode::Rest).unwrap(), "\"rest\"");
    }

    #[test]
    fn n8n_workflow_ref_constructs() {
        let wf = N8nWorkflowRef {
            id: "42".into(),
            name: "Daily Report".into(),
            url: Some(Url::parse("http://localhost:5678/workflow/42").unwrap()),
        };
        assert_eq!(wf.id, "42");
        assert_eq!(wf.name, "Daily Report");
    }

    #[test]
    fn n8n_run_ref_constructs() {
        let run = N8nRunRef {
            id: "exec-100".into(),
            workflow_id: "42".into(),
            status: N8nRunStatus::Running,
        };
        assert_eq!(run.workflow_id, "42");
        assert_eq!(run.status, N8nRunStatus::Running);
    }

    #[test]
    fn n8n_run_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&N8nRunStatus::Success).unwrap(),
            "\"success\""
        );
        assert_eq!(
            serde_json::to_string(&N8nRunStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&N8nRunStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }
}
