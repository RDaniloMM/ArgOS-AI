//! Threshold-gated reuse recommendation (T-026, ADR-008).
//!
//! [`ReuseRecommender`] answers ArgOS's central question: "do I already have a
//! workflow for this intent?" Given a free-text intent it runs a similarity
//! search over indexed workflow-concepts and, when the top hit's score meets
//! the configurable `reuse_threshold` (default 0.82), returns
//! [`ReuseRecommendation::Reuse`] carrying the best match, its [`N8nWorkflowRef`]
//! (so the agent can clone/execute via the n8n connector), and the full ranked
//! hit list; otherwise it returns [`ReuseRecommendation::Create`] and the agent
//! generates a new n8n workflow. The threshold comparison is inclusive (`>=`),
//! matching the spec's `threshold-boundary` scenario.
//!
//! Reuse is prompt-and-confirm (spec: "Reuse UX — Prompt and Confirm"): the
//! recommender only RECOMMENDS, it never clones. The caller (agent) decides
//! reuse / clone-and-modify / create-new. No side effects here.

use argos_core::{Concept, ConceptPath, N8nWorkflowRef, Result, SimilarityHit};
use argos_knowledge::BundleStore;
use argos_provider::Provider;
use argos_storage::traits::VectorStore;

use crate::similarity::SimilaritySearch;

/// Default reuse threshold (ADR-008): a top hit scoring at least this is
/// considered the same intent and reuse is recommended. Tunable via
/// `config.toml` (`threshold-lowered-increases-reuse` scenario).
pub const DEFAULT_REUSE_THRESHOLD: f64 = 0.82;

/// The workflow-intelligence recommendation returned by [`ReuseRecommender`].
///
/// The `Reuse` variant always carries the [`N8nWorkflowRef`] of the best match
/// so the agent can act on it without re-reading the concept. `best_match` and
/// `concept_path` refer to the same concept (the top hit) — `best_match` keeps
/// the score, `concept_path` is the ergonomic path accessor.
///
/// `Reuse` is larger than `Create` (it carries the ref + a second hit + the
/// path), but this enum is a per-intent *return value* — produced once per
/// `recommend` call and consumed immediately by the agent loop. It is never
/// stored in arrays or hot paths, so the size difference is not worth boxing a
/// public field and exposing `Box<N8nWorkflowRef>` to callers. Hence the
/// `#[allow]`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ReuseRecommendation {
    /// Reuse an existing workflow: the top hit met `reuse_threshold`.
    Reuse {
        /// The full ranked hit list (the best match is `hits[0]`).
        hits: Vec<SimilarityHit>,
        /// The top hit (the workflow that crossed the threshold), score included.
        best_match: SimilarityHit,
        /// The n8n workflow ref extracted from the best-match concept's
        /// `resource: n8n://workflows/<id>` frontmatter.
        workflow_ref: N8nWorkflowRef,
        /// The concept path of the best match (`== best_match.concept_path`).
        concept_path: ConceptPath,
    },
    /// Generate a new workflow: no hit met the threshold (or the store is
    /// empty), so the agent should author a fresh n8n workflow and index it.
    Create {
        /// The (possibly empty) ranked hit list that was considered.
        hits: Vec<SimilarityHit>,
    },
}

impl ReuseRecommendation {
    /// `true` when this is a [`ReuseRecommendation::Reuse`] recommendation.
    pub fn is_reuse(&self) -> bool {
        matches!(self, ReuseRecommendation::Reuse { .. })
    }

    /// `true` when this is a [`ReuseRecommendation::Create`] recommendation.
    pub fn is_create(&self) -> bool {
        matches!(self, ReuseRecommendation::Create { .. })
    }

    /// The ranked hit list that informed this recommendation (the best match is
    /// `hits[0]` for `Reuse`; the full considered list for `Create`).
    pub fn hits(&self) -> &[SimilarityHit] {
        match self {
            ReuseRecommendation::Reuse { hits, .. } => hits,
            ReuseRecommendation::Create { hits } => hits,
        }
    }
}

/// Extract the [`N8nWorkflowRef`] encoded in a workflow-concept's frontmatter.
///
/// Reads `frontmatter.resource` and parses the `n8n://workflows/<id>` scheme:
/// the `<id>` becomes `N8nWorkflowRef.id`, the concept's title becomes `name`.
/// Returns `None` when the resource is missing or does not use the
/// `n8n://workflows/` scheme (e.g. a manually-authored workflow concept with no
/// n8n backing). The editor URL lives in the concept body, not the resource, so
/// `url` is `None` in slice 1 (parsing it from the body is a future enhancement).
pub fn extract_workflow_ref(concept: &Concept) -> Option<N8nWorkflowRef> {
    let resource = concept.frontmatter.resource.as_ref()?;
    const PREFIX: &str = "n8n://workflows/";
    let id = resource.strip_prefix(PREFIX)?;
    if id.is_empty() {
        return None;
    }
    Some(N8nWorkflowRef {
        id: id.to_string(),
        name: concept.frontmatter.title.clone(),
        url: None,
    })
}

/// Threshold-gated reuse recommender over indexed workflow-concepts.
///
/// Generic over the embedder `P` and the index backend `V`; borrows a
/// [`BundleStore`] to resolve the best-match concept (for its [`N8nWorkflowRef`])
/// on a reuse decision.
pub struct ReuseRecommender<'a, P: Provider, V: VectorStore> {
    search: SimilaritySearch<'a, P, V>,
    bundle: &'a BundleStore,
    threshold: f64,
}

impl<'a, P: Provider, V: VectorStore> ReuseRecommender<'a, P, V> {
    /// Create a recommender with an explicit `threshold`. Use
    /// [`DEFAULT_REUSE_THRESHOLD`] for the ADR-008 default of 0.82.
    pub fn new(provider: &'a P, vectors: &'a V, bundle: &'a BundleStore, threshold: f64) -> Self {
        Self {
            search: SimilaritySearch::new(provider, vectors),
            bundle,
            threshold,
        }
    }

    /// Recommend reuse or create for `intent`.
    ///
    /// 1. Search the index for the top-5 similar `type=workflow` concepts.
    /// 2. If the top hit's score `>= threshold`, read its concept and extract the
    ///    [`N8nWorkflowRef`] → [`ReuseRecommendation::Reuse`] (or `Create` if the
    ///    best match has no n8n resource and so cannot be cloned). Otherwise →
    ///    [`ReuseRecommendation::Create`].
    pub async fn recommend(&self, intent: &str) -> Result<ReuseRecommendation> {
        let hits = self.search.search(intent, 5).await?;
        // `best` borrows `hits`; clone it into an owned `best_match` up front so
        // the borrow ends before `hits` is moved into either recommendation.
        if let Some(best) = hits.first().filter(|h| h.score >= self.threshold) {
            let best_match = best.clone();
            let concept = self.bundle.read_concept(&best_match.concept_path)?;
            if let Some(workflow_ref) = extract_workflow_ref(&concept) {
                let concept_path = best_match.concept_path.clone();
                return Ok(ReuseRecommendation::Reuse {
                    hits,
                    best_match,
                    workflow_ref,
                    concept_path,
                });
            }
        }
        Ok(ReuseRecommendation::Create { hits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use argos_core::{ConceptPath, ConceptType, Frontmatter};
    use argos_storage::traits::{VectorMetadata, VectorStore};
    use argos_storage::InMemoryVectorStore;
    use chrono::Utc;
    use tempfile::tempdir;

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

    /// Set up a bundle + indexed workflow whose intent text is `concept_to_text(wf)`.
    /// Returns the (bundle dir, bundle, vectors, wf) needed by the threshold tests.
    async fn setup_indexed_workflow(
        provider: &StubProvider,
    ) -> (tempfile::TempDir, BundleStore, InMemoryVectorStore, Concept) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        let vectors = InMemoryVectorStore::new();
        let wf = workflow_concept(
            "workflows/standup.md",
            "Daily Standup",
            Some("team sync summary"),
            "shares blockers and progress",
            Some("n8n://workflows/1"),
        );
        bundle.write_concept(&wf).unwrap();
        index_workflow(
            &vectors,
            provider,
            "workflows/standup.md",
            &crate::concept_to_text(&wf),
        )
        .await;
        (dir, bundle, vectors, wf)
    }

    fn bundle_of(dir: &tempfile::TempDir) -> BundleStore {
        BundleStore::new(dir.path().join("wiki"))
    }

    #[test]
    fn recommender_constructs_with_threshold() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let dir = tempdir().unwrap();
        let bundle = bundle_of(&dir);
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, 0.82);
        let _ = recommender;
    }

    #[tokio::test]
    async fn empty_store_returns_create() {
        let provider = StubProvider::new("ok");
        let vectors = InMemoryVectorStore::new();
        let dir = tempdir().unwrap();
        let bundle = bundle_of(&dir);
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, 0.82);
        let rec = recommender.recommend("anything").await.unwrap();
        assert!(rec.is_create(), "empty index must recommend Create");
        assert!(rec.hits().is_empty());
    }

    #[tokio::test]
    async fn above_threshold_returns_reuse() {
        let provider = StubProvider::new("ok");
        let (_dir, bundle, vectors, wf) = setup_indexed_workflow(&provider).await;
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, 0.82);
        // Recommend with the concept's own intent text -> top score 1.0 >= 0.82.
        let rec = recommender
            .recommend(&crate::concept_to_text(&wf))
            .await
            .unwrap();
        assert!(rec.is_reuse(), "score 1.0 >= 0.82 must recommend Reuse");
    }

    #[tokio::test]
    async fn below_threshold_returns_create() {
        let provider = StubProvider::new("ok");
        let (_dir, bundle, vectors, _wf) = setup_indexed_workflow(&provider).await;
        // Derive the real top score for a query, then set the threshold just
        // above it so the score falls below the threshold -> Create. Robust to
        // the byte-embedding's near-1.0 scores (no magic threshold numbers).
        let probe = SimilaritySearch::new(&provider, &vectors);
        let probe_hits = probe.search("team sync summary", 5).await.unwrap();
        let score = probe_hits[0].score;
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, score + 0.01);
        let rec = recommender.recommend("team sync summary").await.unwrap();
        assert!(
            rec.is_create(),
            "score {score} < threshold {} must be Create",
            score + 0.01
        );
    }

    #[tokio::test]
    async fn hits_are_sorted_by_score() {
        let provider = StubProvider::new("ok");
        let dir = tempdir().unwrap();
        let bundle = bundle_of(&dir);
        let vectors = InMemoryVectorStore::new();
        // Index three workflows AND write their concepts to the bundle. The
        // concepts carry no n8n resource so recommend falls back to Create, but
        // the hit list it returns is still the ranked search results — the
        // sorting behaviour under test.
        for (path, title, text) in [
            ("workflows/a.md", "Alpha", "alpha workflow"),
            ("workflows/b.md", "Beta", "beta workflow"),
            ("workflows/c.md", "Gamma", "gamma workflow"),
        ] {
            let c = workflow_concept(path, title, None, text, None);
            bundle.write_concept(&c).unwrap();
            index_workflow(&vectors, &provider, path, text).await;
        }

        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, 0.0);
        let rec = recommender.recommend("alpha workflow").await.unwrap();
        assert!(
            rec.is_create(),
            "no n8n resource -> Create, but hits still ranked"
        );
        let hits = rec.hits();
        assert!(
            hits.len() >= 2,
            "need >=2 hits to assert ordering, got {}",
            hits.len()
        );
        for w in hits.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "hits must be sorted by descending score, got {} then {}",
                w[0].score,
                w[1].score
            );
        }
    }

    #[test]
    fn recommendation_is_reuse_and_is_create_methods() {
        let create = ReuseRecommendation::Create { hits: vec![] };
        assert!(create.is_create());
        assert!(!create.is_reuse());
        assert!(create.hits().is_empty());

        let reuse = ReuseRecommendation::Reuse {
            hits: vec![SimilarityHit {
                concept_path: ConceptPath::new("workflows/a.md"),
                score: 0.9,
            }],
            best_match: SimilarityHit {
                concept_path: ConceptPath::new("workflows/a.md"),
                score: 0.9,
            },
            workflow_ref: N8nWorkflowRef {
                id: "1".into(),
                name: "Daily Standup".into(),
                url: None,
            },
            concept_path: ConceptPath::new("workflows/a.md"),
        };
        assert!(reuse.is_reuse());
        assert!(!reuse.is_create());
        assert_eq!(reuse.hits().len(), 1);
    }

    #[test]
    fn extract_workflow_ref_parses_n8n_uri() {
        let concept = workflow_concept(
            "workflows/daily.md",
            "Daily Report",
            Some("d"),
            "b",
            Some("n8n://workflows/42"),
        );
        let wf_ref = extract_workflow_ref(&concept).expect("n8n resource must parse");
        assert_eq!(wf_ref.id, "42");
        assert_eq!(wf_ref.name, "Daily Report");
        assert!(
            wf_ref.url.is_none(),
            "url is parsed from the body, not the resource, in slice 1"
        );
    }

    #[test]
    fn extract_workflow_ref_returns_none_for_missing_resource() {
        let concept = workflow_concept("workflows/daily.md", "X", Some("d"), "b", None);
        assert!(extract_workflow_ref(&concept).is_none());
    }

    #[test]
    fn extract_workflow_ref_returns_none_for_non_n8n_resource() {
        let concept = workflow_concept(
            "workflows/daily.md",
            "X",
            Some("d"),
            "b",
            Some("http://example.com/wf/42"),
        );
        assert!(
            extract_workflow_ref(&concept).is_none(),
            "http:// resource is not an n8n workflow ref"
        );
    }

    #[tokio::test]
    async fn exact_threshold_boundary_returns_reuse_inclusive() {
        let provider = StubProvider::new("ok");
        let (_dir, bundle, vectors, wf) = setup_indexed_workflow(&provider).await;
        // Recommend with the concept's intent text, which scores 1.0 exactly.
        let intent = crate::concept_to_text(&wf);
        let probe = SimilaritySearch::new(&provider, &vectors);
        let score = probe.search(&intent, 5).await.unwrap()[0].score;
        // Threshold exactly equal to the top score -> the >= boundary must
        // recommend Reuse (inclusive), per the spec's threshold-boundary scenario.
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, score);
        let rec = recommender.recommend(&intent).await.unwrap();
        assert!(
            rec.is_reuse(),
            "score == threshold ({score}) must be Reuse (inclusive >=)"
        );
    }

    #[tokio::test]
    async fn reuse_recommendation_contains_n8n_workflow_ref() {
        let provider = StubProvider::new("ok");
        let (_dir, bundle, vectors, wf) = setup_indexed_workflow(&provider).await;
        let recommender = ReuseRecommender::new(&provider, &vectors, &bundle, 0.82);
        let rec = recommender
            .recommend(&crate::concept_to_text(&wf))
            .await
            .unwrap();
        match rec {
            ReuseRecommendation::Reuse {
                workflow_ref,
                concept_path,
                best_match,
                ..
            } => {
                assert_eq!(
                    workflow_ref.id, "1",
                    "workflow_ref must come from the concept resource"
                );
                assert_eq!(workflow_ref.name, "Daily Standup");
                assert_eq!(concept_path, wf.path);
                assert_eq!(best_match.concept_path, wf.path);
            }
            _ => panic!("expected Reuse, got Create"),
        }
    }

    #[test]
    fn default_threshold_is_0_82() {
        assert!((DEFAULT_REUSE_THRESHOLD - 0.82).abs() < 1e-9);
    }
}
