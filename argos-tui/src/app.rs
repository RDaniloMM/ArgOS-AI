use crate::action::Action;
use crate::commands::{self, ConfigCommand};
use crate::event::AsyncEvent;
use crate::state::{AppState, FlashMessage, FocusPane, PopupColumn, ResourceStatus, StatusLevel};
use argos_core::{
    Config, ConnMode, N8nConnection, N8nRunRef, N8nRunStatus, ProviderConfig, ToolInvocation,
    ToolResult,
};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    LoadSnapshot,
    SubmitPrompt {
        prompt: String,
    },
    RunWorkflow {
        workflow_id: String,
        workflow_name: String,
    },
    SaveConfig {
        config: Box<Config>,
    },
    StoreSecret {
        key_ref: String,
        secret: String,
    },
    DeleteSecret {
        key_ref: String,
    },
    FetchModels {
        backend: String,
        endpoint: String,
        api_key_ref: Option<String>,
    },
}

pub fn handle_action(state: &mut AppState, action: Action) -> Vec<Command> {
    if state.provider_popup.visible {
        return handle_popup_action(state, action);
    }

    match action {
        Action::FocusNext => state.focus = state.focus.next(),
        Action::FocusPrev => state.focus = state.focus.prev(),
        Action::MoveUp => match state.focus {
            FocusPane::Workflows => state.move_workflow_selection(-1),
            FocusPane::Transcript => state.scroll_transcript_lines(-1),
            FocusPane::Composer => state.composer.move_up(),
            FocusPane::Activity => state.move_activity_selection(-1),
        },
        Action::MoveDown => match state.focus {
            FocusPane::Workflows => state.move_workflow_selection(1),
            FocusPane::Transcript => state.scroll_transcript_lines(1),
            FocusPane::Composer => state.composer.move_down(),
            FocusPane::Activity => state.move_activity_selection(1),
        },
        Action::PageUp => match state.focus {
            FocusPane::Transcript => state.page_transcript_up(),
            FocusPane::Activity => state.move_activity_selection(-8),
            FocusPane::Workflows => state.move_workflow_selection(-8),
            FocusPane::Composer => state.composer.move_up(),
        },
        Action::PageDown => match state.focus {
            FocusPane::Transcript => state.page_transcript_down(),
            FocusPane::Activity => state.move_activity_selection(8),
            FocusPane::Workflows => state.move_workflow_selection(8),
            FocusPane::Composer => state.composer.move_down(),
        },
        Action::Refresh => {
            if state.is_loading_snapshot {
                return Vec::new();
            }

            state.is_loading_snapshot = true;
            state.provider_status.level = StatusLevel::Loading;
            state.provider_status.detail = "Refreshing provider status…".into();
            state.n8n_status.level = StatusLevel::Loading;
            state.n8n_status.detail = "Refreshing n8n status…".into();
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Refreshing provider and n8n status…".into(),
            });
            state.push_activity(
                StatusLevel::Loading,
                "Refresh started",
                "Loading provider connectivity and workflow inventory.",
            );
            return vec![Command::LoadSnapshot];
        }
        Action::SubmitPrompt => {
            if state.is_submitting_prompt {
                return Vec::new();
            }
            if state.composer.is_empty() {
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: "Composer is empty.".into(),
                });
                return Vec::new();
            }

            let prompt = state.composer.to_text();
            let slash = commands::parse_slash_command(&prompt);
            state.composer.clear();
            state.suggestions.clear();

            match slash {
                Some(cmd) => return handle_slash_command(state, cmd),
                None => {
                    state.push_transcript("You", prompt.clone(), None);
                    state.push_activity(
                        StatusLevel::Loading,
                        "Prompt submitted",
                        "Waiting for GenericAgent output.",
                    );
                    state.focus = FocusPane::Transcript;
                    state.is_submitting_prompt = true;
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Loading,
                        text: "Sending prompt through GenericAgent…".into(),
                    });
                    return vec![Command::SubmitPrompt { prompt }];
                }
            }
        }
        Action::RunSelectedWorkflow => {
            if state.is_running_workflow {
                return Vec::new();
            }
            let Some(workflow) = state.selected_workflow().cloned() else {
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: "No workflow is selected.".into(),
                });
                return Vec::new();
            };

            state.is_running_workflow = true;
            state.push_activity(
                StatusLevel::Loading,
                format!("Running workflow {}", workflow.name),
                format!("Dispatching workflow id {}.", workflow.id),
            );
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: format!("Running workflow {}…", workflow.name),
            });
            return vec![Command::RunWorkflow {
                workflow_id: workflow.id,
                workflow_name: workflow.name,
            }];
        }
        Action::Escape => {
            if state.provider_popup.visible {
                state.provider_popup.visible = false;
            } else if state.is_submitting_prompt {
                let now = std::time::Instant::now();
                if let Some(last) = state.esc_last_press {
                    if now.duration_since(last) < std::time::Duration::from_millis(800) {
                        state.is_submitting_prompt = false;
                        state.esc_last_press = None;
                        state.flash = Some(FlashMessage {
                            level: StatusLevel::Error,
                            text: "Agent request cancelled.".into(),
                        });
                        state.push_transcript(
                            "System",
                            "Request cancelled by user.".to_string(),
                            None,
                        );
                        return Vec::new();
                    }
                }
                state.esc_last_press = Some(now);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Loading,
                    text: "Press Esc again to cancel agent request.".into(),
                });
            } else if state.focus == FocusPane::Composer {
                state.focus = FocusPane::Transcript;
            } else {
                state.flash = None;
            }
        }
        Action::Quit => {
            if state.focus != FocusPane::Composer {
                state.should_quit = true;
            }
        }
        Action::ToggleActivity => {
            state.activity_visible = !state.activity_visible;
        }
        Action::ToggleSidebar => {
            state.sidebar_visible = !state.sidebar_visible;
        }
        Action::CopySelection => {
            if let Some((start, end)) = state.composer.selection() {
                let lines = state.composer.lines();
                let text: String = if start.row == end.row {
                    lines[start.row]
                        .chars()
                        .skip(start.col)
                        .take(end.col.saturating_sub(start.col))
                        .collect()
                } else {
                    let mut parts: Vec<String> = Vec::new();
                    parts.push(lines[start.row].chars().skip(start.col).collect());
                    for line in lines[(start.row + 1)..end.row].iter() {
                        parts.push(line.to_string());
                    }
                    parts.push(lines[end.row].chars().take(end.col).collect());
                    parts.join("\n")
                };
                copy_to_clipboard(&text);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Success,
                    text: "Copied to clipboard.".into(),
                });
            } else {
                let text = state.composer.to_text();
                if !text.is_empty() {
                    copy_to_clipboard(&text);
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Success,
                        text: "Copied composer to clipboard.".into(),
                    });
                }
            }
        }
        Action::ComposerInsert(ch) => {
            if state.focus == FocusPane::Composer {
                state.composer.insert_char(ch);
                state.recompute_suggestions();
            }
        }
        Action::ComposerNewline => {
            if state.focus == FocusPane::Composer {
                state.composer.insert_newline();
                state.recompute_suggestions();
            }
        }
        Action::ComposerBackspace => {
            if state.focus == FocusPane::Composer {
                state.composer.backspace();
                state.recompute_suggestions();
            }
        }
        Action::ComposerMoveLeft => {
            if state.focus == FocusPane::Composer {
                state.composer.move_left();
            }
        }
        Action::ComposerMoveRight => {
            if state.focus == FocusPane::Composer {
                state.composer.move_right();
            }
        }
        Action::ComposerMoveUp => {
            if state.focus == FocusPane::Composer {
                state.composer.move_up();
            }
        }
        Action::ComposerMoveDown => {
            if state.focus == FocusPane::Composer {
                state.composer.move_down();
            }
        }
        Action::ComposerMoveHome => {
            if state.focus == FocusPane::Composer {
                state.composer.move_home();
            }
        }
        Action::ComposerMoveEnd => {
            if state.focus == FocusPane::Composer {
                state.composer.move_end();
            }
        }
        Action::ComposerSelectLeft => {
            if state.focus == FocusPane::Composer {
                state.composer.select_left();
            }
        }
        Action::ComposerSelectRight => {
            if state.focus == FocusPane::Composer {
                state.composer.select_right();
            }
        }
        Action::ComposerSelectUp => {
            if state.focus == FocusPane::Composer {
                state.composer.select_up();
            }
        }
        Action::ComposerSelectDown => {
            if state.focus == FocusPane::Composer {
                state.composer.select_down();
            }
        }
        Action::ComposerSelectHome => {
            if state.focus == FocusPane::Composer {
                state.composer.select_home();
            }
        }
        Action::ComposerSelectEnd => {
            if state.focus == FocusPane::Composer {
                state.composer.select_end();
            }
        }
        Action::ComposerAutocomplete => {
            if state.focus == FocusPane::Composer {
                if let Some(completion) =
                    crate::commands::best_completion(&state.composer.to_text())
                {
                    state.composer.clear();
                    for ch in completion.chars() {
                        state.composer.insert_char(ch);
                    }
                    state.recompute_suggestions();
                }
            }
        }
        Action::ShowProviderPopup => {
            state.provider_popup.visible = true;
            state.provider_popup.selected_provider = 0;
            state.provider_popup.selected_model = 0;
            state.provider_popup.column = PopupColumn::Provider;

            let backend = "openrouter".to_string();
            let endpoint = "https://openrouter.ai/api/v1".to_string();
            if !state.dynamic_models.contains_key(&backend) {
                state.push_activity(
                    StatusLevel::Loading,
                    "Fetching models…".to_string(),
                    "GET openrouter.ai/api/v1/models (free, no auth)".to_string(),
                );
                return vec![Command::FetchModels {
                    backend,
                    endpoint,
                    api_key_ref: None,
                }];
            }
        }
        Action::HideProviderPopup => {
            state.provider_popup.visible = false;
        }
        Action::PopupUp => {
            let popup = &mut state.provider_popup;
            if !popup.visible {
                return Vec::new();
            }
            match popup.column {
                PopupColumn::Provider => {
                    popup.selected_provider = popup.selected_provider.saturating_sub(1);
                }
                PopupColumn::Model => {
                    popup.selected_model = popup.selected_model.saturating_sub(1);
                }
            }
        }
        Action::PopupDown => {
            let popup = &mut state.provider_popup;
            if !popup.visible {
                return Vec::new();
            }
            let providers = crate::commands::KNOWN_PROVIDERS;
            match popup.column {
                PopupColumn::Provider => {
                    let max = providers.len().saturating_sub(1);
                    popup.selected_provider = (popup.selected_provider + 1).min(max);
                }
                PopupColumn::Model => {
                    if let Some(kp) = providers.get(popup.selected_provider) {
                        let max = kp.models.len().saturating_sub(1);
                        popup.selected_model = (popup.selected_model + 1).min(max);
                    }
                }
            }
        }
        Action::PopupLeft => {
            if state.provider_popup.visible {
                state.provider_popup.column = PopupColumn::Provider;
            }
        }
        Action::PopupRight => {
            if state.provider_popup.visible {
                state.provider_popup.column = PopupColumn::Model;
                state.provider_popup.selected_model = 0;
            }
        }
        Action::PopupSelect => {
            let popup = &state.provider_popup;
            if !popup.visible {
                return Vec::new();
            }
            let providers = crate::commands::KNOWN_PROVIDERS;
            if let Some(kp) = providers.get(popup.selected_provider) {
                let model = if let Some(m) = kp.models.get(popup.selected_model) {
                    m.to_string()
                } else {
                    kp.backend.to_string()
                };
                let backend = kp.backend.to_string();
                state.provider_popup.visible = false;

                let mut config = ensure_config(state);
                config.provider.backend = backend.clone();
                config.provider.model = model.clone();
                if let Some(endpoint) = kp.default_endpoint {
                    config.provider.endpoint = Some(endpoint.to_string());
                }
                if let Some(key_ref) = kp.default_key_ref {
                    if config.provider.api_key_ref.is_none() {
                        config.provider.api_key_ref = Some(key_ref.to_string());
                    }
                }
                state.current_config = Some(config.clone());

                let tip = if kp.default_key_ref.is_some() {
                    format!(
                        ". Store your API key with `/vault set {} <your-key>`.",
                        config.provider.api_key_ref.as_deref().unwrap_or("")
                    )
                } else {
                    String::new()
                };
                let msg = format!("Switched to {} / {}{tip}", backend, model);
                state.push_transcript("System", msg.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Loading,
                    text: msg,
                });
                trigger_refresh(state);
                return vec![Command::SaveConfig {
                    config: Box::new(config),
                }];
            }
        }
    }

    Vec::new()
}

fn handle_popup_action(state: &mut AppState, action: Action) -> Vec<Command> {
    match action {
        Action::MoveUp | Action::ComposerMoveUp => {
            let popup = &mut state.provider_popup;
            match popup.column {
                PopupColumn::Provider => {
                    popup.selected_provider = popup.selected_provider.saturating_sub(1);
                    return maybe_fetch_models(state);
                }
                PopupColumn::Model => {
                    popup.selected_model = popup.selected_model.saturating_sub(1);
                }
            }
        }
        Action::MoveDown | Action::ComposerMoveDown => {
            let popup = &mut state.provider_popup;
            let providers = commands::KNOWN_PROVIDERS;
            match popup.column {
                PopupColumn::Provider => {
                    let max = providers.len().saturating_sub(1);
                    popup.selected_provider = (popup.selected_provider + 1).min(max);
                    return maybe_fetch_models(state);
                }
                PopupColumn::Model => {
                    let backend = providers
                        .get(popup.selected_provider)
                        .map(|kp| kp.backend)
                        .unwrap_or("");
                    let _ = popup;
                    let max = get_provider_models(state, backend).len().saturating_sub(1);
                    state.provider_popup.selected_model =
                        (state.provider_popup.selected_model + 1).min(max);
                }
            }
        }
        Action::FocusPrev | Action::ComposerMoveLeft => {
            state.provider_popup.column = PopupColumn::Provider;
        }
        Action::FocusNext | Action::ComposerMoveRight => {
            state.provider_popup.column = PopupColumn::Model;
            state.provider_popup.selected_model = 0;
        }
        Action::SubmitPrompt | Action::RunSelectedWorkflow => {
            let providers = commands::KNOWN_PROVIDERS;
            let selected_provider = state.provider_popup.selected_provider;
            let selected_model = state.provider_popup.selected_model;
            state.provider_popup.visible = false;

            if let Some(kp) = providers.get(selected_provider) {
                let models = get_provider_models(state, kp.backend);
                let model = models
                    .get(selected_model)
                    .cloned()
                    .unwrap_or_else(|| kp.backend.to_string());
                let backend = kp.backend.to_string();
                let endpoint = kp.default_endpoint.map(|e| e.to_string());
                let key_ref = kp.default_key_ref.map(|k| k.to_string());

                let mut config = ensure_config(state);
                config.provider.backend = backend.clone();
                config.provider.model = model.clone();
                if let Some(ref ep) = endpoint {
                    config.provider.endpoint = Some(ep.clone());
                }
                if let Some(ref kr) = key_ref {
                    if config.provider.api_key_ref.is_none() {
                        config.provider.api_key_ref = Some(kr.clone());
                    }
                }
                state.current_config = Some(config.clone());

                let tip = if key_ref.is_some() {
                    format!(
                        ". Store your API key with `/vault set {} <your-key>`.",
                        config.provider.api_key_ref.as_deref().unwrap_or("")
                    )
                } else {
                    String::new()
                };
                let msg = format!("Switched to {} / {}{tip}", backend, model);
                state.push_transcript("System", msg.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Loading,
                    text: msg,
                });
                trigger_refresh(state);
                return vec![Command::SaveConfig {
                    config: Box::new(config),
                }];
            }
        }
        Action::Escape => {
            state.provider_popup.visible = false;
        }
        _ => {}
    }
    Vec::new()
}

fn handle_slash_command(state: &mut AppState, cmd: ConfigCommand) -> Vec<Command> {
    match cmd {
        ConfigCommand::Help => {
            state.push_transcript("ArgOS", commands::help_text().to_string(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: "Available commands shown in transcript.".into(),
            });
            Vec::new()
        }
        ConfigCommand::ClearTranscript => {
            state.transcript.clear();
            state.transcript_scroll = 0;
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: "Transcript cleared.".into(),
            });
            Vec::new()
        }
        ConfigCommand::Quit => {
            state.should_quit = true;
            state.push_transcript("System", "Quitting ArgOS TUI…".to_string(), None);
            Vec::new()
        }
        ConfigCommand::Refresh => {
            if state.is_loading_snapshot {
                return Vec::new();
            }
            state.is_loading_snapshot = true;
            state.provider_status.level = StatusLevel::Loading;
            state.provider_status.detail = "Refreshing provider status…".into();
            state.n8n_status.level = StatusLevel::Loading;
            state.n8n_status.detail = "Refreshing n8n status…".into();
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Refreshing provider and n8n status…".into(),
            });
            state.push_activity(
                StatusLevel::Loading,
                "Refresh started",
                "Loading provider connectivity and workflow inventory.",
            );
            vec![Command::LoadSnapshot]
        }
        ConfigCommand::ShowConfig => {
            let text = match &state.current_config {
                Some(config) => format_config(config),
                None => {
                    "No configuration loaded yet. Use /refresh or create .argos/config.toml.".into()
                }
            };
            state.push_transcript("ArgOS", text, None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: "Current configuration shown in transcript.".into(),
            });
            Vec::new()
        }
        ConfigCommand::SetProvider { backend, model } => {
            let mut config = ensure_config(state);
            config.provider.backend = backend.clone();
            config.provider.model = model.clone();

            if let Some(kp) = commands::known_provider(&backend) {
                if let Some(endpoint) = kp.default_endpoint {
                    config.provider.endpoint = Some(endpoint.to_string());
                }
                if let Some(key_ref) = kp.default_key_ref {
                    if config.provider.api_key_ref.is_none() {
                        config.provider.api_key_ref = Some(key_ref.to_string());
                    }
                }
            }

            let tip = if let Some(kp) = commands::known_provider(&backend) {
                if kp.default_key_ref.is_some() {
                    format!(
                        ". Store your API key with `/vault set {} <your-key>` then `/refresh`.",
                        config.provider.api_key_ref.as_deref().unwrap_or("")
                    )
                } else {
                    String::new()
                }
            } else {
                ". Set /endpoint and /key-ref if needed.".into()
            };

            state.current_config = Some(config.clone());
            let msg = format!("Provider set to {} / {}{tip}", backend, model);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: msg,
            });
            trigger_refresh(state);
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::ListProviders => {
            state.push_transcript("ArgOS", commands::providers_list_text(), None);
            state.push_transcript(
                "System",
                "Use /provider <backend> <model> to switch. It auto-configures endpoint and key ref.".to_string(),
                None,
            );
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: "Known providers shown in transcript.".into(),
            });
            Vec::new()
        }
        ConfigCommand::ChangeDir { path } => {
            let new_cwd = if path.is_empty() {
                state.cwd.clone()
            } else {
                let candidate = if std::path::Path::new(&path).is_absolute() {
                    std::path::PathBuf::from(&path)
                } else {
                    state.cwd.join(&path)
                };
                match candidate.canonicalize() {
                    Ok(p) => p,
                    Err(err) => {
                        let msg = format!("Cannot change to `{path}`: {err}");
                        state.flash = Some(FlashMessage {
                            level: StatusLevel::Error,
                            text: msg.clone(),
                        });
                        state.push_transcript("System", msg, None);
                        return Vec::new();
                    }
                }
            };
            let msg = format!("Changed directory to {}", new_cwd.display());
            state.cwd = new_cwd;
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: msg.clone(),
            });
            state.push_transcript("System", msg, None);
            Vec::new()
        }
        ConfigCommand::ClearSessions => {
            let sessions_dir = state.cwd.join(".argos").join("sessions");
            match std::fs::remove_dir_all(&sessions_dir) {
                Ok(()) => {
                    let msg = format!("Cleared sessions from {}", sessions_dir.display());
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Success,
                        text: msg.clone(),
                    });
                    state.push_transcript("System", msg, None);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let msg = "No saved sessions to clear.".to_string();
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Missing,
                        text: msg.clone(),
                    });
                    state.push_transcript("System", msg, None);
                }
                Err(err) => {
                    let msg = format!("Failed to clear sessions: {err}");
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: msg.clone(),
                    });
                    state.push_transcript("System", msg, None);
                }
            }
            Vec::new()
        }
        ConfigCommand::SetModel { model } => {
            let mut config = ensure_config(state);
            config.provider.model = model.clone();
            state.current_config = Some(config.clone());
            let msg = format!("Model set to {}. Refreshing status…", model);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: msg,
            });
            trigger_refresh(state);
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::SetEndpoint { url } => {
            let mut config = ensure_config(state);
            config.provider.endpoint = if url.is_empty() {
                None
            } else {
                Some(url.clone())
            };
            state.current_config = Some(config.clone());
            let msg = format!("Provider endpoint set to {}. Refreshing status…", url);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: msg,
            });
            trigger_refresh(state);
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::SetKeyRef { key_ref } => {
            let mut config = ensure_config(state);
            config.provider.api_key_ref = if key_ref.is_empty() {
                None
            } else {
                Some(key_ref.clone())
            };
            state.current_config = Some(config.clone());
            let msg = format!("API key reference set to `{}`.", key_ref);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: msg,
            });
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::SetN8n { url } => {
            let parsed = match Url::parse(&url) {
                Ok(u) => u,
                Err(err) => {
                    let msg = format!("Invalid n8n URL `{}`: {err}", url);
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: msg.clone(),
                    });
                    state.push_transcript("System", msg, None);
                    return Vec::new();
                }
            };
            let mut config = ensure_config(state);
            let n8n = config.n8n.get_or_insert_with(|| N8nConnection {
                endpoint: parsed.clone(),
                mode: ConnMode::Rest,
                api_key_ref: None,
            });
            n8n.endpoint = parsed;
            state.current_config = Some(config.clone());
            let msg = format!("n8n endpoint set to {}. Refreshing status…", url);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: msg,
            });
            trigger_refresh(state);
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::SetN8nMode { mode } => {
            let conn_mode = match mode.as_str() {
                "rest" => ConnMode::Rest,
                "mcp" => ConnMode::Mcp,
                other => {
                    let msg = format!("Unknown n8n mode `{other}`. Use `rest` or `mcp`.");
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: msg.clone(),
                    });
                    state.push_transcript("System", msg, None);
                    return Vec::new();
                }
            };
            let mut config = ensure_config(state);
            let n8n = config.n8n.get_or_insert_with(|| N8nConnection {
                endpoint: Url::parse("http://localhost:5678").unwrap(),
                mode: ConnMode::Rest,
                api_key_ref: None,
            });
            n8n.mode = conn_mode;
            state.current_config = Some(config.clone());
            let msg = format!("n8n mode set to {}.", mode);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: msg,
            });
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::SetN8nKeyRef { key_ref } => {
            let mut config = ensure_config(state);
            let n8n = config.n8n.get_or_insert_with(|| N8nConnection {
                endpoint: Url::parse("http://localhost:5678").unwrap(),
                mode: ConnMode::Rest,
                api_key_ref: Some(key_ref.clone()),
            });
            n8n.api_key_ref = if key_ref.is_empty() {
                None
            } else {
                Some(key_ref.clone())
            };
            state.current_config = Some(config.clone());
            let msg = format!("n8n API key reference set to `{}`.", key_ref);
            state.push_transcript("System", msg.clone(), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: msg,
            });
            vec![Command::SaveConfig {
                config: Box::new(config),
            }]
        }
        ConfigCommand::StoreSecret { key_ref, secret } => {
            state.push_transcript(
                "System",
                format!("Storing secret `{key_ref}` in OS keyring…"),
                None,
            );
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: format!("Storing secret `{key_ref}`…"),
            });
            vec![Command::StoreSecret { key_ref, secret }]
        }
        ConfigCommand::DeleteSecret { key_ref } => {
            state.push_transcript(
                "System",
                format!("Removing secret `{key_ref}` from OS keyring…"),
                None,
            );
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: format!("Removing secret `{key_ref}`…"),
            });
            vec![Command::DeleteSecret { key_ref }]
        }
    }
}

fn ensure_config(state: &mut AppState) -> Config {
    state.current_config.clone().unwrap_or_else(|| Config {
        n8n: None,
        provider: ProviderConfig {
            backend: String::new(),
            model: String::new(),
            endpoint: None,
            api_key_ref: None,
        },
        embedder: Default::default(),
        storage: Default::default(),
        reuse_threshold: 0.82,
    })
}

fn maybe_fetch_models(_state: &AppState) -> Vec<Command> {
    Vec::new()
}

fn get_provider_models(state: &AppState, backend: &str) -> Vec<String> {
    if let Some(dynamic) = state.dynamic_models.get(backend) {
        if !dynamic.is_empty() {
            return dynamic.iter().map(|m| m.id.clone()).collect();
        }
    }
    commands::known_provider(backend)
        .map(|kp| kp.models.iter().map(|m| m.to_string()).collect())
        .unwrap_or_default()
}

fn trigger_refresh(state: &mut AppState) {
    state.is_loading_snapshot = true;
    state.provider_status.level = StatusLevel::Loading;
    state.provider_status.detail = "Refreshing provider status…".into();
    state.n8n_status.level = StatusLevel::Loading;
    state.n8n_status.detail = "Refreshing n8n status…".into();
}

fn real_cost(backend: &str, _model: &str, prompt_tokens: u64, completion_tokens: u64) -> f64 {
    let pricing = commands::known_provider(backend).and_then(|kp| kp.pricing);

    let Some(p) = pricing else {
        return 0.0;
    };

    (prompt_tokens as f64 / 1_000_000.0) * p.input_per_mtok
        + (completion_tokens as f64 / 1_000_000.0) * p.output_per_mtok
}

fn dynamic_real_cost(
    state: &AppState,
    backend: &str,
    model_id: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> f64 {
    if let Some(models) = state.dynamic_models.get(backend) {
        if let Some(info) = models
            .iter()
            .find(|m| m.id == model_id || m.id.ends_with(&format!("/{model_id}")))
        {
            if let Some(p) = info.pricing {
                return (prompt_tokens as f64 / 1_000_000.0) * p.input_per_mtok
                    + (completion_tokens as f64 / 1_000_000.0) * p.output_per_mtok;
            }
        }
    }
    real_cost(backend, model_id, prompt_tokens, completion_tokens)
}

fn format_config(config: &Config) -> String {
    let mut lines = vec!["Current configuration:".into(), String::new()];
    lines.push(format!(
        "  Provider: {} / {}",
        config.provider.backend, config.provider.model
    ));
    if let Some(ref endpoint) = config.provider.endpoint {
        lines.push(format!("  Endpoint: {endpoint}"));
    }
    match &config.provider.api_key_ref {
        Some(key_ref) => lines.push(format!("  API key ref: `{key_ref}`")),
        None => lines.push("  API key ref: (none)".into()),
    }
    lines.push(String::new());
    match &config.n8n {
        Some(n8n) => {
            lines.push(format!("  n8n endpoint: {}", n8n.endpoint));
            lines.push(format!("  n8n mode: {:?}", n8n.mode));
            match &n8n.api_key_ref {
                Some(key_ref) => lines.push(format!("  n8n API key ref: `{key_ref}`")),
                None => lines.push("  n8n API key ref: (none)".into()),
            }
        }
        None => {
            lines.push("  n8n: (not configured)".into());
        }
    }
    lines.push(String::new());
    lines.push(format!("  Reuse threshold: {}", config.reuse_threshold));
    lines.push(format!("  Storage profile: {:?}", config.storage));
    lines.join("\n")
}

fn copy_to_clipboard(text: &str) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.as_mut().unwrap().write_all(text.as_bytes())
            })
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.as_mut().unwrap().write_all(text.as_bytes())
            })
    } else {
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.as_mut().unwrap().write_all(text.as_bytes())
            })
    };
    if let Err(e) = result {
        eprintln!("clipboard copy failed: {e}");
    }
}

pub fn handle_async(state: &mut AppState, event: AsyncEvent) -> Vec<Command> {
    match event {
        AsyncEvent::SnapshotLoaded(result) => {
            state.is_loading_snapshot = false;
            match *result {
                Ok(snapshot) => {
                    state.provider_status = ResourceStatus {
                        level: snapshot.provider.level,
                        title: snapshot.provider.title,
                        detail: detail_with_metadata(
                            snapshot.provider.detail,
                            snapshot.provider.backend,
                            snapshot.provider.model,
                            Some(snapshot.provider.vault_name.clone()),
                        ),
                    };
                    state.n8n_status = ResourceStatus {
                        level: snapshot.n8n.level,
                        title: snapshot.n8n.title,
                        detail: snapshot.n8n.detail,
                    };
                    state.vault_name = snapshot.provider.vault_name;
                    state.workflows = snapshot.n8n.workflows;
                    state.current_config = snapshot.config;
                    state.clamp_selections();
                    state.push_activity(
                        StatusLevel::Success,
                        "Refresh completed",
                        format!("Loaded {} workflows.", state.workflows.len()),
                    );
                }
                Err(err) => {
                    state.provider_status.level = StatusLevel::Error;
                    state.provider_status.detail = err.clone();
                    state.n8n_status.level = StatusLevel::Error;
                    state.n8n_status.detail = err.clone();
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: err.clone(),
                    });
                    state.push_activity(StatusLevel::Error, "Refresh failed", err);
                }
            }
        }
        AsyncEvent::InputError(message) => {
            state.should_quit = true;
            state.push_activity(StatusLevel::Error, "Input loop failed", message.clone());
            state.push_transcript("System", format!("Input loop failed: {message}"), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Error,
                text: "Input loop failed. Exiting ArgOS TUI.".into(),
            });
        }
        AsyncEvent::PromptCompleted { prompt: _, result } => {
            state.is_submitting_prompt = false;
            state.esc_last_press = None;
            match result {
                Ok(response) => {
                    let total = response.prompt_tokens + response.completion_tokens;
                    state.session_tokens += total;
                    if let Some(ref config) = state.current_config {
                        state.session_cost += dynamic_real_cost(
                            state,
                            &config.provider.backend,
                            &config.provider.model,
                            response.prompt_tokens,
                            response.completion_tokens,
                        );
                    }
                    let tool_count = response.output.tool_invocations.len();
                    state.push_transcript(
                        "ArgOS",
                        response.output.text.clone(),
                        Some(format!(
                            "{} / {} • {:?} • {} tool invocation(s)",
                            response.backend,
                            response.model,
                            response.output.final_state,
                            tool_count
                        )),
                    );
                    state.push_activity(
                        StatusLevel::Success,
                        "Prompt completed",
                        format!(
                            "{} / {} returned {:?}.",
                            response.backend, response.model, response.output.final_state
                        ),
                    );
                    for invocation in response.output.tool_invocations {
                        state.push_activity(
                            tool_invocation_level(&invocation),
                            format!("Tool: {}", invocation.tool.name),
                            summarize_tool_invocation(&invocation),
                        );
                    }
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Success,
                        text: "Agent response received.".into(),
                    });
                }
                Err(err) => {
                    state.push_transcript(
                        "ArgOS",
                        format!("Request failed: {err}"),
                        Some("GenericAgent returned an error.".into()),
                    );
                    state.push_activity(StatusLevel::Error, "Prompt failed", err.clone());
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: err,
                    });
                }
            }
        }
        AsyncEvent::WorkflowCompleted {
            workflow_id,
            workflow_name,
            result,
        } => {
            state.is_running_workflow = false;
            match result {
                Ok(run) => {
                    let presentation = workflow_status_presentation(&workflow_name, &run.run);
                    state.push_activity(
                        presentation.level,
                        presentation.activity_title,
                        format!(
                            "{} run {} status={}.",
                            run.mode_label,
                            run.run.id,
                            workflow_status_label(&run.run.status)
                        ),
                    );
                    state.push_transcript(
                        "System",
                        presentation.transcript_body,
                        Some(format!("workflow_id={} run_id={}", workflow_id, run.run.id)),
                    );
                    state.flash = Some(FlashMessage {
                        level: presentation.level,
                        text: presentation.flash_text,
                    });
                }
                Err(err) => {
                    state.push_activity(
                        StatusLevel::Error,
                        format!("Workflow {} failed", workflow_name),
                        err.clone(),
                    );
                    state.push_transcript(
                        "System",
                        format!("Workflow `{workflow_name}` failed: {err}"),
                        Some(format!("workflow_id={workflow_id}")),
                    );
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: err,
                    });
                }
            }
        }
        AsyncEvent::ConfigSaved { result } => match result {
            Ok(()) => {
                state.push_activity(
                    StatusLevel::Success,
                    "Config saved",
                    "Configuration written to .argos/config.toml.",
                );
            }
            Err(err) => {
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: format!("Failed to save config: {err}"),
                });
                state.push_transcript("System", format!("Config save failed: {err}"), None);
            }
        },
        AsyncEvent::SecretStored { key_ref, result } => match result {
            Ok(()) => {
                let msg = format!("Secret `{key_ref}` stored successfully in OS keyring.");
                state.push_transcript("System", msg.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Success,
                    text: msg,
                });
            }
            Err(err) => {
                let msg = format!("Failed to store secret `{key_ref}`: {err}");
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: msg.clone(),
                });
                state.push_transcript("System", msg, None);
            }
        },
        AsyncEvent::SecretDeleted { key_ref, result } => match result {
            Ok(()) => {
                let msg = format!("Secret `{key_ref}` removed from OS keyring.");
                state.push_transcript("System", msg.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Success,
                    text: msg,
                });
            }
            Err(err) => {
                let msg = format!("Failed to remove secret `{key_ref}`: {err}");
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: msg.clone(),
                });
                state.push_transcript("System", msg, None);
            }
        },
        AsyncEvent::ModelsFetched { backend, models } => match models {
            Ok(list) => {
                let is_openrouter = backend.eq_ignore_ascii_case("openrouter");
                if is_openrouter {
                    let mut grouped: std::collections::HashMap<
                        String,
                        Vec<crate::state::ModelInfo>,
                    > = std::collections::HashMap::new();
                    for model in &list {
                        if let Some((provider, _)) = model.id.split_once('/') {
                            grouped
                                .entry(provider.to_string())
                                .or_default()
                                .push(model.clone());
                        }
                    }
                    grouped.insert(backend.clone(), list.clone());
                    for (provider, models) in grouped {
                        state.dynamic_models.insert(provider, models);
                    }
                    state.push_activity(
                        StatusLevel::Success,
                        "Models: OpenRouter",
                        format!(
                            "Fetched {} models across {} providers.",
                            list.len(),
                            state.dynamic_models.len()
                        ),
                    );
                } else {
                    state.dynamic_models.insert(backend.clone(), list);
                    state.push_activity(
                        StatusLevel::Success,
                        format!("Models: {backend}"),
                        format!(
                            "Fetched {} models.",
                            state.dynamic_models.get(&backend).map_or(0, |v| v.len())
                        ),
                    );
                }
            }
            Err(err) => {
                state.push_activity(
                    StatusLevel::Error,
                    format!("Models: {backend}"),
                    format!("Fetch failed: {err}"),
                );
            }
        },
    }

    Vec::new()
}

fn tool_invocation_level(invocation: &ToolInvocation) -> StatusLevel {
    match &invocation.result {
        ToolResult::Ok(_) => StatusLevel::Success,
        ToolResult::Err(_) => StatusLevel::Error,
    }
}

fn summarize_tool_invocation(invocation: &ToolInvocation) -> String {
    let mut parts = vec![format!("status={}", tool_result_label(&invocation.result))];
    if let Some(duration_ms) = tool_duration_ms(&invocation.result) {
        parts.push(format!("duration_ms={duration_ms}"));
    }
    if let ToolResult::Err(error) = &invocation.result {
        parts.push(format!("error_len={}", error.chars().count()));
    }
    parts.join(" ")
}

fn tool_result_label(result: &ToolResult) -> &'static str {
    match result {
        ToolResult::Ok(_) => "ok",
        ToolResult::Err(_) => "error",
    }
}

fn tool_duration_ms(result: &ToolResult) -> Option<u64> {
    let ToolResult::Ok(value) = result else {
        return None;
    };
    let json = serde_json::from_str::<Value>(value).ok()?;
    ["duration_ms", "elapsed_ms", "latency_ms"]
        .into_iter()
        .find_map(|field| json.get(field).and_then(Value::as_u64))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkflowStatusPresentation {
    level: StatusLevel,
    activity_title: String,
    transcript_body: String,
    flash_text: String,
}

fn workflow_status_presentation(
    workflow_name: &str,
    run: &N8nRunRef,
) -> WorkflowStatusPresentation {
    match &run.status {
        N8nRunStatus::Running => WorkflowStatusPresentation {
            level: StatusLevel::Loading,
            activity_title: format!("Workflow {} running", workflow_name),
            transcript_body: format!("Workflow `{workflow_name}` is still running."),
            flash_text: format!("Workflow {} is still running.", workflow_name),
        },
        N8nRunStatus::Success => WorkflowStatusPresentation {
            level: StatusLevel::Success,
            activity_title: format!("Workflow {} completed", workflow_name),
            transcript_body: format!("Workflow `{workflow_name}` completed successfully."),
            flash_text: format!("Workflow {} completed successfully.", workflow_name),
        },
        N8nRunStatus::Failed => WorkflowStatusPresentation {
            level: StatusLevel::Error,
            activity_title: format!("Workflow {} failed", workflow_name),
            transcript_body: format!("Workflow `{workflow_name}` failed."),
            flash_text: format!("Workflow {} failed.", workflow_name),
        },
        N8nRunStatus::Cancelled => WorkflowStatusPresentation {
            level: StatusLevel::Error,
            activity_title: format!("Workflow {} cancelled", workflow_name),
            transcript_body: format!("Workflow `{workflow_name}` was cancelled."),
            flash_text: format!("Workflow {} was cancelled.", workflow_name),
        },
    }
}

fn workflow_status_label(status: &N8nRunStatus) -> &'static str {
    match status {
        N8nRunStatus::Running => "running",
        N8nRunStatus::Success => "success",
        N8nRunStatus::Failed => "failed",
        N8nRunStatus::Cancelled => "cancelled",
    }
}

fn detail_with_metadata(
    detail: String,
    backend: Option<String>,
    model: Option<String>,
    vault_name: Option<String>,
) -> String {
    let mut parts = vec![detail];
    if let (Some(backend), Some(model)) = (backend, model) {
        parts.push(format!("backend={backend} model={model}"));
    }
    if let Some(vault_name) = vault_name {
        parts.push(format!("vault={vault_name}"));
    }
    parts.join(" • ")
}

#[cfg(test)]
mod tests {
    use super::{handle_action, handle_async, Command};
    use crate::action::Action;
    use crate::event::AsyncEvent;
    use crate::services::{
        N8nSnapshot, PromptResult, ProviderSnapshot, Snapshot, WorkflowRunResult,
    };
    use crate::state::{AppState, FocusPane, StatusLevel, WorkflowItem};
    use argos_agent::AgentOutput;
    use argos_core::{AgentState, N8nRunRef, N8nRunStatus};

    #[test]
    fn submit_prompt_generates_command_and_clears_composer() {
        let mut state = AppState::new();
        state.composer.insert_char('h');
        state.composer.insert_char('i');

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert_eq!(
            commands,
            vec![Command::SubmitPrompt {
                prompt: "hi".into()
            }]
        );
        assert!(state.is_submitting_prompt);
        assert!(state.composer.is_empty());
        assert_eq!(state.focus, FocusPane::Transcript);
    }

    #[test]
    fn empty_submit_does_not_emit_command() {
        let mut state = AppState::new();
        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert!(commands.is_empty());
        assert_eq!(state.flash.unwrap().level, StatusLevel::Error);
    }

    #[test]
    fn async_snapshot_success_updates_statuses() {
        let mut state = AppState::new();
        state.is_loading_snapshot = true;

        handle_async(
            &mut state,
            AsyncEvent::SnapshotLoaded(Box::new(Ok(Snapshot {
                provider: ProviderSnapshot {
                    level: StatusLevel::Success,
                    title: "Provider".into(),
                    detail: "Connected".into(),
                    backend: Some("openai".into()),
                    model: Some("gpt-4o".into()),
                    vault_name: "argos-ui".into(),
                },
                n8n: N8nSnapshot {
                    level: StatusLevel::Success,
                    title: "n8n".into(),
                    detail: "2 workflows available.".into(),
                    workflows: vec![
                        WorkflowItem {
                            id: "a".into(),
                            name: "Alpha".into(),
                        },
                        WorkflowItem {
                            id: "b".into(),
                            name: "Beta".into(),
                        },
                    ],
                },
                config: None,
            }))),
        );

        assert!(!state.is_loading_snapshot);
        assert_eq!(state.provider_status.level, StatusLevel::Success);
        assert_eq!(state.workflows.len(), 2);
    }

    #[test]
    fn async_prompt_failure_adds_error_transcript() {
        let mut state = AppState::new();
        state.is_submitting_prompt = true;

        handle_async(
            &mut state,
            AsyncEvent::PromptCompleted {
                prompt: "hello".into(),
                result: Err("boom".into()),
            },
        );

        assert!(!state.is_submitting_prompt);
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("Request failed: boom"));
    }

    #[test]
    fn async_workflow_success_adds_log_and_transcript() {
        let mut state = AppState::new();
        state.is_running_workflow = true;

        handle_async(
            &mut state,
            AsyncEvent::WorkflowCompleted {
                workflow_id: "wf-1".into(),
                workflow_name: "Daily".into(),
                result: Ok(WorkflowRunResult {
                    mode_label: "REST".into(),
                    run: N8nRunRef {
                        id: "run-1".into(),
                        workflow_id: "wf-1".into(),
                        status: N8nRunStatus::Success,
                    },
                }),
            },
        );

        assert!(!state.is_running_workflow);
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("completed successfully"));
    }

    #[test]
    fn async_workflow_running_reflects_live_status() {
        let mut state = AppState::new();
        state.is_running_workflow = true;

        handle_async(
            &mut state,
            AsyncEvent::WorkflowCompleted {
                workflow_id: "wf-1".into(),
                workflow_name: "Daily".into(),
                result: Ok(WorkflowRunResult {
                    mode_label: "REST".into(),
                    run: N8nRunRef {
                        id: "run-1".into(),
                        workflow_id: "wf-1".into(),
                        status: N8nRunStatus::Running,
                    },
                }),
            },
        );

        assert_eq!(state.flash.as_ref().unwrap().level, StatusLevel::Loading);
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("still running"));
        assert!(!state.transcript.last().unwrap().body.contains("finished"));
    }

    #[test]
    fn prompt_success_redacts_tool_payloads() {
        let mut state = AppState::new();
        state.is_submitting_prompt = true;

        handle_async(
            &mut state,
            AsyncEvent::PromptCompleted {
                prompt: "hello".into(),
                result: Ok(PromptResult {
                    backend: "openai".into(),
                    model: "gpt-4o".into(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    output: AgentOutput {
                        text: "done".into(),
                        tool_invocations: vec![argos_core::ToolInvocation {
                            tool: argos_core::Tool {
                                name: "n8n.run".into(),
                                description: "Run workflow".into(),
                            },
                            args: "{\"id\":\"1\",\"api_key\":\"super-secret\"}".into(),
                            result: argos_core::ToolResult::Ok(
                                "{\"ok\":true,\"duration_ms\":42,\"token\":\"top-secret\"}".into(),
                            ),
                        }],
                        final_state: AgentState::Done,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                    },
                }),
            },
        );

        let entry = state
            .activity
            .iter()
            .find(|entry| entry.title.contains("Tool: n8n.run"))
            .unwrap();
        assert!(entry.detail.contains("status=ok"));
        assert!(entry.detail.contains("duration_ms=42"));
        assert!(!entry.detail.contains("super-secret"));
        assert!(!entry.detail.contains("top-secret"));
        assert!(!entry.detail.contains("args="));
        assert!(!entry.detail.contains("result="));
    }

    #[test]
    fn input_error_marks_state_for_exit() {
        let mut state = AppState::new();

        handle_async(&mut state, AsyncEvent::InputError("poll failed".into()));

        assert!(state.should_quit);
        assert_eq!(state.flash.as_ref().unwrap().level, StatusLevel::Error);
        assert!(state
            .activity
            .last()
            .unwrap()
            .detail
            .contains("poll failed"));
    }
}
