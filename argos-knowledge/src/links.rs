//! Cross-link parsing, typed-relation management, and the in-memory link graph.
//!
//! [`CrossLinkParser`] extracts `[[wikilink]]` and `[text](relative.md)` links
//! from a concept body, resolving relative paths against the concept's own path
//! (ADR-010: cross-links form the slice-1 graph tier). [`RelationManager`]
//! reads/writes the typed `relates_to` block in frontmatter. [`LinkGraph`] is
//! the adjacency view over a whole [`Bundle`] used by wiki lint (orphans,
//! hubs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use argos_core::{Bundle, Concept, ConceptPath, CrossLink, RelationKind, TypedRelation};

use regex::Regex;

/// Extracts cross-links (`[text](relative.md)` and `[[wikilink]]`) from concept
/// bodies, resolving relative paths against the originating concept's path.
pub struct CrossLinkParser;

impl CrossLinkParser {
    /// Parse all cross-links out of a concept's body.
    pub fn parse(concept: &Concept) -> Vec<CrossLink> {
        Self::parse_body(&concept.path, &concept.body)
    }

    /// Parse all cross-links out of `body`, as if it belonged to the concept at
    /// `from`. Relative link targets are resolved against `from`'s parent
    /// directory. External links, anchors, and non-`.md` targets are ignored.
    pub fn parse_body(from: &ConceptPath, body: &str) -> Vec<CrossLink> {
        let mut links = Vec::new();

        let md_re = MD_LINK_RE.get_or_init(|| Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap());
        for caps in md_re.captures_iter(body) {
            let url = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if let Some(target) = clean_md_target(url) {
                if let Some(to) = resolve_relative(from, &target) {
                    links.push(CrossLink {
                        from: from.clone(),
                        to,
                        relation: None,
                    });
                }
            }
        }

        let wiki_re = WIKILINK_RE.get_or_init(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
        for caps in wiki_re.captures_iter(body) {
            let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let target = clean_wikilink_target(raw);
            if let Some(to) = resolve_relative(from, &target) {
                links.push(CrossLink {
                    from: from.clone(),
                    to,
                    relation: None,
                });
            }
        }

        links
    }
}

/// Manage typed relations stored in a concept's frontmatter `relates_to` block.
pub struct RelationManager;

impl RelationManager {
    /// Return all typed relations declared in `concept`'s frontmatter.
    pub fn get_relations(concept: &Concept) -> Vec<TypedRelation> {
        concept.frontmatter.relates_to.clone().unwrap_or_default()
    }

    /// Add a typed relation to `concept`'s frontmatter, creating the
    /// `relates_to` vec if absent. A relation with the same `page` AND `kind` as
    /// an existing one is treated as a duplicate and ignored.
    pub fn add_relation(concept: &mut Concept, relation: TypedRelation) -> argos_core::Result<()> {
        let rels = concept.frontmatter.relates_to.get_or_insert_with(Vec::new);
        if !rels
            .iter()
            .any(|r| r.page == relation.page && r.rel == relation.rel)
        {
            rels.push(relation);
        }
        Ok(())
    }

    /// Remove every relation targeting `target_page`. Resets `relates_to` to
    /// `None` when the vec becomes empty.
    pub fn remove_relation(concept: &mut Concept, target_page: &str) -> argos_core::Result<()> {
        if let Some(rels) = concept.frontmatter.relates_to.as_mut() {
            rels.retain(|r| r.page != target_page);
            if rels.is_empty() {
                concept.frontmatter.relates_to = None;
            }
        }
        Ok(())
    }

    /// Find every `(from, to)` pair where `from` declares a `contradicts`
    /// relation to `to`.
    pub fn find_contradictions(bundle: &Bundle) -> Vec<(ConceptPath, ConceptPath)> {
        let mut out = Vec::new();
        for c in &bundle.concepts {
            if let Some(rels) = &c.frontmatter.relates_to {
                for r in rels {
                    if r.rel == RelationKind::Contradicts {
                        out.push((c.path.clone(), ConceptPath::new(r.page.clone())));
                    }
                }
            }
        }
        out
    }
}

/// In-memory cross-link adjacency view over a [`Bundle`].
///
/// Built from every concept's body links plus frontmatter relations. Used by
/// wiki lint to detect orphans (no inbound links) and hubs (many inbound).
pub struct LinkGraph {
    adjacency: HashMap<ConceptPath, Vec<CrossLink>>,
    inbound_count: HashMap<ConceptPath, usize>,
    all_paths: Vec<ConceptPath>,
}

impl LinkGraph {
    /// Build the link graph from a bundle: parse each concept's body for
    /// outbound cross-links and tally inbound counts per target.
    pub fn from_bundle(bundle: &Bundle) -> LinkGraph {
        let mut adjacency: HashMap<ConceptPath, Vec<CrossLink>> = HashMap::new();
        let mut inbound_count: HashMap<ConceptPath, usize> = HashMap::new();
        let mut all_paths = Vec::with_capacity(bundle.concepts.len());
        for c in &bundle.concepts {
            all_paths.push(c.path.clone());
            let outbound = CrossLinkParser::parse(c);
            for link in &outbound {
                *inbound_count.entry(link.to.clone()).or_insert(0) += 1;
            }
            adjacency.insert(c.path.clone(), outbound);
        }
        LinkGraph {
            adjacency,
            inbound_count,
            all_paths,
        }
    }

    /// Outbound cross-links from `path` (empty if the concept has no links or
    /// is not in the graph).
    pub fn neighbors(&self, path: &ConceptPath) -> Vec<&CrossLink> {
        self.adjacency
            .get(path)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Concepts with no inbound links (potential wiki-health issue).
    pub fn orphans(&self) -> Vec<ConceptPath> {
        self.all_paths
            .iter()
            .filter(|p| !self.inbound_count.contains_key(*p))
            .cloned()
            .collect()
    }

    /// Concepts that receive inbound links, sorted by inbound count descending
    /// (hubs first). Ties break by path ascending for determinism.
    pub fn hubs(&self) -> Vec<(ConceptPath, usize)> {
        let mut v: Vec<(ConceptPath, usize)> = self
            .inbound_count
            .iter()
            .map(|(p, n)| (p.clone(), *n))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.as_path().cmp(b.0.as_path())));
        v
    }
}

// --- link target helpers ----------------------------------------------------

/// Normalise a markdown link URL into a relative `.md` target, or `None` if it
/// is external, an anchor, absolute, or not a concept (`.md`) link.
fn clean_md_target(url: &str) -> Option<String> {
    let url = url.split(['#', '?']).next().unwrap_or(url).trim();
    if url.contains("://")
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("mailto:")
    {
        return None;
    }
    if !url.ends_with(".md") {
        return None;
    }
    Some(url.to_string())
}

/// Normalise a `[[wikilink]]` target: strip `|alias` and `#heading`, and append
/// `.md` if no extension is present.
fn clean_wikilink_target(raw: &str) -> String {
    let raw = raw.split('|').next().unwrap_or(raw);
    let raw = raw.split('#').next().unwrap_or(raw);
    let raw = raw.trim();
    let mut s = raw.to_string();
    if !s.ends_with(".md") {
        s.push_str(".md");
    }
    s
}

/// Resolve a relative `target` against the parent directory of `from`,
/// collapsing `.` and `..` components and emitting forward slashes. Returns
/// `None` for external/absolute/anchor targets.
fn resolve_relative(from: &ConceptPath, target: &str) -> Option<ConceptPath> {
    if target.contains("://")
        || target.starts_with('/')
        || target.starts_with('#')
        || target.starts_with("mailto:")
    {
        return None;
    }
    let base = from
        .as_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let joined = base.join(target);
    Some(ConceptPath::new(normalize_rel(&joined)))
}

/// Collapse `.`/`..` components in a relative path and join with `/`.
fn normalize_rel(p: &Path) -> PathBuf {
    let mut out: Vec<String> = Vec::new();
    for comp in p.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(s) => out.push(s.to_string_lossy().to_string()),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
        }
    }
    PathBuf::from(out.join("/"))
}

static MD_LINK_RE: OnceLock<Regex> = OnceLock::new();
static WIKILINK_RE: OnceLock<Regex> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use argos_core::{Bundle, Concept, ConceptPath, Frontmatter, RelationKind, TypedRelation};
    use chrono::Utc;
    use std::path::PathBuf;

    fn ts() -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-06-18T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn bare_concept(path: &str, body: &str) -> Concept {
        Concept {
            path: ConceptPath::new(path),
            frontmatter: Frontmatter {
                concept_type: argos_core::ConceptType::Concept,
                title: path.into(),
                timestamp: ts(),
                description: None,
                resource: None,
                tags: None,
                relates_to: None,
            },
            body: body.into(),
        }
    }

    fn concept_with_relations(path: &str, rels: Vec<TypedRelation>) -> Concept {
        let mut c = bare_concept(path, "");
        c.frontmatter.relates_to = if rels.is_empty() { None } else { Some(rels) };
        c
    }

    // --- CrossLinkParser -----------------------------------------------------

    #[test]
    fn cross_link_parser_extracts_markdown_links_from_body() {
        let from = ConceptPath::new("workflows/daily.md");
        let links = CrossLinkParser::parse_body(&from, "See [weekly](weekly.md) for details.\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].from, from);
        assert_eq!(links[0].to, ConceptPath::new("workflows/weekly.md"));
        assert!(links[0].relation.is_none());
    }

    #[test]
    fn cross_link_parser_extracts_wikilinks_from_body() {
        let from = ConceptPath::new("workflows/daily.md");
        let links = CrossLinkParser::parse_body(&from, "See [[weekly-report]] for details.\n");
        assert_eq!(links.len(), 1);
        // Wikilinks without an extension resolve to a `.md` concept.
        assert_eq!(links[0].to, ConceptPath::new("workflows/weekly-report.md"));
    }

    #[test]
    fn cross_link_parser_resolves_relative_paths_against_concept_path() {
        let from = ConceptPath::new("workflows/daily.md");
        let body = "[a](weekly.md) [b](../overview.md) [c](sub/task.md)\n";
        let links = CrossLinkParser::parse_body(&from, body);
        let tos: Vec<ConceptPath> = links.into_iter().map(|l| l.to).collect();
        assert_eq!(
            tos,
            vec![
                ConceptPath::new("workflows/weekly.md"),
                ConceptPath::new("overview.md"),
                ConceptPath::new("workflows/sub/task.md"),
            ]
        );
    }

    #[test]
    fn cross_link_parser_returns_empty_for_body_with_no_links() {
        let from = ConceptPath::new("a.md");
        assert!(CrossLinkParser::parse_body(&from, "Just plain text, no links.\n").is_empty());
    }

    #[test]
    fn cross_link_parser_ignores_external_and_non_md_links() {
        let from = ConceptPath::new("a.md");
        let body = "[google](https://google.com) [img](diagram.png) [ok](b.md)\n";
        let links = CrossLinkParser::parse_body(&from, body);
        // Only the relative `.md` link counts as a concept cross-link.
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].to, ConceptPath::new("b.md"));
    }

    #[test]
    fn cross_link_parser_parse_uses_concept_path_and_body() {
        let concept = bare_concept("notes/a.md", "Ref to [b](b.md).\n");
        let links = CrossLinkParser::parse(&concept);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].to, ConceptPath::new("notes/b.md"));
    }

    // --- RelationManager -----------------------------------------------------

    #[test]
    fn relation_manager_get_relations_extracts_from_frontmatter() {
        let rels = vec![
            TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Extends,
            },
            TypedRelation {
                page: "c.md".into(),
                rel: RelationKind::Supports,
            },
        ];
        let concept = concept_with_relations("a.md", rels.clone());
        assert_eq!(RelationManager::get_relations(&concept), rels);
    }

    #[test]
    fn relation_manager_get_relations_empty_when_none() {
        let concept = bare_concept("a.md", "");
        assert!(RelationManager::get_relations(&concept).is_empty());
    }

    #[test]
    fn relation_manager_add_relation_adds_to_none_vec() {
        let mut concept = bare_concept("a.md", "");
        assert!(concept.frontmatter.relates_to.is_none());
        RelationManager::add_relation(
            &mut concept,
            TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Extends,
            },
        )
        .unwrap();
        assert_eq!(RelationManager::get_relations(&concept).len(), 1);
        assert_eq!(
            concept.frontmatter.relates_to.as_ref().unwrap()[0].page,
            "b.md"
        );
    }

    #[test]
    fn relation_manager_add_relation_avoids_duplicates() {
        let mut concept = concept_with_relations(
            "a.md",
            vec![TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Extends,
            }],
        );
        // Same page + same kind -> duplicate, must not be added again.
        RelationManager::add_relation(
            &mut concept,
            TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Extends,
            },
        )
        .unwrap();
        assert_eq!(RelationManager::get_relations(&concept).len(), 1);
        // Same page but different kind -> distinct, must be added.
        RelationManager::add_relation(
            &mut concept,
            TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Supports,
            },
        )
        .unwrap();
        assert_eq!(RelationManager::get_relations(&concept).len(), 2);
    }

    #[test]
    fn relation_manager_remove_relation_removes_by_target() {
        let mut concept = concept_with_relations(
            "a.md",
            vec![
                TypedRelation {
                    page: "b.md".into(),
                    rel: RelationKind::Extends,
                },
                TypedRelation {
                    page: "c.md".into(),
                    rel: RelationKind::Supports,
                },
            ],
        );
        RelationManager::remove_relation(&mut concept, "b.md").unwrap();
        let remaining = RelationManager::get_relations(&concept);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].page, "c.md");
    }

    #[test]
    fn relation_manager_remove_relation_clears_field_when_empty() {
        let mut concept = concept_with_relations(
            "a.md",
            vec![TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Extends,
            }],
        );
        RelationManager::remove_relation(&mut concept, "b.md").unwrap();
        assert!(
            concept.frontmatter.relates_to.is_none(),
            "relates_to must reset to None"
        );
    }

    #[test]
    fn relation_manager_find_contradictions_finds_pairs() {
        let a = concept_with_relations(
            "a.md",
            vec![TypedRelation {
                page: "b.md".into(),
                rel: RelationKind::Contradicts,
            }],
        );
        let b = bare_concept("b.md", "");
        let bundle = Bundle {
            root: PathBuf::from("wiki"),
            concepts: vec![a, b],
        };
        let mut contra = RelationManager::find_contradictions(&bundle);
        contra.sort_by(|x, y| x.0.as_path().cmp(y.0.as_path()));
        assert_eq!(
            contra,
            vec![(ConceptPath::new("a.md"), ConceptPath::new("b.md"))]
        );
    }

    // --- LinkGraph -----------------------------------------------------------

    fn bundle_of(concepts: Vec<Concept>) -> Bundle {
        Bundle {
            root: PathBuf::from("wiki"),
            concepts,
        }
    }

    #[test]
    fn link_graph_from_bundle_builds_adjacency() {
        let a = bare_concept("a.md", "Link to [b](b.md).\n");
        let b = bare_concept("b.md", "");
        let graph = LinkGraph::from_bundle(&bundle_of(vec![a, b]));
        let neighbors = graph.neighbors(&ConceptPath::new("a.md"));
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].to, ConceptPath::new("b.md"));
    }

    #[test]
    fn link_graph_neighbors_returns_outbound_links() {
        let a = bare_concept("a.md", "[b](b.md) [c](c.md)\n");
        let b = bare_concept("b.md", "");
        let c = bare_concept("c.md", "");
        let graph = LinkGraph::from_bundle(&bundle_of(vec![a, b, c]));
        let neighbors = graph.neighbors(&ConceptPath::new("a.md"));
        let tos: Vec<ConceptPath> = neighbors.into_iter().map(|l| l.to.clone()).collect();
        assert_eq!(
            tos,
            vec![ConceptPath::new("b.md"), ConceptPath::new("c.md")]
        );
    }

    #[test]
    fn link_graph_orphans_finds_concepts_with_no_inbound_links() {
        let a = bare_concept("a.md", "[b](b.md)\n"); // a -> b
        let b = bare_concept("b.md", ""); // linked by a
        let c = bare_concept("c.md", ""); // standalone
        let graph = LinkGraph::from_bundle(&bundle_of(vec![a, b, c]));
        let mut orphans = graph.orphans();
        orphans.sort_by(|x, y| x.as_path().cmp(y.as_path()));
        // b has an inbound link; a and c do not.
        assert_eq!(
            orphans,
            vec![ConceptPath::new("a.md"), ConceptPath::new("c.md")]
        );
    }

    #[test]
    fn link_graph_hubs_sorts_by_inbound_count_desc() {
        let a = bare_concept("a.md", "[b](b.md) [c](c.md)\n"); // a -> b, a -> c
        let b = bare_concept("b.md", "");
        let c = bare_concept("c.md", "[b](b.md)\n"); // c -> b
        let d = bare_concept("d.md", "[b](b.md)\n"); // d -> b
        let graph = LinkGraph::from_bundle(&bundle_of(vec![a, b, c, d]));
        let hubs = graph.hubs();
        // b has 3 inbound (a, c, d); c has 1 (a). Sorted descending.
        assert_eq!(hubs.len(), 2);
        assert_eq!(hubs[0], (ConceptPath::new("b.md"), 3));
        assert_eq!(hubs[1], (ConceptPath::new("c.md"), 1));
    }
}
