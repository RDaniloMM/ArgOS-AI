use crate::action::Action;
use crate::commands::{self, ConfigCommand};
use crate::event::AsyncEvent;
use crate::state::{AppState, FlashMessage, FocusPane, PopupColumn, ResourceStatus, StatusLevel};
use argos_core::{
    Config, ConnMode, N8nConnection, N8nRunRef, N8nRunStatus, ProviderAuthMethod, ProviderConfig,
    ToolInvocation, ToolResult,
};
use serde_json::Value;
use url::Url;

const STARTUP_MODELS_BACKEND: &str = "openrouter";
const STARTUP_MODELS_ENDPOINT: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENAI_OAUTH_TOKEN_REF: &str = "provider/openai/oauth";
const DEFAULT_CODEX_TOKEN_REF: &str = "provider/codex/oauth";

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
    StartOpenAiLogin {
        token_ref: String,
    },
    CompleteOpenAiLogin {
        login: crate::services::OpenAiLoginStart,
    },
    StartCodexLogin,
    FetchModels {
        backend: String,
        endpoint: String,
        api_key_ref: Option<String>,
        auth_method: ProviderAuthMethod,
        oauth_token_ref: Option<String>,
    },
}

pub fn startup_commands(state: &mut AppState) -> Vec<Command> {
    let mut commands = handle_action(state, Action::Refresh);
    commands.extend(queue_models_fetch(
        state,
        STARTUP_MODELS_BACKEND,
        STARTUP_MODELS_ENDPOINT,
        None,
        ProviderAuthMethod::ApiKey,
        None,
        "Fetching provider models",
        "Loading OpenRouter model catalog for provider/model pickers.",
    ));
    commands
}

pub fn handle_action(state: &mut AppState, action: Action) -> Vec<Command> {
    // Auto-dismiss completed flash messages on any action
    if let Some(flash) = &state.flash {
        if matches!(flash.level, StatusLevel::Success | StatusLevel::Error) {
            state.flash = None;
        }
    }

    if state.provider_popup.visible {
        return handle_popup_action(state, action);
    }
    if state.command_palette.visible {
        return handle_command_palette_action(state, action);
    }

    match action {
        Action::FocusNext => state.focus = next_visible_focus(state, 1),
        Action::FocusPrev => state.focus = next_visible_focus(state, -1),
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
            state.n8n_status.detail = "Refreshing optional workflow status…".into();
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Refreshing provider and optional workflow status…".into(),
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
                state.flash = None;
                state.push_transcript(
                    "System",
                    "No workflow selected. Configure n8n and pick a workflow before running one."
                        .to_string(),
                    None,
                );
                state.push_activity(
                    StatusLevel::Missing,
                    "No workflow selected",
                    "Workflow automation is optional until you configure n8n.",
                );
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
            if !state.activity_visible && state.focus == FocusPane::Activity {
                state.focus = FocusPane::Transcript;
            }
        }
        Action::ToggleSidebar => {
            state.sidebar_visible = !state.sidebar_visible;
            if !state.sidebar_visible && state.focus == FocusPane::Workflows {
                state.focus = FocusPane::Transcript;
            }
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
            if state.configured_providers().is_empty() {
                state.push_activity(
                    StatusLevel::Missing,
                    "No configured providers",
                    "Use Enter on Add provider or /provider-add to configure one.",
                );
            } else {
                return maybe_fetch_models(state);
            }
        }
        Action::ShowCommandPalette => {
            state.command_palette.visible = true;
            state.command_palette.selected = 0;
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
            if !state.provider_popup.visible {
                return Vec::new();
            }
            match state.provider_popup.column {
                PopupColumn::Provider => {
                    let max = provider_popup_item_count(state).saturating_sub(1);
                    state.provider_popup.selected_provider =
                        (state.provider_popup.selected_provider + 1).min(max);
                }
                PopupColumn::Model => {
                    if let Some(provider) = selected_popup_provider(state) {
                        let max = models_for_configured_provider(state, &provider)
                            .len()
                            .saturating_sub(1);
                        state.provider_popup.selected_model =
                            (state.provider_popup.selected_model + 1).min(max);
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
            if state.provider_popup.visible && !state.provider_popup_is_add_selected() {
                state.provider_popup.column = PopupColumn::Model;
                state.provider_popup.selected_model = 0;
            }
        }
        Action::PopupSelect => {
            let popup = &state.provider_popup;
            if !popup.visible {
                return Vec::new();
            }
            if state.provider_popup_is_add_selected() {
                return insert_add_provider_template(state);
            }
            if let Some(provider) = selected_popup_provider(state) {
                return select_configured_provider(state, provider);
            }
        }
        Action::PopupDelete => {}
        Action::CodexLogin => {
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Opening browser for Codex login…".into(),
            });
            return vec![Command::StartCodexLogin];
        }
    }

    Vec::new()
}

fn handle_command_palette_action(state: &mut AppState, action: Action) -> Vec<Command> {
    let command_count = commands::command_definitions().len();
    match action {
        Action::MoveUp | Action::ComposerMoveUp => {
            state.command_palette.selected = state.command_palette.selected.saturating_sub(1);
        }
        Action::MoveDown | Action::ComposerMoveDown => {
            let max = command_count.saturating_sub(1);
            state.command_palette.selected = (state.command_palette.selected + 1).min(max);
        }
        Action::SubmitPrompt | Action::RunSelectedWorkflow => {
            let Some(command) = commands::command_definitions().get(state.command_palette.selected)
            else {
                state.command_palette.visible = false;
                return Vec::new();
            };

            state.composer.clear();
            for ch in command.insert_text.chars() {
                state.composer.insert_char(ch);
            }
            state.recompute_suggestions();
            state.focus = FocusPane::Composer;
            state.command_palette.visible = false;
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: format!("Inserted {}.", command.signature),
            });
        }
        Action::Escape | Action::ShowCommandPalette => {
            state.command_palette.visible = false;
        }
        _ => {}
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
        Action::MoveDown | Action::ComposerMoveDown => match state.provider_popup.column {
            PopupColumn::Provider => {
                let max = provider_popup_item_count(state).saturating_sub(1);
                state.provider_popup.selected_provider =
                    (state.provider_popup.selected_provider + 1).min(max);
                state.provider_popup.selected_model = 0;
                return maybe_fetch_models(state);
            }
            PopupColumn::Model => {
                let max = selected_popup_provider(state)
                    .map(|provider| models_for_configured_provider(state, &provider).len())
                    .unwrap_or(1)
                    .saturating_sub(1);
                state.provider_popup.selected_model =
                    (state.provider_popup.selected_model + 1).min(max);
            }
        },
        Action::FocusPrev | Action::ComposerMoveLeft => {
            state.provider_popup.column = PopupColumn::Provider;
        }
        Action::FocusNext | Action::ComposerMoveRight => {
            if !state.provider_popup_is_add_selected() {
                state.provider_popup.column = PopupColumn::Model;
                state.provider_popup.selected_model = 0;
            }
        }
        Action::SubmitPrompt | Action::RunSelectedWorkflow => {
            if state.provider_popup_is_add_selected() {
                return insert_add_provider_template(state);
            }
            if let Some(provider) = selected_popup_provider(state) {
                return select_configured_provider(state, provider);
            }
        }
        Action::Escape => {
            state.provider_popup.visible = false;
        }
        Action::PopupDelete => {
            if state.provider_popup_is_add_selected() {
                return Vec::new();
            }
            let backend = state
                .configured_providers()
                .get(state.provider_popup.selected_provider)
                .map(|p| p.backend.clone());
            let Some(backend) = backend else {
                return Vec::new();
            };
            remove_configured_provider(state, &backend);
            state.provider_popup.visible = false;
            state.push_transcript("System", format!("Removed provider `{backend}`."), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: format!("Removed provider `{backend}`."),
            });
            return vec![Command::SaveConfig {
                config: Box::new(ensure_config(state)),
            }];
        }
        _ => {}
    }
    Vec::new()
}

fn provider_popup_item_count(state: &AppState) -> usize {
    state.configured_providers().len() + 1
}

fn selected_popup_provider(state: &AppState) -> Option<ProviderConfig> {
    let mut provider = state.selected_configured_provider()?.clone();
    let models = models_for_configured_provider(state, &provider);
    if let Some(model) = models.get(state.provider_popup.selected_model) {
        provider.model = model.clone();
    }
    validate_provider_model(&provider.backend, &provider.model).ok()?;
    Some(provider)
}

fn models_for_configured_provider(state: &AppState, provider: &ProviderConfig) -> Vec<String> {
    let models = get_provider_models(state, &provider.backend);
    if models.is_empty() {
        vec![provider.model.clone()]
    } else {
        models
    }
}

fn select_configured_provider(state: &mut AppState, provider: ProviderConfig) -> Vec<Command> {
    state.provider_popup.visible = false;
    let mut config = ensure_config(state);
    config.provider = provider.clone();
    upsert_configured_provider(&mut config, provider.clone());
    state.current_config = Some(config.clone());

    let tip = provider_auth_tip(&provider);
    let msg = format!(
        "Switched to {} / {}.{tip}",
        provider.backend, provider.model
    );
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

fn insert_add_provider_template(state: &mut AppState) -> Vec<Command> {
    state.provider_popup.visible = false;
    state.composer.clear();
    for ch in "/provider-add ".chars() {
        state.composer.insert_char(ch);
    }
    state.recompute_suggestions();
    state.focus = FocusPane::Composer;
    state.push_transcript(
        "ArgOS",
        "Add OpenAI with `/provider-add openai <model> [key-ref]` for API keys or `/provider-add-openai-oauth <model> [token-ref]` for ChatGPT OAuth. Custom OpenAI-compatible providers use `/provider-add-custom <backend> <endpoint> <model> [key-ref]`. Store raw secrets separately with `/vault set <ref> <secret>`.".to_string(),
        None,
    );
    state.flash = Some(FlashMessage {
        level: StatusLevel::Success,
        text: "Provider add command inserted.".into(),
    });
    Vec::new()
}

fn next_visible_focus(state: &AppState, direction: isize) -> FocusPane {
    let panes = [
        FocusPane::Workflows,
        FocusPane::Transcript,
        FocusPane::Composer,
        FocusPane::Activity,
    ];
    let current = panes
        .iter()
        .position(|pane| *pane == state.focus)
        .unwrap_or(1) as isize;

    for step in 1..=panes.len() as isize {
        let index = (current + direction * step).rem_euclid(panes.len() as isize) as usize;
        let pane = panes[index];
        if is_focus_visible(state, pane) {
            return pane;
        }
    }

    FocusPane::Composer
}

fn is_focus_visible(state: &AppState, pane: FocusPane) -> bool {
    match pane {
        FocusPane::Workflows => state.sidebar_visible,
        FocusPane::Activity => state.activity_visible,
        FocusPane::Transcript | FocusPane::Composer => true,
    }
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
            state.transcript_follow = true;
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
            state.n8n_status.detail = "Refreshing optional workflow status…".into();
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Refreshing provider and optional workflow status…".into(),
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
            let Some(mut provider) = configured_provider(&config, &backend).cloned() else {
                let msg = format!(
                    "Provider `{backend}` is not configured. Add it with /provider-add or /provider-add-custom first."
                );
                state.push_transcript("System", msg.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Missing,
                    text: msg,
                });
                return Vec::new();
            };

            if let Err(message) = validate_provider_model(&provider.backend, &model) {
                state.push_transcript("System", message.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: message,
                });
                return Vec::new();
            }

            provider.model = model.clone();
            config.provider = provider.clone();
            upsert_configured_provider(&mut config, provider);

            state.current_config = Some(config.clone());
            let msg = format!("Provider set to {} / {}.", backend, model);
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
        ConfigCommand::AddKnownProvider {
            backend,
            model,
            key_ref,
            endpoint,
        } => {
            let fetched_models = get_provider_models(state, &backend);
            if !fetched_models.is_empty() && !fetched_models.iter().any(|id| id == &model) {
                let message = format!(
                    "Model `{model}` was not returned by `{backend}` model discovery. Enter a returned model id or use manual configuration knowing availability is unverified."
                );
                state.push_transcript("System", message.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: message,
                });
                return Vec::new();
            }

            match known_provider_config(&backend, &model, key_ref, endpoint) {
                Ok(provider) => add_provider_entry(state, provider),
                Err(message) => {
                    state.push_transcript("System", message.clone(), None);
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: message,
                    });
                    Vec::new()
                }
            }
        }
        ConfigCommand::AddOpenAiOAuthProvider { model, token_ref } => {
            let login_ref = token_ref
                .as_deref()
                .map(|token_ref| normalize_openai_oauth_ref(Some(token_ref)))
                .unwrap_or_else(|| DEFAULT_OPENAI_OAUTH_TOKEN_REF.to_string());
            match openai_oauth_provider_config(&model, token_ref) {
                Ok(provider) => {
                    let mut commands = add_provider_entry(state, provider);
                    state.push_transcript(
                        "System",
                        format!("Starting OpenAI OAuth login for `{login_ref}`. No tokens will be printed."),
                        None,
                    );
                    commands.push(Command::StartOpenAiLogin {
                        token_ref: login_ref,
                    });
                    commands
                }
                Err(message) => {
                    state.push_transcript("System", message.clone(), None);
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: message,
                    });
                    Vec::new()
                }
            }
        }
        ConfigCommand::OpenAiLogin { token_ref } => {
            let token_ref = match crate::services::validate_openai_oauth_ref(
                &normalize_openai_oauth_ref(token_ref.as_deref()),
            ) {
                Ok(token_ref) => token_ref,
                Err(message) => {
                    state.push_transcript("System", message.clone(), None);
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Error,
                        text: message,
                    });
                    return Vec::new();
                }
            };
            state.push_transcript(
                "System",
                format!(
                    "Starting OpenAI OAuth login for `{token_ref}`. No tokens will be printed."
                ),
                None,
            );
            state.push_activity(
                StatusLevel::Loading,
                "OpenAI OAuth login",
                "Requesting a device login code from OpenAI.",
            );
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Starting OpenAI OAuth login…".into(),
            });
            vec![Command::StartOpenAiLogin { token_ref }]
        }
        ConfigCommand::CodexLogin => {
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Opening browser for Codex login…".into(),
            });
            vec![Command::StartCodexLogin]
        }
        ConfigCommand::AddCustomProvider {
            backend,
            endpoint,
            model,
            key_ref,
        } => match custom_provider_config(&backend, &endpoint, &model, key_ref) {
            Ok(provider) => add_provider_entry(state, provider),
            Err(message) => {
                state.push_transcript("System", message.clone(), None);
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: message,
                });
                Vec::new()
            }
        },
        ConfigCommand::ListProviders => {
            state.push_transcript("ArgOS", configured_providers_text(state), None);
            state.flash = Some(FlashMessage {
                level: StatusLevel::Success,
                text: "Provider configuration help shown in transcript.".into(),
            });
            Vec::new()
        }
        ConfigCommand::RemoveProvider { backend } => {
            remove_configured_provider(state, &backend);
            state.push_transcript("System", format!("Removed provider `{backend}`."), None);
            vec![Command::SaveConfig {
                config: Box::new(ensure_config(state)),
            }]
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
            sync_active_provider_entry(&mut config);
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
            sync_active_provider_entry(&mut config);
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
            config.provider.auth_method = ProviderAuthMethod::ApiKey;
            config.provider.oauth_token_ref = None;
            sync_active_provider_entry(&mut config);
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

fn configured_provider<'a>(config: &'a Config, backend: &str) -> Option<&'a ProviderConfig> {
    config
        .providers
        .iter()
        .find(|provider| provider.backend.eq_ignore_ascii_case(backend))
}

fn upsert_configured_provider(config: &mut Config, provider: ProviderConfig) {
    if let Some(existing) = config
        .providers
        .iter_mut()
        .find(|entry| entry.backend.eq_ignore_ascii_case(&provider.backend))
    {
        *existing = provider;
    } else {
        config.providers.push(provider);
    }
}

fn remove_configured_provider(state: &mut AppState, backend: &str) {
    let mut config = ensure_config(state);
    config
        .providers
        .retain(|p| !p.backend.eq_ignore_ascii_case(backend));
    if config.provider.backend.eq_ignore_ascii_case(backend) {
        if let Some(first) = config.providers.first().cloned() {
            config.provider = first;
        } else {
            config.provider = ProviderConfig {
                backend: String::new(),
                model: String::new(),
                endpoint: None,
                api_key_ref: None,
                auth_method: ProviderAuthMethod::ApiKey,
                oauth_token_ref: None,
            };
        }
    }
    state.current_config = Some(config);
}

fn ensure_openai_oauth_provider(state: &mut AppState, token_ref: &str) {
    let model = state
        .current_config
        .as_ref()
        .map(|c| c.provider.model.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "gpt-4.1".to_string());

    let provider = ProviderConfig {
        backend: "openai".to_string(),
        model,
        endpoint: commands::known_provider("openai")
            .and_then(|p| p.default_endpoint)
            .map(str::to_string),
        api_key_ref: None,
        auth_method: ProviderAuthMethod::OpenAiOAuth,
        oauth_token_ref: Some(token_ref.to_string()),
    };

    let mut config = ensure_config(state);
    upsert_configured_provider(&mut config, provider.clone());
    config.provider = provider;
    state.current_config = Some(config);
}

fn ensure_codex_provider(state: &mut AppState) {
    let model = state
        .current_config
        .as_ref()
        .map(|c| c.provider.model.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "gpt-5.4".to_string());

    const CODEX_ENDPOINT: &str = "https://chatgpt.com/backend-api";

    let provider = ProviderConfig {
        backend: "codex".to_string(),
        model,
        endpoint: Some(CODEX_ENDPOINT.to_string()),
        api_key_ref: None,
        auth_method: ProviderAuthMethod::Codex,
        oauth_token_ref: Some(DEFAULT_CODEX_TOKEN_REF.to_string()),
    };

    let mut config = ensure_config(state);
    upsert_configured_provider(&mut config, provider.clone());
    config.provider = provider;
    state.current_config = Some(config);
}

fn sync_active_provider_entry(config: &mut Config) {
    if config.provider.backend.trim().is_empty() || config.provider.model.trim().is_empty() {
        return;
    }
    upsert_configured_provider(config, config.provider.clone());
}

fn known_provider_config(
    backend: &str,
    model: &str,
    key_ref: Option<String>,
    endpoint: Option<String>,
) -> Result<ProviderConfig, String> {
    let backend = backend.trim().to_lowercase();
    let model = model.trim().to_string();
    let known = commands::known_provider(&backend).ok_or_else(|| {
        format!("Unknown provider `{backend}`. Use /provider-add-custom for custom providers.")
    })?;

    validate_provider_model(&backend, &model)?;

    Ok(ProviderConfig {
        backend,
        model,
        endpoint: endpoint.or_else(|| known.default_endpoint.map(str::to_string)),
        api_key_ref: key_ref.or_else(|| known.default_key_ref.map(str::to_string)),
        auth_method: ProviderAuthMethod::ApiKey,
        oauth_token_ref: None,
    })
}

fn openai_oauth_provider_config(
    model: &str,
    token_ref: Option<String>,
) -> Result<ProviderConfig, String> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("OpenAI OAuth provider model cannot be empty.".into());
    }
    validate_provider_model("openai", &model)?;
    let oauth_token_ref = crate::services::validate_openai_oauth_ref(&normalize_openai_oauth_ref(
        token_ref.as_deref(),
    ))?;

    Ok(ProviderConfig {
        backend: "openai".into(),
        model,
        endpoint: commands::known_provider("openai")
            .and_then(|known| known.default_endpoint)
            .map(str::to_string),
        api_key_ref: None,
        auth_method: ProviderAuthMethod::OpenAiOAuth,
        oauth_token_ref: Some(oauth_token_ref),
    })
}

fn custom_provider_config(
    backend: &str,
    endpoint: &str,
    model: &str,
    key_ref: Option<String>,
) -> Result<ProviderConfig, String> {
    let endpoint = endpoint.trim().to_string();
    Url::parse(&endpoint)
        .map_err(|err| format!("Invalid provider endpoint `{endpoint}`: {err}"))?;

    let backend = backend.trim().to_lowercase();
    if backend.is_empty() {
        return Err("Custom provider backend cannot be empty.".into());
    }
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("Custom provider model cannot be empty.".into());
    }
    if commands::known_provider(&backend).is_some() {
        validate_provider_model(&backend, &model)?;
    }

    Ok(ProviderConfig {
        backend,
        model,
        endpoint: Some(endpoint),
        api_key_ref: key_ref,
        auth_method: ProviderAuthMethod::ApiKey,
        oauth_token_ref: None,
    })
}

fn validate_provider_model(backend: &str, model: &str) -> Result<(), String> {
    if backend.eq_ignore_ascii_case("openrouter") {
        return Ok(());
    }
    if is_known_openrouter_catalog_model(model) {
        return Err(format!(
            "Model `{model}` looks like an OpenRouter catalog id. Add/select `openrouter` instead of `{backend}`."
        ));
    }
    Ok(())
}

fn is_known_openrouter_catalog_model(model: &str) -> bool {
    if let Some((provider_prefix, _)) = model.split_once('/') {
        if commands::known_provider(provider_prefix).is_some() {
            return true;
        }
    }

    let Some(openrouter) = commands::known_provider("openrouter") else {
        return false;
    };
    openrouter.models.iter().any(|known| *known == model)
        || (model.contains('/') && model.ends_with(":free"))
}

fn add_provider_entry(state: &mut AppState, provider: ProviderConfig) -> Vec<Command> {
    let mut config = ensure_config(state);
    config.provider = provider.clone();
    upsert_configured_provider(&mut config, provider.clone());
    state.current_config = Some(config.clone());

    let tip = provider_auth_tip(&provider);
    let msg = format!(
        "Added provider {} / {}.{} Model availability is unverified until models are fetched from the provider.",
        provider.backend, provider.model, tip
    );
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

fn configured_providers_text(state: &AppState) -> String {
    let mut lines = vec!["Configured providers:".to_string(), String::new()];
    let providers = state.configured_providers();
    if providers.is_empty() {
        lines.push("  (none configured yet)".into());
        lines.push(String::new());
    } else {
        for provider in providers {
            let endpoint = provider.endpoint.as_deref().unwrap_or("(default/none)");
            let key_ref = provider.api_key_ref.as_deref().unwrap_or("(none)");
            let oauth_ref = provider.oauth_token_ref.as_deref().unwrap_or("(none)");
            lines.push(format!("  {} / {}", provider.backend, provider.model));
            lines.push(format!("    Endpoint: {endpoint}"));
            lines.push(format!("    Auth: {}", provider_auth_label(provider)));
            lines.push(format!("    API key ref: {key_ref}"));
            lines.push(format!("    OAuth token ref: {oauth_ref}"));
            lines.push(String::new());
        }
    }
    lines.push("Add known provider: /provider-add <backend> <model> [key-ref] [endpoint]".into());
    lines.push("Add OpenAI OAuth provider: /provider-add-openai-oauth <model> [token-ref]".into());
    lines.push(
        "Add custom provider: /provider-add-custom <backend> <endpoint> <model> [key-ref]".into(),
    );
    lines.push("Store secrets separately with /vault set <key-ref> <your-key>.".into());
    lines.push(String::new());
    lines.push(commands::providers_list_text());
    lines.join("\n")
}

fn normalize_configured_providers(mut config: Config) -> Config {
    if config.providers.is_empty()
        && !config.provider.backend.trim().is_empty()
        && !config.provider.model.trim().is_empty()
    {
        config.providers.push(config.provider.clone());
    }
    config
}

fn ensure_config(state: &mut AppState) -> Config {
    state.current_config.clone().unwrap_or_else(|| Config {
        n8n: None,
        provider: ProviderConfig {
            backend: String::new(),
            model: String::new(),
            endpoint: None,
            api_key_ref: None,
            auth_method: ProviderAuthMethod::ApiKey,
            oauth_token_ref: None,
        },
        providers: Vec::new(),
        embedder: Default::default(),
        storage: Default::default(),
        reuse_threshold: 0.82,
    })
}

fn provider_auth_label(provider: &ProviderConfig) -> &'static str {
    match provider.auth_method {
        ProviderAuthMethod::ApiKey => "API key",
        ProviderAuthMethod::OpenAiOAuth => "OpenAI OAuth",
        ProviderAuthMethod::Codex => "Codex",
    }
}

fn provider_auth_tip(provider: &ProviderConfig) -> String {
    match provider.auth_method {
        ProviderAuthMethod::ApiKey => provider
            .api_key_ref
            .as_ref()
            .map(|key_ref| format!(" Store the secret with `/vault set {key_ref} <your-key>`; the provider entry stores only the reference."))
            .unwrap_or_else(|| " No API key reference is configured for this provider.".to_string()),
        ProviderAuthMethod::OpenAiOAuth => {
            let token_ref = provider
                .oauth_token_ref
                .as_deref()
                .unwrap_or("provider/openai/oauth");
            format!(" Run `/openai-login {token_ref}` to authorize ChatGPT/OpenAI OAuth; config stores only this token reference.")
        }
        ProviderAuthMethod::Codex => {
            let token_ref = provider
                .oauth_token_ref
                .as_deref()
                .unwrap_or("provider/codex/oauth");
            format!(" Run `/codex-login` to authorize Codex backend; config stores only this token reference.")
        }
    }
}

fn maybe_fetch_models(state: &mut AppState) -> Vec<Command> {
    let Some(provider) = state.selected_configured_provider().cloned() else {
        return Vec::new();
    };
    if state.dynamic_models.contains_key(&provider.backend) {
        return Vec::new();
    }

    let endpoint = provider
        .endpoint
        .clone()
        .or_else(|| {
            commands::known_provider(&provider.backend)
                .and_then(|known| known.default_endpoint)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if endpoint.is_empty() {
        state.push_activity(
            StatusLevel::Missing,
            format!("Models: {}", provider.backend),
            "No endpoint configured; model availability cannot be fetched.".to_string(),
        );
        return Vec::new();
    }

    if !provider.backend.eq_ignore_ascii_case("openrouter")
        && !provider.backend.eq_ignore_ascii_case("ollama")
        && provider.auth_method != ProviderAuthMethod::OpenAiOAuth
        && provider.api_key_ref.is_none()
    {
        state.push_activity(
            StatusLevel::Missing,
            format!("Models: {}", provider.backend),
            "No API key reference configured; model availability cannot be fetched.".to_string(),
        );
        return Vec::new();
    }

    let backend = provider.backend.clone();
    let detail = format!("Loading {backend} models from {endpoint}.");
    queue_models_fetch(
        state,
        &backend,
        &endpoint,
        provider.api_key_ref,
        provider.auth_method,
        provider.oauth_token_ref,
        "Fetching provider models",
        &detail,
    )
}

fn queue_models_fetch(
    state: &mut AppState,
    backend: &str,
    endpoint: &str,
    api_key_ref: Option<String>,
    auth_method: ProviderAuthMethod,
    oauth_token_ref: Option<String>,
    title: &str,
    detail: &str,
) -> Vec<Command> {
    if state.dynamic_models.contains_key(backend) {
        return Vec::new();
    }

    state.push_activity(StatusLevel::Loading, title, detail);
    vec![Command::FetchModels {
        backend: backend.to_string(),
        endpoint: endpoint.to_string(),
        api_key_ref,
        auth_method,
        oauth_token_ref,
    }]
}

fn current_provider_models_command(state: &mut AppState) -> Vec<Command> {
    let Some(config) = state.current_config.clone() else {
        return Vec::new();
    };

    if state.dynamic_models.contains_key(&config.provider.backend) {
        return Vec::new();
    }

    let endpoint = config
        .provider
        .endpoint
        .clone()
        .or_else(|| {
            commands::known_provider(&config.provider.backend)
                .and_then(|provider| provider.default_endpoint)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if endpoint.is_empty() {
        return Vec::new();
    }

    let backend = config.provider.backend.clone();
    let detail = format!("Loading {backend} models from {endpoint}.");
    queue_models_fetch(
        state,
        &backend,
        &endpoint,
        config.provider.api_key_ref,
        config.provider.auth_method,
        config.provider.oauth_token_ref,
        "Fetching current provider models",
        &detail,
    )
}

fn normalize_openai_oauth_ref(token_ref: Option<&str>) -> String {
    token_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OPENAI_OAUTH_TOKEN_REF)
        .to_string()
}

fn get_provider_models(state: &AppState, backend: &str) -> Vec<String> {
    if let Some(dynamic) = state.dynamic_models.get(backend) {
        if !dynamic.is_empty() {
            return dynamic.iter().map(|m| m.id.clone()).collect();
        }
    }
    Vec::new()
}

fn trigger_refresh(state: &mut AppState) {
    state.is_loading_snapshot = true;
    state.provider_status.level = StatusLevel::Loading;
    state.provider_status.detail = "Refreshing provider status…".into();
    state.n8n_status.level = StatusLevel::Loading;
    state.n8n_status.detail = "Refreshing optional workflow status…".into();
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
                    state.current_config = snapshot.config.map(normalize_configured_providers);
                    state.clamp_selections();
                    state.push_activity(
                        StatusLevel::Success,
                        "Refresh completed",
                        format!("Loaded {} workflows.", state.workflows.len()),
                    );
                    return current_provider_models_command(state);
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
        AsyncEvent::OpenAiLoginStarted { token_ref, result } => match result {
            Ok(login) => {
                let mut body = format!(
                    "OpenAI OAuth login started for `{token_ref}`. Open {} and enter code {}.",
                    login.verification_uri, login.user_code
                );
                if let Some(complete) = &login.verification_uri_complete {
                    body.push_str(&format!(" Direct link: {complete}"));
                }
                body.push_str(
                    " ArgOS is polling for authorization; tokens will be stored only in the vault.",
                );
                state.push_transcript("System", body.clone(), None);
                state.push_activity(
                    StatusLevel::Loading,
                    "OpenAI OAuth login pending",
                    format!(
                        "Waiting for code {} authorization; token_ref={token_ref}.",
                        login.user_code
                    ),
                );
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Loading,
                    text: format!("OpenAI code: {}", login.user_code),
                });
                return vec![Command::CompleteOpenAiLogin { login }];
            }
            Err(err) => {
                state.push_activity(StatusLevel::Error, "OpenAI OAuth login failed", err.clone());
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: err,
                });
            }
        },
        AsyncEvent::OpenAiLoginCompleted { token_ref, result } => match result {
            Ok(()) => {
                let msg = format!(
                    "OpenAI OAuth login completed. Token JSON was stored in vault ref `{token_ref}`."
                );
                state.push_transcript("System", msg.clone(), None);
                state.push_activity(
                    StatusLevel::Success,
                    "OpenAI OAuth login completed",
                    format!("Stored refreshed credentials at token_ref={token_ref}."),
                );
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Success,
                    text: msg,
                });

                ensure_openai_oauth_provider(state, &token_ref);

                let mut cmds: Vec<Command> = vec![
                    Command::SaveConfig {
                        config: Box::new(ensure_config(state)),
                    },
                    Command::LoadSnapshot,
                ];
                cmds.extend(maybe_fetch_models(state));
                return cmds;
            }
            Err(err) => {
                state.push_activity(StatusLevel::Error, "OpenAI OAuth login failed", err.clone());
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: err,
                });
            }
        },
        AsyncEvent::CodexLoginCompleted { result } => match result {
            Ok(()) => {
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Success,
                    text: "Codex login completed!".into(),
                });
                ensure_codex_provider(state);
                return vec![Command::SaveConfig {
                    config: Box::new(ensure_config(state)),
                }];
            }
            Err(err) => {
                state.flash = Some(FlashMessage {
                    level: StatusLevel::Error,
                    text: format!("Codex login failed: {err}"),
                });
            }
        },
        AsyncEvent::ModelsFetched { backend, models } => match models {
            Ok(list) => {
                if backend.eq_ignore_ascii_case("openrouter") {
                    state.dynamic_models.insert(backend.clone(), list);
                    state.push_activity(
                        StatusLevel::Success,
                        "Models: OpenRouter",
                        format!(
                            "Fetched {} OpenRouter model ids.",
                            state.dynamic_models.get(&backend).map_or(0, |v| v.len())
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
    use super::{get_provider_models, handle_action, handle_async, Command};
    use crate::action::Action;
    use crate::event::AsyncEvent;
    use crate::services::{
        N8nSnapshot, PromptResult, ProviderSnapshot, Snapshot, WorkflowRunResult,
    };
    use crate::state::{AppState, FocusPane, ModelInfo, StatusLevel, WorkflowItem};
    use argos_agent::AgentOutput;
    use argos_core::{
        AgentState, Config, N8nRunRef, N8nRunStatus, ProviderAuthMethod, ProviderConfig,
    };

    fn test_config(providers: Vec<ProviderConfig>) -> Config {
        let provider = providers
            .first()
            .cloned()
            .unwrap_or_else(|| ProviderConfig {
                backend: String::new(),
                model: String::new(),
                endpoint: None,
                api_key_ref: None,
                auth_method: ProviderAuthMethod::ApiKey,
                oauth_token_ref: None,
            });
        Config {
            n8n: None,
            provider,
            providers,
            embedder: Default::default(),
            storage: Default::default(),
            reuse_threshold: 0.82,
        }
    }

    fn provider(
        backend: &str,
        endpoint: Option<&str>,
        model: &str,
        key_ref: Option<&str>,
    ) -> ProviderConfig {
        ProviderConfig {
            backend: backend.into(),
            model: model.into(),
            endpoint: endpoint.map(str::to_string),
            api_key_ref: key_ref.map(str::to_string),
            auth_method: ProviderAuthMethod::ApiKey,
            oauth_token_ref: None,
        }
    }

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
    fn slash_insert_opens_and_filters_command_suggestions() {
        let mut state = AppState::new();

        handle_action(&mut state, Action::ComposerInsert('/'));

        assert!(state.suggestions.contains(&"/help".to_string()));
        assert!(state
            .suggestions
            .contains(&"/provider <backend> <model>".to_string()));

        handle_action(&mut state, Action::ComposerInsert('p'));
        handle_action(&mut state, Action::ComposerInsert('r'));
        handle_action(&mut state, Action::ComposerInsert('o'));

        assert_eq!(state.composer.to_text(), "/pro");
        assert!(state
            .suggestions
            .contains(&"/provider <backend> <model>".to_string()));
        assert!(state.suggestions.contains(&"/providers".to_string()));
        assert!(state
            .suggestions
            .contains(&"/provider-add <backend> <model> [key-ref] [endpoint]".to_string()));
    }

    #[test]
    fn command_palette_does_not_replace_provider_picker() {
        let mut state = AppState::new();

        handle_action(&mut state, Action::ShowCommandPalette);
        assert!(state.command_palette.visible);
        assert!(!state.provider_popup.visible);

        handle_action(&mut state, Action::ShowCommandPalette);
        assert!(!state.command_palette.visible);

        handle_action(&mut state, Action::ShowProviderPopup);
        assert!(state.provider_popup.visible);
        assert!(!state.command_palette.visible);
    }

    #[test]
    fn provider_popup_uses_configured_providers_only() {
        let mut state = AppState::new();
        state.current_config = Some(test_config(vec![
            provider(
                "openrouter",
                Some("https://openrouter.ai/api/v1"),
                "openai/gpt-4.1",
                Some("provider/openrouter/api_key"),
            ),
            provider(
                "custom-local",
                Some("http://localhost:8080/v1"),
                "local-model",
                None,
            ),
        ]));

        handle_action(&mut state, Action::ShowProviderPopup);
        handle_action(&mut state, Action::MoveDown);
        handle_action(&mut state, Action::MoveDown);
        handle_action(&mut state, Action::MoveDown);

        assert_eq!(state.configured_providers().len(), 2);
        assert_eq!(state.provider_popup.selected_provider, 2);
        assert!(state.provider_popup_is_add_selected());
    }

    #[test]
    fn no_configured_providers_shows_help_and_add_state() {
        let mut state = AppState::new();

        handle_action(&mut state, Action::ShowProviderPopup);

        assert!(state.provider_popup.visible);
        assert!(state.provider_popup_is_add_selected());
        assert_eq!(state.activity.last().unwrap().level, StatusLevel::Missing);

        handle_action(&mut state, Action::SubmitPrompt);

        assert_eq!(state.composer.to_text(), "/provider-add ");
        assert!(!state.provider_popup.visible);
    }

    #[test]
    fn adding_custom_provider_stores_consistent_entry() {
        let mut state = AppState::new();
        for ch in "/provider-add-custom local-ai http://localhost:8080/v1 llama-local provider/local/api_key".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);
        let Command::SaveConfig { config } = commands.first().unwrap() else {
            panic!("expected config save");
        };

        assert_eq!(config.provider.backend, "local-ai");
        assert_eq!(
            config.provider.endpoint.as_deref(),
            Some("http://localhost:8080/v1")
        );
        assert_eq!(config.provider.model, "llama-local");
        assert_eq!(
            config.provider.api_key_ref.as_deref(),
            Some("provider/local/api_key")
        );
        assert_eq!(config.providers, vec![config.provider.clone()]);
    }

    #[test]
    fn adding_openai_oauth_provider_stores_only_oauth_ref() {
        let mut state = AppState::new();
        for ch in "/provider-add-openai-oauth gpt-4.1 provider/openai/oauth".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);
        let Command::SaveConfig { config } = commands.first().unwrap() else {
            panic!("expected config save");
        };

        assert_eq!(config.provider.backend, "openai");
        assert_eq!(config.provider.auth_method, ProviderAuthMethod::OpenAiOAuth);
        assert_eq!(
            config.provider.oauth_token_ref.as_deref(),
            Some("provider/openai/oauth")
        );
        assert!(config.provider.api_key_ref.is_none());
        assert_eq!(config.providers, vec![config.provider.clone()]);
        assert!(matches!(
            commands.get(1),
            Some(Command::StartOpenAiLogin { token_ref }) if token_ref == "provider/openai/oauth"
        ));
    }

    #[test]
    fn openai_login_command_emits_async_login_without_token_leakage() {
        let mut state = AppState::new();
        for ch in "/openai-login provider/openai/oauth".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert_eq!(
            commands,
            vec![Command::StartOpenAiLogin {
                token_ref: "provider/openai/oauth".into()
            }]
        );
        let body = &state.transcript.last().unwrap().body;
        assert!(body.contains("Starting OpenAI OAuth login"));
        assert!(!body.contains("access_token"));
        assert!(!body.contains("refresh_token"));
    }

    #[test]
    fn openai_login_rejects_api_key_or_other_provider_refs() {
        for input in [
            "/openai-login provider/openai/api_key",
            "/openai-login provider/openrouter/oauth",
            "/provider-login openai provider/anthropic/oauth",
        ] {
            let mut state = AppState::new();
            for ch in input.chars() {
                state.composer.insert_char(ch);
            }

            let commands = handle_action(&mut state, Action::SubmitPrompt);

            assert!(commands.is_empty(), "{input} should not start login");
            assert!(state
                .transcript
                .last()
                .unwrap()
                .body
                .contains("Invalid OpenAI OAuth token ref"));
        }
    }

    #[test]
    fn adding_openai_oauth_provider_rejects_invalid_oauth_ref_before_save() {
        let mut state = AppState::new();
        for ch in "/provider-add-openai-oauth gpt-4.1 provider/openai/api_key".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert!(commands.is_empty());
        assert!(state.current_config.is_none());
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("Invalid OpenAI OAuth token ref"));
    }

    #[test]
    fn native_provider_models_do_not_fallback_to_static_known_models() {
        let mut state = AppState::new();

        assert!(get_provider_models(&state, "openai").is_empty());

        handle_async(
            &mut state,
            AsyncEvent::ModelsFetched {
                backend: "openai".into(),
                models: Err("models endpoint returned 401".into()),
            },
        );

        assert!(get_provider_models(&state, "openai").is_empty());
    }

    #[test]
    fn native_known_provider_rejects_openrouter_catalog_model_ids() {
        let mut state = AppState::new();
        for ch in "/provider-add openai openai/gpt-4.1 provider/openai/api_key".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert!(commands.is_empty());
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("looks like an OpenRouter catalog id"));
    }

    #[test]
    fn native_known_provider_rejects_future_openrouter_qualified_ids() {
        let mut state = AppState::new();
        for ch in "/provider-add openai anthropic/future-model provider/openai/api_key".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        assert!(commands.is_empty());
        assert!(state
            .transcript
            .last()
            .unwrap()
            .body
            .contains("looks like an OpenRouter catalog id"));
    }

    #[test]
    fn native_provider_accepts_api_returned_slash_model_ids() {
        let mut state = AppState::new();
        state.current_config = Some(test_config(vec![provider(
            "google",
            Some("https://generativelanguage.googleapis.com/v1beta"),
            "models/gemini-2.5-flash",
            Some("provider/google/api_key"),
        )]));

        handle_action(&mut state, Action::ShowProviderPopup);
        let commands = handle_action(&mut state, Action::SubmitPrompt);

        let Command::SaveConfig { config } = commands.first().unwrap() else {
            panic!("expected config save");
        };
        assert_eq!(config.provider.backend, "google");
        assert_eq!(config.provider.model, "models/gemini-2.5-flash");
        assert_eq!(config.providers, vec![config.provider.clone()]);
    }

    #[test]
    fn slash_model_updates_matching_configured_provider_entry() {
        let mut state = AppState::new();
        state.current_config = Some(test_config(vec![provider(
            "openai",
            Some("https://api.openai.com/v1"),
            "gpt-4o",
            Some("provider/openai/api_key"),
        )]));
        for ch in "/model gpt-4.1-mini".chars() {
            state.composer.insert_char(ch);
        }

        let commands = handle_action(&mut state, Action::SubmitPrompt);

        let Command::SaveConfig { config } = commands.first().unwrap() else {
            panic!("expected config save");
        };
        assert_eq!(config.provider.model, "gpt-4.1-mini");
        assert_eq!(config.providers[0].model, "gpt-4.1-mini");
    }

    #[test]
    fn slash_endpoint_and_key_ref_update_matching_configured_provider_entry() {
        let mut state = AppState::new();
        state.current_config = Some(test_config(vec![provider(
            "openai",
            Some("https://api.openai.com/v1"),
            "gpt-4o",
            Some("provider/openai/api_key"),
        )]));
        for ch in "/endpoint https://proxy.test/v1".chars() {
            state.composer.insert_char(ch);
        }

        let endpoint_commands = handle_action(&mut state, Action::SubmitPrompt);
        let Command::SaveConfig { config } = endpoint_commands.first().unwrap() else {
            panic!("expected config save");
        };
        assert_eq!(
            config.provider.endpoint.as_deref(),
            Some("https://proxy.test/v1")
        );
        assert_eq!(
            config.providers[0].endpoint.as_deref(),
            Some("https://proxy.test/v1")
        );

        for ch in "/key-ref provider/openai/proxy_key".chars() {
            state.composer.insert_char(ch);
        }
        let key_commands = handle_action(&mut state, Action::SubmitPrompt);
        let Command::SaveConfig { config } = key_commands.first().unwrap() else {
            panic!("expected config save");
        };
        assert_eq!(
            config.provider.api_key_ref.as_deref(),
            Some("provider/openai/proxy_key")
        );
        assert_eq!(
            config.providers[0].api_key_ref.as_deref(),
            Some("provider/openai/proxy_key")
        );
    }

    #[test]
    fn missing_workflow_does_not_leave_unsolicited_error_toast() {
        let mut state = AppState::new();

        let commands = handle_action(&mut state, Action::RunSelectedWorkflow);

        assert!(commands.is_empty());
        assert!(state.flash.is_none());
        assert_eq!(state.activity.last().unwrap().level, StatusLevel::Missing);
    }

    #[test]
    fn openrouter_catalog_keeps_provider_models_endpoint_safe() {
        let mut state = AppState::new();

        handle_async(
            &mut state,
            AsyncEvent::ModelsFetched {
                backend: "openrouter".into(),
                models: Ok(vec![ModelInfo {
                    id: "openai/gpt-4.1".into(),
                    pricing: None,
                }]),
            },
        );

        assert_eq!(
            get_provider_models(&state, "openrouter"),
            vec!["openai/gpt-4.1"]
        );
        assert!(get_provider_models(&state, "openai").is_empty());
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
    fn async_prompt_failure_shows_flash_error() {
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
        assert!(state.transcript.is_empty());
        assert_eq!(state.flash.as_ref().unwrap().text, "boom");
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
