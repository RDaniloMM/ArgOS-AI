//! ArgOS knowledge system.
//!
//! OKF bundle management (concept CRUD, frontmatter parsing, cross-links, typed
//! relations) and the LLM-Wiki operations: ingest, query, lint. The OKF markdown
//! bundle under `.argos/wiki/` is the source of truth; SQLite/sqlite-vec are
//! derived indexes only (ADR-010).
//!
//! `argos-knowledge` performs synchronous filesystem I/O (`std::fs`). The OKF
//! bundle is a small directory of markdown files; the CLI/agent layer wraps
//! these calls in `spawn_blocking` when integrating with the async runtime
//! (matching argos-storage's pattern). Keeping the crate synchronous makes the
//! CRUD operations trivially testable and avoids a tokio dependency here.

pub mod bundle;
pub mod parser;
pub mod raw;

pub use bundle::BundleStore;
pub use parser::{OkfParser, OkfWriter};
pub use raw::RawSourceStore;
