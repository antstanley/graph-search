//! The Open Knowledge Format extractor (`SPEC.md` §7.6), over
//! `tree-sitter-okf`.
//!
//! A bundle document becomes one `concept` (unless it is a reserved `index.md`
//! or `log.md`) and one `section` per heading, nested as the headings nest.
//! Cross-links become `links_to` references and `sources[]` resources become
//! `cites` references: from the concept for every source, and from the
//! section holding each `[^id]` footnote that names a source. `core` resolves
//! both kinds as bundle paths, never as symbol names.

use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_core::okf::is_external;
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use std::collections::BTreeMap;
use std::path::Path;
use tree_sitter::Node;

use crate::walk::{span_of, text};

/// Separates a section's name from its enclosing concept or section.
const SEPARATOR: &str = " > ";
/// The longest signature kept, in characters (a one-line summary).
const MAX_SIGNATURE_CHARS: usize = 160;

/// The OKF extractor.
#[derive(Debug, Default)]
pub struct OkfExtractor;

impl LanguageExtractor for OkfExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::Okf
    }

    /// Every Markdown path; the walk decides which are bundle members (or an
    /// `[extensions]` binding claims them), and only those reach this extractor.
    fn supports(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("md" | "markdown")
        )
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_okf::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("okf grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the okf parser produced no tree"));
        };
        let root = tree.root_node();
        let fields = root
            .child_by_field_name("frontmatter")
            .map_or(Value::Map(Vec::new()), |frontmatter| {
                frontmatter_fields(frontmatter, file.text)
            });
        let mut extractor = Extractor {
            source: file.text,
            definitions: BTreeMap::new(),
            sources: BTreeMap::new(),
            keys: std::collections::BTreeSet::new(),
            extraction: Extraction::default(),
        };
        let concept =
            (!is_reserved(file.path)).then(|| extractor.concept(root, file.path, &fields));
        if let Some(body) = root.child_by_field_name("body") {
            extractor.collect_definitions(body);
            extractor.walk(
                body,
                concept.as_ref().map(|(k, q)| (k.as_str(), q.as_str())),
            );
        }
        Ok(extractor.extraction)
    }
}

/// `index.md` and `log.md` are reserved at every level and are never concepts
/// (OKF §3.1).
fn is_reserved(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(std::ffi::OsStr::to_str),
        Some("index.md" | "log.md")
    )
}

/// A frontmatter value, reduced to what extraction reads. Scalars are strings;
/// OKF-YAML never types them (tree-sitter-okf spec D3).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Scalar(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    fn scalar(&self) -> Option<&str> {
        match self {
            Self::Scalar(text) if !text.is_empty() => Some(text),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Map(pairs) => pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A list, or a lone value as a one-element list (OKF §5.2 does this for
    /// `verified`; producers do it for `sources` and `tags` too).
    fn items(&self) -> Vec<&Value> {
        match self {
            Self::List(items) => items.iter().collect(),
            other => vec![other],
        }
    }
}

/// The top-level frontmatter mapping; later duplicate keys win.
fn frontmatter_fields(frontmatter: Node<'_>, source: &str) -> Value {
    named_children(frontmatter)
        .find(|child| child.kind() == "block_mapping")
        .map_or(Value::Map(Vec::new()), |mapping| value(mapping, source))
}

fn value(node: Node<'_>, source: &str) -> Value {
    match node.kind() {
        "block_mapping" | "flow_mapping" => Value::Map(
            named_children(node)
                .filter(|pair| matches!(pair.kind(), "block_mapping_pair" | "flow_pair"))
                .filter_map(|pair| {
                    let key = scalar_text(pair.child_by_field_name("key")?, source);
                    let value = pair
                        .child_by_field_name("value")
                        .map_or(Value::Scalar(String::new()), |value| {
                            self::value(value, source)
                        });
                    Some((key, value))
                })
                .collect(),
        ),
        "block_sequence" => Value::List(
            named_children(node)
                .filter(|item| item.kind() == "block_sequence_item")
                .map(|item| {
                    named_children(item)
                        .find(|child| !matches!(child.kind(), "anchor" | "tag" | "comment"))
                        .map_or(Value::Scalar(String::new()), |child| value(child, source))
                })
                .collect(),
        ),
        "flow_sequence" => Value::List(
            named_children(node)
                .filter(|child| !matches!(child.kind(), "anchor" | "tag" | "comment"))
                .map(|child| value(child, source))
                .collect(),
        ),
        "plain_scalar" | "double_quote_scalar" | "single_quote_scalar" | "block_scalar" => {
            Value::Scalar(scalar_text(node, source))
        }
        _ => Value::Scalar(String::new()),
    }
}

/// A scalar's text: quotes removed, the common escapes decoded and folded
/// lines joined by single spaces.
fn scalar_text(node: Node<'_>, source: &str) -> String {
    let raw = text(node, source);
    let unquoted = match node.kind() {
        "double_quote_scalar" => raw
            .get(1..raw.len().saturating_sub(1))
            .unwrap_or_default()
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
        "single_quote_scalar" => raw
            .get(1..raw.len().saturating_sub(1))
            .unwrap_or_default()
            .replace("''", "'"),
        "block_scalar" => raw
            .split_once('\n')
            .map_or_else(String::new, |(_, body)| body.to_owned()),
        _ => raw.to_owned(),
    };
    unquoted.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter()
}

/// A reference label, normalized as `CommonMark` matches them: case-folded,
/// with internal whitespace collapsed.
fn label_key(label: &str) -> String {
    label
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_SIGNATURE_CHARS {
        return collapsed;
    }
    let mut cut: String = collapsed
        .chars()
        .take(MAX_SIGNATURE_CHARS.saturating_sub(1))
        .collect();
    cut.push('…');
    cut
}

struct Extractor<'a> {
    source: &'a str,
    /// Link reference definitions by normalized label.
    definitions: BTreeMap<String, String>,
    /// `sources[].id` to its `resource`, when that names a bundle path.
    sources: BTreeMap<String, String>,
    /// Fact keys already issued; a repeated heading gets a line-suffixed key.
    keys: std::collections::BTreeSet<String>,
    extraction: Extraction,
}

impl Extractor<'_> {
    /// The document's concept, returning its key and qualified name. Every
    /// `sources[]` resource that names a path is cited from it.
    fn concept(&mut self, root: Node<'_>, path: &Path, fields: &Value) -> (String, String) {
        let stem = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let name = fields
            .get("title")
            .and_then(Value::scalar)
            .map_or_else(|| stem.to_owned(), str::to_owned);
        let key = format!("{}:{name}", NodeKind::Concept.as_str());
        self.keys.insert(key.clone());
        let span = span_of(root);
        let okf_type = fields.get("type").and_then(Value::scalar);
        let description = fields.get("description").and_then(Value::scalar);
        let signature = match (okf_type, description) {
            (Some(kind), Some(description)) => format!("{kind}: {description}"),
            (Some(kind), None) => kind.to_owned(),
            (None, Some(description)) => description.to_owned(),
            (None, None) => name.clone(),
        };
        let mut fact = SymbolFact::new(&key, NodeKind::Concept, &name, &name, span)
            .with_signature(one_line(&signature))
            .with_attribute("concept_stem", stem);
        for field in ["type", "status", "resource", "stale_after", "description"] {
            if let Some(value) = fields.get(field).and_then(Value::scalar) {
                fact = fact.with_attribute(&format!("okf_{field}"), value);
            }
        }
        if let Some(tags) = fields.get("tags") {
            let tags: Vec<_> = tags.items().into_iter().filter_map(Value::scalar).collect();
            if !tags.is_empty() {
                fact = fact.with_attribute("okf_tags", tags.join(","));
            }
        }
        if let Some(verified) = fields.get("verified") {
            fact = fact.with_attribute("okf_trust", trust_tier(verified));
        }
        self.extraction.symbols.push(fact);

        for source in fields.get("sources").map(Value::items).unwrap_or_default() {
            let Some(resource) = source.get("resource").and_then(Value::scalar) else {
                continue;
            };
            if is_external(resource) {
                continue;
            }
            if let Some(id) = source.get("id").and_then(Value::scalar) {
                self.sources.insert(id.to_owned(), resource.to_owned());
            }
            self.extraction.references.push(
                ReferenceFact::from_symbol(&key, EdgeKind::Cites, resource, span.start_line)
                    .at(span),
            );
        }
        (key, name)
    }

    fn collect_definitions(&mut self, node: Node<'_>) {
        crate::walk::walk(node, &mut |child| {
            if child.kind() == "link_reference_definition" {
                let label = named_children(child).find(|c| c.kind() == "link_label");
                let destination = named_children(child).find(|c| c.kind() == "link_destination");
                if let (Some(label), Some(destination)) = (label, destination) {
                    self.definitions
                        .entry(label_key(text(label, self.source)))
                        .or_insert_with(|| text(destination, self.source).to_owned());
                }
                return false;
            }
            true
        });
    }

    /// Walks body blocks under `owner` (the enclosing section or concept).
    fn walk(&mut self, node: Node<'_>, owner: Option<(&str, &str)>) {
        match node.kind() {
            "section" => {
                if let Some((key, qualified)) = self.section(node, owner) {
                    for child in named_children(node) {
                        self.walk(child, Some((&key, &qualified)));
                    }
                    return;
                }
            }
            "inline_link" | "image" => {
                let destination = named_children(node)
                    .find(|c| c.kind() == "link_destination")
                    .map(|destination| text(destination, self.source).to_owned())
                    // `![alt][label]` is a reference image.
                    .or_else(|| {
                        named_children(node)
                            .find(|c| c.kind() == "link_label")
                            .map(|label| label_key(text(label, self.source)))
                            .and_then(|label| self.definitions.get(&label).cloned())
                    });
                if let Some(destination) = destination {
                    self.link(node, &destination, owner);
                }
            }
            "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
                let label = named_children(node)
                    .find(|c| c.kind() == "link_label")
                    .or_else(|| named_children(node).find(|c| c.kind() == "link_text"));
                if let Some(destination) = label
                    .map(|label| label_key(text(label, self.source)))
                    .and_then(|label| self.definitions.get(&label).cloned())
                {
                    self.link(node, &destination, owner);
                }
            }
            "footnote_reference" => {
                if let Some(resource) = node
                    .child_by_field_name("label")
                    .map(|label| text(label, self.source))
                    .and_then(|label| self.sources.get(label).cloned())
                {
                    self.reference(node, EdgeKind::Cites, &resource, owner);
                }
                return;
            }
            "link_reference_definition"
            | "fenced_code_block"
            | "indented_code_block"
            | "html_block"
            | "code_span" => return,
            _ => {}
        }
        for child in named_children(node) {
            self.walk(child, owner);
        }
    }

    /// A `section` symbol named by its heading; `None` for a heading with no
    /// text.
    fn section(&mut self, node: Node<'_>, owner: Option<(&str, &str)>) -> Option<(String, String)> {
        let heading = named_children(node)
            .find(|child| matches!(child.kind(), "atx_heading" | "setext_heading"))?;
        let content = heading.child_by_field_name("heading_content")?;
        let name = one_line(text(content, self.source));
        if name.is_empty() {
            return None;
        }
        let (mut key, qualified) = crate::walk::qualify(owner, &name, NodeKind::Section, SEPARATOR);
        let span = span_of(node);
        if self.keys.contains(&key) {
            key = format!("{key}@{}", span.start_line);
        }
        self.keys.insert(key.clone());
        let mut fact = SymbolFact::new(&key, NodeKind::Section, &name, &qualified, span)
            .with_signature(one_line(text(heading, self.source)));
        if let Some((parent, _)) = owner {
            fact = fact.with_parent(parent);
        }
        self.extraction.symbols.push(fact);
        Some((key, qualified))
    }

    fn link(&mut self, node: Node<'_>, destination: &str, owner: Option<(&str, &str)>) {
        let destination = destination.trim();
        if !is_external(destination) {
            self.reference(node, EdgeKind::LinksTo, destination, owner);
        }
    }

    fn reference(
        &mut self,
        node: Node<'_>,
        kind: EdgeKind,
        target: &str,
        owner: Option<(&str, &str)>,
    ) {
        let span = span_of(node);
        let mut fact = match owner {
            Some((key, _)) => ReferenceFact::from_symbol(key, kind, target, span.start_line),
            None => ReferenceFact::file_level(kind, target, span.start_line),
        }
        .at(span);
        fact.raw_name = Some(text(node, self.source).to_owned());
        self.extraction.references.push(fact);
    }
}

/// OKF §5.3: `human-reviewed` when any `verified[].by` is a `human:` actor,
/// else `machine-confirmed`. A bare mapping is a one-element list.
fn trust_tier(verified: &Value) -> &'static str {
    let human = verified.items().into_iter().any(|entry| {
        entry
            .get("by")
            .and_then(Value::scalar)
            .is_some_and(|by| by.starts_with("human:"))
    });
    if human {
        "human-reviewed"
    } else {
        "machine-confirmed"
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn extract(path: &str, text: &str) -> Extraction {
        OkfExtractor
            .extract(&SourceFile {
                path: Path::new(path),
                text,
            })
            .unwrap()
    }

    const CONCEPT: &str = "---
type: Metric
title: Gross Margin
description: Revenue minus full COGS.
tags: [finance, margin]
verified:
  - { by: human:jsmith@acme, at: 2026-07-01T09:00:00Z }
sources:
  - id: margin-standard
    resource: policies/margin-standard.md
  - id: web
    resource: https://example.com/standard
---

# Definition

Gross margin equals [Revenue](./revenue.md) minus COGS.[^margin-standard]

See [the table][orders] and [docs](https://example.com) and [top](#definition).

## Formula

Uses the [period](/computations/period.md).

[orders]: ../tables/orders.md

[^margin-standard]: The FY2026 standard.
";

    #[test]
    fn a_concept_its_sections_links_and_citations() {
        let facts = extract("kb/metrics/gross-margin.md", CONCEPT);
        let symbols: Vec<_> = facts
            .symbols
            .iter()
            .map(|s| (s.kind, s.qualified_name.as_str(), s.parent_key.as_deref()))
            .collect();
        assert_eq!(
            symbols,
            [
                (NodeKind::Concept, "Gross Margin", None),
                (
                    NodeKind::Section,
                    "Gross Margin > Definition",
                    Some("concept:Gross Margin")
                ),
                (
                    NodeKind::Section,
                    "Gross Margin > Definition > Formula",
                    Some("concept:Gross Margin>section:Gross Margin > Definition")
                ),
            ]
        );
        let concept = &facts.symbols[0];
        assert_eq!(
            concept.signature.as_deref(),
            Some("Metric: Revenue minus full COGS.")
        );
        assert_eq!(concept.attributes["okf_type"], "Metric");
        assert_eq!(concept.attributes["okf_tags"], "finance,margin");
        assert_eq!(concept.attributes["okf_trust"], "human-reviewed");
        assert_eq!(concept.attributes["concept_stem"], "gross-margin");

        let references: Vec<_> = facts
            .references
            .iter()
            .map(|r| (r.kind, r.name.as_str(), r.from_key.as_deref().unwrap_or("")))
            .collect();
        let definition = "concept:Gross Margin>section:Gross Margin > Definition";
        let formula = "concept:Gross Margin>section:Gross Margin > Definition>section:Gross Margin > Definition > Formula";
        assert_eq!(
            references,
            [
                (
                    EdgeKind::Cites,
                    "policies/margin-standard.md",
                    "concept:Gross Margin"
                ),
                (EdgeKind::LinksTo, "./revenue.md", definition),
                (EdgeKind::Cites, "policies/margin-standard.md", definition),
                (EdgeKind::LinksTo, "../tables/orders.md", definition),
                (EdgeKind::LinksTo, "/computations/period.md", formula),
            ]
        );
    }

    #[test]
    fn repeated_headings_keep_distinct_keys_and_parents() {
        let facts = extract(
            "kb/api.md",
            "---\ntitle: T\n---\n# API\n## Example\n### Sub\n## Example\n\n![d][img]\n\n[img]: d.md\n",
        );
        let keys: std::collections::BTreeSet<_> =
            facts.symbols.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys.len(), facts.symbols.len(), "{:?}", facts.symbols);
        let example = "concept:T>section:T > API>section:T > API > Example";
        let sub = facts.symbols.iter().find(|s| s.name == "Sub").unwrap();
        assert_eq!(sub.parent_key.as_deref(), Some(example));
        let second = facts.symbols.last().unwrap();
        assert_eq!(second.key, format!("{example}@7"));
        let image = &facts.references[0];
        assert_eq!(
            (image.kind, image.name.as_str()),
            (EdgeKind::LinksTo, "d.md")
        );
        assert_eq!(image.from_key.as_deref(), Some(second.key.as_str()));
    }

    #[test]
    fn reserved_files_have_sections_but_no_concept() {
        let facts = extract(
            "kb/index.md",
            "# Tables\n\n* [orders](tables/orders.md) - Orders.\n",
        );
        assert_eq!(facts.symbols.len(), 1);
        assert_eq!(facts.symbols[0].kind, NodeKind::Section);
        assert_eq!(facts.references[0].name, "tables/orders.md");
        assert_eq!(
            facts.references[0].from_key.as_deref(),
            Some("section:Tables")
        );
    }

    #[test]
    fn a_document_without_frontmatter_is_named_by_its_file() {
        let facts = extract("kb/notes.md", "Plain text with a [link](other.md).\n");
        assert_eq!(facts.symbols[0].kind, NodeKind::Concept);
        assert_eq!(facts.symbols[0].name, "notes");
        assert_eq!(
            facts.references[0].from_key.as_deref(),
            Some("concept:notes")
        );
        let empty = extract("kb/log.md", "");
        assert!(empty.symbols.is_empty() && empty.references.is_empty());
    }
}
