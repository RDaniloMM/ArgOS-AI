use crate::action::Action;
use crate::event::AsyncEvent;
use crate::state::{AppState, FlashMessage, FocusPane, ResourceStatus, StatusLevel};
use argos_core::{N8nRunRef, N8nRunStatus, ToolInvocation, ToolResult};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    LoadSnapshot,
    SubmitPrompt {
        prompt: String,
    },
    RunWorkflow {
        workflow_id: String,
        workflow_name: String,
    },
}

pub fn handle_action(state: &mut AppState, action: Action) -> Vec<Command> {
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
            state.push_transcript("You", prompt.clone(), None);
            state.push_activity(
                StatusLevel::Loading,
                "Prompt submitted",
                "Waiting for GenericAgent output.",
            );
            state.composer.clear();
            state.focus = FocusPane::Transcript;
            state.is_submitting_prompt = true;
            state.flash = Some(FlashMessage {
                level: StatusLevel::Loading,
                text: "Sending prompt through GenericAgent…".into(),
            });
            return vec![Command::SubmitPrompt { prompt }];
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
            if state.focus == FocusPane::Composer {
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
        Action::ComposerInsert(ch) => {
            if state.focus == FocusPane::Composer {
                state.composer.insert_char(ch);
            }
        }
        Action::ComposerNewline => {
            if state.focus == FocusPane::Composer {
                state.composer.insert_newline();
            }
        }
        Action::ComposerBackspace => {
            if state.focus == FocusPane::Composer {
                state.composer.backspace();
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
    }

    Vec::new()
}

pub fn handle_async(state: &mut AppState, event: AsyncEvent) -> Vec<Command> {
    match event {
        AsyncEvent::SnapshotLoaded(result) => {
            state.is_loading_snapshot = false;
            match result {
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
                    state.clamp_selections();
                    state.flash = Some(FlashMessage {
                        level: StatusLevel::Success,
                        text: "Status refresh completed.".into(),
                    });
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
            match result {
                Ok(response) => {
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
            AsyncEvent::SnapshotLoaded(Ok(Snapshot {
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
            })),
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
