//! Wiki knowledge commands (ingest, query, lint).
//!
//! Slice 1: CLI stub that demonstrates the command structure.
//! Real LLM integration and service wiring deferred to Phase 2.

use anyhow::Result;

/// Ingest a raw source into the OKF wiki (stub).
///
/// Real implementation requires a Provider + BundleStore + RawSourceStore
/// wired together. Slice 1 prints a placeholder.
pub async fn run_ingest(source: &std::path::Path) -> Result<()> {
    let result = serde_json::json!({
        "source": source.display().to_string(),
        "status": "stub",
        "note": "Wiki ingest stub — real LLM ingestion deferred to Phase 2.",
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Query the OKF knowledge wiki (stub).
pub async fn run_query(question: &[String]) -> Result<()> {
    let query_str = question.join(" ");
    let result = serde_json::json!({
        "question": query_str,
        "answer": "Wiki query stub — real LLM integration deferred to Phase 2.",
        "citations": [],
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Lint the wiki for structural issues (stub).
pub async fn run_lint() -> Result<()> {
    let report = serde_json::json!({
        "contradictions": [],
        "orphans": [],
        "missing_pages": [],
        "missing_index_entries": [],
        "stale_sources": [],
        "healthy": true,
        "note": "Wiki lint stub — real linting deferred to Phase 2.",
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
