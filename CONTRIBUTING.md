# Contributing to ArgOS

Thank you for your interest in contributing to the ArgOS AI Operating System. This guide covers development setup, testing, and our conventions.

## Development Setup

```bash
# Clone and build
git clone https://github.com/RDaniloMM/ArgOS-AI.git
cd ArgOS-AI

# Run all unit tests
cargo test --lib

# Run integration tests (requires n8n running + LLM API key)
cargo test -p argos-agent -- --ignored --test-threads=1

# Lint and format
cargo clippy --all-targets
cargo fmt --all -- --check

# Generate docs
cargo doc --no-deps --document-private-items
```

### Prerequisites

- **Rust 1.75+** (edition 2021)
- **n8n instance** — local via Docker: `docker run -d --name n8n -p 5678:5678 docker.n8n.io/n8nio/n8n`
- **LLM API key** — any OpenAI-compatible provider

## Testing

- **Unit tests**: `cargo test --lib` (fast, no external dependencies)
- **Integration tests**: `cargo test -p argos-agent -- --ignored` (requires n8n + LLM)
- **Strict TDD**: Write tests first (RED), implement the behavior (GREEN), then refactor with clippy and fmt

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): description
```

| Type | Usage |
|------|-------|
| `feat` | New feature |
| `fix` | Bug fix |
| `test` | Adding or updating tests |
| `refactor` | Code restructuring (no behavior change) |
| `docs` | Documentation |
| `chore` | Maintenance, CI, tooling |

## Code Style

- **`cargo fmt`** — enforced in CI. Run before committing.
- **`cargo clippy --all-targets`** — enforced in CI. Zero warnings required.
- **English** — all code, identifiers, comments, and documentation are in English.

## Pull Requests

- Internal contributors push directly to `main`.
- External contributors: fork → feature branch → PR against `main`.
- Keep PRs focused. If a change exceeds ~400 lines, consider splitting into chained PRs.

## Architecture

ArgOS follows domain-driven design with 7 bounded contexts across 11 crates. See the [README](README.md) for the crate map and architecture diagram.

Key design principles:
- **OKF markdown bundles are source of truth** — SQLite/sqlite-vec are derived indexes only
- **n8n owns all workflow execution** — ArgOS delegates, mirrors, and audits
- **MCP is the preferred transport** — REST is fallback, both behind the same trait
- **Trait seams for every I/O boundary** — backends are pluggable (Solo via SQLite/FS, Team via Postgres/S3)

## License

MIT — see [LICENSE](LICENSE). All contributions are under the same license.
