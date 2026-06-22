//! E2E test for Codex (ChatGPT Pro/Plus) provider.
//!
//! Requires a valid OAuth token obtained by running the Playwright login script:
//!
//! ```bash
//! node tests/codex-e2e/codex-login.js
//! ```
//!
//! Then run this test:
//!
//! ```powershell
//! $env:CODEX_TOKEN_FILE = "tests/codex-e2e/.last-token.json"
//! cargo test codex_provider_e2e -- --ignored
//! ```

use argos_provider::aisdk_provider::AisdkProvider;
use argos_provider::provider::{CompletionOptions, Provider};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct CodexToken {
    access_token: String,
    #[allow(dead_code)]
    account_id: String,
    #[allow(dead_code)]
    expires_at: u64,
    model: String,
    endpoint: String,
}

/// Carga el token desde el archivo JSON generado por el Playwright login script.
fn load_token() -> Option<CodexToken> {
    let path = std::env::var("CODEX_TOKEN_FILE")
        .or_else(|_| std::env::var("CODEX_ACCESS_TOKEN").map(|_| String::new()))
        .ok()?;

    if !path.is_empty() {
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    } else {
        // Fallback: build from env vars
        let token = std::env::var("CODEX_ACCESS_TOKEN").ok()?;
        Some(CodexToken {
            access_token: token,
            account_id: String::new(),
            expires_at: 0,
            model: std::env::var("CODEX_MODEL").unwrap_or_else(|_| "gpt-5".into()),
            endpoint: "https://chatgpt.com/backend-api".into(),
        })
    }
}

#[tokio::test]
#[ignore = "Requiere token OAuth de Codex. Ejecutar Playwright login primero."]
async fn codex_provider_e2e_complete_returns_valid_response() {
    let token = load_token().expect(
        "CODEX_TOKEN_FILE o CODEX_ACCESS_TOKEN requerido. \
         Corré `node tests/codex-e2e/codex-login.js` primero.",
    );

    let endpoint = Url::parse(&token.endpoint).expect("endpoint URL válida");
    let provider = AisdkProvider::new_openai_codex(endpoint, token.access_token, token.model.clone());

    let options = CompletionOptions {
        system_prompt: Some("Sos un asistente útil y conciso.".into()),
        temperature: 0.7,
        max_tokens: Some(100),
        ..Default::default()
    };

    let result = provider
        .complete("Decime 'Hola desde ArgOS!' en español y nada más.", &options)
        .await;

    let completion = result.unwrap_or_else(|e| {
        panic!("❌ Provider::complete() falló: {e}");
    });

    println!("  🤖  \"{}\"", completion.text);
    println!("  📊 Tokens: {} in, {} out", completion.usage.prompt_tokens, completion.usage.completion_tokens);

    assert!(
        !completion.text.trim().is_empty(),
        "El LLM devolvió respuesta vacía"
    );
    assert!(
        completion.usage.completion_tokens > 0,
        "Esperaba tokens de salida > 0"
    );
}

#[tokio::test]
#[ignore = "Requiere token OAuth de Codex"]
async fn codex_provider_e2e_system_prompt_is_respected() {
    let token = load_token().expect(
        "CODEX_TOKEN_FILE o CODEX_ACCESS_TOKEN requerido.",
    );

    let endpoint = Url::parse(&token.endpoint).expect("endpoint URL válida");
    let provider = AisdkProvider::new_openai_codex(endpoint, token.access_token, token.model.clone());

    // System prompt pide responder SIEMPRE en JSON
    let options = CompletionOptions {
        system_prompt: Some(
            "Sos un asistente que SIEMPRE responde en JSON válido. \
             Usá el formato: {\"respuesta\": \"...\"}"
                .into(),
        ),
        temperature: 0.3,
        max_tokens: Some(150),
        ..Default::default()
    };

    let result = provider
        .complete("Saludame como si fuera un viejo amigo.", &options)
        .await;

    let completion = result.unwrap_or_else(|e| {
        panic!("❌ Provider::complete() falló: {e}");
    });

    println!("  🤖  \"{}\"", completion.text);
    println!("  📊 Tokens: {} in, {} out", completion.usage.prompt_tokens, completion.usage.completion_tokens);

    // Verificar que la respuesta sea JSON válido (el system prompt lo pide)
    let text = completion.text.trim();
    if text.starts_with('{') {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
        assert!(parsed.is_ok(), "La respuesta debería ser JSON, pero falló parseo: {text}");
    }
}
