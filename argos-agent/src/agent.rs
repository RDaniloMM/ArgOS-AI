//! Agent trait + lifecycle output type.
//!
//! The core agent abstraction. Lifecycle state is modelled by
//! `argos_core::AgentState`, whose `can_transition_to` encodes the tool-call
//! loop (Idle -> Observing -> Thinking -> Acting -> AwaitingTool -> Done|Failed).
//! Implementations drive transitions internally.
//!
//! The trait deliberately depends only on `argos_core` — permissions are a
//! plain `Vec<String>` here to avoid a circular dependency on `argos-security`.
//! When T-006's `PermissionSet` lands, the agent loop can consume it without
//! changing this trait's shape.

use argos_core::{AgentId, AgentState, Hand, Result, ToolInvocation};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// The outcome of an agent run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentOutput {
    /// The final text produced by the agent.
    pub text: String,
    /// Tool invocations performed during the run (audit/debug trail).
    pub tool_invocations: Vec<ToolInvocation>,
    /// The terminal lifecycle state (`Done` or `Failed`).
    pub final_state: AgentState,
    /// Cumulative prompt tokens across all LLM calls in this run.
    pub prompt_tokens: u64,
    /// Cumulative completion tokens across all LLM calls in this run.
    pub completion_tokens: u64,
}

/// The core agent abstraction.
///
/// Slice 1 ships a single generic agent (`Hand::None`) that drives the Reuse
/// Loop via `argos ask`. The six specialised Hands are future specialisations
/// of this same trait.
#[async_trait]
pub trait Agent: Send + Sync {
    /// The agent's unique identifier.
    async fn id(&self) -> &AgentId;
    /// The agent's Hand specialisation (slice 1: `Hand::None`).
    async fn hand(&self) -> Hand;
    /// Current lifecycle state.
    async fn state(&self) -> AgentState;
    /// Permissions granted to this agent, as plain strings.
    ///
    /// Kept as `Vec<String>` to avoid a circular dependency on `argos-security`;
    /// the loop consults `PermissionGate` for actual gating.
    async fn permissions(&self) -> Vec<String>;
    /// Run the agent against `input`, returning the final output.
    async fn run(&mut self, input: &str) -> Result<AgentOutput>;
    /// Reset the agent to `Idle`, ready for a new run.
    async fn reset(&mut self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{AgentId, AgentState, Hand};

    /// In-memory agent stub that drives the full lifecycle on `run`.
    struct StubAgent {
        id: AgentId,
        hand: Hand,
        state: AgentState,
        permissions: Vec<String>,
        transitions: Vec<AgentState>,
    }

    impl StubAgent {
        fn new() -> Self {
            Self {
                id: "stub-1".into(),
                hand: Hand::None,
                state: AgentState::Idle,
                permissions: vec!["wiki.read".into()],
                transitions: vec![AgentState::Idle],
            }
        }

        /// Advance one state, asserting the transition is allowed by the
        /// `argos_core::AgentState` table. Records the step for inspection.
        fn transition(&mut self, next: AgentState) {
            assert!(
                self.state.can_transition_to(&next),
                "invalid transition {:?} -> {:?}",
                self.state,
                next
            );
            self.state = next.clone();
            self.transitions.push(next);
        }
    }

    #[async_trait::async_trait]
    impl Agent for StubAgent {
        async fn id(&self) -> &AgentId {
            &self.id
        }
        async fn hand(&self) -> Hand {
            self.hand.clone()
        }
        async fn state(&self) -> AgentState {
            self.state.clone()
        }
        async fn permissions(&self) -> Vec<String> {
            self.permissions.clone()
        }
        async fn run(&mut self, input: &str) -> argos_core::Result<AgentOutput> {
            self.transition(AgentState::Observing);
            self.transition(AgentState::Thinking);
            self.transition(AgentState::Acting);
            self.transition(AgentState::Done);
            Ok(AgentOutput {
                text: format!("done:{input}"),
                tool_invocations: vec![],
                final_state: self.state.clone(),
                prompt_tokens: 0,
                completion_tokens: 0,
            })
        }
        async fn reset(&mut self) -> argos_core::Result<()> {
            if self.state.can_transition_to(&AgentState::Idle) {
                self.transition(AgentState::Idle);
            }
            Ok(())
        }
    }

    #[test]
    fn agent_trait_can_be_referenced() {
        let a: &dyn Agent = &StubAgent::new();
        let _ = a;
    }

    #[test]
    fn agent_output_constructs() {
        let out = AgentOutput {
            text: "summary".into(),
            tool_invocations: vec![],
            final_state: AgentState::Done,
            prompt_tokens: 0,
            completion_tokens: 0,
        };
        assert_eq!(out.text, "summary");
        assert_eq!(out.final_state, AgentState::Done);
        assert!(out.tool_invocations.is_empty());
    }

    #[tokio::test]
    async fn stub_agent_starts_idle() {
        let a = StubAgent::new();
        assert_eq!(a.state().await, AgentState::Idle);
        assert_eq!(a.hand().await, Hand::None);
        assert_eq!(a.id().await, "stub-1");
    }

    #[tokio::test]
    async fn stub_agent_run_returns_output_and_done() {
        let mut a = StubAgent::new();
        let out = a.run("summarize").await.unwrap();
        assert_eq!(out.text, "done:summarize");
        assert_eq!(out.final_state, AgentState::Done);
        assert_eq!(a.state().await, AgentState::Done);
    }

    #[tokio::test]
    async fn stub_agent_run_transitions_through_lifecycle() {
        let mut a = StubAgent::new();
        a.run("x").await.unwrap();
        // Idle (initial) -> Observing -> Thinking -> Acting -> Done
        assert_eq!(
            a.transitions,
            vec![
                AgentState::Idle,
                AgentState::Observing,
                AgentState::Thinking,
                AgentState::Acting,
                AgentState::Done,
            ]
        );
    }

    #[tokio::test]
    async fn stub_agent_reset_returns_to_idle() {
        let mut a = StubAgent::new();
        a.run("x").await.unwrap();
        assert_eq!(a.state().await, AgentState::Done);
        a.reset().await.unwrap();
        assert_eq!(a.state().await, AgentState::Idle);
    }

    #[tokio::test]
    async fn stub_agent_permissions_listed() {
        let a = StubAgent::new();
        assert_eq!(a.permissions().await, vec!["wiki.read".to_string()]);
    }
}
