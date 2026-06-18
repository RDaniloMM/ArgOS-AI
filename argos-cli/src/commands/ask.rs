//! Ask the ArgOS agent (stub).

use anyhow::Result;

/// Ask the ArgOS agent a question.
///
/// Slice 1 stub — the real agent loop with LLM is deferred to Phase 2.
pub async fn run_ask(prompt: &[String]) -> Result<()> {
    let prompt_str = prompt.join(" ");
    let result = serde_json::json!({
        "prompt": prompt_str,
        "answer": "ArgOS agent stub — real agent loop deferred to Phase 2.",
        "tool_invocations": [],
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
