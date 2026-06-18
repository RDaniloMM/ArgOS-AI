//! LLM-Wiki **lint** operation (ADR-010).
//!
//! Implements the third of Karpathy's three LLM-Wiki operations: a structural
//! health-check of the OKF bundle. Lint is purely deterministic — graph
//! traversal + existence checks, NO LLM required. This makes it fully
//! testable with seeded fixtures and safe to run on every commit.
//!
//! Checks:
//! 1. **Contradictions** — concepts with `RelationKind::Contradicts` relations.
//! 2. **Orphan pages** — concepts with no inbound cross-links.
//! 3. **Missing pages** — cross-link targets that don't exist in the bundle.
//! 4. **Missing index entries** — concepts that exist but aren't listed in `index.md`.
//! 5. **Stale sources** — `type: source` concepts older than the staleness threshold.

use crate::bundle::BundleStore;
use crate::links::{CrossLinkParser, LinkGraph, RelationManager};
use argos_core::okf::{ConceptPath, ConceptType};
use argos_core::Result;
use chrono::{Duration, Utc};

/// The staleness threshold for source concepts (default 90 days).
const DEFAULT_STALE_DAYS: i64 = 90;

/// A complete lint report for an OKF bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct LintReport {
    /// Pairs of concepts that contradict each other.
    pub contradictions: Vec<(ConceptPath, ConceptPath)>,
    /// Concepts with no inbound cross-links (potential isolation).
    pub orphans: Vec<ConceptPath>,
    /// Cross-link targets that don't exist as concept files.
    pub missing_pages: Vec<ConceptPath>,
    /// Concepts that exist but aren't listed in `index.md`.
    pub missing_index_entries: Vec<ConceptPath>,
    /// `type: source` concepts older than the staleness threshold.
    pub stale_sources: Vec<ConceptPath>,
    /// `true` when all finding lists are empty (wiki is healthy).
    pub healthy: bool,
}

impl LintReport {
    /// Returns `true` when every finding list is empty.
    pub fn is_healthy(&self) -> bool {
        self.contradictions.is_empty()
            && self.orphans.is_empty()
            && self.missing_pages.is_empty()
            && self.missing_index_entries.is_empty()
            && self.stale_sources.is_empty()
    }
}

/// The lint operation: `LintOperation::new(bundle).lint()`.
pub struct LintOperation<'a> {
    bundle: &'a BundleStore,
    stale_days: i64,
}

impl<'a> LintOperation<'a> {
    pub fn new(bundle: &'a BundleStore) -> Self {
        Self {
            bundle,
            stale_days: DEFAULT_STALE_DAYS,
        }
    }

    /// Override the staleness threshold (days) for source concepts.
    pub fn with_stale_days(mut self, days: i64) -> Self {
        self.stale_days = days;
        self
    }

    /// Run all lint checks and return a complete report.
    pub fn lint(&self) -> Result<LintReport> {
        let bundle_data = self.bundle.read_bundle()?;
        let graph = LinkGraph::from_bundle(&bundle_data);

        let contradictions = RelationManager::find_contradictions(&bundle_data);
        let orphans = graph.orphans();
        let missing_pages = self.find_missing_pages(&bundle_data);
        let missing_index_entries = self.find_missing_index_entries(&bundle_data);
        let stale_sources = self.find_stale_sources(&bundle_data);

        let healthy = contradictions.is_empty()
            && orphans.is_empty()
            && missing_pages.is_empty()
            && missing_index_entries.is_empty()
            && stale_sources.is_empty();

        Ok(LintReport {
            contradictions,
            orphans,
            missing_pages,
            missing_index_entries,
            stale_sources,
            healthy,
        })
    }

    /// Find cross-link targets that don't exist as concept files.
    fn find_missing_pages(&self, bundle_data: &argos_core::okf::Bundle) -> Vec<ConceptPath> {
        let mut missing = Vec::new();
        let existing: std::collections::HashSet<String> = bundle_data
            .concepts
            .iter()
            .map(|c| c.path.as_path().to_string_lossy().to_string())
            .collect();

        for concept in &bundle_data.concepts {
            let links = CrossLinkParser::parse(concept);
            for link in links {
                let target_str = link.to.as_path().to_string_lossy().to_string();
                if !existing.contains(&target_str) {
                    missing.push(link.to.clone());
                }
            }
        }

        missing.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        missing.dedup();
        missing
    }

    /// Find concepts that exist but aren't mentioned in `index.md`.
    fn find_missing_index_entries(
        &self,
        bundle_data: &argos_core::okf::Bundle,
    ) -> Vec<ConceptPath> {
        let index = self.bundle.read_index().unwrap_or_default();
        if index.is_empty() {
            // No index at all → every concept is "missing from index".
            return bundle_data
                .concepts
                .iter()
                .map(|c| c.path.clone())
                .collect();
        }

        bundle_data
            .concepts
            .iter()
            .filter(|c| {
                let path_str = c.path.as_path().to_string_lossy().to_string();
                !index.contains(&path_str)
            })
            .map(|c| c.path.clone())
            .collect()
    }

    /// Find `type: source` concepts older than the staleness threshold.
    fn find_stale_sources(&self, bundle_data: &argos_core::okf::Bundle) -> Vec<ConceptPath> {
        let threshold = Utc::now() - Duration::days(self.stale_days);
        bundle_data
            .concepts
            .iter()
            .filter(|c| {
                c.frontmatter.concept_type == ConceptType::Source
                    && c.frontmatter.timestamp < threshold
            })
            .map(|c| c.path.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::okf::{Concept, Frontmatter, RelationKind, TypedRelation};
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, BundleStore) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        (dir, bundle)
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

    fn write_concept_with_relations(
        bundle: &BundleStore,
        path: &str,
        title: &str,
        relations: Vec<TypedRelation>,
    ) {
        let concept = Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Concept,
                title: title.to_string(),
                timestamp: Utc::now(),
                description: None,
                resource: None,
                tags: None,
                relates_to: Some(relations),
            },
            body: format!("# {title}\n"),
        };
        bundle.write_concept(&concept).unwrap();
    }

    fn write_source_concept_at(
        bundle: &BundleStore,
        path: &str,
        title: &str,
        timestamp: chrono::DateTime<Utc>,
    ) {
        let concept = Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Source,
                title: title.to_string(),
                timestamp,
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: format!("# {title}\n"),
        };
        bundle.write_concept(&concept).unwrap();
    }

    #[test]
    fn lint_on_empty_bundle_returns_healthy() {
        let (_dir, bundle) = setup();
        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(report.healthy);
        assert!(report.is_healthy());
    }

    #[test]
    fn lint_detects_contradictions() {
        let (_dir, bundle) = setup();
        write_concept_with_relations(
            &bundle,
            "a.md",
            "Claim A",
            vec![TypedRelation {
                page: "b.md".to_string(),
                rel: RelationKind::Contradicts,
            }],
        );
        write_concept(&bundle, "b.md", "Claim B", "# Claim B\n");

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(!report.contradictions.is_empty());
        assert!(!report.healthy);
    }

    #[test]
    fn lint_detects_orphan_pages() {
        let (_dir, bundle) = setup();
        // `a.md` links to `b.md`, so `b.md` has an inbound link.
        // `c.md` has no inbound links → orphan.
        write_concept(&bundle, "a.md", "A", "See [B](b.md).");
        write_concept(&bundle, "b.md", "B", "# B");
        write_concept(&bundle, "c.md", "C", "# C (no inbound)");

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(report.orphans.contains(&ConceptPath::new("c.md")));
    }

    #[test]
    fn lint_detects_missing_pages() {
        let (_dir, bundle) = setup();
        // `a.md` links to `nonexistent.md` which doesn't exist.
        write_concept(&bundle, "a.md", "A", "See [ghost](nonexistent.md).");

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(report
            .missing_pages
            .contains(&ConceptPath::new("nonexistent.md")));
    }

    #[test]
    fn lint_detects_missing_index_entries() {
        let (_dir, bundle) = setup();
        write_concept(&bundle, "a.md", "A", "# A");
        // No index.md written → all concepts are "missing from index".

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(report
            .missing_index_entries
            .contains(&ConceptPath::new("a.md")));
    }

    #[test]
    fn lint_detects_stale_sources() {
        let (_dir, bundle) = setup();
        // Write a source concept with a timestamp 100 days ago.
        let old = Utc::now() - Duration::days(100);
        write_source_concept_at(&bundle, "old.md", "Old Source", old);

        let op = LintOperation::new(&bundle).with_stale_days(90);
        let report = op.lint().unwrap();
        assert!(report.stale_sources.contains(&ConceptPath::new("old.md")));
    }

    #[test]
    fn lint_on_healthy_bundle_returns_healthy_true() {
        let (_dir, bundle) = setup();
        // Two concepts that link to each other, index.md present, no stale sources.
        write_concept(&bundle, "a.md", "A", "See [B](b.md).");
        write_concept(&bundle, "b.md", "B", "See [A](a.md).");
        bundle
            .write_index("# Wiki Index\n\n- [A](a.md)\n- [B](b.md)\n")
            .unwrap();

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(report.healthy, "bundle with no issues should be healthy");
    }

    #[test]
    fn lint_on_bundle_with_issues_returns_healthy_false() {
        let (_dir, bundle) = setup();
        write_concept(&bundle, "orphan.md", "Orphan", "# Orphan");

        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        assert!(!report.healthy);
    }

    #[test]
    fn lint_report_contains_all_finding_categories() {
        let (_dir, bundle) = setup();
        let op = LintOperation::new(&bundle);
        let report = op.lint().unwrap();
        // All fields exist and are accessible.
        let _ = &report.contradictions;
        let _ = &report.orphans;
        let _ = &report.missing_pages;
        let _ = &report.missing_index_entries;
        let _ = &report.stale_sources;
        let _ = report.healthy;
    }

    #[test]
    fn lint_with_custom_stale_days_does_not_flag_recent() {
        let (_dir, bundle) = setup();
        let recent = Utc::now() - Duration::days(10);
        write_source_concept_at(&bundle, "recent.md", "Recent Source", recent);

        let op = LintOperation::new(&bundle).with_stale_days(90);
        let report = op.lint().unwrap();
        assert!(
            !report
                .stale_sources
                .contains(&ConceptPath::new("recent.md")),
            "10-day-old source should not be stale at 90-day threshold"
        );
    }

    #[test]
    fn lint_result_constructs() {
        let report = LintReport {
            contradictions: vec![],
            orphans: vec![],
            missing_pages: vec![],
            missing_index_entries: vec![],
            stale_sources: vec![],
            healthy: true,
        };
        assert!(report.is_healthy());
    }
}
