//! Similarity search over workflow-concept embeddings (T-025, ADR-008).
//!
//! [`SimilaritySearch`] embeds a free-text intent (or a concept's intent text)
//! and runs a cosine search over the [`VectorStore`] filtered to
//! `type=workflow`, returning ranked [`SimilarityHit`]s. The filter means a
//! workflow-intelligence search never mixes in runbooks, entities, or sources —
//! the ranked results are always workflows the user can reuse.
//!
//! Scoring itself lives in the `VectorStore` backend (in-memory cosine for
//! slice 1; sqlite-vec / Qdrant later). This type is the thin embed + filter +
//! sort seam the recommender and the CLI `argos similar` command build on.

use argos_core::{Concept, ConceptType, Result, SimilarityHit};
use argos_provider::Provider;
use argos_storage::traits::VectorStore;

/// Cosine similarity search scoped to `type=workflow` concepts.
pub struct SimilaritySearch<'a, P: Provider, V: VectorStore> {
    provider: &'a P,
    vectors: &'a V,
}

impl<'a, P: Provider, V: VectorStore> SimilaritySearch<'a, P, V> {
    /// Create a search that embeds queries via `provider` and reads hits from
    /// `vectors`.
    pub fn new(provider: &'a P, vectors: &'a V) -> Self {
        Self { provider, vectors }
    }

    /// Embed `query` and search the vector store for the `limit` most similar
    /// `type=workflow` concepts, ranked by descending cosine similarity.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SimilarityHit>> {
        let embedding = self.provider.embed(query).await?;
        self.vectors
            .search(&embedding, limit, Some(ConceptType::Workflow))
            .await
    }

    /// Embed `concept`'s intent text and search for similar workflows. Used by
    /// the recommender's "do I already have something similar to THIS concept?"
    /// path and by duplicate-detection on import.
    pub async fn search_by_concept(
        &self,
        concept: &Concept,
        limit: usize,
    ) -> Result<Vec<SimilarityHit>> {
        let text = crate::concept_to_text(concept);
        let embedding = self.provider.embed(&text).await?;
        self.vectors
            .search(&embedding, limit, Some(ConceptType::Workflow))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concept_to_text;
    use crate::test_support::StubProvider;
    use argos_core::{ConceptPath, Frontmatter};
    use argos_storage::traits::{VectorMetadata, VectorStore};
    use argos_storage::InMemoryVectorStore;
    use chrono::Utc;

    fn ts() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

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

    /// Index a `type=workflow` vector at `path` whose embedding is the provider's
    /// embedding of `text`. Decouples similarity tests from the vectorizer so
    /// the search seam is exercised in isolation.
    async fn index_workflow(
        vectors: &InMemoryVectorStore,
        provider: &StubProvider,
        path: &str,
        text: &str,
    ) {
        let emb = provider.embed(text).await.unwrap();
        vectors
            .upsert(
                &ConceptPath::new(path),
                &emb,
                &VectorMetadata {
                    concept_type: ConceptType::Workflow,
                },
            )
            .await
            .unwrap();
    }

    #[test]
    fn similarity_search_constructs() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let search = SimilaritySearch::new(&provider, &vectors);
        let _ = search;
    }

    #[tokio::test]
    async fn empty_store_returns_empty() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let search = SimilaritySearch::new(&provider, &vectors);
        let hits = search.search("anything", 5).await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn returns_sorted_by_score() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        index_workflow(
            &vectors,
            &provider,
            "workflows/alpha.md",
            "alpha workflow text",
        )
        .await;
        index_workflow(
            &vectors,
            &provider,
            "workflows/beta.md",
            "beta workflow text",
        )
        .await;
        index_workflow(
            &vectors,
            &provider,
            "workflows/gamma.md",
            "gamma workflow text",
        )
        .await;

        let search = SimilaritySearch::new(&provider, &vectors);
        // Query identical to the alpha concept's indexed text -> top hit, score 1.0.
        let hits = search.search("alpha workflow text", 10).await.unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].concept_path, ConceptPath::new("workflows/alpha.md"));
        assert!(
            (hits[0].score - 1.0).abs() < 1e-6,
            "exact match must score 1.0, got {}",
            hits[0].score
        );
        // Ranked by descending score.
        assert!(hits[0].score >= hits[1].score);
        assert!(hits[1].score >= hits[2].score);
    }

    #[tokio::test]
    async fn filters_to_type_workflow() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        // One workflow...
        index_workflow(&vectors, &provider, "workflows/daily.md", "daily report").await;
        // ...and one entity indexed directly with a non-workflow type. The
        // vectorizer would skip it, so we upsert by hand to prove the search
        // filter excludes it.
        let entity_emb = provider.embed("team entity").await.unwrap();
        vectors
            .upsert(
                &ConceptPath::new("entities/team.md"),
                &entity_emb,
                &VectorMetadata {
                    concept_type: ConceptType::Entity,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            vectors.count().await.unwrap(),
            2,
            "precondition: both vectors must be present"
        );

        let search = SimilaritySearch::new(&provider, &vectors);
        let hits = search.search("daily report", 10).await.unwrap();
        // Only the workflow survives the type filter.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept_path, ConceptPath::new("workflows/daily.md"));
    }

    #[tokio::test]
    async fn respects_limit() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        for i in 0..5u32 {
            index_workflow(
                &vectors,
                &provider,
                &format!("workflows/w{i}.md"),
                &format!("workflow number {i}"),
            )
            .await;
        }
        assert_eq!(vectors.count().await.unwrap(), 5);

        let search = SimilaritySearch::new(&provider, &vectors);
        let hits = search.search("workflow", 3).await.unwrap();
        assert_eq!(hits.len(), 3, "limit must truncate the ranked list to 3");
    }

    #[tokio::test]
    async fn search_by_concept_finds_similar() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let wf = workflow_concept(
            "workflows/standup.md",
            "Daily Standup",
            Some("team sync summary"),
            "body content",
            Some("n8n://workflows/1"),
        );
        // Index the concept with its own intent text, then search by the same
        // concept -> it must surface itself as the exact-match top hit.
        let text = concept_to_text(&wf);
        index_workflow(&vectors, &provider, "workflows/standup.md", &text).await;

        let search = SimilaritySearch::new(&provider, &vectors);
        let hits = search.search_by_concept(&wf, 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept_path, wf.path);
        assert!(
            (hits[0].score - 1.0).abs() < 1e-6,
            "exact concept match must score 1.0, got {}",
            hits[0].score
        );
    }
}
