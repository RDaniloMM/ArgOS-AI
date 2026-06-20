//! GenericAgent — the concrete Agent implementation with a tool-call loop.
//!
//! The loop: Observe → Think → Act → (loop) → Done.
//! 1. Build a system prompt listing available tools.
//! 2. Call the Provider with the user input + system prompt.
//! 3. Parse the LLM response:
//!    - JSON with "tool" → invoke the tool, feed result back, go to step 2.
//!    - JSON with "done" → extract answer, return.
//!    - Plain text → treat as the answer, return.
//! 4. If max_iterations reached → Failed.
//!
//! All tool invocations are recorded for audit/crash recovery.

use crate::agent::{Agent, AgentOutput};
use crate::registry::ToolRegistry;
use argos_core::{AgentId, AgentState, ArgosError, Hand, Result, Tool, ToolInvocation, ToolResult};
use argos_provider::{CompletionOptions, Provider};
use std::sync::Arc;

/// The default maximum number of tool-call iterations before the agent fails.
const DEFAULT_MAX_ITERATIONS: usize = 10;

/// A concrete Agent that drives a tool-call loop via a Provider and ToolRegistry.
///
/// Slice 1 ships `Hand::None` (generic). The six specialised Hands are future
/// specialisations that pre-configure the tool registry and system prompt.
pub struct GenericAgent {
    id: AgentId,
    hand: Hand,
    state: AgentState,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    permissions: Vec<String>,
    tool_invocations: Vec<ToolInvocation>,
    max_iterations: usize,
}

impl GenericAgent {
    /// Create a generic agent with a provider and tool registry.
    pub fn new(
        id: impl Into<AgentId>,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            id: id.into(),
            hand: Hand::None,
            state: AgentState::Idle,
            provider,
            tools,
            permissions: Vec::new(),
            tool_invocations: Vec::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }

    /// Builder: set the Hand specialisation.
    pub fn with_hand(mut self, hand: Hand) -> Self {
        self.hand = hand;
        self
    }

    /// Builder: set the maximum tool-call iterations.
    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n;
        self
    }

    /// Builder: set permissions.
    pub fn with_permissions(mut self, perms: Vec<String>) -> Self {
        self.permissions = perms;
        self
    }

    /// Build the system prompt advertising available tools to the LLM.
    fn build_system_prompt(&self) -> String {
        let tool_list: Vec<String> = self
            .tools
            .list()
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect();

        format!(
            "You are an ArgOS agent. You help users automate tasks.\n\
             Available tools:\n{}\n\n\
             To use a tool, respond with JSON: {{\"tool\": \"<name>\", \"args\": {{...}}}}\n\
             When you are done, respond with JSON: {{\"done\": true, \"answer\": \"<your answer>\"}}\n\
             If you have a direct answer with no tools needed, just respond with plain text.",
            tool_list.join("\n")
        )
    }

    /// Parse the LLM response to determine the next action.
    fn parse_response(&self, text: &str) -> AgentAction {
        // Try to parse as JSON.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                let answer = v
                    .get("answer")
                    .and_then(|a| a.as_str())
                    .unwrap_or("")
                    .to_string();
                return AgentAction::Done(answer);
            }
            if let Some(tool_name) = v.get("tool").and_then(|t| t.as_str()) {
                let args = v
                    .get("args")
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                return AgentAction::ToolCall(tool_name.to_string(), args);
            }
        }
        // Not JSON — treat as a plain text answer.
        AgentAction::Done(text.to_string())
    }
}

/// What the agent should do next based on the LLM response.
enum AgentAction {
    /// Invoke a tool with the given args (JSON string).
    ToolCall(String, String),
    /// The agent is done; return this answer.
    Done(String),
}

#[async_trait::async_trait]
impl Agent for GenericAgent {
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

    async fn run(&mut self, input: &str) -> Result<AgentOutput> {
        self.state = AgentState::Observing;

        let system_prompt = self.build_system_prompt();
        let mut conversation = format!("{system_prompt}\n\nUser: {input}");
        let mut total_prompt_tokens: u64 = 0;
        let mut total_completion_tokens: u64 = 0;

        for iteration in 0..self.max_iterations {
            self.state = AgentState::Thinking;

            let completion = self
                .provider
                .complete(&conversation, &CompletionOptions::default())
                .await?;

            total_prompt_tokens += completion.usage.prompt_tokens as u64;
            total_completion_tokens += completion.usage.completion_tokens as u64;

            let action = self.parse_response(&completion.text);

            match action {
                AgentAction::Done(answer) => {
                    self.state = AgentState::Done;
                    let invocations = self.tool_invocations.clone();
                    return Ok(AgentOutput {
                        text: answer,
                        tool_invocations: invocations,
                        final_state: AgentState::Done,
                        prompt_tokens: total_prompt_tokens,
                        completion_tokens: total_completion_tokens,
                    });
                }
                AgentAction::ToolCall(tool_name, args) => {
                    // Thinking → Acting
                    self.state = AgentState::Acting;

                    let result = self.tools.invoke(&tool_name, &args).await;

                    let tool_result = match result {
                        Ok(r) => r,
                        Err(e) => ToolResult::Err(e.to_string()),
                    };

                    // Record the invocation.
                    let tool_info = self.tools.get(&tool_name).map(|t| Tool {
                        name: t.name.clone(),
                        description: t.description.clone(),
                    });

                    if let Some(tool) = tool_info {
                        self.tool_invocations.push(ToolInvocation {
                            tool,
                            args: args.clone(),
                            result: tool_result.clone(),
                        });
                    }

                    // Acting → Thinking (feed result back to LLM)
                    self.state = AgentState::Thinking;

                    let result_str = match &tool_result {
                        ToolResult::Ok(s) => s.clone(),
                        ToolResult::Err(s) => format!("Error: {s}"),
                    };

                    conversation = format!(
                        "{conversation}\n\nAssistant: {{\"tool\": \"{tool_name}\", \"args\": {args}}}\n\nTool result: {result_str}\n\nAssistant:"
                    );
                }
            }

            // Check if we've hit the iteration limit on the next loop.
            if iteration + 1 >= self.max_iterations {
                self.state = AgentState::Failed;
                let invocations = self.tool_invocations.clone();
                return Ok(AgentOutput {
                    text: "Agent reached maximum iterations without completing.".to_string(),
                    tool_invocations: invocations,
                    final_state: AgentState::Failed,
                    prompt_tokens: total_prompt_tokens,
                    completion_tokens: total_completion_tokens,
                });
            }
        }

        // Should not reach here, but just in case.
        self.state = AgentState::Failed;
        Err(ArgosError::Provider(
            "agent loop exhausted without resolution".to_string(),
        ))
    }

    async fn reset(&mut self) -> Result<()> {
        self.state = AgentState::Idle;
        self.tool_invocations.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{ToolHandler, ToolRegistry};
    use crate::test_support::{EchoHandler, ScriptedProvider};
    use argos_core::ToolResult;
    use async_trait::async_trait;

    fn setup_agent(responses: Vec<&str>) -> GenericAgent {
        let provider = Arc::new(ScriptedProvider::new(
            responses.iter().map(|s| s.to_string()).collect(),
        ));
        let mut registry = ToolRegistry::new();
        registry.register(
            "echo",
            "Echoes the args back",
            Box::new(EchoHandler {
                result: ToolResult::Ok(r#"{"echoed": true}"#.to_string()),
            }),
        );
        GenericAgent::new("test-agent", provider, Arc::new(registry))
    }

    #[tokio::test]
    async fn agent_constructs() {
        let agent = setup_agent(vec![]);
        assert_eq!(agent.id, "test-agent");
    }

    #[tokio::test]
    async fn agent_starts_in_idle_state() {
        let agent = setup_agent(vec![]);
        assert_eq!(agent.state().await, AgentState::Idle);
    }

    #[tokio::test]
    async fn run_with_done_response_returns_answer() {
        let mut agent = setup_agent(vec![r#"{"done": true, "answer": "Hello!"}"#]);
        let output = agent.run("hi").await.unwrap();
        assert_eq!(output.text, "Hello!");
        assert_eq!(output.final_state, AgentState::Done);
    }

    #[tokio::test]
    async fn run_transitions_through_states() {
        let mut agent = setup_agent(vec![r#"{"done": true, "answer": "done"}"#]);
        agent.run("test").await.unwrap();
        assert_eq!(agent.state().await, AgentState::Done);
    }

    #[tokio::test]
    async fn run_with_tool_call_invokes_tool_and_records() {
        let mut agent = setup_agent(vec![
            r#"{"tool": "echo", "args": {"msg": "hello"}}"#,
            r#"{"done": true, "answer": "echoed"}"#,
        ]);
        let output = agent.run("echo hello").await.unwrap();
        assert_eq!(output.text, "echoed");
        assert_eq!(output.tool_invocations.len(), 1);
        assert_eq!(output.tool_invocations[0].tool.name, "echo");
    }

    #[tokio::test]
    async fn run_with_tool_then_done_returns_both() {
        let mut agent = setup_agent(vec![
            r#"{"tool": "echo", "args": {"msg": "first"}}"#,
            r#"{"tool": "echo", "args": {"msg": "second"}}"#,
            r#"{"done": true, "answer": "all done"}"#,
        ]);
        let output = agent.run("echo twice").await.unwrap();
        assert_eq!(output.text, "all done");
        assert_eq!(output.tool_invocations.len(), 2);
    }

    #[tokio::test]
    async fn run_respects_max_iterations() {
        // Always respond with a tool call — never "done".
        let responses: Vec<String> = (0..15)
            .map(|_| r#"{"tool": "echo", "args": {}}"#.to_string())
            .collect();
        let mut agent =
            setup_agent(responses.iter().map(|s| s.as_str()).collect()).with_max_iterations(3);
        let output = agent.run("loop forever").await.unwrap();
        assert_eq!(output.final_state, AgentState::Failed);
        assert_eq!(output.tool_invocations.len(), 3);
    }

    #[tokio::test]
    async fn reset_clears_state_and_invocations() {
        let mut agent = setup_agent(vec![
            r#"{"tool": "echo", "args": {}}"#,
            r#"{"done": true, "answer": "done"}"#,
        ]);
        agent.run("test").await.unwrap();
        assert!(!agent.tool_invocations.is_empty());

        agent.reset().await.unwrap();
        assert_eq!(agent.state().await, AgentState::Idle);
        assert!(agent.tool_invocations.is_empty());
    }

    #[tokio::test]
    async fn agent_with_hand_none_is_generic() {
        let agent = setup_agent(vec![]);
        assert_eq!(agent.hand().await, Hand::None);
    }

    #[tokio::test]
    async fn run_with_plain_text_response_treats_as_answer() {
        let mut agent = setup_agent(vec!["This is a plain text answer."]);
        let output = agent.run("hello").await.unwrap();
        assert_eq!(output.text, "This is a plain text answer.");
        assert_eq!(output.final_state, AgentState::Done);
    }

    #[tokio::test]
    async fn run_with_tool_error_records_error_result() {
        struct FailingHandler;
        #[async_trait]
        impl ToolHandler for FailingHandler {
            async fn invoke(&self, _args: &str) -> Result<ToolResult> {
                Err(ArgosError::Provider("tool failed".to_string()))
            }
        }

        let provider = Arc::new(ScriptedProvider::new(vec![
            r#"{"tool": "fail", "args": {}}"#.to_string(),
            r#"{"done": true, "answer": "recovered"}"#.to_string(),
        ]));
        let mut registry = ToolRegistry::new();
        registry.register("fail", "Always fails", Box::new(FailingHandler));
        let mut agent = GenericAgent::new("test", provider, Arc::new(registry));

        let output = agent.run("use failing tool").await.unwrap();
        assert_eq!(output.text, "recovered");
        assert_eq!(output.tool_invocations.len(), 1);
        assert!(matches!(
            output.tool_invocations[0].result,
            ToolResult::Err(_)
        ));
    }
}
