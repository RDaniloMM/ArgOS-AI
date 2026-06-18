//! Import n8n workflows as OKF concepts (spec: Import Workflows as OKF
//! Concepts; ADR-011).
//!
//! [`WorkflowImporter`] lists workflows from n8n via an [`N8nClient`] and
//! writes one `type: workflow` OKF concept per workflow into a
//! [`BundleStore`]. The concept frontmatter carries `resource:
//! n8n://workflows/<id>` so re-imports are idempotent (the importer skips any
//! workflow whose resource already exists in the bundle). ArgOS never copies
//! the workflow definition as the source of truth — n8n owns execution; the
//! concept is the knowledge representation of the workflow's intent.
//!
//! Slice 1 body: an auto-generated summary derived from the workflow ref
//! (name, id, url). Node-level detail (node count/types, raw JSON) and
//! LLM-authored summaries are future enhancements — the latter needs a
//! [`Provider`](argos_provider::Provider), the former needs definition access
//! on the transport.

use std::collections::HashSet;

use argos_core::{Concept, ConceptPath, ConceptType, Frontmatter, N8nWorkflowRef, Result};
use argos_knowledge::BundleStore;
use chrono::{DateTime, Utc};

use crate::client::N8nClient;

/// Outcome of importing workflows — which concepts were created and which n8n
/// workflow ids were skipped because a concept already exists for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    /// Concept paths created by this import run.
    pub imported: Vec<ConceptPath>,
    /// n8n workflow ids that already had a concept (skipped, idempotent).
    pub skipped: Vec<String>,
}

/// The import operation: list n8n workflows → write an OKF concept per
/// workflow into the bundle.
pub struct WorkflowImporter<'a, C: N8nClient> {
    client: &'a C,
    bundle: &'a BundleStore,
}

impl<'a, C: N8nClient> WorkflowImporter<'a, C> {
    /// Create an importer that pulls workflows from `client` and writes
    /// concepts into `bundle`.
    pub fn new(client: &'a C, bundle: &'a BundleStore) -> Self {
        Self { client, bundle }
    }

    /// Import every workflow visible in n8n as an OKF concept. Workflows whose
    /// `n8n://workflows/<id>` resource already exists in the bundle are
    /// skipped (idempotent re-import).
    pub async fn import_all(&self) -> Result<ImportResult> {
        let workflows = self.client.list_workflows().await?;
        let existing = self.existing_resources()?;
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for wf in workflows {
            let resource = workflow_resource(&wf.id);
            if existing.contains(&resource) {
                skipped.push(wf.id.clone());
                continue;
            }
            let path = self.unique_path(&wf.name, &wf.id)?;
            let concept = build_workflow_concept(&wf, path.clone(), Utc::now());
            self.bundle.write_concept(&concept)?;
            imported.push(path);
        }
        Ok(ImportResult { imported, skipped })
    }

    /// Import a single workflow ref. Returns the concept path when a new
    /// concept was written, or `None` when it was skipped (already imported).
    pub async fn import_workflow(&self, wf: &N8nWorkflowRef) -> Result<Option<ConceptPath>> {
        let resource = workflow_resource(&wf.id);
        if self.existing_resources()?.contains(&resource) {
            return Ok(None);
        }
        let path = self.unique_path(&wf.name, &wf.id)?;
        let concept = build_workflow_concept(wf, path.clone(), Utc::now());
        self.bundle.write_concept(&concept)?;
        Ok(Some(path))
    }

    /// Collect the `n8n://workflows/<id>` resources already present in the
    /// bundle so imports can skip workflows that have already been imported.
    fn existing_resources(&self) -> Result<HashSet<String>> {
        let bundle = self.bundle.read_bundle()?;
        Ok(bundle
            .concepts
            .iter()
            .filter_map(|c| c.frontmatter.resource.clone())
            .collect())
    }

    /// Derive a concept path under `workflows/` from the workflow name,
    /// appending a short id suffix when the slug-based path already exists
    /// (name collision between two distinct n8n workflows).
    fn unique_path(&self, name: &str, id: &str) -> Result<ConceptPath> {
        let slug = slugify(name);
        let base = format!("workflows/{slug}.md");
        let path = ConceptPath::new(base);
        if !self.bundle.exists(&path)? {
            return Ok(path);
        }
        let short: String = id.chars().take(8).collect();
        Ok(ConceptPath::new(format!("workflows/{slug}-{short}.md")))
    }
}

/// Build the `n8n://workflows/<id>` resource URI for a workflow id.
pub fn workflow_resource(id: &str) -> String {
    format!("n8n://workflows/{id}")
}

/// Build the OKF concept for a workflow ref at `path` with `timestamp`.
pub(crate) fn build_workflow_concept(
    wf: &N8nWorkflowRef,
    path: ConceptPath,
    timestamp: DateTime<Utc>,
) -> Concept {
    let resource = workflow_resource(&wf.id);
    Concept {
        path,
        frontmatter: Frontmatter {
            concept_type: ConceptType::Workflow,
            title: wf.name.clone(),
            timestamp,
            description: Some(format!(
                "n8n workflow '{}' imported as an OKF concept.",
                wf.name
            )),
            resource: Some(resource.clone()),
            tags: Some(vec!["n8n".to_string(), "imported".to_string()]),
            relates_to: None,
        },
        body: workflow_body(wf),
    }
}

/// Generate the slice-1 markdown body for an imported workflow concept.
///
/// Derived only from the workflow ref (name, id, url) — the transport exposes
/// no workflow definition in slice 1. Node-level detail and an LLM-authored
/// summary are future enhancements.
fn workflow_body(wf: &N8nWorkflowRef) -> String {
    let resource = workflow_resource(&wf.id);
    let url_line = wf
        .url
        .as_ref()
        .map(|u| format!("- **n8n URL**: {u}\n"))
        .unwrap_or_default();
    format!(
        "# {name}\n\
         \n\
         This concept mirrors the n8n workflow **{name}** (id `{id}`).\n\
         \n\
         ArgOS does not execute this workflow — n8n owns execution and \
         durability. This file is the knowledge representation of the \
         workflow's intent, kept in sync via `argos n8n import`.\n\
         \n\
         - **Workflow ID**: `{id}`\n\
         - **Resource**: {resource}\n\
         {url_line}\
         \n\
         _Node-level detail (node count, types, raw JSON) and an LLM-authored \
         summary will be added once the transport exposes workflow definitions \
         and a Provider is wired in._\n",
        name = wf.name,
        id = wf.id,
        resource = resource,
        url_line = url_line,
    )
}

/// Slugify a workflow name into a filesystem-safe concept filename stem.
///
/// Lowercases, keeps alphanumerics, turns every other character into a single
/// hyphen, trims leading/trailing hyphens, and falls back to `"workflow"` when
/// the result would be empty.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = true;
    for ch in name.trim().chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "workflow".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StubN8nClient;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn wf(id: &str, name: &str) -> N8nWorkflowRef {
        N8nWorkflowRef {
            id: id.into(),
            name: name.into(),
            url: None,
        }
    }

    fn setup() -> (tempfile::TempDir, BundleStore) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        (dir, bundle)
    }

    #[test]
    fn workflow_importer_constructs() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::new();
        let importer = WorkflowImporter::new(&client, &bundle);
        // Construction is enough; the importer borrows the client and bundle.
        let _ = importer;
    }

    #[tokio::test]
    async fn import_creates_concept_with_type_workflow() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily Report")]);
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        assert_eq!(result.imported.len(), 1);
        let concept = bundle.read_concept(&result.imported[0]).unwrap();
        assert_eq!(concept.frontmatter.concept_type, ConceptType::Workflow);
    }

    #[tokio::test]
    async fn import_creates_concept_with_n8n_resource() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily Report")]);
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        let concept = bundle.read_concept(&result.imported[0]).unwrap();
        assert_eq!(
            concept.frontmatter.resource.as_deref(),
            Some("n8n://workflows/wf-1")
        );
    }

    #[tokio::test]
    async fn import_body_contains_workflow_name() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily Report")]);
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        let concept = bundle.read_concept(&result.imported[0]).unwrap();
        assert!(
            concept.body.contains("Daily Report"),
            "body must mention the workflow name, got: {}",
            concept.body
        );
    }

    #[tokio::test]
    async fn import_skips_already_imported_workflows() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily Report")]);
        let importer = WorkflowImporter::new(&client, &bundle);

        let first = importer.import_all().await.unwrap();
        assert_eq!(first.imported.len(), 1);
        assert!(first.skipped.is_empty());

        // Second run: the workflow is already imported -> skipped, not duplicated.
        let second = importer.import_all().await.unwrap();
        assert!(second.imported.is_empty(), "re-import must not duplicate");
        assert!(second.skipped.contains(&"wf-1".to_string()));
    }

    #[tokio::test]
    async fn import_returns_result_with_imported_paths() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily"), wf("wf-2", "Weekly")]);
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        assert_eq!(result.imported.len(), 2);
        // Every imported path actually exists in the bundle.
        for p in &result.imported {
            assert!(bundle.exists(p).unwrap(), "imported path {p} must exist");
        }
    }

    #[tokio::test]
    async fn import_on_empty_n8n_returns_empty_result() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::new();
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        assert!(result.imported.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[tokio::test]
    async fn import_creates_concept_in_workflows_directory() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::with_workflows(vec![wf("wf-1", "Daily Report")]);
        let importer = WorkflowImporter::new(&client, &bundle);
        let result = importer.import_all().await.unwrap();
        let path_str = result.imported[0].as_path().to_string_lossy().to_string();
        assert!(
            path_str.starts_with("workflows/"),
            "concept must live under workflows/, got {path_str}"
        );
        assert!(path_str.ends_with(".md"));
    }

    #[tokio::test]
    async fn import_workflow_single_returns_path_when_new() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::new();
        let importer = WorkflowImporter::new(&client, &bundle);
        let opt = importer
            .import_workflow(&wf("wf-1", "Daily Report"))
            .await
            .unwrap();
        assert!(opt.is_some());
    }

    #[tokio::test]
    async fn import_workflow_single_returns_none_when_skipped() {
        let (_dir, bundle) = setup();
        let client = StubN8nClient::new();
        let importer = WorkflowImporter::new(&client, &bundle);
        importer
            .import_workflow(&wf("wf-1", "Daily Report"))
            .await
            .unwrap();
        // Second import of the same workflow -> None (skipped).
        let opt = importer
            .import_workflow(&wf("wf-1", "Daily Report"))
            .await
            .unwrap();
        assert!(opt.is_none());
    }

    #[test]
    fn workflow_resource_builds_n8n_uri() {
        assert_eq!(workflow_resource("42"), "n8n://workflows/42");
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Daily Report"), "daily-report");
        assert_eq!(slugify("Weekly Summary!"), "weekly-summary");
    }

    #[test]
    fn slugify_collapses_separators_and_trims() {
        assert_eq!(slugify("  A   B  "), "a-b");
        assert_eq!(slugify("---Lead/trail---"), "lead-trail");
    }

    #[test]
    fn slugify_empty_falls_back_to_workflow() {
        assert_eq!(slugify(""), "workflow");
        assert_eq!(slugify("!!!"), "workflow");
    }

    #[test]
    fn build_workflow_concept_sets_frontmatter_and_body() {
        let concept = build_workflow_concept(
            &wf("wf-1", "Daily Report"),
            ConceptPath::new(PathBuf::from("workflows/daily-report.md")),
            DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        assert_eq!(concept.frontmatter.concept_type, ConceptType::Workflow);
        assert_eq!(concept.frontmatter.title, "Daily Report");
        assert_eq!(
            concept.frontmatter.resource.as_deref(),
            Some("n8n://workflows/wf-1")
        );
        assert!(concept.body.contains("Daily Report"));
    }

    #[test]
    fn import_result_constructs() {
        let r = ImportResult {
            imported: vec![ConceptPath::new("workflows/a.md")],
            skipped: vec!["wf-2".into()],
        };
        assert_eq!(r.imported.len(), 1);
        assert_eq!(r.skipped, vec!["wf-2"]);
    }
}
