use crossterm::event::KeyEvent;

use crate::services::{OpenAiLoginStart, PromptResult, Snapshot, WorkflowRunResult};
use crate::state::ModelInfo;

#[derive(Debug)]
pub enum Event {
    Input(KeyEvent),
    Resize(u16, u16),
    Async(AsyncEvent),
}

#[derive(Debug)]
pub enum AsyncEvent {
    SnapshotLoaded(Box<Result<Snapshot, String>>),
    InputError(String),
    PromptCompleted {
        prompt: String,
        result: Result<PromptResult, String>,
    },
    WorkflowCompleted {
        workflow_id: String,
        workflow_name: String,
        result: Result<WorkflowRunResult, String>,
    },
    ConfigSaved {
        result: Result<(), String>,
    },
    SecretStored {
        key_ref: String,
        result: Result<(), String>,
    },
    SecretDeleted {
        key_ref: String,
        result: Result<(), String>,
    },
    OpenAiLoginStarted {
        token_ref: String,
        result: Result<OpenAiLoginStart, String>,
    },
    OpenAiLoginCompleted {
        token_ref: String,
        result: Result<(), String>,
    },
    CodexLoginCompleted {
        result: Result<(), String>,
    },
    ModelsFetched {
        backend: String,
        models: Result<Vec<ModelInfo>, String>,
    },
}
