//! OKF schema.md conventions (ADR-010).
//!
//! The schema document tells the LLM (and humans) how the wiki is structured,
//! what concept types exist, what frontmatter fields are required/optional,
//! how cross-links work, and what the ingest/query/lint workflows look like.
//! It is the "CLAUDE.md for the wiki" — the configuration that makes the LLM
//! a disciplined wiki maintainer rather than a generic chatbot.
//!
//! `Schema::default()` produces the ArgOS slice-1 conventions. `Schema::to_markdown`
//! serialises them to the `schema.md` file at the bundle root.

use argos_core::Result;
use std::path::Path;

/// The OKF schema document — conventions for the wiki.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    /// Human-readable wiki name.
    pub name: String,
    /// One-line description of what this wiki covers.
    pub description: String,
    /// Allowed concept types and their descriptions.
    pub concept_types: Vec<(String, String)>,
    /// Required frontmatter fields.
    pub required_fields: Vec<String>,
    /// Optional frontmatter fields.
    pub optional_fields: Vec<String>,
    /// Allowed relation kinds.
    pub relations: Vec<String>,
    /// Cross-link format description.
    pub cross_link_format: String,
    /// Log entry format.
    pub log_format: String,
}

impl Default for Schema {
    fn default() -> Self {
        Self {
            name: "ArgOS Wiki".to_string(),
            description: "Knowledge base for ArgOS — workflows, sources, entities, and concepts.".to_string(),
            concept_types: vec![
                ("workflow".to_string(), "An n8n workflow represented as knowledge. resource: n8n://workflows/<id>".to_string()),
                ("runbook".to_string(), "A step-by-step procedure (playbook, runbook).".to_string()),
                ("entity".to_string(), "A person, organisation, or other named thing.".to_string()),
                ("concept".to_string(), "A general concept, idea, or topic.".to_string()),
                ("source".to_string(), "An immutable raw source ingested into the wiki.".to_string()),
            ],
            required_fields: vec![
                "type".to_string(),
                "title".to_string(),
                "timestamp".to_string(),
            ],
            optional_fields: vec![
                "description".to_string(),
                "resource".to_string(),
                "tags".to_string(),
                "relates_to".to_string(),
            ],
            relations: vec![
                "extends".to_string(),
                "supersedes".to_string(),
                "contradicts".to_string(),
                "supports".to_string(),
                "related".to_string(),
            ],
            cross_link_format: "Relative markdown links [text](path.md) or [[wikilink]]. Links resolve relative to the concept's own path.".to_string(),
            log_format: "## [YYYY-MM-DD] operation | title".to_string(),
        }
    }
}

impl Schema {
    /// Serialise the schema to the `schema.md` markdown format.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# {}\n\n", self.name));
        md.push_str(&self.description);
        md.push_str("\n\n");

        md.push_str("## Concept Types\n\n");
        md.push_str("| Type | Description |\n");
        md.push_str("|------|-------------|\n");
        for (t, d) in &self.concept_types {
            md.push_str(&format!("| `{t}` | {d} |\n"));
        }

        md.push_str("\n## Frontmatter\n\n");
        md.push_str("**Required fields:**\n");
        for f in &self.required_fields {
            md.push_str(&format!("- `{f}`\n"));
        }
        md.push_str("\n**Optional fields:**\n");
        for f in &self.optional_fields {
            md.push_str(&format!("- `{f}`\n"));
        }

        md.push_str("\n## Typed Relations\n\n");
        for r in &self.relations {
            md.push_str(&format!("- `{r}`\n"));
        }

        md.push_str("\n## Cross-Links\n\n");
        md.push_str(&self.cross_link_format);
        md.push('\n');

        md.push_str("\n## Log Format\n\n");
        md.push_str(&self.log_format);
        md.push('\n');

        md
    }

    /// Write the schema to `schema.md` at the bundle root.
    pub fn write_to_bundle(&self, bundle_root: &Path) -> Result<()> {
        let schema_path = bundle_root.join("schema.md");
        std::fs::create_dir_all(bundle_root)
            .map_err(|e| argos_core::ArgosError::Io(e.to_string()))?;
        std::fs::write(&schema_path, self.to_markdown())
            .map_err(|e| argos_core::ArgosError::Io(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_schema_has_five_concept_types() {
        let schema = Schema::default();
        assert_eq!(schema.concept_types.len(), 5);
        assert!(schema.concept_types.iter().any(|(t, _)| t == "workflow"));
        assert!(schema.concept_types.iter().any(|(t, _)| t == "source"));
    }

    #[test]
    fn default_schema_requires_type_title_timestamp() {
        let schema = Schema::default();
        assert!(schema.required_fields.contains(&"type".to_string()));
        assert!(schema.required_fields.contains(&"title".to_string()));
        assert!(schema.required_fields.contains(&"timestamp".to_string()));
    }

    #[test]
    fn default_schema_has_five_relations() {
        let schema = Schema::default();
        assert_eq!(schema.relations.len(), 5);
        assert!(schema.relations.contains(&"extends".to_string()));
        assert!(schema.relations.contains(&"contradicts".to_string()));
    }

    #[test]
    fn to_markdown_contains_concept_types_table() {
        let schema = Schema::default();
        let md = schema.to_markdown();
        assert!(md.contains("## Concept Types"));
        assert!(md.contains("| `workflow` |"));
        assert!(md.contains("| `source` |"));
    }

    #[test]
    fn to_markdown_contains_required_fields() {
        let schema = Schema::default();
        let md = schema.to_markdown();
        assert!(md.contains("## Frontmatter"));
        assert!(md.contains("**Required fields:**"));
        assert!(md.contains("- `type`"));
        assert!(md.contains("- `title`"));
    }

    #[test]
    fn to_markdown_contains_relations() {
        let schema = Schema::default();
        let md = schema.to_markdown();
        assert!(md.contains("## Typed Relations"));
        assert!(md.contains("- `extends`"));
        assert!(md.contains("- `contradicts`"));
    }

    #[test]
    fn to_markdown_contains_cross_link_format() {
        let schema = Schema::default();
        let md = schema.to_markdown();
        assert!(md.contains("## Cross-Links"));
        assert!(md.contains("Relative markdown links"));
    }

    #[test]
    fn to_markdown_contains_log_format() {
        let schema = Schema::default();
        let md = schema.to_markdown();
        assert!(md.contains("## Log Format"));
        assert!(md.contains("## [YYYY-MM-DD] operation | title"));
    }

    #[test]
    fn write_to_bundle_creates_schema_md() {
        let dir = tempdir().unwrap();
        let schema = Schema::default();
        schema.write_to_bundle(dir.path()).unwrap();
        let schema_path = dir.path().join("schema.md");
        assert!(schema_path.exists());
        let content = std::fs::read_to_string(&schema_path).unwrap();
        assert!(content.contains("# ArgOS Wiki"));
    }

    #[test]
    fn write_to_bundle_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("wiki");
        let schema = Schema::default();
        schema.write_to_bundle(&nested).unwrap();
        assert!(nested.join("schema.md").exists());
    }

    #[test]
    fn schema_can_be_customised() {
        let schema = Schema {
            name: "Custom Wiki".to_string(),
            description: "A custom wiki.".to_string(),
            concept_types: vec![("custom".to_string(), "A custom type.".to_string())],
            required_fields: vec!["type".to_string()],
            optional_fields: vec![],
            relations: vec!["related".to_string()],
            cross_link_format: "Custom link format.".to_string(),
            log_format: "Custom log format.".to_string(),
        };
        let md = schema.to_markdown();
        assert!(md.contains("# Custom Wiki"));
        assert!(md.contains("| `custom` |"));
    }
}
