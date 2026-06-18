//! Typed cross-link surfacer for workflow concepts (ADR-008).
//!
//! Surfaces typed relations (extends, supersedes, contradicts, supports)
//! between workflow concepts stored in OKF frontmatter `relates_to` blocks.
//! In slice 1 these relations are stored and surfaced but NOT blended into
//! similarity scoring — the scoring is intent-first (vector embeddings only).
//! Phase 2 will add a structure + I/O + relations blend.

//! Typed cross-link surfacer for workflow concepts (ADR-008).
//!
//! Surfaces typed relations (extends, supersedes, contradicts, supports)
//! between workflow concepts stored in OKF frontmatter `relates_to` blocks.
//! In slice 1 these relations are stored and surfaced but NOT blended into
//! similarity scoring — the scoring is intent-first (vector embeddings only).
//! Phase 2 will add a structure + I/O + relations blend.

use argos_core::okf::{Bundle, Concept, ConceptPath, ConceptType, TypedRelation};
use argos_core::Result;
use argos_knowledge::{BundleStore, RelationManager};

/// Surfaces typed relations between workflow concepts.
pub struct CrossLinkSurfacer<'a> {
    bundle: &'a BundleStore,
}

impl<'a> CrossLinkSurfacer<'a> {
    pub fn new(bundle: &'a BundleStore) -> Self {
        Self { bundle }
    }

    /// Extract typed relations from a concept's frontmatter `relates_to`.
    pub fn get_relations(concept: &Concept) -> Vec<TypedRelation> {
        RelationManager::get_relations(concept)
    }

    /// List all workflow concepts that have at least one typed relation.
    pub fn workflows_with_relations(&self) -> Result<Vec<(ConceptPath, Vec<TypedRelation>)>> {
        let bundle = self.bundle.read_bundle()?;
        let result = bundle
            .concepts
            .iter()
            .filter(|c| c.frontmatter.concept_type == ConceptType::Workflow)
            .filter_map(|c| {
                let rels = Self::get_relations(c);
                if rels.is_empty() {
                    None
                } else {
                    Some((c.path.clone(), rels))
                }
            })
            .collect();
        Ok(result)
    }

    /// Find workflows that `concept` extends.
    pub fn find_extends(concept: &Concept) -> Vec<ConceptPath> {
        Self::get_relations(concept)
            .into_iter()
            .filter(|r| r.rel == argos_core::okf::RelationKind::Extends)
            .map(|r| ConceptPath::new(r.page))
            .collect()
    }

    /// Find workflows that `concept` supersedes.
    pub fn find_supersedes(concept: &Concept) -> Vec<ConceptPath> {
        Self::get_relations(concept)
            .into_iter()
            .filter(|r| r.rel == argos_core::okf::RelationKind::Supersedes)
            .map(|r| ConceptPath::new(r.page))
            .collect()
    }

    /// Find contradicting workflow pairs in the bundle.
    pub fn find_contradictions(&self) -> Result<Vec<(ConceptPath, ConceptPath)>> {
        let bundle = self.bundle.read_bundle()?;
        Ok(Self::find_contradictions_in_bundle(&bundle))
    }

    /// Static helper that delegates to RelationManager but filters to
    /// contradictions involving at least one workflow concept.
    fn find_contradictions_in_bundle(bundle: &Bundle) -> Vec<(ConceptPath, ConceptPath)> {
        let all = RelationManager::find_contradictions(bundle);
        // Filter to pairs where at least one side is a workflow concept.
        let workflow_paths: std::collections::HashSet<_> = bundle
            .concepts
            .iter()
            .filter(|c| c.frontmatter.concept_type == ConceptType::Workflow)
            .map(|c| c.path.as_path().to_string_lossy().to_string())
            .collect();
        all.into_iter()
            .filter(|(a, b)| {
                workflow_paths.contains(&a.as_path().to_string_lossy().to_string())
                    || workflow_paths.contains(&b.as_path().to_string_lossy().to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::okf::{Concept, Frontmatter, RelationKind, TypedRelation};
    use argos_knowledge::BundleStore;
    use chrono::Utc;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, BundleStore) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        (dir, bundle)
    }

    fn write_workflow(
        bundle: &BundleStore,
        path: &str,
        title: &str,
        relations: Vec<TypedRelation>,
    ) {
        let concept = Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: title.to_string(),
                timestamp: Utc::now(),
                description: None,
                resource: Some(format!("n8n://workflows/{}", path)),
                tags: None,
                relates_to: if relations.is_empty() {
                    None
                } else {
                    Some(relations)
                },
            },
            body: format!("# {title}\n"),
        };
        bundle.write_concept(&concept).unwrap();
    }

    #[allow(dead_code)]
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

    #[test]
    fn crosslink_surfacer_constructs() {
        let (_dir, bundle) = setup();
        let _surfacer = CrossLinkSurfacer::new(&bundle);
    }

    #[test]
    fn get_relations_extracts_from_frontmatter() {
        let (_dir, bundle) = setup();
        write_workflow(
            &bundle,
            "a.md",
            "A",
            vec![TypedRelation {
                page: "b.md".to_string(),
                rel: RelationKind::Extends,
            }],
        );
        let concept = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let rels = CrossLinkSurfacer::get_relations(&concept);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].page, "b.md");
        assert_eq!(rels[0].rel, RelationKind::Extends);
    }

    #[test]
    fn get_relations_returns_empty_for_no_relations() {
        let (_dir, bundle) = setup();
        write_workflow(&bundle, "a.md", "A", vec![]);
        let concept = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let rels = CrossLinkSurfacer::get_relations(&concept);
        assert!(rels.is_empty());
    }

    #[test]
    fn workflows_with_relations_lists_only_workflows_with_relations() {
        let (_dir, bundle) = setup();
        // Workflow with relations
        write_workflow(
            &bundle,
            "wf-a.md",
            "WF A",
            vec![TypedRelation {
                page: "wf-b.md".to_string(),
                rel: RelationKind::Extends,
            }],
        );
        // Workflow without relations
        write_workflow(&bundle, "wf-b.md", "WF B", vec![]);
        // Non-workflow concept with relations
        let concept = Concept {
            path: ConceptPath::new("concept.md"),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Concept,
                title: "Concept".to_string(),
                timestamp: Utc::now(),
                description: None,
                resource: None,
                tags: None,
                relates_to: Some(vec![TypedRelation {
                    page: "wf-a.md".to_string(),
                    rel: RelationKind::Supports,
                }]),
            },
            body: "# Concept\n".to_string(),
        };
        bundle.write_concept(&concept).unwrap();

        let surfacer = CrossLinkSurfacer::new(&bundle);
        let result = surfacer.workflows_with_relations().unwrap();
        // Only wf-a should appear (workflow with relations)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ConceptPath::new("wf-a.md"));
    }

    #[test]
    fn find_extends_returns_target_paths() {
        let (_dir, bundle) = setup();
        write_workflow(
            &bundle,
            "a.md",
            "A",
            vec![
                TypedRelation {
                    page: "b.md".to_string(),
                    rel: RelationKind::Extends,
                },
                TypedRelation {
                    page: "c.md".to_string(),
                    rel: RelationKind::Supersedes,
                },
            ],
        );
        let concept = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let extends = CrossLinkSurfacer::find_extends(&concept);
        assert_eq!(extends.len(), 1);
        assert_eq!(extends[0], ConceptPath::new("b.md"));
    }

    #[test]
    fn find_supersedes_returns_target_paths() {
        let (_dir, bundle) = setup();
        write_workflow(
            &bundle,
            "a.md",
            "A",
            vec![
                TypedRelation {
                    page: "b.md".to_string(),
                    rel: RelationKind::Extends,
                },
                TypedRelation {
                    page: "c.md".to_string(),
                    rel: RelationKind::Supersedes,
                },
            ],
        );
        let concept = bundle.read_concept(&ConceptPath::new("a.md")).unwrap();
        let supersedes = CrossLinkSurfacer::find_supersedes(&concept);
        assert_eq!(supersedes.len(), 1);
        assert_eq!(supersedes[0], ConceptPath::new("c.md"));
    }

    #[test]
    fn find_contradictions_finds_workflow_pairs() {
        let (_dir, bundle) = setup();
        write_workflow(
            &bundle,
            "wf-a.md",
            "WF A",
            vec![TypedRelation {
                page: "wf-b.md".to_string(),
                rel: RelationKind::Contradicts,
            }],
        );
        write_workflow(&bundle, "wf-b.md", "WF B", vec![]);

        let surfacer = CrossLinkSurfacer::new(&bundle);
        let contradictions = surfacer.find_contradictions().unwrap();
        assert!(!contradictions.is_empty());
    }

    #[test]
    fn find_contradictions_returns_empty_for_no_contradictions() {
        let (_dir, bundle) = setup();
        write_workflow(&bundle, "wf-a.md", "WF A", vec![]);
        write_workflow(&bundle, "wf-b.md", "WF B", vec![]);

        let surfacer = CrossLinkSurfacer::new(&bundle);
        let contradictions = surfacer.find_contradictions().unwrap();
        assert!(contradictions.is_empty());
    }
}
