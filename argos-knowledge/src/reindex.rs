//! Reindex operation — rebuild all vector embeddings from the OKF wiki.
//!
//! The wiki under `.argos/wiki/` is the source of truth (ADR-002). Vector
//! embeddings in the `VectorStore` are derived state — they can be deleted
//! and rebuilt at any time. This operation walks every concept in the
//! bundle, computes a fresh embedding via the `Provider`, and upserts it
//! into the `VectorStore`.
//!
//! Use cases:
//! - After switching the embedding model (dimension lock requires reindex).
//! - After manual wiki edits (concepts added/modified outside ArgOS).
//! - After deleting the vector index file to reclaim space.

use crate::bundle::BundleStore;
use argos_core::{okf::Concept, ConceptPath, Result};
use argos_provider::Provider;
use argos_storage::traits::{VectorMetadata, VectorStore};

/// The outcome of a reindex operation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReindexResult {
    /// Number of concepts re-embedded.
    pub reindexed: usize,
    /// Number of concepts skipped (embed failed or empty body).
    pub skipped: usize,
    /// Concept paths that were successfully re-embedded.
    pub paths: Vec<ConceptPath>,
}

/// The reindex operation: rebuild all vectors from the wiki.
pub struct ReindexOperation<'a, P: Provider, V: VectorStore> {
    provider: &'a P,
    bundle: &'a BundleStore,
    vectors: &'a V,
}

impl<'a, P: Provider, V: VectorStore> ReindexOperation<'a, P, V> {
    pub fn new(provider: &'a P, bundle: &'a BundleStore, vectors: &'a V) -> Self {
        Self {
            provider,
            bundle,
            vectors,
        }
    }

    /// Rebuild all vector embeddings from the wiki.
    ///
    /// Walks every concept, embeds its title + description + body (intent-first),
    /// and upserts into the vector store. Concepts that fail to embed are
    /// skipped (counted but not fatal).
    pub async fn reindex(&self) -> Result<ReindexResult> {
        let bundle_data = self.bundle.read_bundle()?;
        let mut reindexed = 0usize;
        let mut skipped = 0usize;
        let mut paths = Vec::new();

        for concept in &bundle_data.concepts {
            match self.embed_concept(concept).await {
                Ok(()) => {
                    reindexed += 1;
                    paths.push(concept.path.clone());
                }
                Err(_) => {
                    skipped += 1;
                }
            }
        }

        Ok(ReindexResult {
            reindexed,
            skipped,
            paths,
        })
    }

    /// Embed a single concept and upsert it into the vector store.
    ///
    /// The embedding text is intent-first: title + description + body.
    async fn embed_concept(&self, concept: &Concept) -> Result<()> {
        let text = self.concept_to_text(concept);
        if text.trim().is_empty() {
            return Err(argos_core::ArgosError::Knowledge(
                "empty concept text".to_string(),
            ));
        }
        let embedding = self.provider.embed(&text).await?;
        let metadata = VectorMetadata {
            concept_type: concept.frontmatter.concept_type.clone(),
        };
        self.vectors
            .upsert(&concept.path, &embedding, &metadata)
            .await
    }

    /// Convert a concept to embedding text (intent-first).
    ///
    /// Title carries the most signal, then description, then body.
    fn concept_to_text(&self, concept: &Concept) -> String {
        let mut text = concept.frontmatter.title.clone();
        if let Some(desc) = &concept.frontmatter.description {
            text.push('\n');
            text.push_str(desc);
        }
        text.push('\n');
        text.push_str(&concept.body);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use argos_core::okf::{Concept, ConceptType, Frontmatter};
    use argos_storage::InMemoryVectorStore;
    use chrono::Utc;
    use tempfile::tempdir;

    fn setup() -> (
        tempfile::TempDir,
        BundleStore,
        StubProvider,
        InMemoryVectorStore,
    ) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        let provider = StubProvider::new("completion");
        let vectors = InMemoryVectorStore::new();
        (dir, bundle, provider, vectors)
    }

    fn write_concept(bundle: &BundleStore, path: &str, title: &str, body: &str) {
        let concept = Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Concept,
                title: title.to_string(),
                timestamp: Utc::now(),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: body.to_string(),
        };
        bundle.write_concept(&concept).unwrap();
    }

    #[tokio::test]
    async fn reindex_empty_bundle_returns_zero() {
        let (_dir, bundle, provider, vectors) = setup();
        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        let result = op.reindex().await.unwrap();
        assert_eq!(result.reindexed, 0);
        assert_eq!(result.skipped, 0);
    }

    #[tokio::test]
    async fn reindex_embeds_all_concepts() {
        let (_dir, bundle, provider, vectors) = setup();
        write_concept(&bundle, "a.md", "Alpha", "Body A");
        write_concept(&bundle, "b.md", "Beta", "Body B");
        write_concept(&bundle, "c.md", "Gamma", "Body C");

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        let result = op.reindex().await.unwrap();
        assert_eq!(result.reindexed, 3);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.paths.len(), 3);
    }

    #[tokio::test]
    async fn reindex_upserts_into_vector_store() {
        let (_dir, bundle, provider, vectors) = setup();
        write_concept(&bundle, "a.md", "Alpha", "Body A");

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        op.reindex().await.unwrap();

        let count = vectors.count().await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn reindex_is_idempotent() {
        let (_dir, bundle, provider, vectors) = setup();
        write_concept(&bundle, "a.md", "Alpha", "Body A");

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        op.reindex().await.unwrap();
        op.reindex().await.unwrap();

        let count = vectors.count().await.unwrap();
        assert_eq!(count, 1, "reindexing twice should not duplicate vectors");
    }

    #[tokio::test]
    async fn reindex_returns_paths_of_reindexed_concepts() {
        let (_dir, bundle, provider, vectors) = setup();
        write_concept(&bundle, "alpha.md", "Alpha", "Body");
        write_concept(&bundle, "beta.md", "Beta", "Body");

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        let result = op.reindex().await.unwrap();
        assert!(result.paths.contains(&ConceptPath::new("alpha.md")));
        assert!(result.paths.contains(&ConceptPath::new("beta.md")));
    }

    #[tokio::test]
    async fn reindex_concept_to_text_includes_title_and_body() {
        let (_dir, bundle, provider, vectors) = setup();
        write_concept(&bundle, "a.md", "My Title", "My body text");

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        let concept = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let text = op.concept_to_text(&concept);
        assert!(text.contains("My Title"));
        assert!(text.contains("My body text"));
    }

    #[tokio::test]
    async fn reindex_concept_to_text_includes_description() {
        let (_dir, bundle, provider, vectors) = setup();
        let concept = Concept {
            path: ConceptPath::new("a.md"),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: "Daily Report".to_string(),
                timestamp: Utc::now(),
                description: Some("Sends a daily email summary".to_string()),
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: "Workflow body".to_string(),
        };
        bundle.write_concept(&concept).unwrap();

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        let c = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let text = op.concept_to_text(&c);
        assert!(text.contains("Daily Report"));
        assert!(text.contains("Sends a daily email summary"));
        assert!(text.contains("Workflow body"));
    }

    #[tokio::test]
    async fn reindex_preserves_concept_type_metadata() {
        let (_dir, bundle, provider, vectors) = setup();
        let concept = Concept {
            path: ConceptPath::new("wf.md"),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: "Workflow".to_string(),
                timestamp: Utc::now(),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: "Body".to_string(),
        };
        bundle.write_concept(&concept).unwrap();

        let op = ReindexOperation::new(&provider, &bundle, &vectors);
        op.reindex().await.unwrap();

        // Search with type filter should find it.
        let embedding = provider.embed("workflow").await.unwrap();
        let hits = vectors
            .search(&embedding, 10, Some(ConceptType::Workflow))
            .await
            .unwrap();
        assert!(!hits.is_empty());
    }

    #[test]
    fn reindex_result_constructs() {
        let r = ReindexResult {
            reindexed: 5,
            skipped: 1,
            paths: vec![ConceptPath::new("a.md")],
        };
        assert_eq!(r.reindexed, 5);
        assert_eq!(r.skipped, 1);
    }
}
