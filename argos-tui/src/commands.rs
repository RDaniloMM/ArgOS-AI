#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    Help,
    ClearTranscript,
    Quit,
    Refresh,
    ShowConfig,
    SetProvider { backend: String, model: String },
    SetModel { model: String },
    SetEndpoint { url: String },
    SetKeyRef { key_ref: String },
    SetN8n { url: String },
    SetN8nMode { mode: String },
    SetN8nKeyRef { key_ref: String },
    StoreSecret { key_ref: String, secret: String },
    DeleteSecret { key_ref: String },
    ListProviders,
    ChangeDir { path: String },
    ClearSessions,
}

pub fn parse_slash_command(text: &str) -> Option<ConfigCommand> {
    let input = text.trim();
    if !input.starts_with('/') {
        return None;
    }

    let (cmd, args) = split_first_word(&input[1..]);
    match cmd {
        "help" | "h" => Some(ConfigCommand::Help),
        "clear" | "cls" => Some(ConfigCommand::ClearTranscript),
        "quit" | "exit" | "q" => Some(ConfigCommand::Quit),
        "refresh" | "r" => Some(ConfigCommand::Refresh),
        "config" => Some(ConfigCommand::ShowConfig),
        "providers" => Some(ConfigCommand::ListProviders),
        "provider" => parse_set_provider(args),
        "cd" => Some(ConfigCommand::ChangeDir {
            path: args.trim().to_string(),
        }),
        "clearsessions" => Some(ConfigCommand::ClearSessions),
        "model" if !args.is_empty() => Some(ConfigCommand::SetModel {
            model: args.to_string(),
        }),
        "endpoint" if !args.is_empty() => Some(ConfigCommand::SetEndpoint {
            url: args.trim().to_string(),
        }),
        "key-ref" if !args.is_empty() => Some(ConfigCommand::SetKeyRef {
            key_ref: args.trim().to_string(),
        }),
        "n8n" if !args.is_empty() => Some(ConfigCommand::SetN8n {
            url: args.trim().to_string(),
        }),
        "n8n-mode" if !args.is_empty() => Some(ConfigCommand::SetN8nMode {
            mode: args.trim().to_lowercase().to_string(),
        }),
        "n8n-key-ref" if !args.is_empty() => Some(ConfigCommand::SetN8nKeyRef {
            key_ref: args.trim().to_string(),
        }),
        "vault" => parse_vault_command(args),
        _ => None,
    }
}

fn parse_set_provider(args: &str) -> Option<ConfigCommand> {
    let parts: Vec<&str> = args.splitn(2, char::is_whitespace).collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return None;
    }
    Some(ConfigCommand::SetProvider {
        backend: parts[0].trim().to_string(),
        model: parts[1].trim().to_string(),
    })
}

fn parse_vault_command(args: &str) -> Option<ConfigCommand> {
    let (subcmd, rest) = split_first_word(args);
    match subcmd {
        "set" => {
            let (key_ref, secret) = split_first_word(rest);
            if key_ref.is_empty() || secret.is_empty() {
                return None;
            }
            Some(ConfigCommand::StoreSecret {
                key_ref: key_ref.to_string(),
                secret: secret.to_string(),
            })
        }
        "remove" | "rm" => {
            let key_ref = rest.trim();
            if key_ref.is_empty() {
                return None;
            }
            Some(ConfigCommand::DeleteSecret {
                key_ref: key_ref.to_string(),
            })
        }
        _ => None,
    }
}

fn split_first_word(input: &str) -> (&str, &str) {
    let trimmed = input.trim_start();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let word = &trimmed[..end];
    let rest = trimmed[end..].trim_start();
    (word, rest)
}

pub fn help_text() -> &'static str {
    "\
Available commands:
  /help              Show this help
  /clear             Clear the transcript
  /quit              Quit ArgOS TUI
  /config            Show current configuration
  /providers         List known providers and endpoints
  /refresh           Refresh provider and n8n status
  /provider <bk> <m> Set provider (auto-configures endpoint + key ref)
  /model <name>      Change the model
  /endpoint <url>    Change the provider endpoint
  /key-ref <ref>     Set the API key reference
  /n8n <url>         Set the n8n endpoint URL
  /n8n-mode <m>      Set n8n mode (rest | mcp)
  /n8n-key-ref <ref> Set the n8n API key reference
  /vault set <ref> <s>  Store a secret in the OS keyring
  /vault remove <ref>   Remove a secret from keyring
  /cd <path>            Change working directory
  /clearsessions        Delete all saved session files"
}

/// Pricing in USD per million tokens.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

#[derive(Debug, Clone)]
pub struct KnownProvider {
    pub backend: &'static str,
    pub default_endpoint: Option<&'static str>,
    pub default_key_ref: Option<&'static str>,
    pub models: &'static [&'static str],
    pub pricing: Option<Pricing>,
}

pub const KNOWN_PROVIDERS: &[KnownProvider] = &[
    KnownProvider {
        backend: "openai",
        default_endpoint: Some("https://api.openai.com/v1"),
        default_key_ref: Some("provider/openai/api_key"),
        models: &[
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "o3",
            "o4-mini",
            "gpt-4o",
            "gpt-4o-mini",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 2.50,
            output_per_mtok: 10.00,
        }),
    },
    KnownProvider {
        backend: "openrouter",
        default_endpoint: Some("https://openrouter.ai/api/v1"),
        default_key_ref: Some("provider/openrouter/api_key"),
        models: &[
            "deepseek/deepseek-v4-pro",
            "deepseek/deepseek-v4-flash",
            "openai/gpt-4.1",
            "openai/o3",
            "openai/gpt-4o",
            "anthropic/claude-sonnet-4-20250514",
            "anthropic/claude-opus-4-20250514",
            "google/gemini-2.5-pro",
            "google/gemini-2.5-flash",
            "meta-llama/llama-4-maverick",
            "meta-llama/llama-4-scout",
            "qwen/qwen-3",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 2.00,
            output_per_mtok: 8.00,
        }),
    },
    KnownProvider {
        backend: "anthropic",
        default_endpoint: Some("https://api.anthropic.com/v1"),
        default_key_ref: Some("provider/anthropic/api_key"),
        models: &[
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-3.5-haiku-20241022",
            "claude-3.5-sonnet-20241022",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 3.00,
            output_per_mtok: 15.00,
        }),
    },
    KnownProvider {
        backend: "groq",
        default_endpoint: Some("https://api.groq.com/openai/v1"),
        default_key_ref: Some("provider/groq/api_key"),
        models: &[
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "mixtral-8x7b-32768",
            "deepseek-r1-distill-llama-70b",
            "gemma2-9b-it",
            "qwen-2.5-32b",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 0.0,
            output_per_mtok: 0.0,
        }),
    },
    KnownProvider {
        backend: "deepseek",
        default_endpoint: Some("https://api.deepseek.com/v1"),
        default_key_ref: Some("provider/deepseek/api_key"),
        models: &["deepseek-v4-pro", "deepseek-v4-flash"],
        pricing: Some(Pricing {
            input_per_mtok: 0.27,
            output_per_mtok: 1.10,
        }),
    },
    KnownProvider {
        backend: "opencode",
        default_endpoint: Some("https://opencode.ai/zen/go/v1"),
        default_key_ref: Some("provider/opencode/api_key"),
        models: &[
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "gpt-4.1",
            "gpt-4o",
            "o3",
            "o4-mini",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 2.00,
            output_per_mtok: 8.00,
        }),
    },
    KnownProvider {
        backend: "ollama",
        default_endpoint: Some("http://localhost:11434"),
        default_key_ref: None,
        models: &[
            "llama3.2",
            "llama3.1:8b",
            "mistral",
            "codellama",
            "phi4",
            "gemma2",
            "qwen2.5",
            "deepseek-r1:8b",
            "nomic-embed-text",
        ],
        pricing: None,
    },
    KnownProvider {
        backend: "google",
        default_endpoint: Some("https://generativelanguage.googleapis.com/v1beta"),
        default_key_ref: Some("provider/google/api_key"),
        models: &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.0-flash-lite",
            "gemma-3-27b-it",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 0.0,
            output_per_mtok: 0.0,
        }),
    },
    KnownProvider {
        backend: "xai",
        default_endpoint: Some("https://api.x.ai/v1"),
        default_key_ref: Some("provider/xai/api_key"),
        models: &["grok-3-beta", "grok-3-mini-beta"],
        pricing: Some(Pricing {
            input_per_mtok: 3.00,
            output_per_mtok: 10.00,
        }),
    },
    KnownProvider {
        backend: "mistral",
        default_endpoint: Some("https://api.mistral.ai/v1"),
        default_key_ref: Some("provider/mistral/api_key"),
        models: &[
            "mistral-large-latest",
            "mistral-medium-latest",
            "codestral-latest",
            "mistral-small-latest",
        ],
        pricing: Some(Pricing {
            input_per_mtok: 2.00,
            output_per_mtok: 6.00,
        }),
    },
    KnownProvider {
        backend: "cerebras",
        default_endpoint: Some("https://api.cerebras.ai/v1"),
        default_key_ref: Some("provider/cerebras/api_key"),
        models: &["llama-3.3-70b", "llama-3.1-8b"],
        pricing: Some(Pricing {
            input_per_mtok: 0.60,
            output_per_mtok: 0.60,
        }),
    },
];

pub fn known_provider(backend: &str) -> Option<&'static KnownProvider> {
    KNOWN_PROVIDERS
        .iter()
        .find(|p| p.backend.eq_ignore_ascii_case(backend))
}

pub fn providers_list_text() -> String {
    let mut lines = vec!["Known providers:".into(), String::new()];
    for p in KNOWN_PROVIDERS {
        let endpoint = p.default_endpoint.unwrap_or("(none)");
        let key = p.key_description();
        let models = p.models.join(", ");
        lines.push(format!("  {}  →  {endpoint}", p.backend));
        lines.push(format!("    Models: {models}"));
        lines.push(format!("    Key ref: {key}"));
        lines.push(String::new());
    }
    lines.join("\n")
}

impl KnownProvider {
    pub fn key_description(&self) -> &str {
        match self.default_key_ref {
            Some(k) => k,
            None => "(no API key required)",
        }
    }
}

const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show available commands"),
    ("/clear", "Clear the transcript"),
    ("/quit", "Quit ArgOS TUI"),
    ("/config", "Show current configuration"),
    ("/providers", "List known providers and models"),
    ("/refresh", "Refresh provider and n8n status"),
    (
        "/provider <backend> <model>",
        "Set provider (auto-configures endpoint + key ref)",
    ),
    ("/model <name>", "Change the model"),
    ("/endpoint <url>", "Change the provider endpoint"),
    ("/key-ref <ref>", "Set the API key reference"),
    ("/n8n <url>", "Set the n8n endpoint URL"),
    ("/n8n-mode <rest|mcp>", "Set n8n mode"),
    ("/n8n-key-ref <ref>", "Set the n8n API key reference"),
    (
        "/vault set <ref> <secret>",
        "Store a secret in the OS keyring",
    ),
    ("/vault remove <ref>", "Remove a secret from keyring"),
    ("/cd <path>", "Change working directory"),
    ("/clearsessions", "Delete all saved session files"),
];

pub fn suggest_commands(prefix: &str) -> Vec<String> {
    let text = prefix.trim();
    if !text.starts_with('/') {
        return Vec::new();
    }

    let lower = text.to_lowercase();
    COMMANDS
        .iter()
        .filter(|(sig, _)| sig.to_lowercase().starts_with(&lower))
        .map(|(sig, _)| sig.to_string())
        .collect()
}

pub fn best_completion(prefix: &str) -> Option<String> {
    let suggestions = suggest_commands(prefix);
    suggestions.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_not_a_command() {
        assert_eq!(parse_slash_command("hello world"), None);
        assert_eq!(parse_slash_command("not a command"), None);
    }

    #[test]
    fn slash_help_is_recognised() {
        assert_eq!(parse_slash_command("/help"), Some(ConfigCommand::Help));
        assert_eq!(parse_slash_command("/h"), Some(ConfigCommand::Help));
        assert_eq!(parse_slash_command("  /help  "), Some(ConfigCommand::Help));
    }

    #[test]
    fn slash_clear_quits_and_refreshes() {
        assert_eq!(
            parse_slash_command("/clear"),
            Some(ConfigCommand::ClearTranscript)
        );
        assert_eq!(parse_slash_command("/quit"), Some(ConfigCommand::Quit));
        assert_eq!(
            parse_slash_command("/refresh"),
            Some(ConfigCommand::Refresh)
        );
    }

    #[test]
    fn slash_config_shows() {
        assert_eq!(
            parse_slash_command("/config"),
            Some(ConfigCommand::ShowConfig)
        );
    }

    #[test]
    fn provider_parses_backend_and_model() {
        assert_eq!(
            parse_slash_command("/provider openai gpt-4o"),
            Some(ConfigCommand::SetProvider {
                backend: "openai".into(),
                model: "gpt-4o".into()
            })
        );
        assert_eq!(
            parse_slash_command("/provider ollama llama3.2"),
            Some(ConfigCommand::SetProvider {
                backend: "ollama".into(),
                model: "llama3.2".into()
            })
        );
    }

    #[test]
    fn provider_needs_two_args() {
        assert_eq!(parse_slash_command("/provider openai"), None);
        assert_eq!(parse_slash_command("/provider"), None);
    }

    #[test]
    fn model_and_endpoint_parse() {
        assert_eq!(
            parse_slash_command("/model gpt-4o-mini"),
            Some(ConfigCommand::SetModel {
                model: "gpt-4o-mini".into()
            })
        );
        assert_eq!(
            parse_slash_command("/endpoint https://api.openai.com/v1"),
            Some(ConfigCommand::SetEndpoint {
                url: "https://api.openai.com/v1".into()
            })
        );
        assert_eq!(parse_slash_command("/model"), None);
    }

    #[test]
    fn key_ref_parses() {
        assert_eq!(
            parse_slash_command("/key-ref openai"),
            Some(ConfigCommand::SetKeyRef {
                key_ref: "openai".into()
            })
        );
    }

    #[test]
    fn n8n_commands_parse() {
        assert_eq!(
            parse_slash_command("/n8n http://localhost:5678"),
            Some(ConfigCommand::SetN8n {
                url: "http://localhost:5678".into()
            })
        );
        assert_eq!(
            parse_slash_command("/n8n-mode rest"),
            Some(ConfigCommand::SetN8nMode {
                mode: "rest".into()
            })
        );
        assert_eq!(
            parse_slash_command("/n8n-key-ref n8n_key"),
            Some(ConfigCommand::SetN8nKeyRef {
                key_ref: "n8n_key".into()
            })
        );
    }

    #[test]
    fn vault_set_parses() {
        assert_eq!(
            parse_slash_command("/vault set openai sk-1234"),
            Some(ConfigCommand::StoreSecret {
                key_ref: "openai".into(),
                secret: "sk-1234".into()
            })
        );
    }

    #[test]
    fn vault_remove_parses() {
        assert_eq!(
            parse_slash_command("/vault remove openai"),
            Some(ConfigCommand::DeleteSecret {
                key_ref: "openai".into()
            })
        );
        assert_eq!(
            parse_slash_command("/vault rm openai"),
            Some(ConfigCommand::DeleteSecret {
                key_ref: "openai".into()
            })
        );
    }

    #[test]
    fn vault_needs_subcommand() {
        assert_eq!(parse_slash_command("/vault"), None);
        assert_eq!(parse_slash_command("/vault unknown"), None);
    }

    #[test]
    fn suggestions_filter_by_prefix() {
        let suggestions = suggest_commands("/pro");
        assert!(suggestions.contains(&"/provider <backend> <model>".to_string()));
        assert!(suggestions.contains(&"/providers".to_string()));
        assert_eq!(suggestions.len(), 2);
    }

    #[test]
    fn suggestions_empty_when_not_slash() {
        assert!(suggest_commands("hello").is_empty());
        assert!(suggest_commands("not a command").is_empty());
    }

    #[test]
    fn suggestions_match_case_insensitive() {
        let suggestions = suggest_commands("/HELP");
        assert_eq!(suggestions, vec!["/help".to_string()]);
    }
}
