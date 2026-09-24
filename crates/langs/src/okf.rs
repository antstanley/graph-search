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
            footnote_owners: BTreeMap::new(),
            extraction: Extraction::default(),
        };
        let concept =
            (!is_reserved(file.path)).then(|| extractor.concept(root, file.path, &fields));
        let concept = concept.as_ref().map(|(k, q)| (k.as_str(), q.as_str()));
        if let Some(body) = root.child_by_field_name("body") {
            extractor.collect_definitions(body);
            let mut footnotes = Vec::new();
            extractor.walk(body, concept, &mut footnotes);
            // A footnote's links belong to the section that cites it, not to
            // wherever its definition happens to sit.
            for (definition, fallback) in footnotes {
                let label = definition
                    .child_by_field_name("label")
                    .map(|label| text(label, extractor.source).to_owned());
                let owner = label
                    .and_then(|label| extractor.footnote_owners.get(&label).cloned())
                    .or(fallback);
                let owner = owner.as_ref().map(|(k, q)| (k.as_str(), q.as_str()));
                let mut nested = Vec::new();
                for child in named_children(definition) {
                    extractor.walk(child, owner, &mut nested);
                }
            }
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
        .find(|child| matches!(child.kind(), "block_mapping" | "flow_mapping"))
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
        "double_quote_scalar" => {
            yaml_unescape(raw.get(1..raw.len().saturating_sub(1)).unwrap_or_default())
        }
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

/// Decodes YAML double-quoted escapes (YAML 1.2 §5.7). An escape that is not
/// well formed is kept as written, as tree-sitter-okf's host helpers do.
fn yaml_unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escape) = chars.next() else {
            out.push(ch);
            break;
        };
        let simple = match escape {
            '0' => Some('\0'),
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            't' | '\t' => Some('\t'),
            'n' => Some('\n'),
            'v' => Some('\u{b}'),
            'f' => Some('\u{c}'),
            'r' => Some('\r'),
            'e' => Some('\u{1b}'),
            ' ' => Some(' '),
            '"' => Some('"'),
            '/' => Some('/'),
            '\\' => Some('\\'),
            'N' => Some('\u{85}'),
            '_' => Some('\u{a0}'),
            'L' => Some('\u{2028}'),
            'P' => Some('\u{2029}'),
            _ => None,
        };
        if let Some(decoded) = simple {
            out.push(decoded);
            continue;
        }
        let width = match escape {
            'x' => 2,
            'u' => 4,
            'U' => 8,
            _ => 0,
        };
        let digits: String = chars.clone().take(width).collect();
        let decoded = (width > 0 && digits.len() == width)
            .then(|| u32::from_str_radix(&digits, 16).ok())
            .flatten()
            .and_then(char::from_u32);
        if let Some(decoded) = decoded {
            out.push(decoded);
            for _ in 0..width {
                chars.next();
            }
        } else {
            out.push('\\');
            out.push(escape);
        }
    }
    out
}

/// The text a reader sees in inline content: link and image text without
/// their destinations, code without delimiters, emphasis without markers,
/// escapes and character references decoded, footnote markers and raw HTML
/// dropped.
fn plain(node: Node<'_>, source: &str) -> String {
    let mut out = String::new();
    render(node, source, &mut out);
    out
}

fn render(node: Node<'_>, source: &str, out: &mut String) {
    match node.kind() {
        "inline_link" | "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            if let Some(text) = named_children(node).find(|c| c.kind() == "link_text") {
                render(text, source, out);
            }
            return;
        }
        "image" => {
            if let Some(text) = named_children(node).find(|c| c.kind() == "image_description") {
                render(text, source, out);
            }
            return;
        }
        "backslash_escape" => {
            out.push_str(text(node, source).get(1..).unwrap_or_default());
            return;
        }
        "entity_reference" | "numeric_character_reference" => {
            out.push_str(&decode_reference(text(node, source)));
            return;
        }
        "footnote_reference" | "html_tag" | "block_continuation" => return,
        kind if kind.ends_with("_delimiter") => return,
        _ => {}
    }
    let mut cursor = node.walk();
    let mut position = node.start_byte();
    for child in node.children(&mut cursor) {
        out.push_str(source.get(position..child.start_byte()).unwrap_or_default());
        render(child, source, out);
        position = child.end_byte();
    }
    out.push_str(source.get(position..node.end_byte()).unwrap_or_default());
}

/// Decodes `&name;`, `&#nn;` and `&#xhh;`; an unknown name is kept as written.
fn decode_reference(reference: &str) -> String {
    let body = reference
        .strip_prefix('&')
        .and_then(|rest| rest.strip_suffix(';'))
        .unwrap_or_default();
    let code = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok()
    } else if let Some(decimal) = body.strip_prefix('#') {
        decimal.parse().ok()
    } else {
        None
    };
    let named = match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    };
    named
        .or_else(|| code.and_then(char::from_u32))
        .map_or_else(|| reference.to_owned(), String::from)
}

/// Strips an ATX heading's optional closing sequence (`## Foo ##`): a run of
/// `#` that is the whole text or follows a space.
fn strip_closing_sequence(heading: &str) -> &str {
    let trimmed = heading.trim_end();
    let without = trimmed.trim_end_matches('#');
    if without.len() == trimmed.len() {
        return trimmed;
    }
    if without.is_empty() || without.ends_with([' ', '\t']) {
        without.trim_end()
    } else {
        trimmed
    }
}

/// Whether a `sources[].resource` names a path rather than a scope descriptor
/// (OKF §5.1: "all queries in project X"). A path has no whitespace, and a
/// separator or a file extension.
fn looks_like_path(resource: &str) -> bool {
    !resource.contains(char::is_whitespace)
        && (resource.contains('/')
            || resource
                .rsplit('/')
                .next()
                .is_some_and(|name| name.contains('.')))
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

/// A footnote definition, walked after the body with its enclosing owner as
/// the fallback when no section cites it.
type Deferred<'t> = (Node<'t>, Option<(String, String)>);

struct Extractor<'a> {
    source: &'a str,
    /// Link reference definitions by normalized label.
    definitions: BTreeMap<String, String>,
    /// `sources[].id` to its `resource`, when that names a bundle path.
    sources: BTreeMap<String, String>,
    /// Fact keys already issued; a repeated heading gets a line-suffixed key.
    keys: std::collections::BTreeSet<String>,
    /// The first section (or concept) citing each footnote label.
    footnote_owners: BTreeMap<String, (String, String)>,
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
        fact = fact.with_attribute("okf_trust", trust_tier(fields.get("verified")));
        self.extraction.symbols.push(fact);

        for source in fields.get("sources").map(Value::items).unwrap_or_default() {
            let Some(resource) = source.get("resource").and_then(Value::scalar) else {
                continue;
            };
            if is_external(resource) || !looks_like_path(resource) {
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
                        .or_insert_with(|| plain(destination, self.source));
                }
                return false;
            }
            true
        });
    }

    /// Walks body blocks under `owner` (the enclosing section or concept).
    /// Footnote definitions are deferred with their owner as a fallback.
    fn walk<'t>(
        &mut self,
        node: Node<'t>,
        owner: Option<(&str, &str)>,
        footnotes: &mut Vec<Deferred<'t>>,
    ) {
        match node.kind() {
            "section" => {
                if let Some((key, qualified)) = self.section(node, owner) {
                    for child in named_children(node) {
                        self.walk(child, Some((&key, &qualified)), footnotes);
                    }
                    return;
                }
            }
            "footnote_definition" => {
                footnotes.push((node, owner.map(|(k, q)| (k.to_owned(), q.to_owned()))));
                return;
            }
            "inline_link" | "image" => {
                let destination = named_children(node)
                    .find(|c| c.kind() == "link_destination")
                    .map(|destination| plain(destination, self.source))
                    // `![alt][label]`, `![alt][]` and `![alt]` are reference images.
                    .or_else(|| {
                        named_children(node)
                            .find(|c| c.kind() == "link_label")
                            .or_else(|| {
                                (node.kind() == "image")
                                    .then(|| {
                                        named_children(node)
                                            .find(|c| c.kind() == "image_description")
                                    })
                                    .flatten()
                            })
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
                let label = node
                    .child_by_field_name("label")
                    .map(|label| text(label, self.source).to_owned());
                if let (Some(label), Some((key, qualified))) = (&label, owner) {
                    self.footnote_owners
                        .entry(label.clone())
                        .or_insert_with(|| (key.to_owned(), qualified.to_owned()));
                }
                if let Some(resource) = label.and_then(|label| self.sources.get(&label).cloned()) {
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
            self.walk(child, owner, footnotes);
        }
    }

    /// A `section` symbol named by its heading; `None` for a heading with no
    /// text.
    fn section(&mut self, node: Node<'_>, owner: Option<(&str, &str)>) -> Option<(String, String)> {
        let heading = named_children(node)
            .find(|child| matches!(child.kind(), "atx_heading" | "setext_heading"))?;
        let content = heading.child_by_field_name("heading_content")?;
        let mut rendered = plain(content, self.source);
        if heading.kind() == "atx_heading" {
            let closing = text(content, self.source);
            let kept = strip_closing_sequence(closing);
            // The closing run is literal text at the end of both renderings.
            let dropped = closing.trim_end().len().saturating_sub(kept.len());
            let end = rendered.trim_end().len().saturating_sub(dropped);
            rendered.truncate(end);
        }
        let name = one_line(&rendered);
        if name.is_empty() {
            return None;
        }
        let (base, qualified) = crate::walk::qualify(owner, &name, NodeKind::Section, SEPARATOR);
        let span = span_of(node);
        // Heading text is arbitrary, so a suffixed key may itself be taken
        // (`# X@6` before a repeated `# X` on line 6): suffix until unique.
        let mut key = base.clone();
        let mut attempt = 0u32;
        while self.keys.contains(&key) {
            attempt = attempt.saturating_add(1);
            key = if attempt == 1 {
                format!("{base}@{}", span.start_line)
            } else {
                format!("{base}@{}#{attempt}", span.start_line)
            };
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

/// OKF §5.3: `unverified` without a verified entry, `human-reviewed` when any
/// `verified[].by` is a `human:` actor, else `machine-confirmed`. A bare
/// mapping is a one-element list; entries that are not mappings are dropped.
fn trust_tier(verified: Option<&Value>) -> &'static str {
    let entries: Vec<_> = verified
        .map(Value::items)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| matches!(entry, Value::Map(_)))
        .collect();
    if entries.is_empty() {
        "unverified"
    } else if entries.iter().any(|entry| {
        entry
            .get("by")
            .and_then(Value::scalar)
            .is_some_and(|by| by.starts_with("human:"))
    }) {
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
    fn a_heading_spelling_a_suffixed_key_cannot_collide() {
        let facts = extract("kb/a.md", "---\ntitle: T\n---\n# X@6\n# X\n# X\n# X\n");
        let keys: std::collections::BTreeSet<_> =
            facts.symbols.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys.len(), facts.symbols.len(), "{:?}", facts.symbols);
    }

    #[test]
    fn section_names_are_the_text_a_reader_sees() {
        let facts = extract(
            "kb/a.md",
            "---\ntitle: T\n---\n## Foo ##\n# C#\n# See [x](x.md) *em* `c` a\\_b &amp; [^n] ###\n# ###\n",
        );
        let names: Vec<_> = facts
            .symbols
            .iter()
            .skip(1)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Foo", "C#", "See x em c a_b &"]);
    }

    #[test]
    fn destinations_decode_escapes_and_images_use_every_reference_form() {
        let facts = extract(
            "kb/a.md",
            "---\ntitle: T\n---\n[c](c\\_d.md) [e](e&amp;f.md) ![img][] ![img] [q](?x) [h](//host/b.md)\n\n[img]: d.md\n",
        );
        let names: Vec<_> = facts.references.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["c_d.md", "e&f.md", "d.md", "d.md"]);
    }

    #[test]
    fn footnote_links_belong_to_the_citing_section() {
        let facts = extract(
            "kb/a.md",
            "---\ntitle: T\n---\n# One\n\nClaim.[^n]\n\n# Two\n\nOther.\n\n[^n]: See [x](x.md).\n",
        );
        let link = facts.references.iter().find(|r| r.name == "x.md").unwrap();
        assert_eq!(link.from_key.as_deref(), Some("concept:T>section:T > One"));
    }

    #[test]
    fn frontmatter_decoding_trust_and_scope_descriptors() {
        let facts = extract(
            "kb/a.md",
            "---\n{title: \"a \\u00e9 \\\"q\\\"\", verified: []}\n---\n",
        );
        assert_eq!(facts.symbols[0].name, "a é \"q\"");
        assert_eq!(facts.symbols[0].attributes["okf_trust"], "unverified");
        let facts = extract(
            "kb/b.md",
            "---\ntitle: B\nsources:\n  - id: s\n    resource: all queries in project X\n  - id: r\n    resource: revenue\n  - id: p\n    resource: policies/p.md\n---\n",
        );
        let cited: Vec<_> = facts.references.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(cited, ["policies/p.md"]);
        assert_eq!(facts.symbols[0].attributes["okf_trust"], "unverified");
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
