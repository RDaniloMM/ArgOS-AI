//! Common shared types used across ArgOS crates.
//!
//! These are simple, dependency-light types that appear in multiple
//! bounded contexts: embeddings, similarity results, and timestamps.

use crate::okf::ConceptPath;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A dense vector embedding of a concept or query.
pub type Embedding = Vec<f32>;

/// A type alias for timestamps used throughout ArgOS.
pub type Timestamp = DateTime<Utc>;

/// A single similarity search result.
///
/// Returned by the workflow-intelligence subsystem when searching for
/// workflows similar to a given intent. The score is cosine similarity
/// (0.0–1.0, higher = more similar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarityHit {
    /// The concept path of the matched workflow.
    pub concept_path: ConceptPath,
    /// The cosine similarity score (0.0–1.0).
    pub score: f64,
}

impl SimilarityHit {
    /// Returns `true` if this hit meets or exceeds the reuse threshold.
    pub fn meets_threshold(&self, threshold: f64) -> bool {
        self.score >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn embedding_is_vec_f32() {
        let emb: Embedding = vec![0.1, 0.2, 0.3];
        assert_eq!(emb.len(), 3);
    }

    #[test]
    fn similarity_hit_constructs() {
        let hit = SimilarityHit {
            concept_path: ConceptPath::new("workflows/daily.md"),
            score: 0.92,
        };
        assert_eq!(hit.score, 0.92);
        assert_eq!(
            hit.concept_path,
            ConceptPath(PathBuf::from("workflows/daily.md"))
        );
    }

    #[test]
    fn similarity_hit_meets_threshold() {
        let hit = SimilarityHit {
            concept_path: ConceptPath::new("workflows/daily.md"),
            score: 0.85,
        };
        assert!(hit.meets_threshold(0.82));
    }

    #[test]
    fn similarity_hit_below_threshold() {
        let hit = SimilarityHit {
            concept_path: ConceptPath::new("workflows/daily.md"),
            score: 0.70,
        };
        assert!(!hit.meets_threshold(0.82));
    }

    #[test]
    fn similarity_hit_at_exact_threshold() {
        let hit = SimilarityHit {
            concept_path: ConceptPath::new("workflows/daily.md"),
            score: 0.82,
        };
        assert!(hit.meets_threshold(0.82));
    }
}
