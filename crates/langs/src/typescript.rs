//! The TypeScript extractor, including TSX (`SPEC.md` §7.2).

use graph_search_core::extraction::Extraction;
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use std::path::Path;

/// The TypeScript/TSX extractor.
#[derive(Debug, Default)]
pub struct TypeScriptExtractor;

impl LanguageExtractor for TypeScriptExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::TypeScript
    }

    fn supports(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("ts" | "tsx" | "mts" | "cts")
        )
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&if is_tsx(file.path) {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            })
            .map_err(|error| ParseError::new(format!("typescript grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the typescript parser produced no tree"));
        };
        let dialect = crate::js_common::Dialect {
            language: graph_search_types::Language::TypeScript,
            types: true,
            field_kind: "public_field_definition",
        };
        let mut extractor = crate::js_common::JsExtractor {
            source: file.text,
            dialect,
            extraction: Extraction::default(),
            scope: Vec::new(),
            imports: std::collections::BTreeMap::new(),
        };
        extractor.walk_node(tree.root_node());
        extractor.bind_imports();
        Ok(extractor.extraction)
    }
}

/// The TSX dialect shares the grammar crate; a path-driven helper for the
/// registry.
#[must_use]
pub fn is_tsx(path: &Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str) == Some("tsx")
}

/// The TSX grammar, for the rare file the typescript grammar misparses.
#[must_use]
pub fn tsx_language() -> tree_sitter::Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}
