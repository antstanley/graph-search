//! Tree-sitter extractors implementing core's [`LanguageExtractor`] port
//! (`SPEC.md` §7).
//!
//! Each extractor walks the concrete syntax tree and emits *facts* — symbols
//! and references — that `core` turns into stable ids and resolved or
//! dangling edges. Grammar quirks are pinned by per-language fixture tests
//! (`SPEC.md` §15.1).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod class_bindings;
pub mod css;
mod doc_comments;
pub mod embedded;
pub mod framework;
pub mod html;
pub mod javascript;
pub mod js_common;
mod js_modules;
pub mod okf;
pub mod python;
pub mod rust;
mod rust_receivers;
mod rust_use;
mod scopes;
pub mod typescript;
pub mod walk;

pub use css::CssExtractor;
pub use framework::{AstroExtractor, SvelteExtractor, VueExtractor};
pub use html::HtmlExtractor;
pub use javascript::JavaScriptExtractor;
pub use okf::OkfExtractor;
pub use python::PythonExtractor;
pub use rust::RustExtractor;
pub use typescript::TypeScriptExtractor;

/// Builds the registry of every extractor this crate ships, wired into
/// core's port by the library.
#[must_use]
pub fn all_extractors() -> Vec<Box<dyn graph_search_core::ports::LanguageExtractor>> {
    vec![
        Box::new(RustExtractor),
        Box::new(TypeScriptExtractor),
        Box::new(JavaScriptExtractor),
        Box::new(PythonExtractor),
        Box::new(HtmlExtractor),
        Box::new(CssExtractor),
        Box::new(SvelteExtractor),
        Box::new(VueExtractor),
        Box::new(AstroExtractor),
        Box::new(OkfExtractor),
    ]
}
