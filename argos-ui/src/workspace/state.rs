use std::path::PathBuf;

use crate::services::{WorkspaceFile, WorkspaceWorkflow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSection {
    Explorer,
    Workflows,
    Provider,
    Settings,
}

impl WorkspaceSection {
    pub const ALL: [WorkspaceSection; 4] = [
        WorkspaceSection::Explorer,
        WorkspaceSection::Workflows,
        WorkspaceSection::Provider,
        WorkspaceSection::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WorkspaceSection::Explorer => "Explorer",
            WorkspaceSection::Workflows => "n8n Workflows",
            WorkspaceSection::Provider => "Provider",
            WorkspaceSection::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveDocument {
    Empty,
    File(WorkspaceFile),
    Workflow(WorkspaceWorkflow),
}

impl ActiveDocument {
    pub fn title(&self) -> String {
        match self {
            ActiveDocument::Empty => "No document selected".into(),
            ActiveDocument::File(file) => file.title.clone(),
            ActiveDocument::Workflow(workflow) => workflow.name.clone(),
        }
    }

    pub fn subtitle(&self) -> String {
        match self {
            ActiveDocument::Empty => "Select a curated file or workflow".into(),
            ActiveDocument::File(file) => file.relative_path.clone(),
            ActiveDocument::Workflow(workflow) => format!("Workflow {}", workflow.id),
        }
    }

    pub fn path(&self) -> Option<PathBuf> {
        match self {
            ActiveDocument::File(file) => Some(file.absolute_path.clone()),
            ActiveDocument::Empty | ActiveDocument::Workflow(_) => None,
        }
    }

    pub fn editable(&self) -> bool {
        matches!(self, ActiveDocument::File(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEntry {
    pub role: ChatRole,
    pub content: String,
    pub meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEntry {
    pub title: String,
    pub detail: String,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_document_flags_editability() {
        let file = ActiveDocument::File(WorkspaceFile {
            title: "README.md".into(),
            relative_path: "README.md".into(),
            absolute_path: PathBuf::from("README.md"),
        });
        let workflow = ActiveDocument::Workflow(WorkspaceWorkflow {
            id: "wf-1".into(),
            name: "Daily".into(),
            content: "preview".into(),
        });

        assert!(file.editable());
        assert!(!workflow.editable());
    }

    #[test]
    fn workspace_section_labels_are_stable() {
        assert_eq!(WorkspaceSection::Explorer.label(), "Explorer");
        assert_eq!(WorkspaceSection::Workflows.label(), "n8n Workflows");
    }
}
