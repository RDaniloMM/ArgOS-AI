# ArgOS — AI Operating System

[![CI](https://github.com/RDaniloMM/ArgOS-AI/actions/workflows/ci.yml/badge.svg)](https://github.com/RDaniloMM/ArgOS-AI/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.75+](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://rust-lang.org)

**Open-source AI Operating System in Rust.** Combines n8n workflow automation, OKF knowledge management, agent orchestration, and workflow intelligence. Local-first, MCP-native, CLI-driven.

## Architecture

ArgOS is built around four spines that interoperate through MCP (Model Context Protocol) and shared domain types:

```
┌───────────────────────────────────────────────────────────┐
│                         ArgOS CLI                         │
│          argos wiki | n8n | workflow | ask                │
└───────────────┬───────────────┬───────────────┬───────────┘
                │               │               │
    ┌───────────▼───┐  ┌────────▼────────┐  ┌──▼──────────┐
    │  Agent Runtime │  │  Workflow       │  │  OKF         │
    │  (tool-call    │  │  Intelligence   │  │  Knowledge   │
    │   loop)        │  │  (vectorize,    │  │  Wiki        │
    │               │  │   similarity,    │  │  (LLM-Wiki)  │
    │               │  │   recommend)     │  │              │
    └───────┬───────┘  └────────┬────────┘  └──────┬───────┘
            │                   │                   │
            └───────────────────┼───────────────────┘
                                │
                    ┌───────────▼───────────┐
                    │     MCP Bidirectional  │
                    │  Server ─── Client     │
                    └───────────┬───────────┘
                                │
                    ┌───────────▼───────────┐
                    │   n8n Workflow Engine  │
                    │   (400+ integrations)  │
                    └───────────────────────┘
```

1. **n8n Workflow Engine** — External engine with 400+ integrations. ArgOS connects bidirectionally via MCP (primary) or REST (fallback). n8n owns all execution; ArgOS mirrors run status for audit.

2. **OKF Knowledge Wiki** — LLM-Wiki pattern (Karpathy): markdown + frontmatter concepts under `.argos/wiki/` are the source of truth. Ingest, query, lint operations. Cross-links form a typed knowledge graph.

3. **Agent Runtime** — Tool-call loop: observe → think → act. GenericAgent drives the loop; ToolRegistry holds compiled tools (wiki ops, n8n ops, workflow intelligence).

4. **Workflow Intelligence** — **The differentiator.** Vectorizes workflow concepts (intent-first), searches by similarity, and recommends reuse: "Do I already have a workflow that does this?"

### Storage Model

| Tier | Location | Role |
|------|----------|------|
| Source of truth | `.argos/wiki/` | OKF markdown bundles (git-tracked) |
| Derived indexes | SQLite + sqlite-vec | Regenerable via `argos reindex` |
| Runtime | `.argos/` | Agent state, n8n connection cache |

## Quick Start

### Prerequisites

- **Rust 1.75+** (edition 2021)
- **n8n instance** (local or remote). For local dev:
  ```bash
  docker run -d --name n8n -p 5678:5678 docker.n8n.io/n8nio/n8n
  ```
- **LLM API key** — any OpenAI-compatible provider (OpenCode Go, DeepSeek, Ollama, etc.)

### Install

```bash
cargo install --path argos-cli
```

### Usage

```bash
# Initialize ArgOS and connect to n8n
argos init --n8n-url http://localhost:5678

# Ask the agent
argos ask "find workflows similar to daily standup report"

# Manage wiki knowledge
argos wiki ingest docs/architecture.md
argos wiki query "how does the reuse loop work?"
argos wiki lint

# n8n workflow operations
argos n8n list
argos n8n import <workflow-id>
argos n8n run <workflow-id> --data '{"key": "value"}'

# Workflow intelligence
argos workflow recommend "automate PR review assignment"
argos workflow similar "send weekly report"
```

## Development

```bash
# Clone and build
git clone https://github.com/RDaniloMM/ArgOS-AI.git
cd ArgOS-AI

# Run all unit tests
cargo test --lib

# Run integration tests (requires n8n running + LLM API key)
cargo test -p argos-agent -- --ignored --test-threads=1

# Lint and format (enforced in CI)
cargo clippy --all-targets
cargo fmt --all -- --check

# Generate docs
cargo doc --no-deps --document-private-items
```

## Crates

11 crates across 7 bounded contexts:

| Crate | Bounded Context | Description |
|-------|----------------|-------------|
| `argos-core` | Core | Domain types, AgentState, Tool, error types |
| `argos-storage` | Storage | VectorStore, BlobStore, RelationalStore traits + SQLite/FS impls |
| `argos-provider` | Provider | LLM abstraction (Ollama, OpenAI-compatible, DeepSeek-V4-flash) |
| `argos-security` | Security | PermissionGate, SecretVault, AuditLog (HashChain) |
| `argos-knowledge` | Knowledge | OKF bundle CRUD, LLM-Wiki ingest/query/lint, cross-links |
| `argos-n8n-connector` | n8n Connector | N8nClient trait, REST client, workflow import/export/run |
| `argos-workflow-intelligence` | Intelligence | Vectorize, similarity, recommend_reuse, crosslinks |
| `argos-agent` | Agent | ToolRegistry, GenericAgent, tool-call loop |
| `argos-mcp` | MCP Extension | McpServer, McpClient, JSON-RPC protocol, n8n adapter |
| `argos-wasm` | WASM Extension | WasmRuntime trait + stub (wasmtime deferred) |
| `argos-cli` | CLI | `argos` binary: wiki/n8n/workflow/ask subcommands |
| `argos-lib` | Library | Unified facade re-exporting all 10 context crates |

## License

MIT — see [LICENSE](LICENSE).
