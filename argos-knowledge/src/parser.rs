//! OKF frontmatter parser and writer.
//!
//! An OKF concept file is markdown with a YAML frontmatter block delimited by
//! `---` lines. [`OkfParser`] turns file contents into a [`Concept`]; [`OkfWriter`]
//! serialises a [`Concept`] back to markdown with round-trip fidelity
//! (`parse → write → parse` is the identity on [`Concept`]).

use argos_core::{ArgosError, Concept, ConceptPath, Frontmatter, Result};

/// Parses OKF markdown (frontmatter + body) into a [`Concept`].
pub struct OkfParser;

/// Serialises a [`Concept`] back into OKF markdown.
pub struct OkfWriter;

impl OkfParser {
    /// Parse `content` (a full OKF markdown file) into a [`Concept`] at `path`.
    ///
    /// The frontmatter is deserialised into [`Frontmatter`] via serde_yaml; the
    /// body is everything after the closing `---` delimiter, with the single
    /// separating blank line stripped. Body bytes are preserved exactly
    /// (including any trailing newline).
    pub fn parse(path: ConceptPath, content: &str) -> Result<Concept> {
        let (fm_yaml, body) = Self::split(content)?;
        let frontmatter: Frontmatter = serde_yaml::from_str(&fm_yaml)
            .map_err(|e| ArgosError::Knowledge(format!("invalid frontmatter YAML: {e}")))?;
        Ok(Concept {
            path,
            frontmatter,
            body,
        })
    }

    /// Split OKF markdown into `(frontmatter_yaml, body)`.
    fn split(content: &str) -> Result<(String, String)> {
        // Opening delimiter: the first line must be exactly `---`.
        let (open_len, open_line) = take_line(content);
        match open_line {
            Some("---") => {}
            _ => {
                return Err(ArgosError::Knowledge(
                    "missing frontmatter: content must start with a `---` line".into(),
                ))
            }
        }
        let rest = &content[open_len..];

        // Closing delimiter: the first subsequent line equal to `---`.
        let mut cursor = 0usize;
        let mut close_start: Option<usize> = None;
        while cursor < rest.len() {
            let (line_len, line) = take_line(&rest[cursor..]);
            if line.map(|l| l == "---").unwrap_or(false) {
                close_start = Some(cursor);
                break;
            }
            cursor += line_len;
        }
        let close_start = close_start.ok_or_else(|| {
            ArgosError::Knowledge("missing frontmatter: no closing `---` delimiter".into())
        })?;

        let fm_yaml = rest[..close_start].to_string();
        let (close_len, _) = take_line(&rest[close_start..]);
        let after_close = &rest[close_start + close_len..];
        // Strip the blank line(s) separating frontmatter from body.
        let body = after_close.trim_start_matches(['\r', '\n']).to_string();
        Ok((fm_yaml, body))
    }
}

impl OkfWriter {
    /// Serialise a [`Concept`] into OKF markdown (`---\n<yaml>\n---\n\n<body>`).
    pub fn write(concept: &Concept) -> Result<String> {
        let yaml = serde_yaml::to_string(&concept.frontmatter)
            .map_err(|e| ArgosError::Knowledge(format!("failed to serialize frontmatter: {e}")))?;
        // `serde_yaml::to_string` ends with a trailing newline, so `{yaml}---`
        // produces a well-formed closing delimiter on its own line.
        Ok(format!("---\n{yaml}---\n\n{}", concept.body))
    }
}

/// Return `(bytes_consumed_including_terminator, line_content_without_terminator)`.
///
/// Handles both `\n` and `\r\n` terminators. A trailing line with no terminator
/// is returned whole.
fn take_line(s: &str) -> (usize, Option<&str>) {
    match s.find('\n') {
        Some(i) => {
            let consumed = i + 1;
            let line = if s[..i].ends_with('\r') {
                &s[..i - 1]
            } else {
                &s[..i]
            };
            (consumed, Some(line))
        }
        None => {
            if s.is_empty() {
                (0, None)
            } else {
                (s.len(), Some(s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{ConceptType, Frontmatter, RelationKind, TypedRelation};
    use chrono::Utc;

    /// Canonical OKF sample used across several tests.
    fn sample_markdown() -> &'static str {
        "---\n\
         type: workflow\n\
         title: Daily Report\n\
         timestamp: 2026-06-18T12:00:00Z\n\
         description: Generates and sends a daily summary email.\n\
         resource: n8n://workflows/42\n\
         tags: [email, report]\n\
         relates_to:\n  \
           - page: workflows/weekly-report.md\n    \
             rel: extends\n\
         ---\n\
         \n\
         # Daily Report\n\
         \n\
         This workflow generates and sends a daily summary email to the team.\n"
    }

    #[test]
    fn parser_parses_markdown_with_frontmatter_into_concept() {
        let concept = OkfParser::parse(
            ConceptPath::new("workflows/daily-report.md"),
            sample_markdown(),
        )
        .expect("valid OKF markdown should parse");
        assert_eq!(concept.path, ConceptPath::new("workflows/daily-report.md"));
        assert_eq!(concept.frontmatter.concept_type, ConceptType::Workflow);
        assert_eq!(concept.frontmatter.title, "Daily Report");
        assert_eq!(
            concept.frontmatter.resource.as_deref(),
            Some("n8n://workflows/42")
        );
        assert_eq!(
            concept.frontmatter.tags.as_deref(),
            Some(&["email".to_string(), "report".to_string()][..])
        );
        assert_eq!(
            concept.frontmatter.relates_to.as_ref().unwrap()[0].rel,
            RelationKind::Extends
        );
    }

    #[test]
    fn parser_extracts_body_after_second_delimiter() {
        let concept = OkfParser::parse(ConceptPath::new("a.md"), sample_markdown()).unwrap();
        assert_eq!(
            concept.body,
            "# Daily Report\n\nThis workflow generates and sends a daily summary email to the team.\n"
        );
    }

    #[test]
    fn parser_errors_on_missing_frontmatter() {
        let no_fm = "# just a heading\n\nNo frontmatter here at all.\n";
        let res = OkfParser::parse(ConceptPath::new("a.md"), no_fm);
        assert!(res.is_err(), "content without frontmatter must error");
    }

    #[test]
    fn parser_errors_on_invalid_yaml() {
        let bad_yaml = "---\ntype: workflow\ntitle: X\ntimestamp: 2026-06-18T12:00:00Z\ntags: [unclosed\n---\n\nbody\n";
        let res = OkfParser::parse(ConceptPath::new("a.md"), bad_yaml);
        assert!(res.is_err(), "invalid YAML frontmatter must error");
    }

    #[test]
    fn parser_errors_on_missing_required_type_field() {
        let no_type = "---\ntitle: Untyped\ntimestamp: 2026-06-18T12:00:00Z\n---\n\nbody\n";
        let res = OkfParser::parse(ConceptPath::new("a.md"), no_type);
        assert!(
            res.is_err(),
            "frontmatter missing the required `type` field must error"
        );
    }

    #[test]
    fn parser_errors_on_unterminated_frontmatter() {
        // Opening delimiter but no closing `---`.
        let unterminated =
            "---\ntype: workflow\ntitle: X\ntimestamp: 2026-06-18T12:00:00Z\n\nbody never closes\n";
        let res = OkfParser::parse(ConceptPath::new("a.md"), unterminated);
        assert!(res.is_err(), "unterminated frontmatter must error");
    }

    fn sample_concept() -> Concept {
        Concept {
            path: ConceptPath::new("workflows/daily-report.md"),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Workflow,
                title: "Daily Report".into(),
                timestamp: chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                description: Some("Generates and sends a daily summary email.".into()),
                resource: Some("n8n://workflows/42".into()),
                tags: Some(vec!["email".into(), "report".into()]),
                relates_to: Some(vec![TypedRelation {
                    page: "workflows/weekly-report.md".into(),
                    rel: RelationKind::Extends,
                }]),
            },
            body: "# Daily Report\n\nSends a summary.\n".into(),
        }
    }

    #[test]
    fn writer_serialises_concept_to_markdown() {
        let md = OkfWriter::write(&sample_concept()).expect("write should succeed");
        assert!(
            md.starts_with("---\n"),
            "output must start with frontmatter delimiter"
        );
        assert!(md.contains("type: workflow"));
        assert!(md.contains("title: Daily Report"));
        assert!(
            md.contains("\n---\n\n"),
            "output must close frontmatter and start body"
        );
        assert!(md.ends_with("# Daily Report\n\nSends a summary.\n"));
    }

    #[test]
    fn round_trip_parse_write_parse_yields_same_concept() {
        let original = OkfParser::parse(
            ConceptPath::new("workflows/daily-report.md"),
            sample_markdown(),
        )
        .expect("parse should succeed");
        let written = OkfWriter::write(&original).expect("write should succeed");
        let reparsed = OkfParser::parse(ConceptPath::new("workflows/daily-report.md"), &written)
            .expect("reparse should succeed");
        assert_eq!(
            original, reparsed,
            "parse -> write -> parse must be identity"
        );
    }

    #[test]
    fn round_trip_preserves_optional_none_fields() {
        // A concept with no optional fields must still round-trip.
        let concept = Concept {
            path: ConceptPath::new("bare.md"),
            frontmatter: Frontmatter {
                concept_type: ConceptType::Concept,
                title: "Bare".into(),
                timestamp: chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: "Just a body.\n".into(),
        };
        let written = OkfWriter::write(&concept).unwrap();
        let reparsed = OkfParser::parse(ConceptPath::new("bare.md"), &written).unwrap();
        assert_eq!(concept, reparsed);
        // Optional fields must not be emitted when None.
        assert!(!written.contains("description:"));
        assert!(!written.contains("tags:"));
    }
}
