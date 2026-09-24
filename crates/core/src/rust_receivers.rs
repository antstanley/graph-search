//! Rust method calls bound through their receiver's syntactically inferred
//! type: `x.method()` → `Type::method` (`SPEC.md` § Rust receiver types).
//!
//! The extractor records how a receiver's type is stated ([`ReceiverType`]);
//! this evaluates that description against the workspace: a local's annotation,
//! a struct literal, `self`, a field's declared type, or the declared return
//! type of whatever another call resolves to. Any step that cannot be proven
//! leaves the call to the ordinary rules, which keep it unresolved.
use crate::extraction::ReferenceFact;
use crate::resolve::SymbolTable;
use graph_search_types::extraction::ReceiverType;
use graph_search_types::kind::{EdgeKind, Language, NodeKind};
use graph_search_types::{Node, NodeId};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// Nested evaluations (receiver chains through other calls) admitted per call.
const MAX_DEPTH: usize = 32;

/// One file's receiver evaluation, memoizing the targets of its references.
pub(crate) struct Receivers<'a> {
    facts: &'a [ReferenceFact],
    path: &'a str,
    table: &'a SymbolTable<'a>,
    known: &'a BTreeSet<String>,
    memo: RefCell<BTreeMap<usize, Option<NodeId>>>,
    depth: Cell<usize>,
}

impl<'a> Receivers<'a> {
    pub(crate) fn new(
        facts: &'a [ReferenceFact],
        path: &'a str,
        table: &'a SymbolTable<'a>,
        known: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            facts,
            path,
            table,
            known,
            memo: RefCell::new(BTreeMap::new()),
            depth: Cell::new(0),
        }
    }

    /// The target the reference at `ordinal` resolves to.
    fn target(&self, ordinal: usize) -> Option<NodeId> {
        if let Some(target) = self.memo.borrow().get(&ordinal) {
            return target.clone();
        }
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return None;
        }
        // A cycle through the memo resolves to nothing rather than recursing.
        self.memo.borrow_mut().insert(ordinal, None);
        self.depth.set(depth.saturating_add(1));
        let target = self.facts.get(ordinal).and_then(|fact| {
            crate::resolve::resolve_in(
                fact,
                self.path,
                self.table,
                self.known,
                Language::Rust,
                Some(self),
            )
            .to
        });
        self.depth.set(depth);
        self.memo.borrow_mut().insert(ordinal, target.clone());
        target
    }

    /// The method a receiver call binds to.
    pub(crate) fn method(&self, fact: &ReferenceFact, receiver: &ReceiverType) -> Option<Rc<Node>> {
        if fact.kind != EdgeKind::Calls {
            return None;
        }
        let raw = fact.raw_name.as_deref().unwrap_or(&fact.name);
        let member = raw.rsplit_once('.')?.1;
        let member = member.split("::").next().unwrap_or(member).trim();
        if member.is_empty() || !member.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let ty = self.type_of(receiver, false)?;
        self.table
            .rust_paths
            .member(&ty, member, false, self.path, self.table)
            .ok()
    }

    /// The type `receiver` evaluates to (its success type when `fallible`).
    fn type_of(&self, receiver: &ReceiverType, fallible: bool) -> Option<Rc<Node>> {
        let table = self.table;
        match receiver {
            ReceiverType::SelfType(owner) if !fallible => {
                table.rust_paths.type_named(owner, self.path, table)
            }
            ReceiverType::Annotation(ordinal) if !fallible => {
                let ty = table.get(&self.target(*ordinal)?)?;
                crate::rust_paths::is_type(ty.kind).then_some(ty)
            }
            ReceiverType::Declared(value) if !fallible => self.named(value, self.path, None),
            ReceiverType::Return(ordinal) => {
                let callable = table.get(&self.target(*ordinal)?)?;
                match callable.kind {
                    NodeKind::Method | NodeKind::Function => self.declared(
                        &callable,
                        if fallible {
                            "rust_returns_fallible"
                        } else {
                            "rust_returns"
                        },
                    ),
                    // A tuple struct or variant constructor builds its type.
                    NodeKind::Struct if !fallible => Some(callable),
                    NodeKind::Variant if !fallible => table
                        .get(callable.parent.as_ref()?)
                        .filter(|parent| parent.kind == NodeKind::Enum),
                    _ => None,
                }
            }
            ReceiverType::Field(owner, name) => {
                let owner = self.type_of(owner, false)?;
                let field = self
                    .table
                    .rust_paths
                    .member(&owner, name, true, self.path, table)
                    .ok()?;
                self.declared(
                    &field,
                    if fallible {
                        "rust_type_fallible"
                    } else {
                        "rust_type"
                    },
                )
            }
            ReceiverType::Try(inner) if !fallible => self.type_of(inner, true),
            _ => None,
        }
    }

    /// The type a symbol's declared-type attribute names, resolved from the
    /// symbol's own file.
    fn declared(&self, symbol: &Node, attribute: &str) -> Option<Rc<Node>> {
        self.named(symbol.attribute(attribute)?, &symbol.path, Some(symbol))
    }

    /// The type a declared-type value names from `path`: `self` (the owner of
    /// `symbol`), `key:` a declaration in that file, `path:` a Rust path.
    fn named(&self, value: &str, path: &str, symbol: Option<&Node>) -> Option<Rc<Node>> {
        let table = self.table;
        let ty = if value == "self" {
            let (owner, _) = symbol?.qualified_name.as_deref()?.rsplit_once("::")?;
            table.rust_paths.type_named(owner, path, table)?
        } else if let Some(key) = value.strip_prefix("key:") {
            crate::rust_paths::Paths::lexical(path, key, table)?
        } else {
            let spelled = value.strip_prefix("path:")?;
            let mut fact = ReferenceFact::file_level(EdgeKind::TypeUses, spelled, 0);
            fact.via_import = Some(spelled.to_owned());
            let id = table.rust_paths.resolve(&fact, path, table).ok()?;
            table.get(&id)?
        };
        crate::rust_paths::is_type(ty.kind).then_some(ty)
    }
}
