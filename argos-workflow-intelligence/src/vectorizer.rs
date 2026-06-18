//! Vectorize workflow concepts and index on creation (T-024, ADR-008).
//!
//! [`WorkflowVectorizer`] embeds each `type=workflow` OKF concept (intent-first:
//! title + description + body) and upserts it into the [`VectorStore`] with
//! `VectorMetadata { concept_type: Workflow }`. Non-workflow concepts are
//! skipped silently — the vector index is filtered to workflows so similarity
//! search for "similar workflow" never mixes in runbooks/entities.
//!
//! Indexing happens on concept creation: after an n8n workflow is imported (or
//! authored) as an OKF concept, `vectorize` is called once to land its intent
//! embedding in `vec_concepts`. Re-embedding the same concept path is an upsert
//! (replace, not duplicate) — idempotent re-imports and `argos reindex` rely on
//! this.

use argos_core::{Concept, ConceptType, Result};
use argos_knowledge::BundleStore;
use argos_provider::Provider;
use argos_storage::traits::{VectorMetadata, VectorStore};

/// Embeds `type=workflow` OKF concepts into a [`VectorStore`].
///
/// Generic over the embedder `P: Provider` and the index backend `V:
/// VectorStore` so the same code works under tests (stub provider + in-memory
/// store) and in production (Ollama + sqlite-vec / Qdrant).
pub struct WorkflowVectorizer<'a, P: Provider, V: VectorStore> {
    provider: &'a P,
    vectors: &'a V,
}

impl<'a, P: Provider, V: VectorStore> WorkflowVectorizer<'a, P, V> {
    /// Create a vectorizer that embeds via `provider` and indexes into `vectors`.
    pub fn new(provider: &'a P, vectors: &'a V) -> Self {
        Self { provider, vectors }
    }

    /// Embed `concept` (intent-first) and upsert it into the vector store.
    ///
    /// Non-workflow concepts are skipped silently (no error, no store write):
    /// the workflow-intelligence index only carries `type=workflow` rows so
    /// filtered similarity search stays scoped to workflows.
    pub async fn vectorize(&self, concept: &Concept) -> Result<()> {
        if concept.frontmatter.concept_type != ConceptType::Workflow {
            return Ok(());
        }
        let text = concept_to_text(concept);
        let embedding = self.provider.embed(&text).await?;
        let metadata = VectorMetadata {
            concept_type: concept.frontmatter.concept_type.clone(),
        };
        self.vectors
            .upsert(&concept.path, &embedding, &metadata)
            .await
    }

    /// Read every concept from `bundle`, vectorize the `type=workflow` ones, and
    /// return how many were indexed. Non-workflow concepts are skipped.
    pub async fn vectorize_all(&self, bundle: &BundleStore) -> Result<usize> {
        let bundle_data = bundle.read_bundle()?;
        let mut count = 0;
        for concept in &bundle_data.concepts {
            if concept.frontmatter.concept_type == ConceptType::Workflow {
                self.vectorize(concept).await?;
                count += 1;
            }
        }
        Ok(count)
    }
}

/// Convert a concept to its embedding text (intent-first).
///
/// Title carries the most signal, then the frontmatter description, then the
/// body — the convention used by `ReindexOperation::concept_to_text` in
/// argos-knowledge, so the fresh-index path and the reindexer produce the same
/// vector for the same concept. A free function (not an associated function on
/// the generic struct) so callers do not need to name `P`/`V` to serialise a
/// concept.
pub fn concept_to_text(concept: &Concept) -> String {
    let mut text = concept.frontmatter.title.clone();
    if let Some(desc) = &concept.frontmatter.description {
        text.push('\n');
        text.push_str(desc);
    }
    text.push('\n');
    text.push_str(&concept.body);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use argos_core::{ConceptPath, Frontmatter};
    use argos_storage::InMemoryVectorStore;
    use chrono::Utc;
    use tempfile::tempdir;

    fn ts() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Build a workflow concept in-memory (no filesystem).
    fn workflow_concept(
        path: &str,
        title: &str,
        desc: Option<&str>,
        body: &str,
        resource: Option<&str>,
    ) -> Concept {
        Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: title.into(),
                timestamp: ts(),
                description: desc.map(String::from),
                resource: resource.map(String::from),
                tags: None,
                relates_to: None,
            },
            body: body.into(),
        }
    }

    /// Build a non-workflow concept (e.g. a runbook) to prove the skip path.
    fn non_workflow_concept(path: &str, title: &str, body: &str) -> Concept {
        Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Runbook,
                title: title.into(),
                timestamp: ts(),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: body.into(),
        }
    }

    #[test]
    fn vectorizer_constructs() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        // Construction is enough; the vectorizer borrows the provider and store.
        let _ = vectorizer;
    }

    #[tokio::test]
    async fn vectorize_embeds_type_workflow() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        let concept = workflow_concept(
            "workflows/daily.md",
            "Daily Report",
            Some("Sends a daily email summary"),
            "Body line",
            Some("n8n://workflows/1"),
        );
        vectorizer.vectorize(&concept).await.unwrap();
        // One workflow vector was stored.
        assert_eq!(vectors.count().await.unwrap(), 1);
        // Searching with the concept's own intent text finds it as the top hit.
        let q = provider.embed(&concept_to_text(&concept)).await.unwrap();
        let hits = vectors
            .search(&q, 5, Some(ConceptType::Workflow))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept_path, concept.path);
        assert!((hits[0].score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn vectorize_skips_non_workflow_silently() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        let concept = non_workflow_concept("runbooks/oncall.md", "OnCall", "body");
        // Non-workflow concepts are skipped — no error, no store write.
        vectorizer.vectorize(&concept).await.unwrap();
        assert_eq!(vectors.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn vectorize_all_processes_only_workflows() {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        bundle
            .write_concept(&workflow_concept(
                "workflows/a.md",
                "Daily",
                Some("d"),
                "b",
                Some("n8n://workflows/a"),
            ))
            .unwrap();
        bundle
            .write_concept(&workflow_concept(
                "workflows/b.md",
                "Weekly",
                Some("d"),
                "b",
                Some("n8n://workflows/b"),
            ))
            .unwrap();
        bundle
            .write_concept(&non_workflow_concept("runbooks/c.md", "OnCall", "b"))
            .unwrap();

        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        let count = vectorizer.vectorize_all(&bundle).await.unwrap();
        // Both workflows indexed, the runbook skipped.
        assert_eq!(count, 2);
        assert_eq!(vectors.count().await.unwrap(), 2);
        // The stored vectors are all type=workflow (the runbook never landed).
        let any = vectors.search(&vec![1.0], 10, None).await.unwrap();
        assert_eq!(any.len(), 2);
        let wf_only = vectors
            .search(&vec![1.0], 10, Some(ConceptType::Runbook))
            .await
            .unwrap();
        assert!(wf_only.is_empty(), "no runbook vectors must be present");
    }

    #[tokio::test]
    async fn vectorize_all_returns_count_of_workflows() {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        bundle
            .write_concept(&workflow_concept(
                "workflows/x.md",
                "X",
                Some("d"),
                "b",
                Some("n8n://workflows/x"),
            ))
            .unwrap();
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        let count = vectorizer.vectorize_all(&bundle).await.unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn concept_to_text_includes_title_description_body() {
        let concept = workflow_concept(
            "workflows/daily.md",
            "Daily Report Title",
            Some("Short description here"),
            "Body content",
            Some("n8n://workflows/1"),
        );
        let text = concept_to_text(&concept);
        assert!(text.contains("Daily Report Title"), "title missing: {text}");
        assert!(
            text.contains("Short description here"),
            "description missing: {text}"
        );
        assert!(text.contains("Body content"), "body missing: {text}");
    }

    #[tokio::test]
    async fn vectorize_is_idempotent_upsert_not_duplicate() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let vectorizer = WorkflowVectorizer::new(&provider, &vectors);
        let concept = workflow_concept(
            "workflows/daily.md",
            "Daily",
            Some("d"),
            "b",
            Some("n8n://workflows/1"),
        );
        vectorizer.vectorize(&concept).await.unwrap();
        vectorizer.vectorize(&concept).await.unwrap();
        // Upsert on the same path replaces, never duplicates.
        assert_eq!(vectors.count().await.unwrap(), 1);
    }
}
