//! ArgOS command-line interface.
//!
//! Thin clap adapter over the ArgOS crates: `argos wiki`, `argos n8n`,
//! `argos workflow`, `argos ask`. Slice 1 ships stubbed command handlers;
//! real service wiring is deferred to Phase 2.

#![warn(missing_docs)]

pub mod cli;
pub mod commands;

#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command, N8nAction, WikiAction, WorkflowAction};
    use clap::Parser;

    #[test]
    fn cli_parses_wiki_ingest() {
        let cli = Cli::try_parse_from(["argos", "wiki", "ingest", "file.md"]).unwrap();
        match cli.command {
            Command::Wiki { action } => match action {
                WikiAction::Ingest { source } => {
                    assert_eq!(source, std::path::PathBuf::from("file.md"))
                }
                _ => panic!("expected Ingest"),
            },
            _ => panic!("expected Wiki"),
        }
    }

    #[test]
    fn cli_parses_wiki_query() {
        let cli = Cli::try_parse_from(["argos", "wiki", "query", "what", "is", "okf"]).unwrap();
        match cli.command {
            Command::Wiki { action } => match action {
                WikiAction::Query { question } => {
                    assert_eq!(question, vec!["what", "is", "okf"])
                }
                _ => panic!("expected Query"),
            },
            _ => panic!("expected Wiki"),
        }
    }

    #[test]
    fn cli_parses_wiki_lint() {
        let cli = Cli::try_parse_from(["argos", "wiki", "lint"]).unwrap();
        match cli.command {
            Command::Wiki { action } => match action {
                WikiAction::Lint => {} // just verify it parsed
                _ => panic!("expected Lint"),
            },
            _ => panic!("expected Wiki"),
        }
    }

    #[test]
    fn cli_parses_n8n_list() {
        let cli = Cli::try_parse_from(["argos", "n8n", "list"]).unwrap();
        match cli.command {
            Command::N8n { action } => match action {
                N8nAction::List => {}
                _ => panic!("expected List"),
            },
            _ => panic!("expected N8n"),
        }
    }

    #[test]
    fn cli_parses_n8n_import() {
        let cli = Cli::try_parse_from(["argos", "n8n", "import", "abc123"]).unwrap();
        match cli.command {
            Command::N8n { action } => match action {
                N8nAction::Import { id } => assert_eq!(id, "abc123"),
                _ => panic!("expected Import"),
            },
            _ => panic!("expected N8n"),
        }
    }

    #[test]
    fn cli_parses_n8n_run() {
        let cli =
            Cli::try_parse_from(["argos", "n8n", "run", "abc123", "--data", r#"{"x":1}"#]).unwrap();
        match cli.command {
            Command::N8n { action } => match action {
                N8nAction::Run { id, data } => {
                    assert_eq!(id, "abc123");
                    assert_eq!(data, Some(r#"{"x":1}"#.to_string()));
                }
                _ => panic!("expected Run"),
            },
            _ => panic!("expected N8n"),
        }
    }

    #[test]
    fn cli_parses_n8n_run_no_data() {
        let cli = Cli::try_parse_from(["argos", "n8n", "run", "abc123"]).unwrap();
        match cli.command {
            Command::N8n { action } => match action {
                N8nAction::Run { id, data } => {
                    assert_eq!(id, "abc123");
                    assert_eq!(data, None);
                }
                _ => panic!("expected Run"),
            },
            _ => panic!("expected N8n"),
        }
    }

    #[test]
    fn cli_parses_workflow_recommend() {
        let cli =
            Cli::try_parse_from(["argos", "workflow", "recommend", "automate", "standup"]).unwrap();
        match cli.command {
            Command::Workflow { action } => match action {
                WorkflowAction::Recommend { intent } => {
                    assert_eq!(intent, vec!["automate", "standup"]);
                }
                _ => panic!("expected Recommend"),
            },
            _ => panic!("expected Workflow"),
        }
    }

    #[test]
    fn cli_parses_workflow_similar() {
        let cli =
            Cli::try_parse_from(["argos", "workflow", "similar", "automate", "standup"]).unwrap();
        match cli.command {
            Command::Workflow { action } => match action {
                WorkflowAction::Similar { intent } => {
                    assert_eq!(intent, vec!["automate", "standup"]);
                }
                _ => panic!("expected Similar"),
            },
            _ => panic!("expected Workflow"),
        }
    }

    #[test]
    fn cli_parses_ask() {
        let cli =
            Cli::try_parse_from(["argos", "ask", "what", "workflows", "do", "I", "have"]).unwrap();
        match cli.command {
            Command::Ask { prompt } => {
                assert_eq!(prompt, vec!["what", "workflows", "do", "I", "have"]);
            }
            _ => panic!("expected Ask"),
        }
    }

    #[test]
    fn cli_requires_subcommand() {
        let result = Cli::try_parse_from(["argos"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_help_output() {
        // Verify help text contains all subcommands
        let mut cmd = <Cli as clap::CommandFactory>::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("wiki"));
        assert!(help.contains("n8n"));
        assert!(help.contains("workflow"));
        assert!(help.contains("ask"));
    }
}
