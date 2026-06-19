use std::path::{Path, PathBuf};

use argos_core::{ConnMode, N8nConnection, N8nRunRef, N8nWorkflowRef};

use crate::backend;

const CURATED_FILES: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "Cargo.toml",
    "argos-ui/Cargo.toml",
    "argos-core/Cargo.toml",
    "argos-provider/Cargo.toml",
    "argos-agent/Cargo.toml",
    "argos-n8n-connector/Cargo.toml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub title: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWorkflow {
    pub id: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct N8nSnapshot {
    pub available: bool,
    pub mode_label: String,
    pub message: String,
    pub workflows: Vec<WorkspaceWorkflow>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub provider_presets: Vec<backend::ProviderPreset>,
    pub current_provider: Option<backend::ProviderInput>,
    pub files: Vec<WorkspaceFile>,
    pub n8n: N8nSnapshot,
    pub vault_backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorContext {
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunOutcome {
    pub mode_label: String,
    pub run: N8nRunRef,
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .to_path_buf()
}

pub fn curated_workspace_files(root: &Path) -> Vec<WorkspaceFile> {
    CURATED_FILES
        .iter()
        .filter_map(|relative| {
            let absolute = root.join(relative);
            if absolute.exists() {
                Some(WorkspaceFile {
                    title: absolute
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(relative)
                        .to_string(),
                    relative_path: relative.replace('\\', "/"),
                    absolute_path: absolute,
                })
            } else {
                None
            }
        })
        .collect()
}

fn ensure_curated_file(root: &Path, requested: &Path) -> Result<PathBuf, String> {
    let requested = requested
        .canonicalize()
        .map_err(|e| format!("failed to resolve file path: {e}"))?;

    curated_workspace_files(root)
        .into_iter()
        .map(|file| {
            file.absolute_path
                .canonicalize()
                .map_err(|e| format!("failed to resolve curated file path: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|allowed| *allowed == requested)
        .ok_or_else(|| {
            format!(
                "This desktop slice only allows editing curated workspace files. Refused path: {}",
                requested.display()
            )
        })
}

pub fn build_assistant_prompt(user_prompt: &str, document: Option<&EditorContext>) -> String {
    match document {
        Some(document) => format!(
            "You are helping inside the ArgOS desktop workspace.\n\n\
             Active document: {}\n\
             Document content:\n\
             ```text\n{}\n```\n\n\
             User prompt:\n{}",
            document.title, document.content, user_prompt
        ),
        None => user_prompt.to_string(),
    }
}

pub fn workflow_preview(workflow: &N8nWorkflowRef, connection: &N8nConnection) -> String {
    let mode_label = match connection.mode {
        ConnMode::Mcp => {
            "MCP-configured (desktop slice uses connector-backed HTTP operations for list/run)"
        }
        ConnMode::Rest => "REST",
    };
    let url = workflow
        .url
        .as_ref()
        .map(|url| url.as_str().to_string())
        .unwrap_or_else(|| "Unavailable".to_string());

    format!(
        "# {}\n\n\
         - Workflow ID: `{}`\n\
         - Connection mode: {}\n\
         - Editor URL: {}\n\n\
         Raw workflow definitions are not exposed by the current slice-1 n8n transport.\n\
         This workspace shows real workflow inventory and run actions, while file editing stays scoped to curated local text files.",
        workflow.name, workflow.id, mode_label, url
    )
}

pub async fn load_workspace_snapshot(root: PathBuf) -> Result<WorkspaceSnapshot, String> {
    let config_dir = backend::argos_dir()?;
    let provider_presets = backend::provider_presets();
    let current_provider = backend::load_current_provider(&config_dir).await?;
    let files = curated_workspace_files(&root);
    let vault_backend = backend::desktop_vault_name().to_string();

    let n8n = match backend::list_n8n_workflows(&config_dir).await {
        Ok(Some((connection, workflows))) => {
            let mode_label = match connection.mode {
                ConnMode::Mcp => "MCP-configured",
                ConnMode::Rest => "REST",
            }
            .to_string();
            let message = match connection.mode {
                ConnMode::Mcp => {
                    "n8n is configured for MCP. This desktop slice uses the existing connector APIs with a REST-compatible backend for workflow list/run.".to_string()
                }
                ConnMode::Rest => "Connected to n8n over REST.".to_string(),
            };
            let workflows = workflows
                .into_iter()
                .map(|workflow| WorkspaceWorkflow {
                    id: workflow.id.clone(),
                    name: workflow.name.clone(),
                    content: workflow_preview(&workflow, &connection),
                })
                .collect();

            N8nSnapshot {
                available: true,
                mode_label,
                message,
                workflows,
            }
        }
        Ok(None) => N8nSnapshot {
            available: false,
            mode_label: "Not configured".into(),
            message: "Add an [n8n] section to .argos/config.toml to list and run workflows.".into(),
            workflows: Vec::new(),
        },
        Err(error) => N8nSnapshot {
            available: false,
            mode_label: "Unavailable".into(),
            message: error,
            workflows: Vec::new(),
        },
    };

    Ok(WorkspaceSnapshot {
        provider_presets,
        current_provider,
        files,
        n8n,
        vault_backend,
    })
}

pub async fn load_file_text(root: PathBuf, requested: PathBuf) -> Result<String, String> {
    let path = ensure_curated_file(&root, &requested)?;
    std::fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

pub async fn save_file_text(
    root: PathBuf,
    requested: PathBuf,
    content: String,
) -> Result<String, String> {
    let path = ensure_curated_file(&root, &requested)?;
    std::fs::write(&path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(format!("Saved {}", path.display()))
}

pub async fn run_assistant(
    user_prompt: String,
    document: Option<EditorContext>,
) -> Result<backend::AssistantResponse, String> {
    let config_dir = backend::argos_dir()?;
    let prompt = build_assistant_prompt(&user_prompt, document.as_ref());
    backend::run_assistant(&config_dir, &prompt).await
}

pub async fn run_workflow(workflow_id: String) -> Result<WorkflowRunOutcome, String> {
    let config_dir = backend::argos_dir()?;
    let (connection, run) = backend::run_n8n_workflow(&config_dir, &workflow_id, None).await?;
    let mode_label = match connection.mode {
        ConnMode::Mcp => "MCP-configured",
        ConnMode::Rest => "REST",
    }
    .to_string();
    Ok(WorkflowRunOutcome { mode_label, run })
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{ConnMode, N8nRunStatus};
    use tempfile::tempdir;
    use url::Url;

    #[test]
    fn curated_workspace_files_only_include_allowlisted_paths() {
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("README.md"), "# Hello").unwrap();
        std::fs::write(root.path().join("notes.txt"), "ignore").unwrap();

        let files = curated_workspace_files(root.path());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "README.md");
    }

    #[test]
    fn build_assistant_prompt_includes_active_document_context() {
        let prompt = build_assistant_prompt(
            "Summarize the file",
            Some(&EditorContext {
                title: "README.md".into(),
                content: "ArgOS desktop workspace".into(),
            }),
        );

        assert!(prompt.contains("Active document: README.md"));
        assert!(prompt.contains("ArgOS desktop workspace"));
        assert!(prompt.contains("Summarize the file"));
    }

    #[test]
    fn workflow_preview_explains_definition_limitation() {
        let workflow = N8nWorkflowRef {
            id: "wf-1".into(),
            name: "Daily Report".into(),
            url: Some(Url::parse("http://localhost:5678/workflow/wf-1").unwrap()),
        };
        let connection = N8nConnection {
            endpoint: Url::parse("http://localhost:5678").unwrap(),
            mode: ConnMode::Rest,
            api_key_ref: None,
        };

        let preview = workflow_preview(&workflow, &connection);
        assert!(preview.contains("Daily Report"));
        assert!(preview.contains("Raw workflow definitions are not exposed"));
    }

    #[test]
    fn workflow_run_outcome_is_structured() {
        let outcome = WorkflowRunOutcome {
            mode_label: "REST".into(),
            run: N8nRunRef {
                id: "run-1".into(),
                workflow_id: "wf-1".into(),
                status: N8nRunStatus::Success,
            },
        };

        assert_eq!(outcome.mode_label, "REST");
        assert_eq!(outcome.run.workflow_id, "wf-1");
    }
}
