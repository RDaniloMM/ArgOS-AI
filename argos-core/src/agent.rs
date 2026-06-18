//! Agent runtime domain types.
//!
//! An agent is a core architectural entity in ArgOS — not a plugin. These
//! types model the agent identity, the Hand specialisation (future), the
//! lifecycle state machine, and the tool-call loop primitives.
//!
//! Slice 1 uses a single generic agent (`Hand::None`). The six specialised
//! Hands (Research, Automation, Coding, Knowledge, Operations, Planning)
//! are future specialisations of the same `Agent` trait.

use serde::{Deserialize, Serialize};

/// Unique identifier for an agent instance.
pub type AgentId = String;

/// The specialised role of an agent.
///
/// `None` is the generic agent used in slice 1. The six named Hands are
/// designed as future specialisations with scoped memory, tool sets, and
/// permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hand {
    /// Generic agent — no specialisation (slice 1 default).
    None,
    /// Autonomous research and investigation.
    Research,
    /// Workflow creation, modification, and execution.
    Automation,
    /// Code generation and software engineering.
    Coding,
    /// LLM-Wiki maintenance and knowledge management.
    Knowledge,
    /// System operations, deployment, monitoring.
    Operations,
    /// Task decomposition, scheduling, coordination.
    Planning,
}

/// The state of an agent in its lifecycle state machine.
///
/// Transitions follow the tool-call loop:
/// `Idle → Observing → Thinking → Acting → AwaitingTool → (Acting | Done | Failed)`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    /// Not yet started or has been reset.
    Idle,
    /// Gathering context (reading wiki, checking memory).
    Observing,
    /// Processing context and deciding next action (LLM completion).
    Thinking,
    /// Executing an action (tool invocation, workflow step).
    Acting,
    /// Waiting for a tool to return.
    AwaitingTool,
    /// Completed successfully.
    Done,
    /// Failed (error, timeout, or permission denied).
    Failed,
}

impl AgentState {
    /// Returns `true` if the transition from `self` to `next` is valid.
    ///
    /// This encodes the tool-call loop: Observe → Think → Act → (Await → Act)* → Done/Failed.
    pub fn can_transition_to(&self, next: &AgentState) -> bool {
        use AgentState::*;
        match self {
            Idle => matches!(next, Observing | Done | Failed),
            Observing => matches!(next, Thinking | Failed),
            Thinking => matches!(next, Acting | Done | Failed),
            Acting => matches!(next, AwaitingTool | Thinking | Done | Failed),
            AwaitingTool => matches!(next, Acting | Failed),
            Done => matches!(next, Idle),
            Failed => matches!(next, Idle),
        }
    }
}

/// Metadata describing a tool available to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// The tool name (e.g. `wiki.ingest`, `n8n.run`).
    pub name: String,
    /// Human/LLM-readable description of what the tool does.
    pub description: String,
}

/// A single tool invocation within the agent loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// The tool that was called.
    pub tool: Tool,
    /// The arguments passed (JSON string for provider compatibility).
    pub args: String,
    /// The result returned by the tool (JSON string), or an error message.
    pub result: ToolResult,
}

/// The outcome of a tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResult {
    /// The tool returned successfully (JSON payload).
    Ok(String),
    /// The tool failed with an error message.
    Err(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_state_idle_to_observing_is_valid() {
        assert!(AgentState::Idle.can_transition_to(&AgentState::Observing));
    }

    #[test]
    fn agent_state_observing_to_thinking_is_valid() {
        assert!(AgentState::Observing.can_transition_to(&AgentState::Thinking));
    }

    #[test]
    fn agent_state_thinking_to_acting_is_valid() {
        assert!(AgentState::Thinking.can_transition_to(&AgentState::Acting));
    }

    #[test]
    fn agent_state_acting_to_awaiting_tool_is_valid() {
        assert!(AgentState::Acting.can_transition_to(&AgentState::AwaitingTool));
    }

    #[test]
    fn agent_state_awaiting_tool_to_acting_is_valid() {
        assert!(AgentState::AwaitingTool.can_transition_to(&AgentState::Acting));
    }

    #[test]
    fn agent_state_acting_to_done_is_valid() {
        assert!(AgentState::Acting.can_transition_to(&AgentState::Done));
    }

    #[test]
    fn agent_state_done_to_idle_is_valid() {
        assert!(AgentState::Done.can_transition_to(&AgentState::Idle));
    }

    #[test]
    fn agent_state_idle_to_acting_is_invalid() {
        assert!(!AgentState::Idle.can_transition_to(&AgentState::Acting));
    }

    #[test]
    fn agent_state_thinking_to_awaiting_tool_is_invalid() {
        assert!(!AgentState::Thinking.can_transition_to(&AgentState::AwaitingTool));
    }

    #[test]
    fn hand_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Hand::None).unwrap(), "\"none\"");
        assert_eq!(
            serde_json::to_string(&Hand::Research).unwrap(),
            "\"research\""
        );
        assert_eq!(
            serde_json::to_string(&Hand::Knowledge).unwrap(),
            "\"knowledge\""
        );
    }

    #[test]
    fn tool_constructs() {
        let tool = Tool {
            name: "wiki.ingest".into(),
            description: "Ingest a source into the OKF wiki".into(),
        };
        assert_eq!(tool.name, "wiki.ingest");
    }

    #[test]
    fn tool_invocation_with_ok_result() {
        let inv = ToolInvocation {
            tool: Tool {
                name: "wiki.query".into(),
                description: "Query the wiki".into(),
            },
            args: r#"{"q":"rust async"}"#.into(),
            result: ToolResult::Ok(r#"{"answer":"..."}"#.into()),
        };
        assert_eq!(inv.tool.name, "wiki.query");
        assert!(matches!(inv.result, ToolResult::Ok(_)));
    }

    #[test]
    fn tool_invocation_with_err_result() {
        let inv = ToolInvocation {
            tool: Tool {
                name: "n8n.run".into(),
                description: "Run an n8n workflow".into(),
            },
            args: r#"{"id":"42"}"#.into(),
            result: ToolResult::Err("connection refused".into()),
        };
        assert!(matches!(inv.result, ToolResult::Err(_)));
    }
}
