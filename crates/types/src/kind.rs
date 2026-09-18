//! The closed, versioned vocabularies: languages, node kinds, edge kinds,
//! visibility, and traversal direction (`SPEC.md` §5.1, §5.2).
//!
//! Adding a variant is a schema change (`SCHEMA_VERSION`), not an incidental
//! one. Every variant serializes as its `snake_case` name so the JSON contract
//! is stable.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A source language with an extraction story (`SPEC.md` §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Rust (`.rs`).
    Rust,
    /// TypeScript, including TSX (`.ts`, `.tsx`).
    TypeScript,
    /// JavaScript, including JSX (`.js`, `.jsx`, `.mjs`, `.cjs`).
    JavaScript,
    /// HTML (`.html`, `.htm`).
    Html,
    /// CSS (`.css`).
    Css,
    /// A file no enabled extractor claims. Its `file` node is still indexed so
    /// a glob can be answered from the index (`SPEC.md` §6.2).
    Unknown,
}

impl Language {
    /// The canonical lower-case name used in config files and output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Html => "html",
            Self::Css => "css",
            Self::Unknown => "unknown",
        }
    }

    /// Parses a language name (case-insensitive); the CLI and config spellings.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "typescript" | "ts" | "tsx" => Some(Self::TypeScript),
            "javascript" | "js" | "jsx" => Some(Self::JavaScript),
            "html" | "htm" => Some(Self::Html),
            "css" => Some(Self::Css),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The kind of a node. Closed and versioned (`SPEC.md` §5.1).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// One walked file, whether or not it parses.
    #[default]
    File,
    /// A named module (`mod`, ES module).
    Module,
    /// A free function.
    Function,
    /// An associated function or class/impl method.
    Method,
    /// A Rust `struct`.
    Struct,
    /// A Rust or TS `enum`.
    Enum,
    /// A Rust `trait`.
    Trait,
    /// A Rust `impl` block; the parent of its methods.
    Impl,
    /// A `type` alias (Rust) or `type` declaration (TS).
    TypeAlias,
    /// A `const` item.
    Const,
    /// A `static` item.
    Static,
    /// A `macro_rules!` definition.
    Macro,
    /// A struct/class member field.
    Field,
    /// An enum variant.
    Variant,
    /// A TS/JS `class`.
    Class,
    /// A TS `interface`.
    Interface,
    /// A top-level `let`/`var` (and `const` where not a function).
    Variable,
    /// An exported binding (TS/JS).
    Export,
    /// An HTML element with an `id` and/or notable attributes.
    Element,
    /// A CSS selector block.
    CssRule,
    /// A CSS at-rule (`@media`, `@keyframes`, ...).
    CssAtRule,
    /// A CSS custom property (`--var`).
    CssCustomProperty,
}

impl NodeKind {
    /// Every kind, in vocabulary order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::File,
            Self::Module,
            Self::Function,
            Self::Method,
            Self::Struct,
            Self::Enum,
            Self::Trait,
            Self::Impl,
            Self::TypeAlias,
            Self::Const,
            Self::Static,
            Self::Macro,
            Self::Field,
            Self::Variant,
            Self::Class,
            Self::Interface,
            Self::Variable,
            Self::Export,
            Self::Element,
            Self::CssRule,
            Self::CssAtRule,
            Self::CssCustomProperty,
        ]
    }

    /// The canonical `snake_case` name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::TypeAlias => "type_alias",
            Self::Const => "const",
            Self::Static => "static",
            Self::Macro => "macro",
            Self::Field => "field",
            Self::Variant => "variant",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Variable => "variable",
            Self::Export => "export",
            Self::Element => "element",
            Self::CssRule => "css_rule",
            Self::CssAtRule => "css_at_rule",
            Self::CssCustomProperty => "css_custom_property",
        }
    }

    /// Parses a kind name (case-insensitive); the `--kind` spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let lowered = name.trim().to_ascii_lowercase();
        Self::all()
            .iter()
            .copied()
            .find(|kind| kind.as_str() == lowered)
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The kind of an edge. Closed and versioned (`SPEC.md` §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Lexical nesting: file/module/impl/class to child symbol.
    Contains,
    /// `use`, `mod`, `import`, `require`, `from ... import`.
    Imports,
    /// A TS/JS export or Rust `pub use`.
    Exports,
    /// A call site, resolved or dangling.
    Calls,
    /// A name use that is not a call or import.
    References,
    /// Inheritance: class/trait to class/trait.
    Extends,
    /// Implementation: class/impl to interface/trait.
    Implements,
    /// A type position: parameter, return, field.
    TypeUses,
    /// An HTML `href`/`src` resolving to a workspace file.
    LinksTo,
    /// `<link rel="stylesheet">`.
    LoadsStylesheet,
    /// An element `class="..."` matched to a CSS selector.
    UsesClass,
    /// A CSS selector matched to an element `id`/class.
    Selects,
}

impl EdgeKind {
    /// Every kind, in vocabulary order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Contains,
            Self::Imports,
            Self::Exports,
            Self::Calls,
            Self::References,
            Self::Extends,
            Self::Implements,
            Self::TypeUses,
            Self::LinksTo,
            Self::LoadsStylesheet,
            Self::UsesClass,
            Self::Selects,
        ]
    }

    /// The canonical `snake_case` name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Imports => "imports",
            Self::Exports => "exports",
            Self::Calls => "calls",
            Self::References => "references",
            Self::Extends => "extends",
            Self::Implements => "implements",
            Self::TypeUses => "type_uses",
            Self::LinksTo => "links_to",
            Self::LoadsStylesheet => "loads_stylesheet",
            Self::UsesClass => "uses_class",
            Self::Selects => "selects",
        }
    }

    /// Parses a kind name (case-insensitive); the `--rel` spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let lowered = name.trim().to_ascii_lowercase();
        Self::all()
            .iter()
            .copied()
            .find(|kind| kind.as_str() == lowered)
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source visibility of a symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Visible outside its module (`pub`, `export`).
    Public,
    /// Visible within the crate (`pub(crate)`).
    Crate,
    /// Visible to the parent module (`pub(super)`).
    Super,
    /// Private to its file/module.
    Private,
}

/// The direction of a traversal step.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Follow edges away from the seed (`calls` to callees).
    #[default]
    Out,
    /// Follow edges toward the seed (`calls` from callers).
    In,
    /// Both directions.
    Both,
}
