//! Integration tests for `AisdkProvider` using httpmock.
//!
//! Starts a local mock HTTP server, instantiates `AisdkProvider` (both OpenAI and
//! Ollama variants) pointing at the mock, and verifies:
//!
//! - Correct HTTP requests (method, path, headers, body shape)
//! - Correct response parsing (text, usage, embeddings)
//! - Health check behaviour (GET /v1/models)

use argos_provider::aisdk_provider::AisdkProvider;
use argos_provider::provider::{CompletionOptions, Provider};
use httpmock::prelude::*;
use url::Url;

// ---------------------------------------------------------------------------
// Helper: standard OpenAI-compatible response bodies
// ---------------------------------------------------------------------------

fn chat_completion_body(text: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    })
    .to_string()
}

fn embedding_body() -> String {
    serde_json::json!({
        "object": "list",
        "data": [{
            "object": "embedding",
            "index": 0,
            "embedding": [0.1, 0.2, 0.3]
        }],
        "model": "text-embedding-model",
        "usage": {
            "prompt_tokens": 5,
            "total_tokens": 5
        }
    })
    .to_string()
}

fn models_list_body() -> String {
    serde_json::json!({
        "object": "list",
        "data": [
            {"id": "model-1", "object": "model"},
            {"id": "model-2", "object": "model"}
        ]
    })
    .to_string()
}

/// Base endpoint URL for a given mock server (uses `/v1` path prefix).
fn v1_endpoint(server: &MockServer) -> Url {
    Url::parse(&format!("http://127.0.0.1:{}/v1", server.port())).unwrap()
}

// ---------------------------------------------------------------------------
// OpenAI-compatible endpoint integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_compatible_complete_returns_parsed_text() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .header("Authorization", "Bearer sk-test-123")
            .json_body_partial(r#"{"model": "gpt-4o"}"#);
        then.status(200)
            .header("Content-Type", "application/json")
            .body(chat_completion_body("Hello from OpenAI!"));
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test-123".into(), "gpt-4o".into());
    let result = provider
        .complete("test prompt", &CompletionOptions::default())
        .await
        .unwrap();

    assert_eq!(result.text, "Hello from OpenAI!");
    assert_eq!(result.usage.prompt_tokens, 10);
    assert_eq!(result.usage.completion_tokens, 5);
    mock.assert();
}

#[tokio::test]
async fn openai_compatible_embed_returns_vector() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            // OpenAICompatible delegates embeddings to OpenAI which uses
            // hardcoded path "/v1/embeddings". Combined with base_url having
            // "/v1", the effective path is "/v1/v1/embeddings".
            .path("/v1/v1/embeddings")
            .header("Authorization", "Bearer sk-test-123");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(embedding_body());
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test-123".into(), "gpt-4o".into());
    let result = provider.embed("test text").await.unwrap();

    assert_eq!(result.len(), 3);
    assert!((result[0] - 0.1).abs() < 1e-6);
    mock.assert();
}

#[tokio::test]
async fn openai_compatible_health_check_succeeds() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(models_list_body());
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test-123".into(), "gpt-4o".into());
    let result = provider.health_check().await;

    assert!(result.is_ok());
    mock.assert();
}

#[tokio::test]
async fn openai_compatible_health_check_fails_on_non_200() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(500);
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test-123".into(), "gpt-4o".into());
    let result = provider.health_check().await;

    assert!(result.is_err());
    mock.assert();
}

// ---------------------------------------------------------------------------
// Ollama endpoint integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ollama_complete_returns_parsed_text() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .json_body_partial(r#"{"model": "llama3"}"#);
        then.status(200)
            .header("Content-Type", "application/json")
            .body(chat_completion_body("Hello from Ollama!"));
    });

    let provider = AisdkProvider::new_ollama(endpoint, "llama3".into());
    let result = provider
        .complete("test prompt", &CompletionOptions::default())
        .await
        .unwrap();

    assert_eq!(result.text, "Hello from Ollama!");
    assert_eq!(result.usage.prompt_tokens, 10);
    mock.assert();
}

#[tokio::test]
async fn ollama_embed_returns_vector() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        // OpenAICompatible delegates embeddings to OpenAI which uses
        // hardcoded path "/v1/embeddings". Combined with base_url having
        // "/v1", the effective path is "/v1/v1/embeddings".
        when.method(POST).path("/v1/v1/embeddings");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(embedding_body());
    });

    let provider = AisdkProvider::new_ollama(endpoint, "llama3".into());
    let result = provider.embed("test text").await.unwrap();

    assert_eq!(result.len(), 3);
    mock.assert();
}

#[tokio::test]
async fn ollama_health_check_succeeds() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(models_list_body());
    });

    let provider = AisdkProvider::new_ollama(endpoint, "llama3".into());
    let result = provider.health_check().await;

    assert!(result.is_ok());
    mock.assert();
}

// ---------------------------------------------------------------------------
// Request verification tests (method, path, headers, body shape)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_compatible_request_includes_auth_header() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .header("Authorization", "Bearer sk-test-req");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(chat_completion_body("OK"));
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test-req".into(), "gpt-4o".into());
    let _ = provider
        .complete("Hello", &CompletionOptions::default())
        .await
        .unwrap();

    mock.assert_hits(1);
}

#[tokio::test]
async fn openai_request_body_contains_model_and_messages() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .body_contains(r#""model":"gpt-4o""#)
            .body_contains(r#""messages""#);
        then.status(200)
            .header("Content-Type", "application/json")
            .body(chat_completion_body("OK"));
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test".into(), "gpt-4o".into());
    let _ = provider
        .complete("Hello", &CompletionOptions::default())
        .await
        .unwrap();

    mock.assert_hits(1);
}

#[tokio::test]
async fn ollama_request_body_contains_model_name() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .body_contains(r#""model":"llama3""#);
        then.status(200)
            .header("Content-Type", "application/json")
            .body(chat_completion_body("OK"));
    });

    let provider = AisdkProvider::new_ollama(endpoint, "llama3".into());
    let _ = provider
        .complete("Hello", &CompletionOptions::default())
        .await
        .unwrap();

    mock.assert_hits(1);
}

#[tokio::test]
async fn health_check_sends_get_request() {
    let server = MockServer::start();
    let endpoint = v1_endpoint(&server);

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/models");
        then.status(200)
            .header("Content-Type", "application/json")
            .body(models_list_body());
    });

    let provider = AisdkProvider::new_openai(endpoint, "sk-test".into(), "gpt-4o".into());
    let _ = provider.health_check().await.unwrap();

    mock.assert_hits(1);
}
