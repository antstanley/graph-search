//! The extraction vocabulary: what a language adapter emits, before ids and
//! edges exist (`SPEC.md` §6.2, §7).
//!
//! Extractors speak *facts* — symbols, references, attributes. `core` turns
//! facts into stable node ids, containment edges, and resolved or dangling
//! reference edges. Keeping this boundary declarative is what makes the
//! extractors testable with fixtures (`SPEC.md` §15.1).

use crate::kind::{EdgeKind, NodeKind, Visibility};
use crate::node::Span;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A symbol the extractor found in one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolFact {
    /// A key unique within the file: the qualified name plus, when the file
    /// holds a duplicate, the `#line` disambiguator. `core` builds the stable
    /// node id from it.
    pub key: String,
    /// The lexical parent's `key`, when nested (an impl for a method, a class
    /// for a method, a file for a top-level item).
    pub parent_key: Option<String>,
    /// The kind.
    pub kind: NodeKind,
    /// The bare name.
    pub name: String,
    /// The lexical path (`Parser::parse`).
    pub qualified_name: String,
    /// The one-line truncated declaration.
    pub signature: Option<String>,
    /// Where the item sits.
    pub span: Span,
    /// Source visibility, when the language has it.
    pub visibility: Option<Visibility>,
    /// Whether the item is async.
    pub is_async: bool,
    /// Language-specific attributes (`id`, `classes`, `href`, `selector`,
    /// `tag`, ...) that `core` matches cross-file for HTML/CSS.
    pub attributes: BTreeMap<String, String>,
}

impl SymbolFact {
    /// A bare symbol fact with no attributes.
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        kind: NodeKind,
        name: impl Into<String>,
        qualified_name: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            key: key.into(),
            parent_key: None,
            kind,
            name: name.into(),
            qualified_name: qualified_name.into(),
            signature: None,
            span,
            visibility: None,
            is_async: false,
            attributes: BTreeMap::new(),
        }
    }

    /// Sets the parent key.
    #[must_use]
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parent_key = Some(parent.into());
        self
    }

    /// Sets the signature.
    #[must_use]
    pub fn with_signature(mut self, signature: impl Into<String>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    /// Sets visibility and async-ness.
    #[must_use]
    pub const fn with_visibility(mut self, visibility: Option<Visibility>, is_async: bool) -> Self {
        self.visibility = visibility;
        self.is_async = is_async;
        self
    }

    /// Records one attribute.
    #[must_use]
    pub fn with_attribute(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attributes.insert(key.to_owned(), value.into());
        self
    }
}

/// A reference the extractor found: a name used somewhere, not yet resolved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceFact {
    /// The referencing symbol's `key`; `None` for a file-level reference
    /// (an import statement, an HTML attribute).
    pub from_key: Option<String>,
    /// The edge this reference becomes.
    pub kind: EdgeKind,
    /// The name as written: `crate::foo::Bar`, `ToolRegistry::execute`,
    /// `./styles/site.css`, `submit-button`.
    pub name: String,
    /// The 1-based line of the use.
    pub line: u32,
    /// The import specifier this binding came in through, when the reference
    /// is to an imported binding (`SPEC.md` §7.4 rule 2).
    pub via_import: Option<String>,
    /// A callable parameter shadows this name; its runtime target is unknown.
    pub dynamic: bool,
}

impl ReferenceFact {
    /// A file-level reference.
    #[must_use]
    pub fn file_level(kind: EdgeKind, name: impl Into<String>, line: u32) -> Self {
        Self {
            from_key: None,
            kind,
            name: name.into(),
            line,
            via_import: None,
            dynamic: false,
        }
    }

    /// A reference from a symbol.
    #[must_use]
    pub fn from_symbol(
        from_key: impl Into<String>,
        kind: EdgeKind,
        name: impl Into<String>,
        line: u32,
    ) -> Self {
        Self {
            from_key: Some(from_key.into()),
            kind,
            name: name.into(),
            line,
            via_import: None,
            dynamic: false,
        }
    }

    /// Marks the reference as coming in through an import.
    #[must_use]
    pub fn via_import(mut self, specifier: impl Into<String>) -> Self {
        self.via_import = Some(specifier.into());
        self
    }
}

/// Everything one file yielded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    /// The symbols, parents first where possible.
    pub symbols: Vec<SymbolFact>,
    /// The references, unresolved.
    pub references: Vec<ReferenceFact>,
}

impl Extraction {
    /// Joins another extraction into this one (used by multi-pass extractors).
    pub fn merge(&mut self, other: Extraction) {
        self.symbols.extend(other.symbols);
        self.references.extend(other.references);
    }
}
