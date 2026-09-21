//! Anchored Rust paths over physical module scopes, retaining shared contexts.
use crate::rust_modules::Catalog;
use graph_search_types::extraction::ReferenceFact;
use graph_search_types::kind::Visibility;
use graph_search_types::{Node, NodeId, NodeKind, Span};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Default)]
pub(crate) struct Paths {
    members: BTreeMap<NodeId, BTreeMap<String, Vec<NodeId>>>,
    interiors: BTreeMap<NodeId, Result<NodeId, &'static str>>,
    parents: BTreeMap<NodeId, BTreeSet<NodeId>>,
    roots: BTreeSet<NodeId>,
    reachable: BTreeMap<NodeId, BTreeSet<NodeId>>,
    inline: BTreeMap<String, Vec<(Span, NodeId)>>,
    incomplete: bool,
}

impl Paths {
    pub(crate) fn build(symbols: &BTreeMap<NodeId, Node>, catalog: &Catalog) -> Self {
        let mut result = Self {
            roots: catalog.roots().map(NodeId::file).collect(),
            ..Self::default()
        };
        for node in symbols.values().filter(|n| {
            std::path::Path::new(&n.path)
                .extension()
                .is_some_and(|ext| ext == "rs")
        }) {
            if node.attribute("rust_module_form") == Some("inline")
                && let Some(span) = node.span
            {
                result
                    .inline
                    .entry(node.path.clone())
                    .or_default()
                    .push((span, node.id.clone()));
            }
            let owner = match &node.parent {
                None => NodeId::file(&node.path),
                Some(parent)
                    if symbols
                        .get(parent)
                        .is_some_and(|p| p.attribute("rust_module_form") == Some("inline")) =>
                {
                    parent.clone()
                }
                _ => continue,
            };
            if let Some(name) = &node.name {
                result
                    .members
                    .entry(owner.clone())
                    .or_default()
                    .entry(identifier(name).into())
                    .or_default()
                    .push(node.id.clone());
            }
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
        symbols: &BTreeMap<NodeId, Node>,
    ) -> Result<NodeId, &'static str> {
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
        let mut parts = fact.name.split("::");
        let first = parts.next().ok_or("rust_path_missing")?;
        let mut scopes = match first {
            "crate" => self
                .reachable
                .get(&origin)
                .cloned()
                .ok_or("rust_crate_context_unknown")?,
            "self" => BTreeSet::from([origin.clone()]),
            "super" => self.up(&BTreeSet::from([origin.clone()]))?,
            _ => return Err("rust_import_path_unanchored"),
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
            let mut next = BTreeSet::new();
            for scope in &scopes {
                let candidates = self
                    .members
                    .get(scope)
                    .and_then(|names| names.get(identifier(part)))
                    .ok_or("rust_path_member_missing")?;
                let matching: Vec<_> = candidates
                    .iter()
                    .filter_map(|id| symbols.get(id))
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
                            crate::resolve::compatible(fact.kind, node.kind)
                        }
                    })
                    .collect();
                let [node] = matching.as_slice() else {
                    return Err("rust_path_member_missing_or_ambiguous");
                };
                self.visible(node, scope, &origin)?;
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
            capped.resolve(&fact, "a.rs", &BTreeMap::new()),
            Err("rust_module_context_limit")
        );
    }
}
