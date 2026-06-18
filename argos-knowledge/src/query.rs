//! LLM-Wiki **query** operation (ADR-010).
//!
//! Implements the second of Karpathy's three LLM-Wiki operations: given a
//! question, find relevant wiki concepts by keyword, pass their bodies to
//! the LLM as context, and synthesise a cited answer. Good answers can be
//! filed back into the wiki — that filing is a future enhancement; slice 1
//! returns the answer and citations without auto-filing.

use crate::bundle::BundleStore;
use argos_core::{
    okf::{Concept, ConceptPath},
    Result,
};
use argos_provider::{CompletionOptions, Provider};

/// The outcome of a query operation.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    /// The synthesised answer text (markdown, may contain citation links).
    pub answer: String,
    /// Concept paths that were used as context / cited in the answer.
    pub cited_concepts: Vec<ConceptPath>,
}

/// The query operation: `QueryOperation::new(provider, bundle).query(question)`.
pub struct QueryOperation<'a, P: Provider> {
    provider: &'a P,
    bundle: &'a BundleStore,
}

/// Maximum number of concepts to select as context for a single query.
const MAX_CONTEXT_CONCEPTS: usize = 5;

impl<'a, P: Provider> QueryOperation<'a, P> {
    pub fn new(provider: &'a P, bundle: &'a BundleStore) -> Self {
        Self { provider, bundle }
    }

    /// Answer `question` using the wiki as context.
    ///
    /// Steps:
    /// 1. List all concepts and select the top `MAX_CONTEXT_CONCEPTS` by
    ///    case-insensitive keyword overlap with the question.
    /// 2. Build a prompt containing the selected concept bodies.
    /// 3. Ask the LLM to synthesise an answer with markdown citations.
    /// 4. Return the answer and the list of cited concept paths.
    pub async fn query(&self, question: &str) -> Result<QueryResult> {
        let concepts = self.select_relevant(question);

        if concepts.is_empty() {
            return Ok(QueryResult {
                answer: String::new(),
                cited_concepts: vec![],
            });
        }

        let cited: Vec<ConceptPath> = concepts.iter().map(|c| c.path.clone()).collect();

        let context_block = concepts
            .iter()
            .map(|c| {
                format!(
                    "### [{}]({})\n\n{}\n",
                    c.frontmatter.title,
                    c.path.as_path().display(),
                    c.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n---\n\n");

        let prompt = format!(
            "You are answering a question using an OKF knowledge wiki. \
             Use ONLY the following wiki pages as context. Cite each claim \
             with a markdown link to the source concept. If the wiki does \
             not contain enough information, say so.\n\n\
             Question: {question}\n\n\
             Wiki context:\n{context_block}\n\n\
             Answer (with citations):"
        );

        let completion = self
            .provider
            .complete(&prompt, &CompletionOptions::default())
            .await?;

        Ok(QueryResult {
            answer: completion.text,
            cited_concepts: cited,
        })
    }

    /// Select the top concepts by keyword overlap with `question`.
    ///
    /// Scoring: for each concept, count how many question keywords (length > 2)
    /// appear (case-insensitive) in the title + body. Return the top N by score,
    /// excluding zero-score concepts.
    fn select_relevant(&self, question: &str) -> Vec<Concept> {
        let keywords: Vec<&str> = question
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        if keywords.is_empty() {
            return vec![];
        }

        let paths = match self.bundle.list_concepts() {
            Ok(p) => p,
            Err(_) => return vec![],
        };

        let mut scored: Vec<(usize, Concept)> = Vec::new();
        for path in paths {
            if let Ok(concept) = self.bundle.read_concept(&path) {
                let haystack =
                    format!("{} {}", concept.frontmatter.title, concept.body).to_lowercase();
                let score = keywords
                    .iter()
                    .filter(|kw| haystack.contains(&kw.to_lowercase()))
                    .count();
                if score > 0 {
                    scored.push((score, concept));
                }
            }
        }

        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        scored
            .into_iter()
            .take(MAX_CONTEXT_CONCEPTS)
            .map(|(_, c)| c)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::StubProvider;
    use argos_core::okf::{ConceptType, Frontmatter};
    use chrono::Utc;
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

    #[tokio::test]
    async fn query_on_empty_wiki_returns_empty() {
        let (_dir, bundle) = setup();
        let provider = StubProvider::new("answer");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("what is rust?").await.unwrap();
        assert!(result.answer.is_empty());
        assert!(result.cited_concepts.is_empty());
    }

    #[tokio::test]
    async fn query_finds_concepts_by_keyword_in_title() {
        let (_dir, bundle) = setup();
        write_concept(
            &bundle,
            "rust.md",
            "Rust Programming",
            "Rust is a systems language.",
        );
        let provider = StubProvider::new("Rust is a systems language.");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("tell me about rust").await.unwrap();
        assert!(!result.cited_concepts.is_empty());
        assert!(result
            .cited_concepts
            .iter()
            .any(|p| p.as_path().to_string_lossy() == "rust.md"));
    }

    #[tokio::test]
    async fn query_finds_concepts_by_keyword_in_body() {
        let (_dir, bundle) = setup();
        write_concept(
            &bundle,
            "langs.md",
            "Languages",
            "Rust offers memory safety without GC.",
        );
        let provider = StubProvider::new("Rust offers memory safety.");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("rust memory safety").await.unwrap();
        assert!(!result.cited_concepts.is_empty());
    }

    #[tokio::test]
    async fn query_returns_answer_from_provider() {
        let (_dir, bundle) = setup();
        write_concept(&bundle, "rust.md", "Rust", "Rust is fast and safe.");
        let provider = StubProvider::new("Rust is a fast and safe language.");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("what is rust").await.unwrap();
        assert_eq!(result.answer, "Rust is a fast and safe language.");
    }

    #[tokio::test]
    async fn query_returns_cited_concepts() {
        let (_dir, bundle) = setup();
        write_concept(&bundle, "rust.md", "Rust", "Rust is fast and safe.");
        let provider = StubProvider::new("Rust is safe.");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("rust").await.unwrap();
        assert_eq!(result.cited_concepts.len(), 1);
    }

    #[tokio::test]
    async fn query_limits_to_top_n_relevant() {
        let (_dir, bundle) = setup();
        // Write 7 concepts all matching "rust".
        for i in 0..7 {
            write_concept(
                &bundle,
                &format!("r{i}.md"),
                &format!("Rust {i}"),
                "rust rust rust",
            );
        }
        let provider = StubProvider::new("Many rust concepts.");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("rust rust rust").await.unwrap();
        assert!(
            result.cited_concepts.len() <= MAX_CONTEXT_CONCEPTS,
            "should not exceed max context concepts"
        );
    }

    #[tokio::test]
    async fn query_ignores_concepts_with_no_keyword_overlap() {
        let (_dir, bundle) = setup();
        write_concept(&bundle, "cooking.md", "Cooking", "How to bake a cake.");
        let provider = StubProvider::new("answer");
        let op = QueryOperation::new(&provider, &bundle);

        let result = op.query("rust programming").await.unwrap();
        assert!(result.cited_concepts.is_empty());
    }
}
