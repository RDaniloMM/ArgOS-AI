//! Open Knowledge Format (OKF) domain types.
//!
//! These types model the OKF v0.1 specification: knowledge lives as a
//! directory of markdown files (a "bundle") where each file is a "concept"
//! with YAML frontmatter and a markdown body. Concepts link to each other
//! with normal markdown links, forming a natural graph. Optional typed
//! relations (extends, supersedes, contradicts, supports) add semantic
//! edges stored in frontmatter.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The type of a concept — the only required frontmatter field in OKF v0.1.
///
/// ArgOS defines five concept types for slice 1. The `Other` variant allows
/// arbitrary user-defined types without breaking deserialization.
///
/// Serialization is a plain lowercase string for all variants, including
/// `Other("custom")` which serializes as `"custom"` (not `{"other":"custom"}`).
/// This matches the OKF frontmatter convention where `type: workflow` or
/// `type: custom` is a single YAML scalar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConceptType {
    /// An n8n workflow represented as knowledge.
    Workflow,
    /// A step-by-step procedure (playbook, runbook).
    Runbook,
    /// A person, organisation, or other named thing.
    Entity,
    /// A general concept, idea, or topic.
    Concept,
    /// An immutable raw source ingested into the wiki.
    Source,
    /// A user-defined concept type not in the built-in vocabulary.
    Other(String),
}

impl Serialize for ConceptType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ConceptType::Workflow => serializer.serialize_str("workflow"),
            ConceptType::Runbook => serializer.serialize_str("runbook"),
            ConceptType::Entity => serializer.serialize_str("entity"),
            ConceptType::Concept => serializer.serialize_str("concept"),
            ConceptType::Source => serializer.serialize_str("source"),
            ConceptType::Other(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for ConceptType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "workflow" => Ok(ConceptType::Workflow),
            "runbook" => Ok(ConceptType::Runbook),
            "entity" => Ok(ConceptType::Entity),
            "concept" => Ok(ConceptType::Concept),
            "source" => Ok(ConceptType::Source),
            other => Ok(ConceptType::Other(other.to_string())),
        }
    }
}

/// The kind of typed relationship between two concepts.
///
/// Stored in frontmatter `relates_to` blocks. Bare markdown links carry no
/// relation type; these add semantic edges to the OKF graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationKind {
    /// This concept extends or builds upon another.
    Extends,
    /// This concept replaces a superseded one.
    Supersedes,
    /// This concept contradicts another.
    Contradicts,
    /// This concept supports or confirms another.
    Supports,
    /// A generic relationship (default when no specific kind applies).
    Related,
}

/// A typed outbound relationship declared in a concept's frontmatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedRelation {
    /// The target concept path (relative to the bundle root).
    pub page: String,
    /// The kind of relationship.
    pub rel: RelationKind,
}

/// OKF v0.1 frontmatter — the structured, queryable metadata block.
///
/// Only `type` and `title` and `timestamp` are required by ArgOS conventions.
/// All other fields are optional and follow the OKF "minimally opinionated"
/// principle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// The concept type — the only field OKF v0.1 strictly requires.
    #[serde(rename = "type")]
    pub concept_type: ConceptType,
    /// Human-readable concept title.
    pub title: String,
    /// When the concept was created or last modified.
    pub timestamp: DateTime<Utc>,
    /// Optional one-line description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional URI to an external resource (e.g. `n8n://workflows/42`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Optional free-form tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Optional typed outbound relations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relates_to: Option<Vec<TypedRelation>>,
}

/// An OKF concept — one markdown file in a bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    /// Relative path within the bundle (e.g. `workflows/daily-report.md`).
    pub path: ConceptPath,
    /// The YAML frontmatter block.
    pub frontmatter: Frontmatter,
    /// The markdown body (everything after the frontmatter delimiter).
    pub body: String,
}

/// A newtype around `PathBuf` representing a concept's location within a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConceptPath(pub PathBuf);

impl ConceptPath {
    /// Create a concept path from any path-like value.
    pub fn new<P: Into<PathBuf>>(p: P) -> Self {
        Self(p.into())
    }

    /// Get the underlying path reference.
    pub fn as_path(&self) -> &PathBuf {
        &self.0
    }
}

impl From<PathBuf> for ConceptPath {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

impl std::fmt::Display for ConceptPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// A cross-link between two concepts — parsed from markdown links in the body.
///
/// Unlike `TypedRelation` (stored in frontmatter), a `CrossLink` is a bare
/// `[[wikilink]]` or `[text](relative-path.md)` with no semantic type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CrossLink {
    /// The concept this link originates from.
    pub from: ConceptPath,
    /// The target concept path.
    pub to: ConceptPath,
    /// Optional typed relation if this link also appears in frontmatter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<RelationKind>,
}

/// An OKF bundle — a directory of concept files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    /// The filesystem root of the bundle (e.g. `.argos/wiki/`).
    pub root: PathBuf,
    /// All concepts discovered in the bundle.
    pub concepts: Vec<Concept>,
}

/// An immutable raw source — a document ingested into the wiki.
///
/// Raw sources are never modified by the LLM. The hash enables idempotent
/// re-ingest: if the hash hasn't changed, skip processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawSource {
    /// Filesystem path to the source file.
    pub path: PathBuf,
    /// Content hash (SHA-256 hex) for dedup and change detection.
    pub hash: String,
    /// When the source was ingested.
    pub ingested_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frontmatter() -> Frontmatter {
        Frontmatter {
            concept_type: ConceptType::Workflow,
            title: "Daily Report".into(),
            timestamp: DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            description: Some("Generates and sends a daily summary email.".into()),
            resource: Some("n8n://workflows/42".into()),
            tags: Some(vec!["email".into(), "report".into()]),
            relates_to: None,
        }
    }

    #[test]
    fn concept_constructs_with_frontmatter_and_body() {
        let concept = Concept {
            path: ConceptPath::new("workflows/daily-report.md"),
            frontmatter: sample_frontmatter(),
            body: "# Daily Report\n\nSends a summary.".into(),
        };
        assert_eq!(concept.frontmatter.title, "Daily Report");
        assert_eq!(concept.body, "# Daily Report\n\nSends a summary.");
    }

    #[test]
    fn frontmatter_serializes_to_yaml() {
        let fm = sample_frontmatter();
        let yaml = serde_yaml::to_string(&fm).unwrap();
        assert!(yaml.contains("type: workflow"));
        assert!(yaml.contains("title: Daily Report"));
        assert!(yaml.contains("resource: n8n://workflows/42"));
    }

    #[test]
    fn frontmatter_deserializes_from_yaml() {
        let yaml = r#"
type: workflow
title: Daily Report
timestamp: 2026-06-18T12:00:00Z
description: Generates and sends a daily summary email.
resource: n8n://workflows/42
tags:
  - email
  - report
"#;
        let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(fm.concept_type, ConceptType::Workflow);
        assert_eq!(fm.title, "Daily Report");
        assert_eq!(fm.tags.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn concept_type_serializes_as_lowercase_string() {
        let json = serde_json::to_string(&ConceptType::Workflow).unwrap();
        assert_eq!(json, "\"workflow\"");

        let json = serde_json::to_string(&ConceptType::Runbook).unwrap();
        assert_eq!(json, "\"runbook\"");
    }

    #[test]
    fn concept_type_other_serializes_with_inner_string() {
        let ct = ConceptType::Other("custom".into());
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, "\"custom\"");
    }

    #[test]
    fn relation_kind_serializes_as_lowercase_string() {
        let json = serde_json::to_string(&RelationKind::Extends).unwrap();
        assert_eq!(json, "\"extends\"");

        let json = serde_json::to_string(&RelationKind::Contradicts).unwrap();
        assert_eq!(json, "\"contradicts\"");
    }

    #[test]
    fn typed_relation_constructs() {
        let rel = TypedRelation {
            page: "workflows/weekly-report.md".into(),
            rel: RelationKind::Extends,
        };
        assert_eq!(rel.page, "workflows/weekly-report.md");
        assert_eq!(rel.rel, RelationKind::Extends);
    }

    #[test]
    fn concept_path_display() {
        let path = ConceptPath::new("workflows/daily.md");
        // On all platforms, forward-slash input is preserved in display output.
        assert!(path.to_string().contains("daily.md"));
    }

    #[test]
    fn cross_link_constructs() {
        let link = CrossLink {
            from: ConceptPath::new("a.md"),
            to: ConceptPath::new("b.md"),
            relation: Some(RelationKind::Supports),
        };
        assert_eq!(link.relation, Some(RelationKind::Supports));
    }

    #[test]
    fn bundle_constructs() {
        let bundle = Bundle {
            root: PathBuf::from(".argos/wiki"),
            concepts: vec![],
        };
        assert_eq!(bundle.concepts.len(), 0);
    }

    #[test]
    fn raw_source_constructs() {
        let src = RawSource {
            path: PathBuf::from("raw/article.md"),
            hash: "abc123".into(),
            ingested_at: Utc::now(),
        };
        assert_eq!(src.hash, "abc123");
    }
}
