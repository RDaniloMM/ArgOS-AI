//! ArgOS workflow intelligence.
//!
//! Intent-first vectorization of OKF workflow-concepts, cosine similarity search,
//! and threshold-gated reuse recommendation over n8n workflows stored as knowledge
//! (ADR-008). This crate is ArgOS's differentiator: "do I already have a
//! workflow that does this?" The OKF wiki under `.argos/wiki/` is the source of
//! truth; the vector index is derived state, rebuilt via `argos reindex`.
//!
//! Slice 1 is intent-first only — typed cross-links (extends/supersedes/
//! contradicts/supports) are stored and surfaced but NOT blended into similarity
//! scoring. Phase 2 adds the structure + I/O + relations blend.

#![warn(missing_docs)]

pub mod crosslinks;
pub mod recommend;
pub mod reindexer;
pub mod similarity;
pub mod vectorizer;

pub use crosslinks::CrossLinkSurfacer;
pub use recommend::{extract_workflow_ref, ReuseRecommendation, ReuseRecommender};
pub use reindexer::WorkflowReindexer;
pub use similarity::SimilaritySearch;
pub use vectorizer::{concept_to_text, WorkflowVectorizer};

#[cfg(test)]
mod test_support;
