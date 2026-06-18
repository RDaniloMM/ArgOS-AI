//! ArgOS CLI entry point.

use argos_cli::cli::{Cli, Command};
use argos_cli::commands;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Command::Wiki { action } => match action {
            argos_cli::cli::WikiAction::Ingest { source } => {
                commands::wiki::run_ingest(&source).await
            }
            argos_cli::cli::WikiAction::Query { question } => {
                commands::wiki::run_query(&question).await
            }
            argos_cli::cli::WikiAction::Lint => commands::wiki::run_lint().await,
        },
        Command::N8n { action } => match action {
            argos_cli::cli::N8nAction::List => commands::n8n::run_list().await,
            argos_cli::cli::N8nAction::Import { id } => commands::n8n::run_import(&id).await,
            argos_cli::cli::N8nAction::Run { id, data } => {
                commands::n8n::run_run(&id, data.as_deref()).await
            }
        },
        Command::Workflow { action } => match action {
            argos_cli::cli::WorkflowAction::Recommend { intent } => {
                commands::workflow::run_recommend(&intent).await
            }
            argos_cli::cli::WorkflowAction::Similar { intent } => {
                commands::workflow::run_similar(&intent).await
            }
        },
        Command::Ask { prompt } => commands::ask::run_ask(&prompt).await,
    }
}
