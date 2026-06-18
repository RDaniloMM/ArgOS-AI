//! LLM-Wiki **ingest** operation (ADR-010).
//!
//! Implements the first of Karpathy's three LLM-Wiki operations: read a raw
//! source, summarise it via the LLM, write a `type: source` concept into the
//! OKF bundle, update `index.md`, and append a `## [date] ingest | title`
//! entry to `log.md`.
//!
//! Slice 1 scope: one source concept per ingest + log + index. Auto-creating
//! 10–15 entity/concept pages (the full Karpathy vision) is a future
//! enhancement; the trait seam (`Provider`) makes that a drop-in addition.

use crate::bundle::BundleStore;
use crate::raw::RawSourceStore;
use argos_core::{
    okf::{Concept, ConceptPath, ConceptType, Frontmatter},
    Result,
};
use argos_provider::{CompletionOptions, Provider};
use chrono::Utc;
use std::path::Path;

/// Outcome of an ingest operation — what was written and where.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestResult {
    /// The `type: source` concept path created (or reused if idempotent).
    pub source_concept: ConceptPath,
    /// `true` when the source was already ingested (hash match) and skipped.
    pub skipped: bool,
    /// The `log.md` entry that was appended (empty if skipped).
    pub log_entry: String,
}

/// The ingest operation: `IngestOperation::new(provider, bundle, raw).ingest(data, title)`.
pub struct IngestOperation<'a, P: Provider> {
    provider: &'a P,
    bundle: &'a BundleStore,
    raw: &'a RawSourceStore,
}

impl<'a, P: Provider> IngestOperation<'a, P> {
    pub fn new(provider: &'a P, bundle: &'a BundleStore, raw: &'a RawSourceStore) -> Self {
        Self {
            provider,
            bundle,
            raw,
        }
    }

    /// Ingest `data` titled `title` into the wiki.
    ///
    /// Steps:
    /// 1. Store the raw source (content-addressed) and check idempotency by hash.
    /// 2. Ask the LLM to summarise the source.
    /// 3. Write a `type: source` concept with the summary as its body.
    /// 4. Update `index.md` with the new concept.
    /// 5. Append `## [date] ingest | title` to `log.md`.
    pub async fn ingest(&self, data: &[u8], title: &str) -> Result<IngestResult> {
        let raw_source = self.raw.store_raw(data, title)?;

        // Idempotent: if the raw source already existed, skip.
        if self.raw.raw_exists(&raw_source.hash)? {
            // Check if the concept already exists too.
            let concept_path = source_concept_path(&raw_source.hash);
            if self.bundle.exists(&concept_path)? {
                return Ok(IngestResult {
                    source_concept: concept_path,
                    skipped: true,
                    log_entry: String::new(),
                });
            }
        }

        // Ask the LLM for a summary.
        let prompt = format!(
            "You are maintaining an OKF knowledge wiki. Summarise the following source into \
             3-5 sentences of key information. Source title: {title}\n\n---\n{}\n---",
            String::from_utf8_lossy(data),
        );
        let completion = self
            .provider
            .complete(&prompt, &CompletionOptions::default())
            .await?;

        // Create the source concept.
        let concept_path = source_concept_path(&raw_source.hash);
        let concept = Concept {
            path: concept_path.clone(),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Source,
                title: title.to_string(),
                timestamp: Utc::now(),
                description: Some(format!(
                    "Ingested raw source (sha256: {}).",
                    raw_source.hash
                )),
                resource: None,
                tags: Some(vec!["ingested".to_string()]),
                relates_to: None,
            },
            body: format!("# {title}\n\n{}", completion.text),
        };
        self.bundle.write_concept(&concept)?;

        // Update index.md: append a line for the new concept.
        let index_line = format!(
            "- [{title}]({}) — source\n",
            concept_path.as_path().display()
        );
        let existing_index = self.bundle.read_index()?;
        let new_index = if existing_index.is_empty() {
            format!("# Wiki Index\n\n{index_line}")
        } else {
            // Avoid duplicate lines.
            if existing_index.contains(&index_line) {
                existing_index
            } else {
                format!("{existing_index}{index_line}")
            }
        };
        self.bundle.write_index(&new_index)?;

        // Append to log.md.
        let date = Utc::now().format("%Y-%m-%d");
        let log_entry = format!("## [{date}] ingest | {title}\n");
        self.bundle.append_log(&log_entry)?;

        Ok(IngestResult {
            source_concept: concept_path,
            skipped: false,
            log_entry,
        })
    }
}

/// Derive the concept path for a source from its hash.
fn source_concept_path(hash: &str) -> ConceptPath {
    // Use the first 12 chars of the hash for a readable filename.
    ConceptPath::new(format!("sources/{}.md", &hash[..12]))
}

/// Ensure `path`'s parent directory exists (used in tests).
#[allow(dead_code)]
fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, BundleStore, RawSourceStore) {
        let dir = tempdir().unwrap();
        let bundle = BundleStore::new(dir.path().join("wiki"));
        let raw = RawSourceStore::new(dir.path().join("raw"));
        (dir, bundle, raw)
    }

    #[tokio::test]
    async fn ingest_creates_source_concept() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("This is a summary of the article.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let result = op
            .ingest(b"Hello world article", "Test Article")
            .await
            .unwrap();

        assert!(!result.skipped);
        assert!(bundle.exists(&result.source_concept).unwrap());
    }

    #[tokio::test]
    async fn ingest_creates_source_concept_with_type_source() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary text.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let result = op.ingest(b"content", "My Source").await.unwrap();
        let concept = bundle.read_concept(&result.source_concept).unwrap();
        assert_eq!(concept.frontmatter.concept_type, ConceptType::Source);
    }

    #[tokio::test]
    async fn ingest_appends_entry_to_log_md() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let result = op.ingest(b"data", "Logged Source").await.unwrap();
        let log = bundle.read_log().unwrap();
        assert!(log.contains("ingest | Logged Source"));
        assert!(!result.log_entry.is_empty());
    }

    #[tokio::test]
    async fn ingest_updates_index_md() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        op.ingest(b"data", "Indexed Source").await.unwrap();
        let index = bundle.read_index().unwrap();
        assert!(index.contains("Indexed Source"));
        assert!(index.contains("# Wiki Index"));
    }

    #[tokio::test]
    async fn ingest_returns_result_with_concept_path() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let result = op.ingest(b"data", "Result Test").await.unwrap();
        assert!(result
            .source_concept
            .as_path()
            .to_string_lossy()
            .contains("sources/"));
    }

    #[tokio::test]
    async fn ingest_is_idempotent_on_same_content() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let data = b"identical content";
        let r1 = op.ingest(data, "First").await.unwrap();
        let r2 = op.ingest(data, "First").await.unwrap();

        assert!(!r1.skipped);
        assert!(r2.skipped, "second ingest of same content should skip");
        assert_eq!(r1.source_concept, r2.source_concept);
    }

    #[tokio::test]
    async fn ingest_writes_raw_source_to_raw_directory() {
        let (dir, bundle, raw) = setup();
        let provider = StubProvider::new("Summary.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        op.ingest(b"raw data here", "Raw Test").await.unwrap();
        // The raw directory should contain at least one file.
        let raw_dir = dir.path().join("raw");
        assert!(raw_dir.exists(), "raw/ directory should exist after ingest");
        let count = std::fs::read_dir(&raw_dir).unwrap().count();
        assert!(count > 0, "raw/ should contain the ingested source");
    }

    #[tokio::test]
    async fn ingest_concept_body_contains_summary() {
        let (_dir, bundle, raw) = setup();
        let provider = StubProvider::new("The LLM summary text.");
        let op = IngestOperation::new(&provider, &bundle, &raw);

        let result = op.ingest(b"data", "Body Test").await.unwrap();
        let concept = bundle.read_concept(&result.source_concept).unwrap();
        assert!(concept.body.contains("The LLM summary text."));
    }

    #[test]
    fn source_concept_path_uses_hash_prefix() {
        let path = source_concept_path("abcdef1234567890");
        let s = path.as_path().to_string_lossy().to_string();
        assert!(s.starts_with("sources/"));
        assert!(s.ends_with(".md"));
    }

    #[test]
    fn ingest_result_constructs() {
        let r = IngestResult {
            source_concept: ConceptPath::new(PathBuf::from("sources/abc.md")),
            skipped: false,
            log_entry: "## [2026-06-18] ingest | Test\n".to_string(),
        };
        assert!(!r.skipped);
        assert!(!r.log_entry.is_empty());
    }
}
