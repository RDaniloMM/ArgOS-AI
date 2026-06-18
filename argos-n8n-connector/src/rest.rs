//! Pure parsers for n8n Public REST API responses.
//!
//! These functions turn n8n JSON bodies into ArgOS domain refs WITHOUT any
//! HTTP. Keeping them pure (and out of the feature-gated `ReqwestN8nClient`)
//! means the parsing contract is unit-tested in the default, network-free test
//! suite — the same approach the OllamaProvider takes with its `parse_chat` /
//! `parse_embedding` helpers. The `ReqwestN8nClient` (feature `reqwest-backend`)
//! just wires HTTP around these parsers.

// The parsers are consumed by the feature-gated `ReqwestN8nClient` and by the
// unit tests. Without `reqwest-backend` the lib target has no non-test caller,
// so silence dead-code for that configuration (tests still exercise them).
#![cfg_attr(not(feature = "reqwest-backend"), allow(dead_code))]

use argos_core::{ArgosError, N8nRunRef, N8nRunStatus, N8nWorkflowRef, Result};
use serde_json::Value;

/// Map an n8n execution status string to the ArgOS [`N8nRunStatus`] enum.
///
/// n8n uses a few status strings (`running`, `success`, `failed`, `error`,
/// `canceled`, `crashed`, `waiting`); only the four ArgOS variants are
/// modelled. `waiting` is treated as still in progress; anything unrecognised
/// is conservatively treated as `Failed`.
pub fn map_status(s: &str) -> N8nRunStatus {
    match s {
        "running" | "waiting" => N8nRunStatus::Running,
        "success" => N8nRunStatus::Success,
        "canceled" => N8nRunStatus::Cancelled,
        // `failed`, `error`, `crashed`, and anything unknown are failures.
        _ => N8nRunStatus::Failed,
    }
}

/// Parse a `GET /api/v1/workflows` body into workflow refs.
///
/// Accepts both the paginated shape `{"data":[...], "nextCursor":...}` and a
/// bare top-level array.
pub(crate) fn parse_workflow_list(body: &str) -> Result<Vec<N8nWorkflowRef>> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ArgosError::N8nConnection(format!("invalid workflows response: {e}")))?;
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())
        .ok_or_else(|| {
            ArgosError::N8nConnection("workflows response missing `data` array".into())
        })?;
    arr.iter().map(workflow_from_value).collect()
}

/// Parse a single workflow object (`GET /api/v1/workflows/{id}` body).
pub(crate) fn parse_workflow(body: &str) -> Result<N8nWorkflowRef> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ArgosError::N8nConnection(format!("invalid workflow response: {e}")))?;
    workflow_from_value(&v)
}

/// Parse an execution/run object (`POST /api/v1/workflows/{id}/run` or
/// `GET /api/v1/executions/{id}` body) into a run ref. Accepts the execution
/// at the top level or nested under `data`.
pub(crate) fn parse_run(body: &str) -> Result<N8nRunRef> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ArgosError::N8nConnection(format!("invalid run response: {e}")))?;
    let exec = v.get("data").unwrap_or(&v);
    let id = value_to_string(
        exec.get("id")
            .ok_or_else(|| ArgosError::N8nConnection("run response missing `id`".into()))?,
    )
    .ok_or_else(|| ArgosError::N8nConnection("run `id` is not a string or number".into()))?;
    let status = exec
        .get("status")
        .and_then(|s| s.as_str())
        .map(map_status)
        .ok_or_else(|| ArgosError::N8nConnection("run response missing `status` string".into()))?;
    let workflow_id =
        workflow_id_from_value(exec.get("workflowId").ok_or_else(|| {
            ArgosError::N8nConnection("run response missing `workflowId`".into())
        })?)
        .ok_or_else(|| ArgosError::N8nConnection("run `workflowId` has no usable id".into()))?;
    Ok(N8nRunRef {
        id,
        workflow_id,
        status,
    })
}

/// Parse only the status field out of an execution body
/// (`GET /api/v1/executions/{id}`).
pub(crate) fn parse_status(body: &str) -> Result<N8nRunStatus> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| ArgosError::N8nConnection(format!("invalid execution response: {e}")))?;
    let exec = v.get("data").unwrap_or(&v);
    let status = exec
        .get("status")
        .and_then(|s| s.as_str())
        .map(map_status)
        .ok_or_else(|| ArgosError::N8nConnection("execution response missing `status`".into()))?;
    Ok(status)
}

/// Coerce a JSON value (string or number) into a `String`.
fn value_to_string(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = v.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = v.as_u64() {
        return Some(n.to_string());
    }
    if let Some(f) = v.as_f64() {
        // n8n ids are integers; fall back to the f64 string only if needed.
        return Some(f.to_string());
    }
    None
}

/// Extract a workflow ref from a JSON object value.
fn workflow_from_value(v: &Value) -> Result<N8nWorkflowRef> {
    let id = value_to_string(
        v.get("id")
            .ok_or_else(|| ArgosError::N8nConnection("workflow response missing `id`".into()))?,
    )
    .ok_or_else(|| ArgosError::N8nConnection("workflow `id` is not a string or number".into()))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| ArgosError::N8nConnection("workflow response missing `name`".into()))?
        .to_string();
    Ok(N8nWorkflowRef {
        id,
        name,
        url: None,
    })
}

/// Extract the workflow id from a `workflowId` field that may be a string or
/// an object `{ "id": "...", "name": "..." }`.
fn workflow_id_from_value(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(obj) = v.as_object() {
        if let Some(id) = obj.get("id") {
            return value_to_string(id);
        }
    }
    value_to_string(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_status_known_variants() {
        assert_eq!(map_status("running"), N8nRunStatus::Running);
        assert_eq!(map_status("success"), N8nRunStatus::Success);
        assert_eq!(map_status("failed"), N8nRunStatus::Failed);
        assert_eq!(map_status("canceled"), N8nRunStatus::Cancelled);
    }

    #[test]
    fn map_status_aliases() {
        // n8n's `error` and `crashed` are failure states.
        assert_eq!(map_status("error"), N8nRunStatus::Failed);
        assert_eq!(map_status("crashed"), N8nRunStatus::Failed);
        // `waiting` is still in progress (waiting on a trigger).
        assert_eq!(map_status("waiting"), N8nRunStatus::Running);
    }

    #[test]
    fn map_status_unknown_is_failed() {
        assert_eq!(map_status("something-new"), N8nRunStatus::Failed);
        assert_eq!(map_status(""), N8nRunStatus::Failed);
    }

    #[test]
    fn parse_workflow_list_paginated_shape() {
        let body = r#"{"data":[
            {"id":"wf-1","name":"Daily Report","active":true,"nodes":[],"connections":{}},
            {"id":"wf-2","name":"Weekly Report","active":false}
        ],"nextCursor":null}"#;
        let list = parse_workflow_list(body).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "wf-1");
        assert_eq!(list[0].name, "Daily Report");
        assert_eq!(list[1].id, "wf-2");
        assert_eq!(list[1].name, "Weekly Report");
    }

    #[test]
    fn parse_workflow_list_bare_array_shape() {
        let body = r#"[{"id":"a","name":"A"}]"#;
        let list = parse_workflow_list(body).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "a");
    }

    #[test]
    fn parse_workflow_list_empty_data() {
        assert_eq!(parse_workflow_list(r#"{"data":[]}"#).unwrap(), Vec::new());
    }

    #[test]
    fn parse_workflow_single_object() {
        let body = r#"{"id":"wf-1","name":"Daily Report","active":true,"nodes":[]}"#;
        let wf = parse_workflow(body).unwrap();
        assert_eq!(wf.id, "wf-1");
        assert_eq!(wf.name, "Daily Report");
        assert!(wf.url.is_none());
    }

    #[test]
    fn parse_workflow_numeric_id_coerced_to_string() {
        let body = r#"{"id":42,"name":"Numeric"}"#;
        let wf = parse_workflow(body).unwrap();
        assert_eq!(wf.id, "42");
        assert_eq!(wf.name, "Numeric");
    }

    #[test]
    fn parse_run_with_string_ids() {
        let body = r#"{"id":"exec-100","status":"success","workflowId":"wf-1"}"#;
        let run = parse_run(body).unwrap();
        assert_eq!(run.id, "exec-100");
        assert_eq!(run.workflow_id, "wf-1");
        assert_eq!(run.status, N8nRunStatus::Success);
    }

    #[test]
    fn parse_run_with_numeric_run_id() {
        let body = r#"{"id":99,"status":"running","workflowId":"wf-1"}"#;
        let run = parse_run(body).unwrap();
        assert_eq!(run.id, "99");
        assert_eq!(run.status, N8nRunStatus::Running);
    }

    #[test]
    fn parse_run_with_object_workflow_id() {
        let body = r#"{"id":"e","status":"failed","workflowId":{"id":"wf-7","name":"X"}}"#;
        let run = parse_run(body).unwrap();
        assert_eq!(run.workflow_id, "wf-7");
        assert_eq!(run.status, N8nRunStatus::Failed);
    }

    #[test]
    fn parse_run_nested_under_data() {
        let body = r#"{"data":{"id":"exec-1","status":"success","workflowId":"wf-1"}}"#;
        let run = parse_run(body).unwrap();
        assert_eq!(run.id, "exec-1");
        assert_eq!(run.workflow_id, "wf-1");
    }

    #[test]
    fn parse_status_extracts_status_field() {
        let body = r#"{"id":"exec-100","status":"running"}"#;
        assert_eq!(parse_status(body).unwrap(), N8nRunStatus::Running);
    }

    #[test]
    fn parse_status_canceled() {
        let body = r#"{"id":"exec-100","status":"canceled"}"#;
        assert_eq!(parse_status(body).unwrap(), N8nRunStatus::Cancelled);
    }

    #[test]
    fn parse_workflow_list_invalid_json_errors() {
        assert!(parse_workflow_list("not json").is_err());
    }

    #[test]
    fn parse_run_missing_id_errors() {
        assert!(parse_run(r#"{"status":"success","workflowId":"wf-1"}"#).is_err());
    }
}
