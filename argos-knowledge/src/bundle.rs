//! Filesystem operations over an OKF bundle directory.
//!
//! [`BundleStore`] reads and writes [`Concept`] files under a root directory
//! (typically `.argos/wiki/`). File path is concept identity (ADR-010): all
//! concept paths use forward slashes regardless of platform so that wiki links
//! and frontmatter `page` references match deterministically.

use std::fs;
use std::path::{Path, PathBuf};

use argos_core::{ArgosError, Bundle, Concept, ConceptPath, Result};

use crate::parser::{OkfParser, OkfWriter};

/// Filesystem-backed OKF bundle store.
pub struct BundleStore {
    root: PathBuf,
}

impl BundleStore {
    /// Create a bundle store rooted at `root`. The directory is created lazily
    /// on the first write.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Resolve a concept path to its on-disk location under the bundle root.
    fn file_of(&self, path: &ConceptPath) -> PathBuf {
        self.root.join(path.as_path())
    }

    /// Read and parse a concept file. Returns [`ArgosError::NotFound`] if the
    /// file does not exist.
    pub fn read_concept(&self, path: &ConceptPath) -> Result<Concept> {
        let file_path = self.file_of(path);
        let content = match fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ArgosError::NotFound(format!("concept not found: {path}")))
            }
            Err(e) => return Err(ArgosError::Io(e.to_string())),
        };
        OkfParser::parse(path.clone(), &content)
    }

    /// Serialise and write a concept file, creating parent directories as
    /// needed. Overwrites any existing file at the same path.
    pub fn write_concept(&self, concept: &Concept) -> Result<()> {
        let file_path = self.file_of(&concept.path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = OkfWriter::write(concept)?;
        fs::write(&file_path, content)?;
        Ok(())
    }

    /// Delete a concept file. Returns [`ArgosError::NotFound`] if absent.
    pub fn delete_concept(&self, path: &ConceptPath) -> Result<()> {
        let file_path = self.file_of(path);
        match fs::remove_file(&file_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ArgosError::NotFound(format!("concept not found: {path}")))
            }
            Err(e) => Err(ArgosError::Io(e.to_string())),
        }
    }

    /// Whether a concept file exists at `path`.
    pub fn exists(&self, path: &ConceptPath) -> Result<bool> {
        Ok(self.file_of(path).is_file())
    }

    /// Walk the bundle and list every `.md` concept path (forward slashes,
    /// sorted). Returns an empty vec if the root does not exist yet.
    pub fn list_concepts(&self) -> Result<Vec<ConceptPath>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let rel = p.strip_prefix(&self.root).map_err(|e| {
                ArgosError::Knowledge(format!("concept path outside bundle root: {e}"))
            })?;
            // Exclude OKF special files (index.md, log.md, schema.md) — they
            // are bundle-level metadata, not concepts with frontmatter.
            let rel_str = rel.to_string_lossy();
            if rel_str == "index.md" || rel_str == "log.md" || rel_str == "schema.md" {
                continue;
            }
            paths.push(ConceptPath::new(to_forward_slashes(rel)));
        }
        // `ConceptPath` is `Eq + Hash` but not `Ord`; order by the underlying
        // path so listing is deterministic without touching the shared type.
        paths.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(paths)
    }

    /// Read every concept in the bundle into a [`Bundle`].
    pub fn read_bundle(&self) -> Result<Bundle> {
        let paths = self.list_concepts()?;
        let mut concepts = Vec::with_capacity(paths.len());
        for p in paths {
            concepts.push(self.read_concept(&p)?);
        }
        Ok(Bundle {
            root: self.root.clone(),
            concepts,
        })
    }

    /// Read the OKF `index.md` content catalogue at the bundle root.
    ///
    /// Returns an empty string when no index exists yet (not an error): a
    /// fresh wiki has no index until the first ingest writes one.
    pub fn read_index(&self) -> Result<String> {
        read_text_or_empty(&self.root.join("index.md"))
    }

    /// Overwrite the OKF `index.md` content catalogue, creating the bundle
    /// root if needed.
    pub fn write_index(&self, content: &str) -> Result<()> {
        write_text(&self.root.join("index.md"), content)
    }

    /// Read the OKF `log.md` chronological log at the bundle root.
    ///
    /// Returns an empty string when no log exists yet (not an error).
    pub fn read_log(&self) -> Result<String> {
        read_text_or_empty(&self.root.join("log.md"))
    }

    /// Append `entry` to the OKF `log.md`, creating the file (and root) if
    /// absent. Log entries follow `## [date] operation | title` (ADR-010).
    pub fn append_log(&self, entry: &str) -> Result<()> {
        append_text(&self.root.join("log.md"), entry)
    }
}

/// Normalise a relative path to forward slashes (the OKF path convention).
///
/// On Windows, directory traversal yields backslash-separated paths; the wiki
/// uses forward slashes everywhere so links and frontmatter `page` values match
/// regardless of platform.
fn to_forward_slashes(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().replace('\\', "/");
    PathBuf::from(s)
}

/// Read a text file, returning an empty string when it is absent (NotFound is
/// treated as "no file yet", not an error). Other I/O errors propagate.
fn read_text_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(ArgosError::Io(e.to_string())),
    }
}

/// Overwrite `path` with `content`, creating parent directories as needed.
fn write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// Append `content` to `path`, creating the file (and parents) if absent.
fn append_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{ConceptType, Frontmatter};
    use chrono::Utc;

    fn tmp_store() -> (tempfile::TempDir, BundleStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = BundleStore::new(dir.path().join("wiki"));
        (dir, store)
    }

    fn concept(path: &str, title: &str, body: &str) -> Concept {
        Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: title.into(),
                timestamp: chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: body.into(),
        }
    }

    #[test]
    fn write_concept_creates_parent_directories() {
        let (_dir, store) = tmp_store();
        let c = concept("workflows/nested/deep/daily.md", "Daily", "# Daily\n");
        store.write_concept(&c).expect("write should succeed");
        assert!(store.exists(&c.path).unwrap());
    }

    #[test]
    fn read_concept_returns_parsed_concept() {
        let (_dir, store) = tmp_store();
        let c = concept("workflows/daily.md", "Daily", "# Daily Report\n\nBody.\n");
        store.write_concept(&c).unwrap();
        let read = store.read_concept(&c.path).expect("read should succeed");
        assert_eq!(read.path, c.path);
        assert_eq!(read.frontmatter.title, "Daily");
        assert_eq!(read.body, "# Daily Report\n\nBody.\n");
    }

    #[test]
    fn delete_concept_removes_file() {
        let (_dir, store) = tmp_store();
        let c = concept("a.md", "A", "body\n");
        store.write_concept(&c).unwrap();
        assert!(store.exists(&c.path).unwrap());
        store
            .delete_concept(&c.path)
            .expect("delete should succeed");
        assert!(!store.exists(&c.path).unwrap());
    }

    #[test]
    fn list_concepts_returns_all_md_files() {
        let (_dir, store) = tmp_store();
        store
            .write_concept(&concept("workflows/daily.md", "Daily", "b\n"))
            .unwrap();
        store
            .write_concept(&concept("workflows/weekly.md", "Weekly", "b\n"))
            .unwrap();
        store
            .write_concept(&concept("runbooks/oncall.md", "OnCall", "b\n"))
            .unwrap();
        let mut paths = store.list_concepts().unwrap();
        paths.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        assert_eq!(
            paths,
            vec![
                ConceptPath::new("runbooks/oncall.md"),
                ConceptPath::new("workflows/daily.md"),
                ConceptPath::new("workflows/weekly.md"),
            ]
        );
    }

    #[test]
    fn exists_returns_true_after_write_false_before() {
        let (_dir, store) = tmp_store();
        let p = ConceptPath::new("notes/x.md");
        assert!(!store.exists(&p).unwrap(), "must not exist before write");
        store
            .write_concept(&concept("notes/x.md", "X", "body\n"))
            .unwrap();
        assert!(store.exists(&p).unwrap(), "must exist after write");
    }

    #[test]
    fn read_bundle_returns_all_concepts() {
        let (_dir, store) = tmp_store();
        store
            .write_concept(&concept("workflows/daily.md", "Daily", "b\n"))
            .unwrap();
        store
            .write_concept(&concept("workflows/weekly.md", "Weekly", "b\n"))
            .unwrap();
        let bundle = store.read_bundle().expect("read_bundle should succeed");
        assert_eq!(bundle.concepts.len(), 2);
        let titles: Vec<&str> = bundle
            .concepts
            .iter()
            .map(|c| c.frontmatter.title.as_str())
            .collect();
        assert!(titles.contains(&"Daily"));
        assert!(titles.contains(&"Weekly"));
    }

    #[test]
    fn list_concepts_on_missing_root_returns_empty() {
        let store = BundleStore::new(PathBuf::from("this/does/not/exist"));
        assert_eq!(store.list_concepts().unwrap(), Vec::new());
    }

    #[test]
    fn read_concept_missing_returns_not_found() {
        let (_dir, store) = tmp_store();
        let res = store.read_concept(&ConceptPath::new("nope.md"));
        assert!(res.is_err());
    }

    // --- index.md / log.md (OKF bundle special files, ADR-010) --------------

    #[test]
    fn read_index_returns_empty_when_no_index_md() {
        let (_dir, store) = tmp_store();
        // No `index.md` written yet -> empty string, NOT an error.
        assert_eq!(store.read_index().unwrap(), "");
    }

    #[test]
    fn write_index_creates_index_md_with_content() {
        let (_dir, store) = tmp_store();
        store
            .write_index("# Wiki Index\n\n- [Daily](workflows/daily.md)\n")
            .expect("write_index should succeed");
        // The file exists at the bundle root and round-trips.
        assert_eq!(
            store.read_index().unwrap(),
            "# Wiki Index\n\n- [Daily](workflows/daily.md)\n"
        );
    }

    #[test]
    fn read_index_returns_content_after_write_index() {
        let (_dir, store) = tmp_store();
        let body = "- [A](a.md)\n- [B](b.md)\n";
        store.write_index(body).unwrap();
        assert_eq!(store.read_index().unwrap(), body);
    }

    #[test]
    fn read_log_returns_empty_when_no_log_md() {
        let (_dir, store) = tmp_store();
        // No `log.md` written yet -> empty string, NOT an error.
        assert_eq!(store.read_log().unwrap(), "");
    }

    #[test]
    fn append_log_creates_log_md_with_first_entry() {
        let (_dir, store) = tmp_store();
        let entry = "## [2026-06-18] ingest | Daily Standup\n";
        store.append_log(entry).expect("append_log should succeed");
        // First append creates `log.md` containing exactly the entry.
        assert_eq!(store.read_log().unwrap(), entry);
    }

    #[test]
    fn append_log_appends_to_existing_log_preserving_order() {
        let (_dir, store) = tmp_store();
        let first = "## [2026-06-17] ingest | A\n";
        let second = "## [2026-06-18] ingest | B\n";
        store.append_log(first).unwrap();
        store.append_log(second).unwrap();
        // Both entries present in insertion order.
        assert_eq!(store.read_log().unwrap(), format!("{first}{second}"));
    }
}
