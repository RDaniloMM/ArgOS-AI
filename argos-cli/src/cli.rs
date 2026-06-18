//! CLI argument definitions (clap derive).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "argos", about = "AI Operating System")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

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
