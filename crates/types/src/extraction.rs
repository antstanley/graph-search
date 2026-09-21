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

/// One authored Rust use-tree binding; target path is the reference's `name`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustUseFact {
    /// A trailing/grouped `self` imports only the target's type namespace.
    #[serde(default)]
    pub type_only: bool,
    /// Name introduced into scope; absent for globs and `as _`.
    pub local_name: Option<String>,
    /// Whether this leaf imports all names from its target module.
    pub glob: bool,
    /// Exact authored visibility modifier, when this is a reexport.
    pub visibility: Option<String>,
}

/// A reference the extractor found: a name used somewhere, not yet resolved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceFact {
    /// Source-backed Rust use-tree leaf, before logical module resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust_use: Option<RustUseFact>,
    /// Native Rust `mod name;` syntax; authoritative even if its symbol is missing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rust_module_declaration: bool,
    /// Original source range of the reference expression, when the adapter supplies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// Source spelling before import or receiver qualification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_name: Option<String>,
    /// File-local lexical scope ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<usize>,
    /// Selected file-local binding ordinal, independent of the graph target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<usize>,
    /// Explicit declaration key selected by lexical lookup, before workspace lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_target: Option<String>,
    /// Why syntactic binding cannot provide a static callable target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_reason: Option<String>,
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
    /// A lexical value or unsupported expression has an unknown runtime target.
    pub dynamic: bool,
}

impl ReferenceFact {
    /// A file-level reference.
    #[must_use]
    pub fn file_level(kind: EdgeKind, name: impl Into<String>, line: u32) -> Self {
        Self {
            rust_use: None,
            rust_module_declaration: false,
            span: None,
            raw_name: None,
            scope: None,
            binding: None,
            lexical_target: None,
            unresolved_reason: None,
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
            rust_use: None,
            rust_module_declaration: false,
            span: None,
            raw_name: None,
            scope: None,
            binding: None,
            lexical_target: None,
            unresolved_reason: None,
            from_key: Some(from_key.into()),
            kind,
            name: name.into(),
            line,
            via_import: None,
            dynamic: false,
        }
    }

    /// Attaches original coordinates. Adapters set raw spelling when they have it.
    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    /// Marks the reference as coming in through an import.
    #[must_use]
    pub fn via_import(mut self, specifier: impl Into<String>) -> Self {
        self.via_import = Some(specifier.into());
        self
    }
}

/// A parser-identified documentation comment, preserving its authored delimiters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocCommentFact {
    /// Exact original source coordinates, without decoding or stripping markers.
    pub span: Span,
    /// Conservatively associated declaration key; absent for file or unattached docs.
    pub owner_key: Option<String>,
    /// Rust inner documentation (`//!` or `/*!`), attached to its container.
    pub inner: bool,
}

/// Everything one file yielded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    /// Authored ESM imports/exports; absent for other adapters or legacy facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub js_module: Option<crate::js_module::JsModule>,
    /// Parser-owned documentation comment occurrences, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doc_comments: Vec<DocCommentFact>,
    /// Whether the adapter omitted eligible documentation comment facts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub doc_comments_truncated: bool,
    /// Native lexical scopes, parents before children.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<ScopeFact>,
    /// File-owned binding declarations used before workspace heuristics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingFact>,
    /// The symbols, parents first where possible.
    pub symbols: Vec<SymbolFact>,
    /// The references, unresolved.
    pub references: Vec<ReferenceFact>,
}

impl Extraction {
    /// Joins another extraction into this one (used by multi-pass extractors).
    pub fn merge(&mut self, mut other: Extraction) {
        if let Some(module) = other.js_module.take() {
            if let Some(existing) = &mut self.js_module {
                // Separate script/module passes do not establish one shared scope.
                existing.complete = false;
                existing.is_module |= module.is_module;
            } else {
                self.js_module = Some(module);
            }
        }
        let scopes = self.scopes.len();
        let bindings = self.bindings.len();
        for scope in &mut other.scopes {
            scope.parent = scope.parent.map(|id| id.saturating_add(scopes));
        }
        for binding in &mut other.bindings {
            binding.scope = binding.scope.saturating_add(scopes);
        }
        for reference in &mut other.references {
            reference.scope = reference.scope.map(|id| id.saturating_add(scopes));
            reference.binding = reference.binding.map(|id| id.saturating_add(bindings));
        }
        self.doc_comments.extend(other.doc_comments);
        self.doc_comments.sort_by(|a, b| {
            (a.span.start_byte, a.span.end_byte, a.inner, &a.owner_key).cmp(&(
                b.span.start_byte,
                b.span.end_byte,
                b.inner,
                &b.owner_key,
            ))
        });
        self.doc_comments.dedup();
        self.doc_comments_truncated |= other.doc_comments_truncated;
        if self.doc_comments.len() > crate::limits::MAX_DOC_COMMENTS_PER_FILE {
            self.doc_comments
                .truncate(crate::limits::MAX_DOC_COMMENTS_PER_FILE);
            self.doc_comments_truncated = true;
        }
        self.scopes.extend(other.scopes);
        self.bindings.extend(other.bindings);
        self.symbols.extend(other.symbols);
        self.references.extend(other.references);
    }
}

/// One source-bounded lexical scope. The kind names a native grammar category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeFact {
    /// Enclosing scope ordinal; absent for the file scope.
    pub parent: Option<usize>,
    /// Exact original source bounds.
    pub span: Span,
    /// `file`, `function`, `block`, `module`, or `class`.
    pub kind: String,
}

/// A file-owned lexical binding, not a dataflow-inferred runtime value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingFact {
    /// Declaring scope ordinal.
    pub scope: usize,
    /// Original bound identifier.
    pub name: String,
    /// Original binding-pattern or import-leaf coordinates.
    pub span: Span,
    /// First byte where the binding participates in lookup (including JS TDZ).
    pub visible_from: u32,
    /// First byte where initialization has completed; zero for hoisted declarations.
    pub initialized_from: u32,
    /// `parameter`, `value`, `declaration`, or `rust_import`.
    pub kind: String,
    /// Direct declaration key, if syntax identifies a callable/type declaration.
    pub target_key: Option<String>,
}

/// Shared parser facts. Mutable access detaches both shared and weak identities.
/// Serialization is identical to an owned `Extraction`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedExtraction(std::sync::Arc<Extraction>);

impl SharedExtraction {
    /// A non-owning identity for detecting unchanged cached facts.
    #[must_use]
    pub fn downgrade(&self) -> std::sync::Weak<Extraction> {
        std::sync::Arc::downgrade(&self.0)
    }

    /// Whether a still-live identity names these exact immutable facts.
    #[must_use]
    pub fn matches_identity(&self, identity: &std::sync::Weak<Extraction>) -> bool {
        identity
            .upgrade()
            .is_some_and(|value| std::sync::Arc::ptr_eq(&value, &self.0))
    }
}

impl From<Extraction> for SharedExtraction {
    fn from(value: Extraction) -> Self {
        Self(std::sync::Arc::new(value))
    }
}

impl std::ops::Deref for SharedExtraction {
    type Target = Extraction;
    fn deref(&self) -> &Extraction {
        &self.0
    }
}

impl std::ops::DerefMut for SharedExtraction {
    fn deref_mut(&mut self) -> &mut Extraction {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl Serialize for SharedExtraction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SharedExtraction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Extraction::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    #[test]
    fn js_module_facts_roundtrip_and_merge_without_inventing_shared_scope() {
        let mut legacy: Extraction =
            serde_json::from_str(r#"{"symbols":[],"references":[]}"#).unwrap();
        assert!(legacy.js_module.is_none());
        let facts = Extraction {
            js_module: Some(crate::js_module::JsModule {
                complete: true,
                is_module: true,
                imports: Vec::new(),
                exports: vec![crate::js_module::JsExport {
                    exported: "publicName".into(),
                    local: Some("local".into()),
                    source: None,
                    type_only: false,
                    span: Span::new(1, 1, 0, 10),
                }],
            }),
            ..Default::default()
        };
        legacy.merge(facts.clone());
        assert_eq!(legacy, facts);
        assert_eq!(
            serde_json::from_slice::<Extraction>(&serde_json::to_vec(&facts).unwrap()).unwrap(),
            facts
        );
        legacy.merge(facts);
        let module = legacy.js_module.unwrap();
        assert!(!module.complete && module.is_module);
        assert_eq!(module.exports.len(), 1);
    }

    #[test]
    fn rust_use_facts_roundtrip_without_changing_legacy_references() {
        let mut fact = ReferenceFact::file_level(EdgeKind::Imports, "crate::api::send", 2);
        let legacy = serde_json::to_value(&fact).unwrap();
        assert!(legacy.get("rust_use").is_none());
        assert_eq!(
            serde_json::from_value::<ReferenceFact>(legacy).unwrap(),
            fact
        );
        fact.rust_use = Some(RustUseFact {
            type_only: false,
            local_name: Some("relay".into()),
            glob: false,
            visibility: Some("pub(crate)".into()),
        });
        assert_eq!(
            serde_json::from_value::<ReferenceFact>(serde_json::to_value(&fact).unwrap()).unwrap(),
            fact
        );
    }

    #[test]
    fn documentation_defaults_roundtrips_merges_and_reports_actual_omission() {
        let legacy: Extraction = serde_json::from_str(r#"{"symbols":[],"references":[]}"#).unwrap();
        assert!(legacy.doc_comments.is_empty());
        assert!(!legacy.doc_comments_truncated);
        let fact = |number: usize| {
            let start = u32::try_from(number).unwrap().saturating_mul(2);
            DocCommentFact {
                span: Span::new(1, 1, start, start.saturating_add(1)),
                owner_key: None,
                inner: false,
            }
        };
        let limit = crate::limits::MAX_DOC_COMMENTS_PER_FILE;
        let mut value = Extraction {
            doc_comments: (0..limit).rev().map(fact).collect(),
            ..Default::default()
        };
        value.merge(Extraction {
            doc_comments: vec![fact(0)],
            ..Default::default()
        });
        assert_eq!(value.doc_comments.len(), limit);
        assert_eq!(value.doc_comments[0], fact(0));
        assert!(!value.doc_comments_truncated);
        value.merge(Extraction {
            doc_comments: vec![fact(limit)],
            ..Default::default()
        });
        assert_eq!(value.doc_comments.len(), limit);
        assert!(value.doc_comments_truncated);
        let encoded = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            serde_json::from_slice::<Extraction>(&encoded).unwrap(),
            value
        );
    }

    #[test]
    fn identity_expires_on_mutation_with_one_or_multiple_owners() {
        for shared in [false, true] {
            let mut value = SharedExtraction::from(Extraction::default());
            let identity = value.downgrade();
            let other = shared.then(|| value.clone());
            assert!(value.matches_identity(&identity));
            value
                .references
                .push(ReferenceFact::file_level(EdgeKind::Calls, "changed", 1));
            assert!(!value.matches_identity(&identity));
            if let Some(other) = other {
                assert!(other.references.is_empty());
                assert!(other.matches_identity(&identity));
            }
            assert!(identity.upgrade().is_none());
        }
    }

    #[test]
    fn wire_bytes_and_value_equality_are_unchanged() {
        let owned = Extraction::default();
        let bytes = serde_json::to_vec(&owned).unwrap();
        let shared = SharedExtraction::from(owned);
        assert_eq!(serde_json::to_vec(&shared).unwrap(), bytes);
        let decoded: SharedExtraction = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, shared);
        assert!(!decoded.matches_identity(&shared.downgrade()));
        let weak = shared.downgrade();
        drop(shared);
        assert!(weak.upgrade().is_none());
    }
}
