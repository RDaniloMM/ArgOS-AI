//! Filesystem content-addressed [`BlobStore`] implementation.
//!
//! Solo-profile blobs live under `.argos/objects/` (ADR-002), sharded by the
//! first two hex characters of the SHA-256 content hash to keep any single
//! directory small (`objects/ab/cdef1234...`). `store` is idempotent: identical
//! bytes always yield the same hash and a second store of the same content does
//! not overwrite an existing blob.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use argos_core::{ArgosError, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::traits::BlobStore;

/// Filesystem content-addressed [`BlobStore`].
///
/// Blobs are stored under `root/<first-2-hex>/<remaining-hex>` where the hex
/// string is the SHA-256 of the blob bytes. The shard keeps any one directory
/// small even with many blobs.
pub struct FsBlobStore {
    root: Arc<PathBuf>,
}

impl FsBlobStore {
    /// Open (or create) a blob store rooted at `root`, creating the directory
    /// tree if it does not already exist.
    pub fn open<P: AsRef<Path>>(root: P) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    /// Resolve the sharded on-disk path for a content `hash`.
    ///
    /// The hash is validated as a 64-char ASCII-hex string first, which also
    /// prevents path traversal (no `/` or `..` can survive validation).
    fn sharded_path(&self, hash: &str) -> Result<PathBuf> {
        validate_hash(hash)?;
        Ok(self.root.join(&hash[..2]).join(&hash[2..]))
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    async fn store(&self, data: &[u8]) -> Result<String> {
        let hash = hex_sha256(data);
        let path = self.sharded_path(&hash)?;
        let data = data.to_vec();
        spawn(move || -> Result<()> {
            // Idempotent: never overwrite an existing blob.
            if path.exists() {
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &data)?;
            Ok(())
        })
        .await?;
        Ok(hash)
    }

    async fn retrieve(&self, hash: &str) -> Result<Vec<u8>> {
        let hash = hash.to_string();
        let path = self.sharded_path(&hash)?;
        spawn(move || -> Result<Vec<u8>> {
            std::fs::read(&path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ArgosError::NotFound(format!("blob {hash}")),
                _ => ArgosError::Io(e.to_string()),
            })
        })
        .await
    }

    async fn exists(&self, hash: &str) -> Result<bool> {
        let path = self.sharded_path(hash)?;
        spawn(move || Ok(path.exists())).await
    }
}

/// SHA-256 of `data` as a lowercase hex string.
fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Validate that `hash` is a 64-character ASCII-hex string (SHA-256 shape).
fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ArgosError::Storage(format!("invalid content hash: {hash}")))
    }
}

/// Run a blocking closure on Tokio's blocking thread pool, flattening the
/// `JoinError`/inner-`Result` into a single [`Result`].
async fn spawn<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ArgosError::Storage(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::FsBlobStore;
    use crate::traits::BlobStore;
    use argos_core::ArgosError;
    use tempfile::tempdir;

    fn open_temp() -> (tempfile::TempDir, FsBlobStore) {
        let dir = tempdir().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn store_then_retrieve_returns_same_data() {
        let (_dir, store) = open_temp();
        let data = b"hello argos blob";
        let hash = store.store(data).await.unwrap();
        let retrieved = store.retrieve(&hash).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn store_is_idempotent_no_duplicate_write() {
        let (dir, store) = open_temp();
        let data = b"idempotent blob";
        let hash1 = store.store(data).await.unwrap();

        // Corrupt the stored file to detect a re-write on the second store.
        let shard = &hash1[0..2];
        let rest = &hash1[2..];
        let path = path_of(dir.path(), shard, rest);
        std::fs::write(&path, b"CORRUPTED").unwrap();

        // A second store of identical data must NOT overwrite (skip-if-exists).
        let hash2 = store.store(data).await.unwrap();
        assert_eq!(hash1, hash2);
        let retrieved = store.retrieve(&hash1).await.unwrap();
        assert_eq!(retrieved, b"CORRUPTED");
    }

    #[tokio::test]
    async fn exists_true_after_store() {
        let (_dir, store) = open_temp();
        let hash = store.store(b"present").await.unwrap();
        assert!(store.exists(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn exists_false_for_unknown_hash() {
        let (_dir, store) = open_temp();
        let unknown = "0".repeat(64);
        assert!(!store.exists(&unknown).await.unwrap());
    }

    #[tokio::test]
    async fn store_creates_sharded_directory_structure() {
        let (dir, store) = open_temp();
        let data = b"sharded blob";
        let hash = store.store(data).await.unwrap();
        let shard = &hash[0..2];
        let rest = &hash[2..];
        let expected = dir.path().join(shard).join(rest);
        assert!(expected.exists(), "sharded blob path should exist");
        // The shard is a 2-char directory.
        assert!(dir.path().join(shard).is_dir());
    }

    #[tokio::test]
    async fn retrieve_missing_hash_returns_not_found() {
        let (_dir, store) = open_temp();
        let unknown = "f".repeat(64);
        let err = store.retrieve(&unknown).await.unwrap_err();
        assert!(matches!(err, ArgosError::NotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn large_blob_roundtrips() {
        let (_dir, store) = open_temp();
        let data = vec![0xABu8; 1_000_000]; // 1 MB
        let hash = store.store(&data).await.unwrap();
        let retrieved = store.retrieve(&hash).await.unwrap();
        assert_eq!(retrieved.len(), data.len());
        assert_eq!(retrieved, data);
    }

    /// Build the relative sharded path `shard/rest` joined onto `root`.
    fn path_of(root: &std::path::Path, shard: &str, rest: &str) -> std::path::PathBuf {
        root.join(shard).join(rest)
    }
}
