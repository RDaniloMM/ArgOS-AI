//! Storage trait definitions.
//!
//! Backend-agnostic seams over the derived indexes (ADR-002). The OKF wiki under
//! `.argos/wiki/` is the source of truth; these traits cover only regenerable
//! derived state. The Solo profile backs them with SQLite + sqlite-vec + the
//! filesystem; the Team profile swaps in Postgres + Qdrant + S3. No
//! embedded-specific SQL or extension leaks through this seam.

use argos_core::{ConceptPath, ConceptType, Embedding, Result, SimilarityHit};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Metadata attached to a vector embedding in the vector store.
///
/// Carries the concept type so similarity searches can be filtered (e.g. only
/// `type=workflow` for workflow intelligence). The struct is intentionally
/// small and extensible so backends can persist extra columns without breaking
/// the trait contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorMetadata {
    /// The concept type associated with the embedded concept.
    pub concept_type: ConceptType,
}

/// Backend-agnostic relational store for derived supporting state.
///
/// Holds audit log, metadata, n8n refs, concept index, permissions, provider
/// configs, and episodic memory. The trait exposes simple key/value row
/// operations and MUST NOT leak embedded-specific SQL — Solo (SQLite) and Team
/// (Postgres) backends implement it identically.
#[async_trait]
pub trait RelationalStore: Send + Sync {
    /// Store `value` under `key`, replacing any existing value.
    async fn put(&self, key: &str, value: &str) -> Result<()>;
    /// Read the value stored under `key`, if present.
    async fn get(&self, key: &str) -> Result<Option<String>>;
    /// Remove the value stored under `key`. No-op if absent.
    async fn delete(&self, key: &str) -> Result<()>;
    /// Return all rows whose key starts with `prefix`, ordered by key.
    async fn query(&self, prefix: &str) -> Result<Vec<(String, String)>>;
}

/// Backend-agnostic vector store for concept embeddings and similarity search.
///
/// Solo uses sqlite-vec; Team uses Qdrant. This trait is the single seam that
/// isolates the pre-v1 sqlite-vec dependency (ADR-002 / ADR-008). The embedder
/// dimension is locked at init; switching embedders requires `argos reindex`.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Insert or replace the embedding for `path` with attached metadata.
    async fn upsert(
        &self,
        path: &ConceptPath,
        embedding: &Embedding,
        metadata: &VectorMetadata,
    ) -> Result<()>;
    /// Cosine similarity search. `filter_type` restricts hits to a concept type
    /// (e.g. `Some(ConceptType::Workflow)` for workflow intelligence).
    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        filter_type: Option<ConceptType>,
    ) -> Result<Vec<SimilarityHit>>;
    /// Remove the embedding for `path`. No-op if absent.
    async fn delete(&self, path: &ConceptPath) -> Result<()>;
    /// Number of embeddings currently stored.
    async fn count(&self) -> Result<usize>;
}

/// Backend-agnostic content-addressed blob store.
///
/// Solo uses a sha256-sharded filesystem under `.argos/objects/`; Team uses S3.
/// `store` is idempotent: storing identical bytes yields the same hash.
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Store `data` and return its content hash (SHA-256 hex).
    async fn store(&self, data: &[u8]) -> Result<String>;
    /// Retrieve the blob identified by `hash`. Errors if absent.
    async fn retrieve(&self, hash: &str) -> Result<Vec<u8>>;
    /// Returns `true` if a blob with `hash` is present.
    async fn exists(&self, hash: &str) -> Result<bool>;
}

/// Composite marker: a full storage backend implements all three stores.
///
/// Implementors get `Storage` for free via the blanket impl; consumers can
/// depend on the single composite trait when they need relational + vector +
/// blob access behind one boundary.
pub trait Storage: RelationalStore + VectorStore + BlobStore {}

impl<T> Storage for T where T: RelationalStore + VectorStore + BlobStore {}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{ConceptPath, ConceptType, SimilarityHit};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Cosine similarity for the in-memory vector stub.
    fn cosine(a: &[f32], b: &[f32]) -> f64 {
        let dot = a
            .iter()
            .zip(b)
            .map(|(x, y)| (*x as f64) * (*y as f64))
            .sum::<f64>();
        let na = (a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>()).sqrt();
        let nb = (b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>()).sqrt();
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb)
        }
    }

    // --- RelationalStore in-memory stub ---
    struct MemRelational {
        rows: Mutex<HashMap<String, String>>,
    }
    impl MemRelational {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl RelationalStore for MemRelational {
        async fn put(&self, key: &str, value: &str) -> argos_core::Result<()> {
            self.rows
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        async fn get(&self, key: &str) -> argos_core::Result<Option<String>> {
            Ok(self.rows.lock().unwrap().get(key).cloned())
        }
        async fn delete(&self, key: &str) -> argos_core::Result<()> {
            self.rows.lock().unwrap().remove(key);
            Ok(())
        }
        async fn query(&self, prefix: &str) -> argos_core::Result<Vec<(String, String)>> {
            let mut out: Vec<(String, String)> = self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(out)
        }
    }

    // --- VectorStore in-memory stub ---
    struct VectorRow {
        embedding: Vec<f32>,
        concept_type: ConceptType,
    }
    struct MemVector {
        rows: Mutex<HashMap<ConceptPath, VectorRow>>,
    }
    impl MemVector {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl VectorStore for MemVector {
        async fn upsert(
            &self,
            path: &ConceptPath,
            embedding: &argos_core::Embedding,
            metadata: &VectorMetadata,
        ) -> argos_core::Result<()> {
            self.rows.lock().unwrap().insert(
                path.clone(),
                VectorRow {
                    embedding: embedding.clone(),
                    concept_type: metadata.concept_type.clone(),
                },
            );
            Ok(())
        }
        async fn search(
            &self,
            query_embedding: &argos_core::Embedding,
            limit: usize,
            filter_type: Option<ConceptType>,
        ) -> argos_core::Result<Vec<SimilarityHit>> {
            let rows = self.rows.lock().unwrap();
            let mut hits: Vec<SimilarityHit> = rows
                .iter()
                .filter(|(_, r)| filter_type.as_ref().is_none_or(|t| t == &r.concept_type))
                .map(|(path, r)| SimilarityHit {
                    concept_path: path.clone(),
                    score: cosine(query_embedding, &r.embedding),
                })
                .collect();
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.truncate(limit);
            Ok(hits)
        }
        async fn delete(&self, path: &ConceptPath) -> argos_core::Result<()> {
            self.rows.lock().unwrap().remove(path);
            Ok(())
        }
        async fn count(&self) -> argos_core::Result<usize> {
            Ok(self.rows.lock().unwrap().len())
        }
    }

    // --- BlobStore in-memory stub ---
    struct MemBlob {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
    }
    impl MemBlob {
        fn new() -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
            }
        }
    }
    #[async_trait::async_trait]
    impl BlobStore for MemBlob {
        async fn store(&self, data: &[u8]) -> argos_core::Result<String> {
            // Simple deterministic hash for the stub (not cryptographic — the real
            // FsBlobStore uses SHA-256). Stable so store is idempotent.
            let hash = format!("stub-{:x}", data.len());
            self.blobs
                .lock()
                .unwrap()
                .entry(hash.clone())
                .or_insert_with(|| data.to_vec());
            Ok(hash)
        }
        async fn retrieve(&self, hash: &str) -> argos_core::Result<Vec<u8>> {
            self.blobs
                .lock()
                .unwrap()
                .get(hash)
                .cloned()
                .ok_or_else(|| argos_core::ArgosError::NotFound(format!("blob {hash}")))
        }
        async fn exists(&self, hash: &str) -> argos_core::Result<bool> {
            Ok(self.blobs.lock().unwrap().contains_key(hash))
        }
    }

    #[test]
    fn relational_store_trait_can_be_referenced() {
        // Compile-time + trait-object check: the trait exists and is dyn-usable.
        let store: &dyn RelationalStore = &MemRelational::new();
        let _ = store;
    }

    #[test]
    fn vector_store_trait_can_be_referenced() {
        let store: &dyn VectorStore = &MemVector::new();
        let _ = store;
    }

    #[test]
    fn blob_store_trait_can_be_referenced() {
        let store: &dyn BlobStore = &MemBlob::new();
        let _ = store;
    }

    #[tokio::test]
    async fn relational_stub_put_get_query_delete() {
        let store = MemRelational::new();
        store.put("n8n:1", "alpha").await.unwrap();
        store.put("n8n:2", "beta").await.unwrap();
        store.put("perm:1", "gamma").await.unwrap();

        assert_eq!(store.get("n8n:1").await.unwrap(), Some("alpha".to_string()));
        assert_eq!(store.get("missing").await.unwrap(), None);

        let n8n_rows = store.query("n8n:").await.unwrap();
        assert_eq!(n8n_rows.len(), 2);
        assert_eq!(n8n_rows[0], ("n8n:1".to_string(), "alpha".to_string()));

        store.delete("n8n:1").await.unwrap();
        assert_eq!(store.get("n8n:1").await.unwrap(), None);
        assert_eq!(store.query("n8n:").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn vector_stub_upsert_search_filter_delete_count() {
        let store = MemVector::new();
        let wf = ConceptType::Workflow;
        let entity = ConceptType::Entity;

        store
            .upsert(
                &ConceptPath::new("workflows/daily.md"),
                &vec![1.0, 0.0, 0.0],
                &VectorMetadata {
                    concept_type: wf.clone(),
                },
            )
            .await
            .unwrap();
        store
            .upsert(
                &ConceptPath::new("workflows/weekly.md"),
                &vec![0.9, 0.1, 0.0],
                &VectorMetadata {
                    concept_type: wf.clone(),
                },
            )
            .await
            .unwrap();
        store
            .upsert(
                &ConceptPath::new("entities/team.md"),
                &vec![0.0, 0.0, 1.0],
                &VectorMetadata {
                    concept_type: entity.clone(),
                },
            )
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 3);

        // Query near the first workflow — ranked, top hit is daily.md.
        let hits = store.search(&vec![1.0, 0.0, 0.0], 10, None).await.unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].concept_path, ConceptPath::new("workflows/daily.md"));
        assert!(hits[0].score > hits[1].score);

        // Filter to workflow only — excludes the entity.
        let wf_hits = store
            .search(&vec![1.0, 0.0, 0.0], 10, Some(wf.clone()))
            .await
            .unwrap();
        assert_eq!(wf_hits.len(), 2);
        assert!(wf_hits
            .iter()
            .all(|h| h.concept_path.to_string().starts_with("workflows/")));

        store
            .delete(&ConceptPath::new("workflows/daily.md"))
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn blob_stub_store_retrieve_exists() {
        let store = MemBlob::new();
        let data = b"hello argos";
        let hash = store.store(data).await.unwrap();

        assert!(store.exists(&hash).await.unwrap());
        let retrieved = store.retrieve(&hash).await.unwrap();
        assert_eq!(retrieved, data);

        // Idempotent: storing identical bytes yields the same hash.
        let hash2 = store.store(data).await.unwrap();
        assert_eq!(hash, hash2);

        // Missing blob errors.
        let missing = store.retrieve("nope").await;
        assert!(missing.is_err());
    }
}
