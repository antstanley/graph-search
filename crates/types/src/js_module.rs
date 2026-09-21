//! Authored JavaScript/TypeScript module bindings, independent of resolution.
use crate::Span;
use serde::{Deserialize, Serialize};

/// A file's syntactically collected ESM surface. Absence means unavailable facts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsModule {
    /// False if syntax or record limits prevented a complete module surface.
    pub complete: bool,
    /// Whether an import/export statement establishes an explicit module.
    pub is_module: bool,
    /// Local imports; duplicate bindings are retained as ambiguity.
    pub imports: Vec<JsImport>,
    /// Exports, including forwarding and star exports.
    pub exports: Vec<JsExport>,
}

/// One imported local binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsImport {
    /// Authored local identifier.
    pub local: String,
    /// Exported identifier, `default`, or `*` for a namespace.
    pub imported: String,
    /// Authored module specifier, without string delimiters.
    pub source: String,
    /// Type-only imports cannot provide a runtime callable.
    pub type_only: bool,
    /// Original binding syntax.
    pub span: Span,
}

/// One explicit export or star-forwarding record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsExport {
    /// Public name, `default`, or `*` for star forwarding.
    pub exported: String,
    /// Local/imported name; absent when the value is not statically named.
    pub local: Option<String>,
    /// Forwarding module, or None for a local binding.
    pub source: Option<String>,
    /// Authored TypeScript type-only export.
    pub type_only: bool,
    /// Original export syntax.
    pub span: Span,
}
