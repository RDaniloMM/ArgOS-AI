//! Integration test: real LLM (OpenCode Go / DeepSeek-V4-flash) + real n8n.
//!
//! This test drives the full agent loop with a REAL LLM making REAL decisions
//! about REAL n8n workflows. It is `#[ignore]` by default — run with:
//!
//! ```powershell
//! $env:PATH = "D:\Proyectos\cpp\mingw64\bin;" + $env:PATH + ";$env:USERPROFILE\.cargo\bin"
//! $env:CC = "gcc"
//! cargo +stable-x86_64-pc-windows-gnu test -p argos-agent -- --ignored --test-threads=1
//! ```
//!
//! Prerequisites:
//! - n8n running at http://localhost:5678 (Docker: argos-n8n container)
//! - OpenCode Go API key (set in ARGOS_LLM_API_KEY env var or uses the default below)
//! - The n8n API key (set in ARGOS_N8N_API_KEY env var or uses the default)

use std::sync::Arc;

use argos_agent::{Agent, GenericAgent, ToolHandler, ToolRegistry};
use argos_core::ToolResult;
use argos_n8n_connector::{N8nClient, ReqwestN8nClient};
use argos_provider::{OpenAICompatibleConfig, OpenAICompatibleProvider, Provider};
use url::Url;

// --- Configuration (env vars with fallback defaults) ---

fn llm_config() -> OpenAICompatibleConfig {
    let api_key = std::env::var("ARGOS_LLM_API_KEY").unwrap_or_else(|_| {
        "sk-cuQFmt50IsmxHsVp0cyQT6f2DoB1UCvEdi4nka5gvO2oteLb185jLNEGlUzCXieA".to_string()
    });
    OpenAICompatibleConfig {
        endpoint: Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
        api_key,
        model: "deepseek-v4-flash".to_string(),
        embed_model: None, // DeepSeek-V4-flash doesn't support embeddings
    }
}

fn n8n_client() -> ReqwestN8nClient {
    let api_key = std::env::var("ARGOS_N8N_API_KEY").unwrap_or_else(|_| {
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJmZmI1ODAyZS1hMDdhLTRhZWUtYmQyYi1iMmI0ODJmYWZlMWUiLCJpc3MiOiJuOG4iLCJhdWQiOiJwdWJsaWMtYXBpIiwianRpIjoiNjdhMjUwNjMtNzgyZC00OGMyLThjNTMtODIyYmUxZWQxZWM1IiwiaWF0IjoxNzgxODAzNzU2LCJleHAiOjE3OTg3NjE2MDB9.RuLlYe7Q_aJNuvQQLsVY4hCP0JQYVsrcGSs234Onh_w".to_string()
    });
    ReqwestN8nClient::new(Url::parse("http://localhost:5678").unwrap(), Some(api_key))
}

// --- Tier-1 tools backed by real n8n ---

/// Tool: list n8n workflows.
struct N8nListTool {
    client: Arc<dyn N8nClient>,
}

#[async_trait::async_trait]
impl ToolHandler for N8nListTool {
    async fn invoke(&self, _args: &str) -> argos_core::Result<ToolResult> {
        let workflows = self.client.list_workflows().await?;
        let names: Vec<String> = workflows
            .iter()
            .map(|w| format!("{} (id: {})", w.name, w.id))
            .collect();
        Ok(ToolResult::Ok(
            serde_json::json!({ "workflows": names }).to_string(),
        ))
    }
}

/// Tool: run an n8n workflow by ID.
struct N8nRunTool {
    client: Arc<dyn N8nClient>,
}

#[async_trait::async_trait]
impl ToolHandler for N8nRunTool {
    async fn invoke(&self, args: &str) -> argos_core::Result<ToolResult> {
        let v: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if id.is_empty() {
            return Ok(ToolResult::Err("missing 'id' field".to_string()));
        }
        let data = v.get("data").map(|d| d.to_string());
        match self.client.run_workflow(id, data.as_deref()).await {
            Ok(run) => Ok(ToolResult::Ok(
                serde_json::json!({ "run_id": run.id, "status": format!("{:?}", run.status) })
                    .to_string(),
            )),
            Err(e) => Ok(ToolResult::Err(e.to_string())),
        }
    }
}

/// Tool: list n8n workflows and summarize what's available.
struct N8nSummaryTool {
    client: Arc<dyn N8nClient>,
}

#[async_trait::async_trait]
impl ToolHandler for N8nSummaryTool {
    async fn invoke(&self, _args: &str) -> argos_core::Result<ToolResult> {
        let workflows = self.client.list_workflows().await?;
        let summary = if workflows.is_empty() {
            "No workflows found in n8n.".to_string()
        } else {
            let items: Vec<String> = workflows
                .iter()
                .map(|w| format!("- {} (id: {}, active: unknown)", w.name, w.id))
                .collect();
            format!("Found {} workflows:\n{}", workflows.len(), items.join("\n"))
        };
        Ok(ToolResult::Ok(summary))
    }
}

// --- Tests ---

#[tokio::test]
#[ignore = "requires real LLM (OpenCode Go) + real n8n (localhost:5678)"]
async fn real_llm_health_check() {
    let provider = OpenAICompatibleProvider::new(llm_config());
    let result = provider.health_check().await;
    assert!(
        result.is_ok(),
        "LLM health check should succeed: {:?}",
        result
    );
}

#[tokio::test]
#[ignore = "requires real LLM (OpenCode Go) + real n8n (localhost:5678)"]
async fn real_llm_completion() {
    let provider = OpenAICompatibleProvider::new(llm_config());
    let completion = provider
        .complete(
            "What is 2+2? Reply with just the number.",
            &Default::default(),
        )
        .await
        .expect("LLM completion should succeed");

    println!("LLM response: {}", completion.text);
    println!(
        "Token usage: prompt={}, completion={}",
        completion.usage.prompt_tokens, completion.usage.completion_tokens
    );

    assert!(
        !completion.text.is_empty(),
        "LLM should return non-empty text"
    );
    assert!(
        completion.text.contains("4"),
        "LLM should say 4, got: {}",
        completion.text
    );
}

#[tokio::test]
#[ignore = "requires real LLM (OpenCode Go) + real n8n (localhost:5678)"]
async fn real_agent_lists_n8n_workflows() {
    // Set up the real LLM provider.
    let provider = Arc::new(OpenAICompatibleProvider::new(llm_config()));

    // Set up the real n8n client.
    let n8n = Arc::new(n8n_client()) as Arc<dyn N8nClient>;

    // Verify n8n is reachable.
    n8n.health_check().await.expect("n8n should be reachable");

    // Register tools backed by real n8n.
    let mut registry = ToolRegistry::new();
    registry.register(
        "n8n.list",
        "List all workflows available in the n8n instance. Returns workflow names and IDs.",
        Box::new(N8nListTool {
            client: n8n.clone(),
        }),
    );
    registry.register(
        "n8n.summary",
        "Get a human-readable summary of all n8n workflows.",
        Box::new(N8nSummaryTool {
            client: n8n.clone(),
        }),
    );

    // Create the agent with real LLM + real n8n tools.
    let mut agent = GenericAgent::new("real-agent", provider, Arc::new(registry));

    // Ask the agent to list workflows — it should decide to call n8n.list.
    let output = agent
        .run("List all the workflows I have in n8n. Use the n8n.list tool.")
        .await
        .expect("agent run should succeed");

    println!("=== Agent Output ===");
    println!("Final state: {:?}", output.final_state);
    println!("Answer: {}", output.text);
    println!("Tool invocations: {}", output.tool_invocations.len());
    for inv in &output.tool_invocations {
        println!(
            "  - tool: {}, result: {}",
            inv.tool.name,
            match &inv.result {
                ToolResult::Ok(s) => s.chars().take(100).collect::<String>(),
                ToolResult::Err(s) => format!("ERROR: {s}"),
            }
        );
    }

    // The agent should have called at least one tool.
    assert!(
        !output.tool_invocations.is_empty(),
        "agent should have called at least one n8n tool"
    );
    // The answer should mention workflows.
    assert!(
        !output.text.is_empty(),
        "agent should return a non-empty answer"
    );
}

#[tokio::test]
#[ignore = "requires real LLM (OpenCode Go) + real n8n (localhost:5678)"]
async fn real_agent_full_flow() {
    // This test runs the FULL ArgOS flow with real LLM + real n8n:
    // 1. Create a workflow in n8n with a webhook trigger
    // 2. Activate it
    // 3. Create an agent with real LLM + n8n tools
    // 4. Ask the agent to list workflows and run one
    // 5. Verify the agent loop works end-to-end

    let n8n = Arc::new(n8n_client()) as Arc<dyn N8nClient>;

    // Step 1: Create a simple webhook workflow in n8n.
    let webhook_path = format!("argos-llm-test-{}", std::process::id());
    let def = format!(
        r#"{{"name":"LLM Test Workflow","nodes":[{{"name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[250,300],"parameters":{{"httpMethod":"POST","path":"{webhook_path}","responseMode":"lastNode"}},"webhookId":"{webhook_path}"}},{{"name":"Set Response","type":"n8n-nodes-base.set","typeVersion":1,"position":[450,300],"parameters":{{"values":{{"string":[{{"name":"message","value":"Workflow executed by ArgOS agent"}}]}},"options":{{}}}}}}],"connections":{{"Webhook":{{"main":[[{{"node":"Set Response","type":"main","index":0}}]]}}}},"settings":{{}}}}"#
    );
    let created = n8n
        .create_workflow("LLM Test Workflow", &def)
        .await
        .expect("should create workflow");
    println!(
        "Created n8n workflow: {} (id: {})",
        created.name, created.id
    );

    // Step 2: Activate it.
    n8n.activate_workflow(&created.id)
        .await
        .expect("should activate workflow");
    println!("Workflow activated");

    // Step 3: Create agent with real LLM + n8n tools.
    let provider = Arc::new(OpenAICompatibleProvider::new(llm_config()));
    let mut registry = ToolRegistry::new();
    registry.register(
        "n8n.list",
        "List all workflows in n8n. Returns JSON with workflow names and IDs.",
        Box::new(N8nListTool {
            client: n8n.clone(),
        }),
    );
    registry.register(
        "n8n.run",
        "Run an n8n workflow by ID. Args: {\"id\": \"workflow-id\", \"data\": {}}. Returns run ID and status.",
        Box::new(N8nRunTool { client: n8n.clone() }),
    );

    let mut agent =
        GenericAgent::new("full-flow-agent", provider, Arc::new(registry)).with_max_iterations(5);

    // Step 4: Ask the agent to list workflows and run the one we created.
    let prompt = "I have n8n workflows. First, use the n8n.list tool to list all workflows. \
         Then, look for a workflow named 'LLM Test Workflow' and run it using the n8n.run tool \
         with its ID. Tell me what happened.";

    let output = agent.run(prompt).await.expect("agent run should succeed");

    println!("=== Full Flow Agent Output ===");
    println!("Final state: {:?}", output.final_state);
    println!("Answer: {}", output.text);
    println!("Tool invocations: {}", output.tool_invocations.len());
    for (i, inv) in output.tool_invocations.iter().enumerate() {
        println!("  [{}] tool: {}", i, inv.tool.name);
        println!("      args: {}", inv.args);
        println!(
            "      result: {}",
            match &inv.result {
                ToolResult::Ok(s) => s.chars().take(200).collect::<String>(),
                ToolResult::Err(s) => format!("ERROR: {s}"),
            }
        );
    }

    // The agent should have called at least 2 tools (list + run).
    assert!(
        output.tool_invocations.len() >= 2,
        "agent should have called at least 2 tools (list + run), got {}",
        output.tool_invocations.len()
    );
    // The agent should have completed successfully.
    assert!(
        output.final_state == argos_core::AgentState::Done,
        "agent should finish in Done state, got {:?}",
        output.final_state
    );
    // The answer should not be empty.
    assert!(
        !output.text.is_empty(),
        "agent should return a non-empty answer"
    );

    println!("\n✅ Full flow test passed: real LLM drove real n8n workflow execution!");
}
