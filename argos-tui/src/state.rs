use std::collections::HashMap;
use std::path::PathBuf;

use argos_core::Config;

use crate::composer::ComposerBuffer;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub pricing: Option<ModelPricing>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderPopupState {
    pub visible: bool,
    pub selected_provider: usize,
    pub selected_model: usize,
    pub column: PopupColumn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopupColumn {
    #[default]
    Provider,
    Model,
}

const PAGE_SIZE: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Workflows,
    Transcript,
    Composer,
    Activity,
}

impl FocusPane {
    pub fn next(self) -> Self {
        match self {
            Self::Workflows => Self::Transcript,
            Self::Transcript => Self::Composer,
            Self::Composer => Self::Activity,
            Self::Activity => Self::Workflows,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Workflows => Self::Activity,
            Self::Transcript => Self::Workflows,
            Self::Composer => Self::Transcript,
            Self::Activity => Self::Composer,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Workflows => "Workflows",
            Self::Transcript => "Transcript",
            Self::Composer => "Composer",
            Self::Activity => "Activity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Loading,
    Success,
    Missing,
    Error,
}

impl StatusLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Loading => "Loading",
            Self::Success => "Ready",
            Self::Missing => "Missing",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceStatus {
    pub level: StatusLevel,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowItem {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub speaker: String,
    pub body: String,
    pub meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub title: String,
    pub detail: String,
    pub level: StatusLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashMessage {
    pub level: StatusLevel,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub focus: FocusPane,
    pub provider_status: ResourceStatus,
    pub n8n_status: ResourceStatus,
    pub vault_name: String,
    pub workflows: Vec<WorkflowItem>,
    pub selected_workflow: usize,
    pub transcript: Vec<TranscriptEntry>,
    pub transcript_scroll: u16,
    pub composer: ComposerBuffer,
    pub activity: Vec<ActivityEntry>,
    pub selected_activity: usize,
    pub flash: Option<FlashMessage>,
    pub is_loading_snapshot: bool,
    pub is_submitting_prompt: bool,
    pub is_running_workflow: bool,
    pub should_quit: bool,
    pub current_config: Option<Config>,
    pub suggestions: Vec<String>,
    pub provider_popup: ProviderPopupState,
    pub dynamic_models: HashMap<String, Vec<ModelInfo>>,
    pub activity_visible: bool,
    pub session_tokens: u64,
    pub session_cost: f64,
    pub esc_last_press: Option<std::time::Instant>,
    pub cwd: PathBuf,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            focus: FocusPane::Composer,
            provider_status: ResourceStatus {
                level: StatusLevel::Loading,
                title: "Provider".into(),
                detail: "Waiting for refresh…".into(),
            },
            n8n_status: ResourceStatus {
                level: StatusLevel::Loading,
                title: "n8n".into(),
                detail: "Waiting for refresh…".into(),
            },
            vault_name: "KeyringVault".into(),
            workflows: Vec::new(),
            selected_workflow: 0,
            transcript: Vec::new(),
            transcript_scroll: 0,
            composer: ComposerBuffer::new(),
            activity: Vec::new(),
            selected_activity: 0,
            flash: None,
            is_loading_snapshot: false,
            is_submitting_prompt: false,
            is_running_workflow: false,
            should_quit: false,
            current_config: None,
            suggestions: Vec::new(),
            provider_popup: ProviderPopupState::default(),
            dynamic_models: HashMap::new(),
            activity_visible: false,
            session_tokens: 0,
            session_cost: 0.0,
            esc_last_press: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn selected_workflow(&self) -> Option<&WorkflowItem> {
        self.workflows.get(self.selected_workflow)
    }

    pub fn move_workflow_selection(&mut self, delta: isize) {
        if self.workflows.is_empty() {
            self.selected_workflow = 0;
            return;
        }

        let max = self.workflows.len().saturating_sub(1) as isize;
        let next = (self.selected_workflow as isize + delta).clamp(0, max);
        self.selected_workflow = next as usize;
    }

    pub fn move_activity_selection(&mut self, delta: isize) {
        if self.activity.is_empty() {
            self.selected_activity = 0;
            return;
        }

        let max = self.activity.len().saturating_sub(1) as isize;
        let next = (self.selected_activity as isize + delta).clamp(0, max);
        self.selected_activity = next as usize;
    }

    pub fn scroll_transcript_lines(&mut self, delta: i16) {
        if delta.is_negative() {
            self.transcript_scroll = self.transcript_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.transcript_scroll = self.transcript_scroll.saturating_add(delta as u16);
        }
    }

    pub fn page_transcript_up(&mut self) {
        self.transcript_scroll = self.transcript_scroll.saturating_sub(PAGE_SIZE);
    }

    pub fn page_transcript_down(&mut self) {
        self.transcript_scroll = self.transcript_scroll.saturating_add(PAGE_SIZE);
    }

    pub fn push_activity(
        &mut self,
        level: StatusLevel,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.activity.push(ActivityEntry {
            title: title.into(),
            detail: detail.into(),
            level,
        });
        self.selected_activity = self.activity.len().saturating_sub(1);
    }

    pub fn push_transcript(
        &mut self,
        speaker: impl Into<String>,
        body: impl Into<String>,
        meta: Option<String>,
    ) {
        self.transcript.push(TranscriptEntry {
            speaker: speaker.into(),
            body: body.into(),
            meta,
        });
        self.transcript_scroll = self.transcript_line_count().saturating_sub(1) as u16;
    }

    pub fn clamp_selections(&mut self) {
        if self.workflows.is_empty() {
            self.selected_workflow = 0;
        } else {
            self.selected_workflow = self.selected_workflow.min(self.workflows.len() - 1);
        }

        if self.activity.is_empty() {
            self.selected_activity = 0;
        } else {
            self.selected_activity = self.selected_activity.min(self.activity.len() - 1);
        }
    }

    pub fn transcript_line_count(&self) -> usize {
        self.transcript
            .iter()
            .map(|entry| {
                1 + entry.body.lines().count() + usize::from(entry.meta.as_ref().is_some())
            })
            .sum()
    }

    pub fn recompute_suggestions(&mut self) {
        let text = self.composer.to_text();
        if text.starts_with('/') {
            self.suggestions = crate::commands::suggest_commands(&text);
        } else {
            self.suggestions.clear();
        }
    }

    pub fn composer_status(&self) -> String {
        let Some(ref config) = self.current_config else {
            return String::new();
        };

        let mut parts = vec![format!("{} · {}", config.provider.backend, config.provider.model)];

        if is_thinking_model(&config.provider.model) {
            parts.push("🧠".into());
        }

        if self.session_tokens > 0 {
            parts.push(format_tokens(self.session_tokens));
            parts.push(format_cost(self.session_cost));
        }

        parts.join("  ")
    }
}

fn is_thinking_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("reasoner")
        || m.contains("o1")
        || m.contains("o3")
        || m.contains("o4")
        || m.starts_with("claude") && m.contains("thinking")
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M tk", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k tk", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens} tk")
    }
}

fn format_cost(cost: f64) -> String {
    if cost >= 1.0 {
        format!("${cost:.2}")
    } else if cost >= 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.6}")
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, FocusPane, WorkflowItem};

    #[test]
    fn focus_cycles_forward_and_backward() {
        assert_eq!(FocusPane::Workflows.next(), FocusPane::Transcript);
        assert_eq!(FocusPane::Composer.next(), FocusPane::Activity);
        assert_eq!(FocusPane::Workflows.prev(), FocusPane::Activity);
        assert_eq!(FocusPane::Transcript.prev(), FocusPane::Workflows);
    }

    #[test]
    fn workflow_selection_stays_in_bounds() {
        let mut state = AppState::new();
        state.workflows = vec![
            WorkflowItem {
                id: "1".into(),
                name: "One".into(),
            },
            WorkflowItem {
                id: "2".into(),
                name: "Two".into(),
            },
        ];

        state.move_workflow_selection(10);
        assert_eq!(state.selected_workflow, 1);

        state.move_workflow_selection(-10);
        assert_eq!(state.selected_workflow, 0);
    }
}
