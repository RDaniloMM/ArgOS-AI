use crossterm::event::KeyEvent;

use crate::services::{PromptResult, Snapshot, WorkflowRunResult};

#[derive(Debug)]
pub enum Event {
    Input(KeyEvent),
    Resize(u16, u16),
    Async(AsyncEvent),
}

#[derive(Debug)]
pub enum AsyncEvent {
    SnapshotLoaded(Result<Snapshot, String>),
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
}
