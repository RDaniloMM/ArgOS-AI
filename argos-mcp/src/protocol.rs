//! JSON-RPC 2.0 protocol handler for MCP.
//!
//! Parses incoming JSON-RPC messages, dispatches to the appropriate method
//! (`initialize`, `tools/list`, `tools/call`), and serialises the response.
//! Malformed messages and unknown methods return standard JSON-RPC error
//! responses — the server never crashes on bad input.
//!
//! # Pure-function design
//! `handle_message` is a free function that takes a JSON string and
//! pre-resolved server capabilities + tool list. No I/O, no async — the
//! transport layer and server loop handle the async dispatch.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::McpCapabilities;

/// A parsed JSON-RPC request.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// A JSON-RPC response (success).
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcErrorBody>,
}

/// A JSON-RPC error body.
#[derive(Debug, Serialize)]
struct JsonRpcErrorBody {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// JSON-RPC standard error codes.
#[allow(dead_code)]
mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// Context needed to handle a JSON-RPC message — capabilities and tool list
/// are resolved by the async server loop before calling this sync handler.
pub struct ProtocolContext {
    pub capabilities: McpCapabilities,
    pub server_name: String,
    pub server_version: String,
}

/// Parse a JSON-RPC message string and dispatch to the handler.
///
/// `ctx` carries pre-resolved server capabilities (avoiding async in the
/// handler). Returns the response as a JSON string, or `None` for
/// notifications (id: null).
pub fn handle_message(message: &str, ctx: &ProtocolContext) -> Option<String> {
    let request: JsonRpcRequest = match parse_request(message) {
        Ok(req) => req,
        Err(_) => return Some(build_error(None, error_codes::PARSE_ERROR, "Parse error")),
    };

    let id = request.id.clone();
    let is_notification = id.is_none();

    match request.method.as_str() {
        "initialize" => {
            let result = handle_initialize(&request, ctx);
            if is_notification {
                None
            } else {
                Some(build_response(id, result))
            }
        }
        "tools/list" => {
            let result = handle_tools_list();
            if is_notification {
                None
            } else {
                Some(build_response(id, result))
            }
        }
        "tools/call" => {
            let result = handle_tools_call(&request);
            if is_notification {
                None
            } else {
                Some(build_response(id, result))
            }
        }
        _ => {
            if is_notification {
                None
            } else {
                Some(build_error(
                    id,
                    error_codes::METHOD_NOT_FOUND,
                    &format!("Method not found: {}", request.method),
                ))
            }
        }
    }
}

/// Parse a JSON-RPC request string. Returns error on invalid JSON or missing
/// required fields.
fn parse_request(message: &str) -> Result<JsonRpcRequest, String> {
    let req: JsonRpcRequest =
        serde_json::from_str(message).map_err(|e| format!("invalid JSON: {e}"))?;
    if req.jsonrpc != "2.0" {
        return Err("jsonrpc must be \"2.0\"".into());
    }
    if req.method.is_empty() {
        return Err("method is required".into());
    }
    Ok(req)
}

/// Build a success response JSON string.
fn build_response(id: Option<Value>, result: Value) -> String {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    };
    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"}}"#.into()
    })
}

/// Build an error response JSON string.
fn build_error(id: Option<Value>, code: i32, message: &str) -> String {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcErrorBody {
            code,
            message: message.into(),
            data: None,
        }),
    };
    serde_json::to_string(&response).unwrap_or_else(|_| {
        format!(r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":{code},"message":"{message}"}}}}"#)
    })
}

fn handle_initialize(_request: &JsonRpcRequest, ctx: &ProtocolContext) -> Value {
    // Negotiate: intersect client capabilities with server capabilities.
    // For slice 1, we return the server's declared capabilities.
    let mut result = serde_json::Map::new();
    result.insert("protocolVersion".into(), Value::String("2024-11-05".into()));
    result.insert(
        "capabilities".into(),
        serde_json::to_value(&ctx.capabilities).unwrap_or(Value::Null),
    );
    result.insert(
        "serverInfo".into(),
        serde_json::json!({
            "name": ctx.server_name,
            "version": ctx.server_version
        }),
    );

    Value::Object(result)
}

fn handle_tools_list() -> Value {
    let tools: Vec<Value> = vec![
        serde_json::json!({
            "name": "wiki.query",
            "description": "Query the ArgOS OKF knowledge bundle",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Natural language query"}
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "workflow.recommend_reuse",
            "description": "Ask whether a similar workflow already exists",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "intent": {"type": "string", "description": "Description of the workflow intent"}
                },
                "required": ["intent"]
            }
        }),
        serde_json::json!({
            "name": "workflow.similar",
            "description": "Similarity search over workflow-concepts",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query for similar workflows"}
                },
                "required": ["query"]
            }
        }),
    ];

    let mut result = serde_json::Map::new();
    result.insert("tools".into(), Value::Array(tools));
    Value::Object(result)
}

fn handle_tools_call(request: &JsonRpcRequest) -> Value {
    let params = match request.params.as_ref() {
        Some(p) => p,
        None => {
            return serde_json::json!({
                "content": [{"type": "text", "text": "error: missing params"}],
                "isError": true
            });
        }
    };

    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let _args = params
        .get("arguments")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".into());

    if name.is_empty() {
        return serde_json::json!({
            "content": [{"type": "text", "text": "error: tool name is required"}],
            "isError": true
        });
    }

    // The actual tool call happens in the async server loop. Here we return
    // a protocol-level acknowledgment.
    serde_json::json!({
        "content": [{"type": "text", "text": "tool call dispatched"}],
        "isError": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> ProtocolContext {
        ProtocolContext {
            capabilities: McpCapabilities::default(),
            server_name: "argos-mcp".into(),
            server_version: "0.1.0".into(),
        }
    }

    // =====================================================================
    // T-030 Test 8: handles_initialize
    // =====================================================================
    #[test]
    fn handles_initialize() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{"tools":true}}}"#;
        let response = handle_message(request, &ctx).expect("should return a response");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert!(parsed["result"].is_object());
        assert!(parsed["result"]["capabilities"].is_object());
        assert!(parsed["result"]["capabilities"]["tools"] == true);
        assert!(parsed["result"]["protocolVersion"].is_string());
    }

    // =====================================================================
    // T-030 Test 9: handles_tools_list_rpc
    // =====================================================================
    #[test]
    fn handles_tools_list_rpc() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let response = handle_message(request, &ctx).expect("should return a response");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["id"], 2);
        let tools = parsed["result"]["tools"]
            .as_array()
            .expect("tools should be array");
        assert!(tools.len() >= 3, "should list at least 3 tools");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"wiki.query"));
        assert!(names.contains(&"workflow.recommend_reuse"));
        assert!(names.contains(&"workflow.similar"));
    }

    // =====================================================================
    // T-030 Test 10: handles_tools_call_rpc
    // =====================================================================
    #[test]
    fn handles_tools_call_rpc() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"wiki.query","arguments":{"query":"rust"}}}"#;
        let response = handle_message(request, &ctx).expect("should return a response");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["id"], 3);
        assert!(parsed["result"]["content"].is_array());
    }

    // =====================================================================
    // T-030 Test 11: handles_malformed_json
    // =====================================================================
    #[test]
    fn handles_malformed_json() {
        let ctx = make_ctx();
        let response = handle_message("not-json", &ctx).expect("should return parse error");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["error"]["code"], -32700);
        assert!(parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Parse"));
    }

    // =====================================================================
    // T-030 Test 12: handles_unknown_method
    // =====================================================================
    #[test]
    fn handles_unknown_method() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"2.0","id":99,"method":"nonexistent/method","params":{}}"#;
        let response = handle_message(request, &ctx).expect("should return error");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["id"], 99);
        assert_eq!(parsed["error"]["code"], -32601);
        assert!(parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not found"));
    }

    // =====================================================================
    // T-030 Test 13: notification_returns_none
    // =====================================================================
    #[test]
    fn notification_returns_none() {
        let ctx = make_ctx();
        // Notification — no "id" field.
        let request = r#"{"jsonrpc":"2.0","method":"tools/list","params":{}}"#;
        let response = handle_message(request, &ctx);
        assert!(
            response.is_none(),
            "notifications should not get a response"
        );
    }

    // =====================================================================
    // T-030 Test 14: handles_initialize_without_params
    // =====================================================================
    #[test]
    fn handles_initialize_without_params() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let response = handle_message(request, &ctx).expect("should return a response");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert!(parsed["result"]["capabilities"].is_object());
    }

    // =====================================================================
    // T-030 Test 15: tools_call_with_missing_name_returns_error
    // =====================================================================
    #[test]
    fn tools_call_with_missing_name_returns_error() {
        let ctx = make_ctx();
        let request =
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"arguments":"{}"}}"#;
        let response = handle_message(request, &ctx).expect("should return a response");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        let is_error = parsed["result"]["isError"].as_bool().unwrap_or(false);
        assert!(is_error, "missing tool name should produce an error result");
    }

    // =====================================================================
    // T-030 Test 16: jsonrpc_field_not_2_0_returns_parse_error
    // =====================================================================
    #[test]
    fn jsonrpc_field_not_2_0_returns_parse_error() {
        let ctx = make_ctx();
        let request = r#"{"jsonrpc":"1.0","id":1,"method":"tools/list"}"#;
        let response = handle_message(request, &ctx).expect("should return error");

        let parsed: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed["error"]["code"], -32700);
    }
}
