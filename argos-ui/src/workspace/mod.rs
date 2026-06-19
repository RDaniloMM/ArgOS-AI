pub mod state;
mod view;

use std::path::PathBuf;

use iced::widget::text_editor;
use iced::{Task, Theme};

use crate::backend;
use crate::services::{self, EditorContext, WorkspaceSnapshot};
use state::{ActiveDocument, ChatEntry, ChatRole, OutputEntry, WorkspaceSection};

pub struct WorkspaceApp {
    pub(crate) root: PathBuf,
    pub(crate) active_section: WorkspaceSection,
    pub(crate) presets: Vec<backend::ProviderPreset>,
    pub(crate) current_provider: Option<backend::ProviderInput>,
    pub(crate) files: Vec<services::WorkspaceFile>,
    pub(crate) workflows: Vec<services::WorkspaceWorkflow>,
    pub(crate) n8n_mode_label: String,
    pub(crate) n8n_message: String,
    pub(crate) n8n_available: bool,
    pub(crate) vault_backend: String,
    pub(crate) active_document: ActiveDocument,
    pub(crate) editor: text_editor::Content,
    pub(crate) saved_text: String,
    pub(crate) workspace_loading: bool,
    pub(crate) file_loading: bool,
    pub(crate) file_saving: bool,
    pub(crate) assistant_loading: bool,
    pub(crate) workflow_running: Option<String>,
    pub(crate) chat_input: String,
    pub(crate) chat_entries: Vec<ChatEntry>,
    pub(crate) output: Vec<OutputEntry>,
    pub(crate) load_error: Option<String>,
    pub(crate) selected_preset_idx: Option<usize>,
    pub(crate) modal_open: bool,
    pub(crate) form_endpoint: String,
    pub(crate) form_model: String,
    pub(crate) form_api_key: String,
    pub(crate) form_testing: bool,
    pub(crate) form_saving: bool,
    pub(crate) form_test_result: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    WorkspaceLoaded(Result<WorkspaceSnapshot, String>),
    SectionSelected(WorkspaceSection),
    FileSelected(PathBuf),
    FileLoaded(PathBuf, Result<String, String>),
    WorkflowSelected(String),
    RunWorkflow(String),
    WorkflowRunFinished(Result<services::WorkflowRunOutcome, String>),
    EditorAction(text_editor::Action),
    SaveActiveFile,
    ActiveFileSaved(Result<String, String>),
    ChatInputChanged(String),
    SendChat,
    ChatCompleted(Result<backend::AssistantResponse, String>),
    OpenPresetForm(usize),
    CloseModal,
    EndpointChanged(String),
    ModelChanged(String),
    ApiKeyChanged(String),
    TestConnection,
    ConnectionTested(Result<backend::ProviderStatus, String>),
    SaveProvider,
    ProviderSaved(Result<(backend::ProviderInput, String), String>),
}

impl WorkspaceApp {
    pub fn new() -> (Self, Task<Message>) {
        let root = services::workspace_root();
        (
            Self {
                root: root.clone(),
                active_section: WorkspaceSection::Explorer,
                presets: backend::provider_presets(),
                current_provider: None,
                files: Vec::new(),
                workflows: Vec::new(),
                n8n_mode_label: "Not configured".into(),
                n8n_message: "Add an [n8n] section to .argos/config.toml.".into(),
                n8n_available: false,
                vault_backend: backend::desktop_vault_name().into(),
                active_document: ActiveDocument::Empty,
                editor: text_editor::Content::with_text(""),
                saved_text: String::new(),
                workspace_loading: true,
                file_loading: false,
                file_saving: false,
                assistant_loading: false,
                workflow_running: None,
                chat_input: String::new(),
                chat_entries: Vec::new(),
                output: Vec::new(),
                load_error: None,
                selected_preset_idx: None,
                modal_open: false,
                form_endpoint: String::new(),
                form_model: String::new(),
                form_api_key: String::new(),
                form_testing: false,
                form_saving: false,
                form_test_result: None,
            },
            Task::perform(
                services::load_workspace_snapshot(root),
                Message::WorkspaceLoaded,
            ),
        )
    }

    pub fn theme(&self) -> Theme {
        Theme::TokyoNight
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WorkspaceLoaded(result) => {
                self.workspace_loading = false;
                match result {
                    Ok(snapshot) => {
                        self.presets = snapshot.provider_presets;
                        self.current_provider = snapshot.current_provider;
                        self.files = snapshot.files;
                        self.workflows = snapshot.n8n.workflows;
                        self.n8n_mode_label = snapshot.n8n.mode_label;
                        self.n8n_message = snapshot.n8n.message;
                        self.n8n_available = snapshot.n8n.available;
                        self.vault_backend = snapshot.vault_backend;
                        self.load_error = None;

                        if let Some(provider) = &self.current_provider {
                            self.selected_preset_idx = self
                                .presets
                                .iter()
                                .position(|preset| preset.id == provider.preset_id);
                        }

                        if let Some(file) = self.files.first() {
                            self.file_loading = true;
                            let path = file.absolute_path.clone();
                            return Task::perform(
                                services::load_file_text(self.root.clone(), path.clone()),
                                move |result| Message::FileLoaded(path.clone(), result),
                            );
                        }
                    }
                    Err(error) => {
                        self.load_error = Some(error.clone());
                        self.push_output("Workspace load failed", error, true);
                    }
                }
                Task::none()
            }
            Message::SectionSelected(section) => {
                self.active_section = section;
                Task::none()
            }
            Message::FileSelected(path) => {
                self.active_section = WorkspaceSection::Explorer;
                self.file_loading = true;
                Task::perform(
                    services::load_file_text(self.root.clone(), path.clone()),
                    move |result| Message::FileLoaded(path.clone(), result),
                )
            }
            Message::FileLoaded(path, result) => {
                self.file_loading = false;
                match result {
                    Ok(content) => {
                        self.saved_text.clone_from(&content);
                        self.editor = text_editor::Content::with_text(&content);
                        if let Some(file) =
                            self.files.iter().find(|file| file.absolute_path == path)
                        {
                            self.active_document = ActiveDocument::File(file.clone());
                            self.push_output(
                                "File opened",
                                format!("Loaded {}", file.relative_path),
                                false,
                            );
                        }
                    }
                    Err(error) => {
                        self.push_output("Open failed", error, true);
                    }
                }
                Task::none()
            }
            Message::WorkflowSelected(workflow_id) => {
                self.active_section = WorkspaceSection::Workflows;
                if let Some(workflow) = self
                    .workflows
                    .iter()
                    .find(|workflow| workflow.id == workflow_id)
                {
                    self.active_document = ActiveDocument::Workflow(workflow.clone());
                    self.push_output(
                        "Workflow selected",
                        format!("Loaded metadata view for {}", workflow.name),
                        false,
                    );
                }
                Task::none()
            }
            Message::RunWorkflow(workflow_id) => {
                self.workflow_running = Some(workflow_id.clone());
                Task::perform(
                    services::run_workflow(workflow_id),
                    Message::WorkflowRunFinished,
                )
            }
            Message::WorkflowRunFinished(result) => {
                self.workflow_running = None;
                match result {
                    Ok(outcome) => {
                        self.push_output(
                            "Workflow run started",
                            format!(
                                "{} execution {} is {:?}",
                                outcome.mode_label, outcome.run.id, outcome.run.status
                            ),
                            false,
                        );
                    }
                    Err(error) => self.push_output("Workflow run failed", error, true),
                }
                Task::none()
            }
            Message::EditorAction(action) => {
                if self.active_document.editable() {
                    self.editor.perform(action);
                }
                Task::none()
            }
            Message::SaveActiveFile => {
                let Some(path) = self.active_document.path() else {
                    return Task::none();
                };
                self.file_saving = true;
                Task::perform(
                    services::save_file_text(self.root.clone(), path, self.editor.text()),
                    Message::ActiveFileSaved,
                )
            }
            Message::ActiveFileSaved(result) => {
                self.file_saving = false;
                match result {
                    Ok(message) => {
                        self.saved_text = self.editor.text();
                        self.push_output("File saved", message, false);
                    }
                    Err(error) => self.push_output("Save failed", error, true),
                }
                Task::none()
            }
            Message::ChatInputChanged(value) => {
                self.chat_input = value;
                Task::none()
            }
            Message::SendChat => {
                if self.chat_input.trim().is_empty() {
                    return Task::none();
                }

                let prompt = self.chat_input.trim().to_string();
                self.chat_entries.push(ChatEntry {
                    role: ChatRole::User,
                    content: prompt.clone(),
                    meta: None,
                });
                self.chat_input.clear();
                self.assistant_loading = true;

                Task::perform(
                    services::run_assistant(prompt, self.active_editor_context()),
                    Message::ChatCompleted,
                )
            }
            Message::ChatCompleted(result) => {
                self.assistant_loading = false;
                match result {
                    Ok(response) => {
                        let meta = Some(format!(
                            "{} · {} · {} tool events",
                            response.provider_backend,
                            response.final_state,
                            response.tool_invocations.len()
                        ));
                        let tool_summary = if response.tool_invocations.is_empty() {
                            "No tools were invoked in this slice.".to_string()
                        } else {
                            response
                                .tool_invocations
                                .iter()
                                .map(|event| format!("{} {}", event.name, event.args))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        };

                        self.chat_entries.push(ChatEntry {
                            role: ChatRole::Assistant,
                            content: response.text,
                            meta,
                        });
                        self.push_output("Assistant completed", tool_summary, false);
                    }
                    Err(error) => {
                        self.chat_entries.push(ChatEntry {
                            role: ChatRole::System,
                            content: error.clone(),
                            meta: Some("Assistant error".into()),
                        });
                        self.push_output("Assistant failed", error, true);
                    }
                }
                Task::none()
            }
            Message::OpenPresetForm(idx) => {
                self.active_section = WorkspaceSection::Provider;
                self.selected_preset_idx = Some(idx);
                if let Some(preset) = self.presets.get(idx) {
                    if let Some(input) = &self.current_provider {
                        if input.preset_id == preset.id {
                            self.form_endpoint.clone_from(&input.endpoint);
                            self.form_model.clone_from(&input.model);
                            self.form_api_key.clone_from(&input.api_key);
                        } else {
                            self.form_endpoint = preset.default_endpoint.clone();
                            self.form_model = preset.default_model.clone();
                            self.form_api_key.clear();
                        }
                    } else {
                        self.form_endpoint = preset.default_endpoint.clone();
                        self.form_model = preset.default_model.clone();
                        self.form_api_key.clear();
                    }
                }
                self.form_test_result = None;
                self.modal_open = true;
                Task::none()
            }
            Message::CloseModal => {
                self.modal_open = false;
                self.form_test_result = None;
                Task::none()
            }
            Message::EndpointChanged(value) => {
                self.form_endpoint = value;
                Task::none()
            }
            Message::ModelChanged(value) => {
                self.form_model = value;
                Task::none()
            }
            Message::ApiKeyChanged(value) => {
                self.form_api_key = value;
                Task::none()
            }
            Message::TestConnection => {
                let Some(preset) = self.current_preset().cloned() else {
                    return Task::none();
                };
                let input = backend::ProviderInput {
                    preset_id: preset.id,
                    api_key: self.form_api_key.clone(),
                    endpoint: self.form_endpoint.clone(),
                    model: self.form_model.clone(),
                };
                self.form_testing = true;
                self.form_test_result = None;
                Task::perform(
                    async move { backend::test_provider(&input).await },
                    Message::ConnectionTested,
                )
            }
            Message::ConnectionTested(result) => {
                self.form_testing = false;
                match result {
                    Ok(status) => {
                        self.form_test_result = Some(status.message.clone());
                        self.push_output(
                            if status.connected {
                                "Provider connected"
                            } else {
                                "Provider rejected"
                            },
                            status.message,
                            !status.connected,
                        );
                    }
                    Err(error) => {
                        self.form_test_result = Some(format!("Error: {error}"));
                        self.push_output("Provider test failed", error, true);
                    }
                }
                Task::none()
            }
            Message::SaveProvider => {
                let Some(preset) = self.current_preset().cloned() else {
                    return Task::none();
                };
                let input = backend::ProviderInput {
                    preset_id: preset.id.clone(),
                    api_key: self.form_api_key.clone(),
                    endpoint: self.form_endpoint.clone(),
                    model: self.form_model.clone(),
                };
                let name = preset.name.clone();
                self.form_saving = true;
                Task::perform(
                    async move {
                        let dir = backend::argos_dir()?;
                        let mut vault = backend::desktop_vault();
                        backend::save_provider(&dir, &mut vault, &input).await?;
                        Ok((input, name))
                    },
                    Message::ProviderSaved,
                )
            }
            Message::ProviderSaved(result) => {
                self.form_saving = false;
                match result {
                    Ok((input, name)) => {
                        self.current_provider = Some(input);
                        self.modal_open = false;
                        self.form_test_result = None;
                        self.push_output(
                            "Provider saved",
                            format!(
                                "{name} credentials were saved through {}",
                                self.vault_backend
                            ),
                            false,
                        );
                    }
                    Err(error) => self.push_output("Provider save failed", error, true),
                }
                Task::none()
            }
        }
    }

    pub fn current_preset(&self) -> Option<&backend::ProviderPreset> {
        self.selected_preset_idx
            .and_then(|idx| self.presets.get(idx))
    }

    pub(crate) fn active_editor_context(&self) -> Option<EditorContext> {
        match &self.active_document {
            ActiveDocument::File(file) => Some(EditorContext {
                title: file.relative_path.clone(),
                content: self.editor.text(),
            }),
            ActiveDocument::Empty | ActiveDocument::Workflow(_) => None,
        }
    }

    pub(crate) fn push_output(
        &mut self,
        title: impl Into<String>,
        detail: impl Into<String>,
        is_error: bool,
    ) {
        self.output.insert(
            0,
            OutputEntry {
                title: title.into(),
                detail: detail.into(),
                is_error,
            },
        );
        self.output.truncate(24);
    }
}
