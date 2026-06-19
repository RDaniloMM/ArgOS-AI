//! CLI argument definitions (clap derive).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Top-level CLI entry point for the ArgOS AI Operating System.
///
/// Parses one of four subcommands: `wiki`, `n8n`, `workflow`, or `ask`.
#[derive(Parser)]
#[command(name = "argos", about = "AI Operating System")]
pub struct Cli {
    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level ArgOS subcommand — dispatches to wiki, n8n, workflow, or ask.
#[derive(Subcommand)]
pub enum Command {
    /// Wiki knowledge operations
    Wiki {
        #[command(subcommand)]
        action: WikiAction,
    },
    /// n8n workflow operations
    N8n {
        #[command(subcommand)]
        action: N8nAction,
    },
    /// Workflow intelligence operations
    Workflow {
        #[command(subcommand)]
        action: WorkflowAction,
    },
    /// Ask the ArgOS agent
    Ask {
        /// The prompt/question to ask
        prompt: Vec<String>,
    },
}

/// Wiki subcommand actions — ingest, query, or lint OKF knowledge.
#[derive(Subcommand)]
pub enum WikiAction {
    /// Ingest a raw source into the wiki
    Ingest {
        /// Path to raw source file
        source: PathBuf,
    },
    /// Query the OKF knowledge wiki
    Query {
        /// The question to query
        question: Vec<String>,
    },
    /// Lint the wiki for issues
    Lint,
}

/// n8n subcommand actions — list, import, or run n8n workflows.
#[derive(Subcommand)]
pub enum N8nAction {
    /// List n8n workflows
    List,
    /// Import an n8n workflow as an OKF concept
    Import {
        /// n8n workflow ID
        id: String,
    },
    /// Run an n8n workflow
    Run {
        /// n8n workflow ID
        id: String,
        /// Optional input data as JSON
        #[arg(short, long)]
        data: Option<String>,
    },
}

/// Workflow intelligence subcommand actions — recommend reuse or find similar.
#[derive(Subcommand)]
pub enum WorkflowAction {
    /// Recommend reuse for an intent
    Recommend {
        /// The intent description
        intent: Vec<String>,
    },
    /// Find similar workflows
    Similar {
        /// The intent description
        intent: Vec<String>,
    },
}
