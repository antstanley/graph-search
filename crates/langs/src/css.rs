//! The CSS extractor (`SPEC.md` §7.3).
//!
//! Rule sets, at-rules, and custom properties become nodes; the selector
//! text rides on the fact for `core`'s exact class/id matching. `@import`
//! becomes an import reference between stylesheets.

use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use std::path::Path;
use tree_sitter::Node;

/// The CSS extractor.
#[derive(Debug, Default)]
pub struct CssExtractor;

impl LanguageExtractor for CssExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::Css
    }

    fn supports(&self, path: &Path) -> bool {
        path.extension().and_then(std::ffi::OsStr::to_str) == Some("css")
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_css::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("css grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the css parser produced no tree"));
        };
        let mut extractor = Extractor {
            source: file.text,
            extraction: Extraction::default(),
        };
        extractor.walk(tree.root_node());
        Ok(extractor.extraction)
    }
}

struct Extractor<'a> {
    source: &'a str,
    extraction: Extraction,
}

impl<'a> Extractor<'a> {
    fn text(&self, node: Node<'_>) -> &'a str {
        self.source
            .get(node.start_byte()..node.end_byte())
            .unwrap_or_default()
    }

    fn walk(&mut self, node: Node<'_>) {
        match node.kind() {
            "rule_set" => {
                self.rule_set(node);
                return;
            }
            "import_statement" => {
                let line = crate::walk::line_of(node.start_position().row);
                if let Some(specifier) = string_content(node, self.source) {
                    self.extraction.references.push(ReferenceFact::file_level(
                        EdgeKind::Imports,
                        specifier,
                        line,
                    ));
                }
                return;
            }
            "media_statement"
            | "supports_statement"
            | "keyframes_statement"
            | "font_feature_values_statement"
            | "font_palette_values_statement"
            | "property_statement"
            | "counter_style_statement"
            | "namespace_statement"
            | "layer_statement"
            | "container_query_statement" => {
                self.at_rule(node);
                return;
            }
            "declaration" => {
                self.declaration(node);
                return;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named() {
                    self.walk(child);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn rule_set(&mut self, node: Node<'_>) {
        // The grammar names no fields: the children are `selectors` and
        // `block` by kind.
        let Some(selectors) = child_of_kind(node, "selectors") else {
            return;
        };
        let selector = self.text(selectors).trim().to_owned();
        let span = crate::walk::span_of(node);
        let fact = SymbolFact::new(
            format!("rule:{selector}@{}", span.start_line),
            NodeKind::CssRule,
            selector.clone(),
            selector.clone(),
            span,
        )
        .with_signature(selector.clone())
        .with_attribute("selector", selector);
        self.extraction.symbols.push(fact);
        // Nested rules (`@media` blocks are handled above; plain nesting
        // does not exist in CSS outside at-rules).
        if let Some(block) = child_of_kind(node, "block") {
            self.walk(block);
        }
    }

    fn at_rule(&mut self, node: Node<'_>) {
        let text = self.text(node);
        let first_line = text.lines().next().unwrap_or_default();
        let at_name = first_line
            .split([' ', '{', '('])
            .next()
            .unwrap_or("@media")
            .trim()
            .to_owned();
        let name = match node.kind() {
            "keyframes_statement" => first_line.trim_start_matches('@').to_owned(),
            _ => at_name.trim_start_matches('@').to_owned(),
        };
        let span = crate::walk::span_of(node);
        let fact = SymbolFact::new(
            format!("at:{name}@{}", span.start_line),
            NodeKind::CssAtRule,
            name.clone(),
            name,
            span,
        )
        .with_signature(graph_search_core::text_search::truncate_line(
            first_line.trim(),
            200,
        ));
        self.extraction.symbols.push(fact);
        // Rules nested inside the at-rule body.
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named() && child.kind() != "feature_query" {
                    self.walk(child);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn declaration(&mut self, node: Node<'_>) {
        let Some(property) = child_of_kind(node, "property_name") else {
            return;
        };
        let name = self.text(property).trim().to_owned();
        if !name.starts_with("--") {
            return; // a plain declaration, not a custom property
        }
        let span = crate::walk::span_of(node);
        let fact = SymbolFact::new(
            format!("var:{name}@{}", span.start_line),
            NodeKind::CssCustomProperty,
            name.clone(),
            name,
            span,
        );
        self.extraction.symbols.push(fact);
    }
}

/// The first child of `node` with `kind` (tree-sitter 0.27's `children`
/// wants a cursor; this stays allocation-free and simple).
fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    (0..node.child_count())
        .map(|i| node.child(i))
        .find(|child| child.is_some_and(|c| c.kind() == kind))
        .flatten()
}

fn string_content(node: Node<'_>, source: &str) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "string_content" {
            return source
                .get(current.start_byte()..current.end_byte())
                .map(str::to_owned);
        }
        let mut cursor = current.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    None
}
