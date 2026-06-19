# Changelog

All notable changes to ArgOS are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-06-18

### Added

- **Spine architecture**: n8n workflow engine + Agent Runtime + OKF Knowledge Wiki + Workflow Intelligence
- **11 crates** across 7 bounded contexts (core, storage, provider, security, knowledge, n8n connector, intelligence, agent, MCP, WASM, CLI, lib)
- **451 tests**, 0 failures (strict TDD)
- **n8n integration**: bidirectional MCP connector (REST fallback), workflow import/export/run
- **OKF Knowledge Wiki**: LLM-Wiki ingest/query/lint, typed cross-links, schema conventions
- **Workflow Intelligence**: intent-first vectorization, similarity search, reuse recommendation
- **Agent Runtime**: tool-call loop with GenericAgent, ToolRegistry, Tier-1 tools
- **MCP Server**: ArgOS exposes `wiki.query`, `workflow.recommend_reuse`, `workflow.similar` as MCP tools
- **MCP Client**: ArgOS discovers and invokes n8n MCP tools
- **OpenAI-compatible provider**: OpenCode Go / DeepSeek-V4-flash integration with real E2E tests
- **CLI**: `argos` binary with wiki/n8n/workflow/ask subcommands
- **WASM extension** stub (Tier-3, deferred)

### Infrastructure

- Cargo workspace, edition 2021, MSRV 1.75
- MIT license
- GitHub Actions CI (Ubuntu + Windows)
- Rust toolchain: stable-x86_64-pc-windows-gnu (MinGW)

[0.1.0]: https://github.com/RDaniloMM/ArgOS-AI/releases/tag/v0.1.0
