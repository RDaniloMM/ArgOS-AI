//! VectorStore implementation.
//!
//! Slice-1 uses an in-memory cosine-similarity store (`InMemoryVectorStore`).
//! The `sqlite-vec` native extension was attempted first but failed to load on
//! the Windows GNU toolchain (`no such module: vec0` after auto-extension
//! registration), so the pure-Rust fallback is used. The [`VectorStore`] trait
//! isolates this decision: callers are unchanged and a `sqlite-vec`-backed
//! implementation can replace this one later without touching consumers.

use std::collections::HashMap;
use std::sync::Mutex;

use argos_core::{ConceptPath, ConceptType, Embedding, Result, SimilarityHit};
use async_trait::async_trait;

use crate::traits::{VectorMetadata, VectorStore};

/// In-memory [`VectorStore`] backed by a `HashMap` and pure-Rust cosine
/// similarity.
///
/// Correct and sufficient for slice-1 scale (~100–1000 workflows). The
/// [`VectorStore`] trait means a `sqlite-vec`/Qdrant backend can replace this
/// implementation without changing any caller.
pub struct InMemoryVectorStore {
    rows: Mutex<HashMap<String, (Embedding, VectorMetadata)>>,
}

impl InMemoryVectorStore {
    /// Create an empty in-memory vector store.
    pub fn new() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryVectorStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn upsert(
        &self,
        path: &ConceptPath,
        embedding: &Embedding,
        metadata: &VectorMetadata,
    ) -> Result<()> {
        self.rows
            .lock()
            .map_err(|e| argos_core::ArgosError::Storage(e.to_string()))?
            .insert(path.to_string(), (embedding.clone(), metadata.clone()));
        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        filter_type: Option<ConceptType>,
    ) -> Result<Vec<SimilarityHit>> {
        let rows = self
            .rows
            .lock()
            .map_err(|e| argos_core::ArgosError::Storage(e.to_string()))?;
        let mut hits: Vec<SimilarityHit> = rows
            .iter()
            .filter(|(_, (_, meta))| filter_type.as_ref().is_none_or(|t| t == &meta.concept_type))
            .map(|(path, (emb, _))| SimilarityHit {
                concept_path: ConceptPath::new(path),
                score: cosine(query_embedding, emb),
            })
            .collect();
        // Highest similarity first; ties broken deterministically by path.
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.concept_path.to_string().cmp(&b.concept_path.to_string()))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    async fn delete(&self, path: &ConceptPath) -> Result<()> {
        self.rows
            .lock()
            .map_err(|e| argos_core::ArgosError::Storage(e.to_string()))?
            .remove(&path.to_string());
        Ok(())
    }

    async fn count(&self) -> Result<usize> {
        Ok(self
            .rows
            .lock()
            .map_err(|e| argos_core::ArgosError::Storage(e.to_string()))?
            .len())
    }
}

/// Cosine similarity between two vectors, in `f64`. Returns `0.0` when either
/// vector has zero magnitude (avoids division by zero).
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

#[cfg(test)]
mod tests {
    use super::InMemoryVectorStore;
    use crate::traits::{VectorMetadata, VectorStore};
    use argos_core::{ConceptPath, ConceptType};

    fn make_store() -> InMemoryVectorStore {
        InMemoryVectorStore::new()
    }

    #[tokio::test]
    async fn upsert_then_search_returns_inserted_item() {
        let store = make_store();
        store
            .upsert(
                &ConceptPath::new("workflows/daily.md"),
                &vec![1.0, 0.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();

        let hits = store.search(&vec![1.0, 0.0, 0.0], 10, None).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept_path, ConceptPath::new("workflows/daily.md"));
        // Identical vectors → cosine similarity of 1.0.
        assert!((hits[0].score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn search_with_limit_returns_at_most_n() {
        let store = make_store();
        for i in 0..5u32 {
            let emb = vec![i as f32, 0.0, 0.0];
            store
                .upsert(
                    &ConceptPath::new(format!("workflows/w{i}.md")),
                    &emb,
                    &VectorMetadata {
                        concept_type: ConceptType::Workflow,
                    },
                )
                .await
                .unwrap();
        }
        let hits = store.search(&vec![1.0, 0.0, 0.0], 3, None).await.unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[tokio::test]
    async fn search_with_type_filter_returns_only_matching_type() {
        let store = make_store();
        store
            .upsert(
                &ConceptPath::new("workflows/daily.md"),
                &vec![1.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();
        store
            .upsert(
                &ConceptPath::new("entities/team.md"),
                &vec![1.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Entity,
                },
            )
            .await
            .unwrap();

        let wf = store
            .search(&vec![1.0, 0.0], 10, Some(ConceptType::Workflow))
            .await
            .unwrap();
        assert_eq!(wf.len(), 1);
        assert_eq!(wf[0].concept_path, ConceptPath::new("workflows/daily.md"));

        let all = store.search(&vec![1.0, 0.0], 10, None).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn search_on_empty_store_returns_empty() {
        let store = make_store();
        let hits = store.search(&vec![1.0, 2.0, 3.0], 5, None).await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_vector() {
        let store = make_store();
        let path = ConceptPath::new("workflows/daily.md");
        store
            .upsert(
                &path,
                &vec![1.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
        store.delete(&path).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
        let hits = store.search(&vec![1.0, 0.0], 10, None).await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn count_returns_number_of_vectors() {
        let store = make_store();
        assert_eq!(store.count().await.unwrap(), 0);
        store
            .upsert(
                &ConceptPath::new("a.md"),
                &vec![1.0],
                &VectorMetadata {
                    concept_type: ConceptType::Concept,
                },
            )
            .await
            .unwrap();
        store
            .upsert(
                &ConceptPath::new("b.md"),
                &vec![1.0],
                &VectorMetadata {
                    concept_type: ConceptType::Concept,
                },
            )
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn upsert_twice_on_same_path_updates_not_duplicates() {
        let store = make_store();
        let path = ConceptPath::new("workflows/daily.md");
        store
            .upsert(
                &path,
                &vec![1.0, 0.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();
        store
            .upsert(
                &path,
                &vec![0.0, 1.0, 0.0],
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();
        // One row, holding the latest embedding.
        assert_eq!(store.count().await.unwrap(), 1);
        let hits = store.search(&vec![0.0, 1.0, 0.0], 10, None).await.unwrap();
        assert_eq!(hits.len(), 1);
        // The updated embedding now scores 1.0 against the matching query.
        assert!((hits[0].score - 1.0).abs() < 1e-6);
    }
}
