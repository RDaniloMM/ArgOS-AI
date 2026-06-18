//! Integration tests against a REAL n8n instance.
//!
//! These tests are `#[ignore]` by default — they require a running n8n
//! instance with a valid API key. Run them manually with:
//!
//! ```powershell
//! $env:PATH = "D:\Proyectos\cpp\mingw64\bin;" + $env:PATH + ";$env:USERPROFILE\.cargo\bin"
//! $env:CC = "gcc"
//! cargo +stable-x86_64-pc-windows-gnu test -p argos-n8n-connector --features reqwest-backend -- --ignored --test-threads=1
//! ```
//!
//! Prerequisites:
//! - n8n running at http://localhost:5678 (Docker: `docker run -d --name argos-n8n -p 5678:5678 ...`)
//! - API key set in the `ARGOS_N8N_API_KEY` env var
//! - Owner account set up in n8n
//!
//! The tests create and delete workflows in n8n, so they are NOT safe to run
//! against a production instance. Use a fresh Docker n8n only.

#![cfg(feature = "reqwest-backend")]

use argos_core::{ConceptType, N8nRunStatus};
use argos_knowledge::BundleStore;
use argos_n8n_connector::{
    N8nClient, N8nConnector, ReqwestN8nClient, WorkflowExporter, WorkflowImporter,
};
use url::Url;

/// n8n endpoint (default: http://localhost:5678).
fn n8n_endpoint() -> Url {
    Url::parse("http://localhost:5678").unwrap()
}

/// API key from env var (set during integration test setup).
fn n8n_api_key() -> String {
    std::env::var("ARGOS_N8N_API_KEY").unwrap_or_else(|_| {
        // Fallback: the key generated during this session's n8n setup.
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJmZmI1ODAyZS1hMDdhLTRhZWUtYmQyYi1iMmI0ODJmYWZlMWUiLCJpc3MiOiJuOG4iLCJhdWQiOiJwdWJsaWMtYXBpIiwianRpIjoiNjdhMjUwNjMtNzgyZC00OGMyLThjNTMtODIyYmUxZWQxZWM1IiwiaWF0IjoxNzgxODAzNzU2LCJleHAiOjE3OTg3NjE2MDB9.RuLlYe7Q_aJNuvQQLsVY4hCP0JQYVsrcGSs234Onh_w".to_string()
    })
}

/// Create a REST client connected to the real n8n instance.
fn real_client() -> ReqwestN8nClient {
    ReqwestN8nClient::new(n8n_endpoint(), Some(n8n_api_key()))
}

/// A simple test workflow definition (n8n JSON format).
fn test_workflow_def() -> &'static str {
    r#"{"name":"ArgOS Test Workflow","nodes":[{"name":"Start","type":"n8n-nodes-base.manualTrigger","typeVersion":1,"position":[250,300],"parameters":{}},{"name":"NoOp","type":"n8n-nodes-base.noOp","typeVersion":1,"position":[450,300],"parameters":{}}],"connections":{"Start":{"main":[[{"node":"NoOp","type":"main","index":0}]]}},"settings":{}}"#
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_health_check() {
    let client = real_client();
    let result = client.health_check().await;
    assert!(result.is_ok(), "health_check should succeed: {:?}", result);
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_list_workflows() {
    let client = real_client();
    let workflows = client.list_workflows().await;
    assert!(
        workflows.is_ok(),
        "list_workflows should succeed: {:?}",
        workflows
    );
    let workflows = workflows.unwrap();
    // There should be at least the "Daily Email Summary" workflow we created
    // during setup, or any workflows that exist in this n8n instance.
    println!("Found {} workflows in n8n:", workflows.len());
    for wf in &workflows {
        println!("  - {} (id: {})", wf.name, wf.id);
    }
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_create_and_get_workflow() {
    let client = real_client();

    // Create a test workflow.
    let created = client
        .create_workflow("ArgOS Integration Test", test_workflow_def())
        .await
        .expect("create_workflow should succeed");
    println!("Created workflow: {} (id: {})", created.name, created.id);

    // Get it back.
    let fetched = client
        .get_workflow(&created.id)
        .await
        .expect("get_workflow should succeed");
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, "ArgOS Integration Test");

    // Note: n8n doesn't have a delete workflow endpoint in the public API v1,
    // so the test workflow stays in n8n. This is fine for a test instance.
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_run_workflow() {
    let client = real_client();

    // Create a workflow with a webhook trigger (required for external execution).
    // Use a unique webhook path per test run to avoid 409 conflicts with
    // leftover workflows from previous test runs.
    let webhook_path = format!("argos-run-{}", std::process::id());
    let def = format!(
        r#"{{"name":"ArgOS Run Test","nodes":[{{"name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[250,300],"parameters":{{"httpMethod":"POST","path":"{webhook_path}","responseMode":"onReceived"}},"webhookId":"{webhook_path}"}},{{"name":"NoOp","type":"n8n-nodes-base.noOp","typeVersion":1,"position":[450,300],"parameters":{{}}}}],"connections":{{"Webhook":{{"main":[[{{"node":"NoOp","type":"main","index":0}}]]}}}},"settings":{{}}}}"#
    );
    let created = client
        .create_workflow("ArgOS Run Test", &def)
        .await
        .expect("create_workflow should succeed");
    println!("Created workflow: {} (id: {})", created.name, created.id);

    // Activate the workflow (webhooks only work when the workflow is active).
    client
        .activate_workflow(&created.id)
        .await
        .expect("activate_workflow should succeed");
    println!("Workflow activated");

    // Run the workflow by triggering its webhook with data.
    // The connector polls executions with backoff until n8n registers the run.
    let run = client
        .run_workflow(&created.id, Some(r#"{"message":"hello from ArgOS"}"#))
        .await
        .expect("run_workflow should succeed via webhook");
    println!(
        "Run started: id={}, workflow={}, status={:?}",
        run.id, run.workflow_id, run.status
    );

    // The run should have a valid execution ID.
    assert!(
        !run.id.is_empty(),
        "run should have a non-empty execution id"
    );
    assert_eq!(run.workflow_id, created.id);

    // Check the run status via the executions API.
    let status = client
        .get_run_status(&run.id)
        .await
        .expect("get_run_status should succeed");
    println!("Final run status: {:?}", status);
    assert!(
        matches!(status, N8nRunStatus::Success | N8nRunStatus::Running),
        "webhook-triggered workflow should succeed or be running, got {:?}",
        status
    );
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_connector_round_trip() {
    let client = real_client();
    let connector = N8nConnector::new(
        Box::new(client),
        argos_core::N8nConnection {
            endpoint: n8n_endpoint(),
            mode: argos_core::ConnMode::Rest,
            api_key_ref: Some("argos-n8n".to_string()),
        },
    );

    // Connect.
    connector.connect().await.expect("connect should succeed");

    // List workflows through the connector.
    let workflows = connector
        .list_workflows()
        .await
        .expect("list should succeed");
    println!("Connector found {} workflows", workflows.len());
    assert!(!workflows.is_empty(), "should find at least one workflow");
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_import_to_okf_concept() {
    let client = real_client();
    let tmp = tempfile::tempdir().unwrap();
    let bundle = BundleStore::new(tmp.path().join("wiki"));

    // Create a workflow in n8n first.
    let _created = client
        .create_workflow("ArgOS Import Test Workflow", test_workflow_def())
        .await
        .expect("create should succeed");

    // Import all n8n workflows as OKF concepts.
    let importer = WorkflowImporter::new(&client, &bundle);
    let result = importer.import_all().await.expect("import should succeed");

    println!(
        "Imported {} workflows, skipped {}",
        result.imported.len(),
        result.skipped.len()
    );
    assert!(
        !result.imported.is_empty(),
        "should import at least one workflow"
    );

    // Verify the concept was created with the right type.
    for path in &result.imported {
        let concept = bundle.read_concept(path).expect("concept should exist");
        assert_eq!(
            concept.frontmatter.concept_type,
            ConceptType::Workflow,
            "imported concept should be type=workflow"
        );
        assert!(
            concept
                .frontmatter
                .resource
                .as_ref()
                .map(|r| r.starts_with("n8n://workflows/"))
                .unwrap_or(false),
            "concept resource should be n8n://workflows/<id>"
        );
        println!(
            "  Imported: {} -> {} (resource: {})",
            path,
            concept.frontmatter.title,
            concept.frontmatter.resource.as_deref().unwrap_or("?")
        );
    }
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_export_workflow() {
    let client = real_client();
    let exporter = WorkflowExporter::new(&client);

    let def = r#"{"name":"Exported by ArgOS","nodes":[{"name":"Start","type":"n8n-nodes-base.manualTrigger","typeVersion":1,"position":[250,300],"parameters":{}}],"connections":{},"settings":{}}"#;

    let wf = exporter
        .export("Exported by ArgOS", def)
        .await
        .expect("export should succeed");
    println!("Exported workflow: {} (id: {})", wf.name, wf.id);

    // Verify it's in n8n.
    let fetched = client
        .get_workflow(&wf.id)
        .await
        .expect("get should succeed");
    assert_eq!(fetched.name, "Exported by ArgOS");
}

#[tokio::test]
#[ignore = "requires running n8n at localhost:5678"]
async fn real_n8n_full_reuse_loop_setup() {
    // This test simulates the beginning of the Reuse Loop:
    // 1. Create a workflow in n8n
    // 2. Import it as an OKF concept
    // 3. Verify the concept exists and has the right metadata

    let client = real_client();
    let tmp = tempfile::tempdir().unwrap();
    let bundle = BundleStore::new(tmp.path().join("wiki"));

    // Step 1: Create a workflow in n8n
    let wf = client
        .create_workflow("Reuse Loop Test Workflow", test_workflow_def())
        .await
        .expect("create should succeed");
    println!("Step 1: Created n8n workflow '{}' (id: {})", wf.name, wf.id);

    // Step 2: Import as OKF concept
    let importer = WorkflowImporter::new(&client, &bundle);
    let result = importer.import_all().await.expect("import should succeed");
    println!("Step 2: Imported {} concepts", result.imported.len());

    // Step 3: Verify the concept — find the one matching our workflow ID
    // (import_all imports ALL n8n workflows, not just the one we created).
    let matching_concept = result.imported.iter().find_map(|path| {
        let concept = bundle.read_concept(path).ok()?;
        if concept
            .frontmatter
            .resource
            .as_ref()
            .map(|r| r.contains(&wf.id))
            .unwrap_or(false)
        {
            Some((path.clone(), concept))
        } else {
            None
        }
    });
    let (concept_path, concept) =
        matching_concept.expect("should find the concept matching the created workflow");
    assert_eq!(concept.frontmatter.concept_type, ConceptType::Workflow);
    assert_eq!(concept.frontmatter.title, "Reuse Loop Test Workflow");
    println!(
        "Step 3: Verified concept at {} — title={}, resource={}",
        concept_path,
        concept.frontmatter.title,
        concept.frontmatter.resource.as_deref().unwrap_or("?")
    );
    println!("Reuse Loop setup verified! Ready for workflow intelligence (PR 9).");
}
