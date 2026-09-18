//! The HTML extractor (`SPEC.md` §7.3).
//!
//! Only elements with an `id` or a `class` become nodes — a per-file element
//! cap applies through the shared node cap. The `id`/`classes`/`href`/`src`/
//! `rel`/`tag` attributes ride on the fact for `core` to match cross-file.

use graph_search_core::extraction::{Extraction, SymbolFact};
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::kind::NodeKind;
use graph_search_types::node::Span;
use std::path::Path;
use tree_sitter::Node;

/// The HTML extractor.
#[derive(Debug, Default)]
pub struct HtmlExtractor;

impl LanguageExtractor for HtmlExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::Html
    }

    fn supports(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("html" | "htm")
        )
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_html::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("html grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the html parser produced no tree"));
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
        if matches!(node.kind(), "element" | "script_element" | "style_element") {
            let start_tag = (0..node.child_count())
                .map(|i| node.child(i))
                .find(|child| {
                    child
                        .as_ref()
                        .is_some_and(|c| c.kind() == "start_tag" || c.kind() == "self_closing_tag")
                })
                .flatten();
            if let Some(tag) = start_tag {
                self.element(node, tag);
                return;
            }
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

    #[allow(clippy::too_many_lines)] // attribute folding is flat by nature
    fn element(&mut self, element: Node<'_>, start_tag: Node<'_>) {
        let mut tag = String::new();
        let mut id: Option<String> = None;
        let mut classes = String::new();
        let mut href: Option<String> = None;
        let mut src: Option<String> = None;
        let mut rel: Option<String> = None;

        let mut cursor = start_tag.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                match child.kind() {
                    "tag_name" => self.text(child).clone_into(&mut tag),
                    "attribute" => {
                        let Some(name_node) = child.child_by_field_name("name") else {
                            let mut inner = child.walk();
                            if inner.goto_first_child() {
                                loop {
                                    if inner.node().kind() == "attribute_name" {
                                        self.attribute(
                                            inner.node(),
                                            child,
                                            &mut id,
                                            &mut classes,
                                            &mut href,
                                            &mut src,
                                            &mut rel,
                                        );
                                        break;
                                    }
                                    if !inner.goto_next_sibling() {
                                        break;
                                    }
                                }
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                            continue;
                        };
                        self.attribute(
                            name_node,
                            child,
                            &mut id,
                            &mut classes,
                            &mut href,
                            &mut src,
                            &mut rel,
                        );
                    }
                    _ => {}
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        // A node only when it carries an identity or a relationship.
        if id.is_some() || !classes.is_empty() || href.is_some() || src.is_some() {
            let name = id.clone().unwrap_or_else(|| {
                if classes.is_empty() {
                    tag.clone()
                } else {
                    format!(".{classes}")
                }
            });
            let mut fact = SymbolFact::new(
                format!("el:{name}@{}", span(element).start_line),
                NodeKind::Element,
                name.clone(),
                name,
                span(element),
            )
            .with_attribute("tag", tag);
            if let Some(id) = id {
                fact = fact.with_attribute("id", id);
            }
            if !classes.is_empty() {
                fact = fact.with_attribute("classes", classes);
            }
            if let Some(href) = href {
                fact = fact.with_attribute("href", href);
            }
            if let Some(src) = src {
                fact = fact.with_attribute("src", src);
            }
            if let Some(rel) = rel {
                fact = fact.with_attribute("rel", rel);
            }
            self.extraction.symbols.push(fact);
        }

        // Children still walk (a page's interesting nodes nest).
        let mut cursor = element.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named() && child.kind() != "start_tag" && child.kind() != "end_tag" {
                    self.walk(child);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn attribute(
        &self,
        name_node: Node<'_>,
        attribute: Node<'_>,
        id: &mut Option<String>,
        classes: &mut String,
        href: &mut Option<String>,
        src: &mut Option<String>,
        rel: &mut Option<String>,
    ) {
        let name = self.text(name_node).to_owned();
        let value = attribute_value(attribute, self.source).unwrap_or_default();
        match name.as_str() {
            "id" => *id = Some(value),
            "class" => *classes = value,
            "href" => *href = Some(value),
            "src" => *src = Some(value),
            "rel" => *rel = Some(value),
            _ => {}
        }
    }
}

fn span(node: Node<'_>) -> Span {
    Span::new(
        crate::walk::line_of(node.start_position().row),
        crate::walk::line_of(node.end_position().row),
        u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
        u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
    )
}

fn attribute_value(node: Node<'_>, source: &str) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "attribute_value" {
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
