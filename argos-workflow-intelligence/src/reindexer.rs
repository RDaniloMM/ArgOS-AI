//! Workflow reindexer — rebuilds workflow vector embeddings from OKF concepts.
//!
//! Unlike `argos_knowledge::ReindexOperation` (which reindexes ALL concepts),
//! this reindexes only `type=workflow` concepts. Used when the embedder model
//! changes or after manual wiki edits.

use crate::vectorizer::concept_to_text;
use argos_core::{
    okf::{Concept, ConceptType},
    Result,
};
use argos_knowledge::{BundleStore, ReindexResult};
use argos_provider::Provider;
use argos_storage::traits::{VectorMetadata, VectorStore};

/// Reindexes only workflow concept vectors.
pub struct WorkflowReindexer<'a, P: Provider, V: VectorStore> {
    provider: &'a P,
    vectors: &'a V,
    bundle: &'a BundleStore,
}

impl<'a, P: Provider, V: VectorStore> WorkflowReindexer<'a, P, V> {
    pub fn new(provider: &'a P, vectors: &'a V, bundle: &'a BundleStore) -> Self {
        Self {
            provider,
            vectors,
            bundle,
        }
    }

    /// Re-embed all workflow concepts and upsert into the vector store.
    ///
    /// Reads the bundle, filters to `type=workflow` concepts, embeds each
    /// (title + description + body, intent-first), and upserts into the
    /// VectorStore. Non-workflow concepts are skipped. Idempotent — upsert
    /// replaces, not duplicates.
    pub async fn reindex_workflows(&self) -> Result<ReindexResult> {
        let bundle_data = self.bundle.read_bundle()?;
        let mut reindexed = 0usize;
        let mut skipped = 0usize;
        let mut paths = Vec::new();

        for concept in &bundle_data.concepts {
            if concept.frontmatter.concept_type != ConceptType::Workflow {
                skipped += 1;
                continue;
            }
            match self.embed_and_upsert(concept).await {
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

    /// Embed a single workflow concept and upsert into the vector store.
    async fn embed_and_upsert(&self, concept: &Concept) -> Result<()> {
        let text = concept_to_text(concept);
        if text.trim().is_empty() {
            return Err(argos_core::ArgosError::Knowledge(
                "empty workflow concept text".to_string(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use argos_core::okf::{Concept, ConceptPath, Frontmatter};
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

    fn write_workflow(bundle: &BundleStore, path: &str, title: &str, body: &str) {
        let concept = Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
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

    fn write_concept(bundle: &BundleStore, path: &str, title: &str) {
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
            body: format!("# {title}\n"),
        };
        bundle.write_concept(&concept).unwrap();
    }

    #[tokio::test]
    async fn reindexer_constructs() {
        let (_dir, bundle, provider, vectors) = setup();
        let _r = WorkflowReindexer::new(&provider, &vectors, &bundle);
    }

    #[tokio::test]
    async fn reindex_workflows_re_embeds_all_workflows() {
        let (_dir, bundle, provider, vectors) = setup();
        write_workflow(&bundle, "wf-a.md", "Workflow A", "Body A");
        write_workflow(&bundle, "wf-b.md", "Workflow B", "Body B");

        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        let result = reindexer.reindex_workflows().await.unwrap();
        assert_eq!(result.reindexed, 2);
        assert_eq!(result.skipped, 0);
    }

    #[tokio::test]
    async fn reindex_workflows_skips_non_workflow_concepts() {
        let (_dir, bundle, provider, vectors) = setup();
        write_workflow(&bundle, "wf-a.md", "Workflow A", "Body A");
        write_concept(&bundle, "concept.md", "A concept");

        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        let result = reindexer.reindex_workflows().await.unwrap();
        assert_eq!(result.reindexed, 1);
        assert_eq!(result.skipped, 1);
    }

    #[tokio::test]
    async fn reindex_workflows_returns_paths() {
        let (_dir, bundle, provider, vectors) = setup();
        write_workflow(&bundle, "wf-a.md", "Workflow A", "Body A");

        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        let result = reindexer.reindex_workflows().await.unwrap();
        assert!(result.paths.contains(&ConceptPath::new("wf-a.md")));
    }

    #[tokio::test]
    async fn reindex_workflows_is_idempotent() {
        let (_dir, bundle, provider, vectors) = setup();
        write_workflow(&bundle, "wf-a.md", "Workflow A", "Body A");

        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        reindexer.reindex_workflows().await.unwrap();
        reindexer.reindex_workflows().await.unwrap();

        let count = vectors.count().await.unwrap();
        assert_eq!(count, 1, "reindexing twice should not duplicate");
    }

    #[tokio::test]
    async fn reindex_workflows_on_empty_bundle_returns_zero() {
        let (_dir, bundle, provider, vectors) = setup();
        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        let result = reindexer.reindex_workflows().await.unwrap();
        assert_eq!(result.reindexed, 0);
        assert_eq!(result.skipped, 0);
    }

    #[tokio::test]
    async fn reindex_workflows_upserts_into_vector_store() {
        let (_dir, bundle, provider, vectors) = setup();
        write_workflow(&bundle, "wf-a.md", "Workflow A", "Body A");

        let reindexer = WorkflowReindexer::new(&provider, &vectors, &bundle);
        reindexer.reindex_workflows().await.unwrap();

        let count = vectors.count().await.unwrap();
        assert_eq!(count, 1);
    }
}
