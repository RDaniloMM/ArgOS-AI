//! Real n8n integration tests for the TUI service layer.
//!
//! These tests use the local `~/.argos/config.toml` and OS keyring. They are
//! ignored by default because they require a running, disposable n8n instance.

use argos_core::N8nRunStatus;
use argos_tui::services::{AppServices, RealServices};
use argos_tui::state::StatusLevel;

#[tokio::test]
#[ignore = "requires configured local n8n and OS keyring"]
async fn real_tui_snapshot_loads_n8n_workflows() {
    let services = RealServices::new().expect("real services should initialize");
    let snapshot = services
        .load_snapshot()
        .await
        .expect("TUI snapshot should load");

    assert_eq!(snapshot.n8n.level, StatusLevel::Success);
    assert!(
        !snapshot.n8n.workflows.is_empty(),
        "real n8n should expose at least one workflow"
    );
}

#[tokio::test]
#[ignore = "requires an active webhook workflow in local n8n"]
async fn real_tui_runs_selected_n8n_workflow() {
    let workflow_id = std::env::var("ARGOS_N8N_WORKFLOW_ID")
        .expect("ARGOS_N8N_WORKFLOW_ID must identify an active webhook workflow");
    let services = RealServices::new().expect("real services should initialize");
    let result = services
        .run_workflow(workflow_id)
        .await
        .expect("TUI service should run the selected workflow");

    assert!(matches!(
        result.run.status,
        N8nRunStatus::Success | N8nRunStatus::Running
    ));
}
