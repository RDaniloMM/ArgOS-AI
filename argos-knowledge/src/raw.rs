//! Immutable raw source storage — the `.argos/wiki/raw/` layer.
//!
//! Raw sources are never modified by the LLM (ADR-010). [`RawSourceStore`]
//! content-addresses them by SHA-256 so re-ingest is idempotent: identical
//! bytes yield the same hash and are not rewritten.

use std::fs;
use std::path::{Path, PathBuf};

use argos_core::{ArgosError, RawSource, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};

/// Content-addressed immutable raw source store.
///
/// Files are stored as `<sha256-hex>.<original-extension>` under `root`.
/// Lookup by hash scans the (small) directory for the matching prefix.
pub struct RawSourceStore {
    root: PathBuf,
}

impl RawSourceStore {
    /// Create a raw source store rooted at `root`. The directory is created
    /// lazily on the first store.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Store `data` content-addressed. Returns the [`RawSource`] descriptor
    /// with its SHA-256 hash. Idempotent: storing identical bytes twice does
    /// not rewrite or duplicate the file.
    pub fn store_raw(&self, data: &[u8], original_path: &str) -> Result<RawSource> {
        let hash = hex_sha256(data);
        let ext = Path::new(original_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("md");
        let file_path = self.root.join(format!("{hash}.{ext}"));
        fs::create_dir_all(&self.root)?;
        if !file_path.exists() {
            fs::write(&file_path, data)?;
        }
        Ok(RawSource {
            path: file_path,
            hash,
            ingested_at: Utc::now(),
        })
    }

    /// Read a raw source by its SHA-256 hash.
    pub fn read_raw(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self
            .raw_file_path(hash)
            .ok_or_else(|| ArgosError::NotFound(format!("raw source not found: {hash}")))?;
        fs::read(&path).map_err(|e| ArgosError::Io(e.to_string()))
    }

    /// Whether a raw source with `hash` is present.
    pub fn raw_exists(&self, hash: &str) -> Result<bool> {
        Ok(self.raw_file_path(hash).is_some())
    }

    /// Locate the on-disk file for `hash` (stored as `<hash>.<ext>`).
    fn raw_file_path(&self, hash: &str) -> Option<PathBuf> {
        if !self.root.exists() {
            return None;
        }
        let entries = fs::read_dir(&self.root).ok()?;
        let prefix = format!("{hash}.");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) || name == hash {
                return Some(entry.path());
            }
        }
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn tmp_store() -> (tempfile::TempDir, RawSourceStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = RawSourceStore::new(dir.path().join("raw"));
        (dir, store)
    }

    fn expected_hash(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn store_raw_returns_hash() {
        let (_dir, store) = tmp_store();
        let data = b"# An article\n\nSome content.\n";
        let src = store
            .store_raw(data, "articles/standup.md")
            .expect("store should succeed");
        assert_eq!(src.hash, expected_hash(data), "hash must be sha256 of data");
        assert_eq!(src.hash.len(), 64);
        assert_eq!(src.path.extension().and_then(|e| e.to_str()), Some("md"));
    }

    #[test]
    fn read_raw_returns_same_data() {
        let (_dir, store) = tmp_store();
        let data = b"raw bytes here \x00\x01\x02";
        let src = store.store_raw(data, "docs/binary.bin").unwrap();
        let read_back = store.read_raw(&src.hash).expect("read should succeed");
        assert_eq!(read_back, data);
    }

    #[test]
    fn raw_exists_true_after_store_false_before() {
        let (_dir, store) = tmp_store();
        let data = b"some content";
        let hash = expected_hash(data);
        assert!(
            !store.raw_exists(&hash).unwrap(),
            "must not exist before store"
        );
        let src = store.store_raw(data, "x.md").unwrap();
        assert!(
            store.raw_exists(&src.hash).unwrap(),
            "must exist after store"
        );
    }

    #[test]
    fn store_raw_is_idempotent_same_hash_no_duplicate() {
        let (_dir, store) = tmp_store();
        let data = b"identical content";
        let a = store.store_raw(data, "a.md").unwrap();
        let b = store.store_raw(data, "b.md").unwrap();
        assert_eq!(a.hash, b.hash, "identical bytes => identical hash");
        // Only one file should exist for that hash.
        let mut count = 0;
        for entry in fs::read_dir(&store.root).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{}.", a.hash))
            {
                count += 1;
            }
        }
        assert_eq!(count, 1, "idempotent store must not duplicate files");
    }

    #[test]
    fn read_raw_errors_on_missing_hash() {
        let (_dir, store) = tmp_store();
        let res =
            store.read_raw("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        assert!(res.is_err(), "missing raw source must error");
    }
}
