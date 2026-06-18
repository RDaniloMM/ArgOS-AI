//! n8n workflow commands (list, import, run).

use anyhow::Result;
use argos_n8n_connector::N8nClient;

/// List n8n workflows via the stubbed client.
pub async fn run_list() -> Result<()> {
    let client = argos_n8n_connector::StubN8nClient::new();
    let workflows: Vec<_> = client.list_workflows().await?;
    println!("{}", serde_json::to_string_pretty(&workflows)?);
    Ok(())
}

/// Import an n8n workflow as an OKF concept (stub).
pub async fn run_import(id: &str) -> Result<()> {
    let result = serde_json::json!({
        "id": id,
        "status": "stub",
        "note": "n8n import stub — real workflow import deferred to Phase 2.",
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Run an n8n workflow by delegation.
pub async fn run_run(id: &str, data: Option<&str>) -> Result<()> {
    let client = argos_n8n_connector::StubN8nClient::new();
    let run_ref: argos_core::N8nRunRef = client.run_workflow(id, data).await?;
    println!("{}", serde_json::to_string_pretty(&run_ref)?);
    Ok(())
}
