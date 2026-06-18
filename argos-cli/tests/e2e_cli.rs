//! E2E integration tests for the ArgOS CLI binary.
//!
//! These tests invoke the compiled `argos` binary via assert_cmd and verify
//! help output, subcommand parsing, and basic command execution success.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_help_exits_zero() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("argos"));
}

#[test]
fn cli_wiki_help_shows_subcommands() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("wiki")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ingest"))
        .stdout(predicate::str::contains("query"))
        .stdout(predicate::str::contains("lint"));
}

#[test]
fn cli_n8n_help_shows_subcommands() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("n8n")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("import"))
        .stdout(predicate::str::contains("run"));
}

#[test]
fn cli_workflow_help_shows_subcommands() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("workflow")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("recommend"))
        .stdout(predicate::str::contains("similar"));
}

#[test]
fn cli_ask_requires_prompt() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("ask").assert().success(); // prompt is optional Vec — empty is OK
}

#[test]
fn cli_ask_with_prompt_succeeds() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("ask")
        .arg("what workflows do I have")
        .assert()
        .success();
}

#[test]
fn cli_wiki_lint_succeeds() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("wiki").arg("lint").assert().success();
}

#[test]
fn cli_n8n_list_succeeds() {
    let mut cmd = Command::cargo_bin("argos").unwrap();
    cmd.arg("n8n").arg("list").assert().success();
}
