//! ArgOS storage layer.
//!
//! Derived indexes only (SQLite / sqlite-vec / content-addressed FS). The OKF wiki
//! under `.argos/wiki/` is the source of truth; everything here is regenerable via
//! `argos reindex`. The traits in [`traits`] are backend-agnostic: Solo profile
//! backs them with SQLite + sqlite-vec + the filesystem, Team profile with
//! Postgres + Qdrant + S3. No embedded-specific SQL leaks through the trait seam.

pub mod traits;

pub use traits::{BlobStore, RelationalStore, Storage, VectorMetadata, VectorStore};
