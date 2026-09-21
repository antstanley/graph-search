//! The JavaScript extractor, including JSX (`SPEC.md` §7.2).

use graph_search_core::extraction::Extraction;
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use std::path::Path;

/// The JavaScript/JSX extractor.
#[derive(Debug, Default)]
pub struct JavaScriptExtractor;

impl LanguageExtractor for JavaScriptExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::JavaScript
    }

    fn supports(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("js" | "jsx" | "mjs" | "cjs")
        )
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("javascript grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the javascript parser produced no tree"));
        };
        let dialect = crate::js_common::Dialect {
            language: graph_search_types::Language::JavaScript,
            types: false,
            field_kind: "field_definition",
        };
        let mut extractor = crate::js_common::JsExtractor {
            source: file.text,
            dialect,
            extraction: Extraction::default(),
            scope: Vec::new(),
        };
        extractor.walk_node(tree.root_node());
        crate::scopes::enrich(tree.root_node(), file.text, &mut extractor.extraction);
        crate::js_modules::enrich(tree.root_node(), file.text, &mut extractor.extraction);
        extractor.bind_imports();
        crate::doc_comments::enrich(
            tree.root_node(),
            file.text,
            false,
            &mut extractor.extraction,
        );
        Ok(extractor.extraction)
    }
}
