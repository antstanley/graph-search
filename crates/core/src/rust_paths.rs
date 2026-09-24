//! Anchored Rust paths over physical module scopes, retaining shared contexts.
use crate::resolve::SymbolTable;
use crate::rust_modules::Catalog;
use graph_search_types::extraction::{ReferenceFact, RustUseFact};
use graph_search_types::kind::Visibility;
use graph_search_types::{Node, NodeId, NodeKind, Span};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

/// Maximum `pub use` hops followed for one anchored path.
const MAX_REEXPORT_DEPTH: usize = 16;

/// One file's module members: per scope (the file, or an inline module in
/// it), each name's symbols.
type FileMembers = BTreeMap<NodeId, BTreeMap<String, Vec<NodeId>>>;

#[derive(Clone, Debug, Default)]
pub(crate) struct Paths {
    interiors: BTreeMap<NodeId, Result<NodeId, &'static str>>,
    parents: BTreeMap<NodeId, BTreeSet<NodeId>>,
    roots: BTreeSet<NodeId>,
    reachable: BTreeMap<NodeId, BTreeSet<NodeId>>,
    inline: BTreeMap<String, Vec<(Span, NodeId)>>,
    /// Each inline module's file.
    inline_files: BTreeMap<NodeId, String>,
    /// Workspace library crate roots by crate name (see [`Catalog::libraries`]).
    crates: BTreeMap<String, Result<NodeId, &'static str>>,
    /// Module members, indexed one file at a time as paths walk into it.
    members: RefCell<BTreeMap<String, Arc<FileMembers>>>,
    incomplete: bool,
}

impl Paths {
    /// The module structure from every Rust module declaration; members are
    /// read from `table` as paths reach their files.
    pub(crate) fn build(modules: &[Rc<Node>], catalog: &Catalog) -> Self {
        let mut result = Self {
            roots: catalog.roots().map(NodeId::file).collect(),
            crates: catalog.libraries(),
            ..Self::default()
        };
        let rust = |node: &&Rc<Node>| {
            std::path::Path::new(&node.path)
                .extension()
                .is_some_and(|ext| ext == "rs")
        };
        for node in modules.iter().filter(rust) {
            if node.attribute("rust_module_form") == Some("inline") {
                result
                    .inline_files
                    .insert(node.id.clone(), node.path.clone());
                if let Some(span) = node.span {
                    result
                        .inline
                        .entry(node.path.clone())
                        .or_default()
                        .push((span, node.id.clone()));
                }
            }
        }
        for node in modules.iter().filter(rust) {
            let owner = match &node.parent {
                None => NodeId::file(&node.path),
                Some(parent) if result.inline_files.contains_key(parent) => parent.clone(),
                _ => continue,
            };
            let interior = match node.attribute("rust_module_form") {
                Some("inline") => {
                    if node.attribute("rust_module_unavailable").is_some() {
                        Err("rust_module_attribute_unsupported")
                    } else {
                        Ok(node.id.clone())
                    }
                }
                Some("external") => catalog
                    .declaration(&node.id)
                    .map(|path| NodeId::file(&path)),
                _ => continue,
            };
            if let Ok(scope) = &interior {
                result
                    .parents
                    .entry(scope.clone())
                    .or_default()
                    .insert(owner);
            }
            result.interiors.insert(node.id.clone(), interior);
        }
        let mut children: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for (child, parents) in &result.parents {
            for parent in parents {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(child.clone());
            }
        }
        result.propagate(&children, 65_536);
        result
    }

    /// This structure with no file's members indexed yet, for reuse by a
    /// later sync whose files may have changed.
    pub(crate) fn without_members(&self) -> Self {
        Self {
            members: RefCell::default(),
            ..self.clone()
        }
    }

    /// The symbols named `name` directly in `scope`.
    fn members(&self, scope: &NodeId, name: &str, table: &SymbolTable<'_>) -> Option<Vec<NodeId>> {
        let path = match scope.as_str().strip_prefix("file:") {
            Some(path) => path,
            None => self.inline_files.get(scope)?.as_str(),
        };
        self.file_members(path, table)
            .get(scope)?
            .get(identifier(name))
            .cloned()
    }

    /// Every scope's members in the file `path`.
    fn file_members(&self, path: &str, table: &SymbolTable<'_>) -> Arc<FileMembers> {
        if let Some(members) = self.members.borrow().get(path) {
            return Arc::clone(members);
        }
        let mut members = FileMembers::new();
        for node in table.in_path(path).iter() {
            let owner = match &node.parent {
                None => NodeId::file(&node.path),
                Some(parent) if self.inline_files.contains_key(parent) => parent.clone(),
                _ => continue,
            };
            if let Some(name) = &node.name {
                members
                    .entry(owner)
                    .or_default()
                    .entry(identifier(name).into())
                    .or_default()
                    .push(node.id.clone());
            }
        }
        let members = Arc::new(members);
        self.members
            .borrow_mut()
            .insert(path.to_owned(), Arc::clone(&members));
        members
    }

    /// The symbols qualified `qualified` in Rust files whose kind `admit`s.
    fn qualified(
        table: &SymbolTable<'_>,
        qualified: &str,
        admit: impl Fn(NodeKind) -> bool,
    ) -> Vec<Rc<Node>> {
        table
            .qualified(qualified)
            .into_iter()
            .filter(|node| {
                node.qualified_name.as_deref() == Some(qualified)
                    && admit(node.kind)
                    && std::path::Path::new(&node.path)
                        .extension()
                        .is_some_and(|ext| ext == "rs")
            })
            .collect()
    }

    fn propagate(&mut self, children: &BTreeMap<NodeId, Vec<NodeId>>, limit: usize) {
        let mut pending = VecDeque::new();
        let mut count = 0usize;
        for root in self.roots.clone() {
            if count == limit {
                self.incomplete = true;
                return;
            }
            self.reachable
                .entry(root.clone())
                .or_default()
                .insert(root.clone());
            pending.push_back((root.clone(), root));
            count = count.saturating_add(1);
        }
        while let Some((scope, root)) = pending.pop_front() {
            for child in children.get(&scope).into_iter().flatten() {
                let roots = self.reachable.entry(child.clone()).or_default();
                if roots.contains(&root) {
                    continue;
                }
                if count == limit {
                    self.incomplete = true;
                    return;
                }
                roots.insert(root.clone());
                count = count.saturating_add(1);
                pending.push_back((child.clone(), root.clone()));
            }
        }
    }

    pub(crate) fn resolve(
        &self,
        fact: &ReferenceFact,
        path: &str,
        table: &SymbolTable<'_>,
    ) -> Result<NodeId, &'static str> {
        self.resolve_at(fact, path, table, 0)
    }

    #[allow(clippy::too_many_lines)] // one path walk with explicit bounds
    fn resolve_at(
        &self,
        fact: &ReferenceFact,
        path: &str,
        table: &SymbolTable<'_>,
        depth: usize,
    ) -> Result<NodeId, &'static str> {
        if depth >= MAX_REEXPORT_DEPTH {
            return Err("rust_reexport_depth_limit");
        }
        if self.incomplete {
            return Err("rust_module_context_limit");
        }
        if fact.rust_use.as_ref().is_some_and(|r| r.glob) {
            return Err("rust_glob_exports_unavailable");
        }
        let origin = self.origin(path, fact.span);
        if !self.reachable.contains_key(&origin) {
            return Err("rust_crate_context_unknown");
        }
        let mut parts = fact.name.split("::").peekable();
        let first = *parts.peek().ok_or("rust_path_missing")?;
        // Edition-2018 paths: a module in scope is walked from the origin as
        // its first member; any other anchor is consumed here.
        let local = !matches!(first, "crate" | "self" | "super")
            && self.local_module(&origin, first, table);
        if !local {
            parts.next();
        }
        let mut scopes = match first {
            "crate" => self
                .reachable
                .get(&origin)
                .cloned()
                .ok_or("rust_crate_context_unknown")?,
            "self" => BTreeSet::from([origin.clone()]),
            "super" => self.up(&BTreeSet::from([origin.clone()]))?,
            _ if local => BTreeSet::from([origin.clone()]),
            // Another workspace crate's root. An external crate (`std`,
            // `serde`) has no target.
            _ => match self.crates.get(identifier(first)) {
                Some(root) => BTreeSet::from([root.clone()?]),
                None => return Err("rust_import_path_unanchored"),
            },
        };
        let remaining: Vec<_> = parts.take(257).collect();
        if remaining.len() > 256 {
            return Err("rust_path_depth_limit");
        }
        let mut offset = 0usize;
        if first == "super" {
            for _ in remaining.iter().take_while(|part| **part == "super") {
                scopes = self.up(&scopes)?;
                offset = offset.saturating_add(1);
            }
        }
        let remaining = &remaining[offset..];
        if remaining.is_empty() {
            if fact.rust_use.is_none() {
                return Err("rust_path_member_missing");
            }
            return unique(scopes);
        }
        for (index, part) in remaining.iter().enumerate() {
            let terminal = index.saturating_add(1) == remaining.len();
            // `Type::member`: the last segment is an associated item or variant
            // of the type the penultimate segment names.
            if index.saturating_add(2) == remaining.len()
                && let Some(member) = remaining.last()
                && let Some(result) =
                    self.associated_path(&scopes, part, member, fact, &origin, table, depth)
            {
                return result;
            }
            let mut next = BTreeSet::new();
            for scope in &scopes {
                let candidates = self
                    .members(scope, part, table)
                    .ok_or("rust_path_member_missing")?;
                let matching: Vec<_> = candidates
                    .iter()
                    .filter_map(|id| table.get(id))
                    .filter(|node| {
                        if !terminal {
                            node.kind == NodeKind::Module
                        } else if let Some(import) = &fact.rust_use {
                            !import.type_only
                                || matches!(
                                    node.kind,
                                    NodeKind::Module | NodeKind::Enum | NodeKind::Trait
                                )
                        } else {
                            // A reexport is followed to its target below.
                            crate::resolve::compatible(fact.kind, node.kind)
                                || node.attribute("rust_reexport").is_some()
                        }
                    })
                    .collect();
                let [node] = matching.as_slice() else {
                    return Err("rust_path_member_missing_or_ambiguous");
                };
                self.visible(node, scope, &origin)?;
                if let Some(target) = node.attribute("rust_reexport") {
                    let resolved = self.follow_reexport(target, node, fact, table, depth)?;
                    if terminal {
                        next.insert(resolved);
                    } else {
                        next.insert(
                            self.interiors
                                .get(&resolved)
                                .ok_or("rust_module_interior_missing")?
                                .clone()?,
                        );
                    }
                    continue;
                }
                if terminal {
                    next.insert(node.id.clone());
                } else {
                    next.insert(
                        self.interiors
                            .get(&node.id)
                            .ok_or("rust_module_interior_missing")?
                            .clone()?,
                    );
                }
            }
            scopes = next;
        }
        unique(scopes)
    }

    /// Resolves `Type::member` from `scopes`, when `name` names exactly one
    /// type there (directly or through a reexport) and no module. `None` leaves
    /// the path to ordinary module traversal.
    #[allow(clippy::too_many_arguments)]
    fn associated_path(
        &self,
        scopes: &BTreeSet<NodeId>,
        name: &str,
        member: &str,
        fact: &ReferenceFact,
        origin: &NodeId,
        table: &SymbolTable<'_>,
        depth: usize,
    ) -> Option<Result<NodeId, &'static str>> {
        let mut targets = BTreeSet::new();
        for scope in scopes {
            let candidates: Vec<Rc<Node>> = self
                .members(scope, name, table)?
                .iter()
                .filter_map(|id| table.get(id))
                .collect();
            if candidates.iter().any(|node| node.kind == NodeKind::Module) {
                return None;
            }
            let types: Vec<&Rc<Node>> = candidates
                .iter()
                .filter(|node| is_type(node.kind) || node.attribute("rust_reexport").is_some())
                .collect();
            let [node] = types.as_slice() else {
                return None;
            };
            if let Err(error) = self.visible(node, scope, origin) {
                return Some(Err(error));
            }
            let ty = match node.attribute("rust_reexport") {
                Some(target) => match self.follow_reexport(target, node, fact, table, depth) {
                    Ok(id) => id,
                    Err(error) => return Some(Err(error)),
                },
                None => node.id.clone(),
            };
            let ty = table.get(&ty).filter(|ty| is_type(ty.kind))?;
            targets.insert(self.associated_member(&ty, member, fact, origin, table));
        }
        match targets.len() {
            1 => targets.pop_first(),
            _ => Some(Err("rust_path_context_ambiguous")),
        }
    }

    /// The associated item or variant `member` of `ty`, within `ty`'s crate
    /// (an `impl` may sit in any of its files). A same-named type elsewhere in
    /// the crate is told apart by `ty`'s own file; another crate sees only
    /// `pub` items.
    fn associated_member(
        &self,
        ty: &Node,
        member: &str,
        fact: &ReferenceFact,
        origin: &NodeId,
        table: &SymbolTable<'_>,
    ) -> Result<NodeId, &'static str> {
        let qualified = format!(
            "{}::{}",
            ty.qualified_name
                .as_deref()
                .or(ty.name.as_deref())
                .unwrap_or_default(),
            identifier(member)
        );
        let crate_of = |path: &str| self.reachable.get(&NodeId::file(path));
        let type_roots = crate_of(&ty.path).ok_or("rust_crate_context_unknown")?;
        let foreign = self
            .reachable
            .get(origin)
            .is_none_or(|roots| roots.is_disjoint(type_roots));
        let candidates: Vec<Rc<Node>> = Self::qualified(table, &qualified, is_associated)
            .into_iter()
            .filter(|node| {
                crate_of(&node.path).is_some_and(|roots| !roots.is_disjoint(type_roots))
                    && node.kind != NodeKind::Field
                    && (fact.rust_use.is_some()
                        || node.kind == NodeKind::Variant
                        || crate::resolve::compatible(fact.kind, node.kind))
                    && (!foreign
                        || node.kind == NodeKind::Variant
                        || node.visibility == Some(Visibility::Public))
            })
            .collect();
        let candidates = if candidates.len() > 1 {
            candidates
                .into_iter()
                .filter(|node| node.path == ty.path)
                .collect()
        } else {
            candidates
        };
        match candidates.as_slice() {
            [node] => Ok(node.id.clone()),
            [] => Err("rust_associated_member_missing"),
            _ => Err("rust_associated_member_ambiguous"),
        }
    }

    /// The field (`field`) or callable member `member` of `ty`, reached from
    /// `from_path`: an `impl` or field in `ty`'s crate, preferring `ty`'s own
    /// file among same-named types; another crate sees only `pub` members.
    pub(crate) fn member(
        &self,
        ty: &Node,
        member: &str,
        field: bool,
        from_path: &str,
        table: &SymbolTable<'_>,
    ) -> Result<Rc<Node>, &'static str> {
        let qualified = format!(
            "{}::{}",
            ty.qualified_name
                .as_deref()
                .or(ty.name.as_deref())
                .unwrap_or_default(),
            identifier(member)
        );
        let crate_of = |path: &str| self.reachable.get(&NodeId::file(path));
        let type_roots = crate_of(&ty.path).ok_or("rust_crate_context_unknown")?;
        let foreign = crate_of(from_path).is_none_or(|roots| roots.is_disjoint(type_roots));
        let candidates: Vec<Rc<Node>> = Self::qualified(table, &qualified, is_associated)
            .into_iter()
            .filter(|node| {
                crate_of(&node.path).is_some_and(|roots| !roots.is_disjoint(type_roots))
                    && (node.kind == NodeKind::Field) == field
                    && (field || matches!(node.kind, NodeKind::Method | NodeKind::Function))
                    && (!foreign || node.visibility == Some(Visibility::Public))
            })
            .collect();
        let candidates = if candidates.len() > 1 {
            candidates
                .into_iter()
                .filter(|node| node.path == ty.path)
                .collect()
        } else {
            candidates
        };
        match candidates.as_slice() {
            [node] => Ok(Rc::clone(node)),
            [] => Err("rust_receiver_member_missing"),
            _ => Err("rust_receiver_member_ambiguous"),
        }
    }

    /// The type qualified `qualified` in `near`'s crate, preferring `near`'s
    /// own file.
    pub(crate) fn type_named(
        &self,
        qualified: &str,
        near: &str,
        table: &SymbolTable<'_>,
    ) -> Option<Rc<Node>> {
        let roots = self.reachable.get(&NodeId::file(near))?;
        let candidates: Vec<Rc<Node>> = Self::qualified(table, qualified, is_type)
            .into_iter()
            .filter(|node| {
                self.reachable
                    .get(&NodeId::file(&node.path))
                    .is_some_and(|other| !other.is_disjoint(roots))
            })
            .collect();
        match candidates.as_slice() {
            [node] => Some(Rc::clone(node)),
            _ => candidates.into_iter().find(|node| node.path == near),
        }
    }

    /// The symbol a same-file lexical declaration key names.
    pub(crate) fn lexical(path: &str, key: &str, table: &SymbolTable<'_>) -> Option<Rc<Node>> {
        if std::path::Path::new(path)
            .extension()
            .is_none_or(|ext| ext != "rs")
        {
            return None;
        }
        // Keys are unique per file; a repeat keeps the last by id.
        table
            .in_path(path)
            .iter()
            .rfind(|node| node.attribute("lexical_key") == Some(key))
            .cloned()
    }

    /// Whether `name` is a module declared directly in `scope`, which an
    /// unanchored edition-2018 path names before any crate.
    fn local_module(&self, scope: &NodeId, name: &str, table: &SymbolTable<'_>) -> bool {
        self.members(scope, name, table).is_some_and(|ids| {
            ids.iter()
                .filter_map(|id| table.get(id))
                .any(|node| node.kind == NodeKind::Module)
        })
    }

    /// Whether `name` spells a workspace library crate.
    pub(crate) fn is_crate(&self, name: &str) -> bool {
        self.crates.contains_key(identifier(name))
    }

    /// Resolves a `pub use` target from the republishing module's own scope.
    fn follow_reexport(
        &self,
        target: &str,
        node: &Node,
        fact: &ReferenceFact,
        table: &SymbolTable<'_>,
        depth: usize,
    ) -> Result<NodeId, &'static str> {
        let mut synthetic = fact.clone();
        target.clone_into(&mut synthetic.name);
        synthetic.span = node.span;
        synthetic.rust_use = Some(RustUseFact {
            type_only: node.attribute("rust_reexport_type_only") == Some("true"),
            local_name: None,
            glob: false,
            visibility: None,
        });
        synthetic.via_import = None;
        synthetic.from_key = None;
        synthetic.lexical_target = None;
        synthetic.raw_name = None;
        synthetic.dynamic = false;
        synthetic.unresolved_reason = None;
        self.resolve_at(&synthetic, &node.path, table, depth.saturating_add(1))
    }

    fn origin(&self, path: &str, span: Option<Span>) -> NodeId {
        span.and_then(|span| {
            self.inline
                .get(path)?
                .iter()
                .filter(|(s, _)| s.start_byte <= span.start_byte && span.end_byte <= s.end_byte)
                .min_by_key(|(s, _)| s.end_byte.saturating_sub(s.start_byte))
                .map(|(_, id)| id.clone())
        })
        .unwrap_or_else(|| NodeId::file(path))
    }

    fn up(&self, scopes: &BTreeSet<NodeId>) -> Result<BTreeSet<NodeId>, &'static str> {
        let mut result = BTreeSet::new();
        for scope in scopes {
            if self.roots.contains(scope) {
                return Err("rust_super_at_crate_root");
            }
            let parents = self
                .parents
                .get(scope)
                .ok_or("rust_parent_context_unknown")?;
            result.extend(parents.iter().cloned());
        }
        Ok(result)
    }

    fn visible(&self, node: &Node, owner: &NodeId, origin: &NodeId) -> Result<(), &'static str> {
        let allowed = match node.visibility {
            Some(Visibility::Public) => true,
            Some(Visibility::Crate) => self
                .reachable
                .get(origin)
                .zip(self.reachable.get(owner))
                .is_some_and(|(a, b)| !a.is_empty() && a.is_subset(b)),
            Some(Visibility::Super) => {
                let parents = self.up(&BTreeSet::from([owner.clone()]))?;
                parents.iter().all(|parent| self.descendant(origin, parent))
            }
            None if node.attribute("rust_visibility_modifier").is_some() => {
                return Err("rust_visibility_restriction_unsupported");
            }
            Some(Visibility::Private) | None => self.descendant(origin, owner),
        };
        allowed.then_some(()).ok_or("rust_path_not_visible")
    }

    // A shared physical module with different parent contexts cannot prove privacy.
    fn descendant(&self, origin: &NodeId, owner: &NodeId) -> bool {
        let mut scope = origin;
        let mut seen = BTreeSet::new();
        loop {
            if scope == owner {
                return true;
            }
            if !seen.insert(scope) || self.roots.contains(scope) {
                return false;
            }
            let Some(parents) = self.parents.get(scope) else {
                return false;
            };
            if parents.len() != 1 {
                return false;
            }
            let Some(parent) = parents.first() else {
                return false;
            };
            scope = parent;
        }
    }
}

/// A nominal type a method can be called on.
pub(crate) fn is_type(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Trait | NodeKind::TypeAlias
    )
}

/// Kinds indexed for `Type::member` paths and receiver members.
fn is_associated(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Method
            | NodeKind::Function
            | NodeKind::Const
            | NodeKind::Variant
            | NodeKind::Field
    )
}

fn identifier(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}
fn unique(values: BTreeSet<NodeId>) -> Result<NodeId, &'static str> {
    if values.len() != 1 {
        return Err("rust_path_context_ambiguous");
    }
    values.into_iter().next().ok_or("rust_path_member_missing")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_context_worklist_deduplicates_diamonds_cycles_and_caps_before_enqueue() {
        let a = NodeId::file("a.rs");
        let b = NodeId::file("b.rs");
        let c = NodeId::file("c.rs");
        let children = BTreeMap::from([
            (a.clone(), vec![b.clone(), b.clone(), c.clone()]),
            (b.clone(), vec![a.clone(), c.clone()]),
        ]);
        let mut complete = Paths {
            roots: BTreeSet::from([a.clone()]),
            ..Default::default()
        };
        complete.propagate(&children, 3);
        assert!(!complete.incomplete);
        assert_eq!(
            complete
                .reachable
                .values()
                .map(BTreeSet::len)
                .sum::<usize>(),
            3
        );
        let mut capped = Paths {
            roots: BTreeSet::from([a]),
            ..Default::default()
        };
        capped.propagate(&children, 2);
        assert!(capped.incomplete);
        assert_eq!(
            capped.reachable.values().map(BTreeSet::len).sum::<usize>(),
            2
        );
        let fact = ReferenceFact::file_level(graph_search_types::EdgeKind::Calls, "self::run", 1);
        assert_eq!(
            capped.resolve(&fact, "a.rs", &SymbolTable::new()),
            Err("rust_module_context_limit")
        );
    }
}
