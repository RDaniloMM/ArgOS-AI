use url::Url;
fn main() {
    // Our current encoding (Url::parse_with_params)
    let url = Url::parse_with_params("https://auth.openai.com/oauth/authorize", &[
        ("response_type", "code"),
        ("client_id", "app_EMoamEEZ73f0CkXaXp7hrann"),
        ("redirect_uri", "http://localhost:1455/auth/callback"),
        ("scope", "openid profile email offline_access api.connectors.read api.connectors.invoke"),
        ("code_challenge", "abc123"),
        ("code_challenge_method", "S256"),
        ("state", "xyz789"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "argos-ui"),
    ]).unwrap();
    println!("=== Url::parse_with_params (our code) ===");
    println!("{}", url);
}
