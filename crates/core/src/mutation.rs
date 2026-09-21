//! Shared replacement semantics for graph adapters.
use graph_search_types::{Edge, Node, NodeId, WriteBatch};
use std::collections::{BTreeMap, BTreeSet};

/// A mutation plan derived from the graph before replacement.
pub struct Mutation {
    /// Nodes whose identities do not survive this batch.
    pub removed: BTreeSet<NodeId>,
    touched: BTreeSet<String>,
    owners: BTreeMap<NodeId, String>,
}

impl Mutation {
    /// Preserve identities only when id, path and kind agree, without explicit removal.
    pub fn new<'a>(batch: &WriteBatch, nodes: impl Iterator<Item = &'a Node>) -> Self {
        let explicit: BTreeSet<_> = batch.removed_files.iter().collect();
        let touched: BTreeSet<_> = batch
            .removed_files
            .iter()
            .cloned()
            .chain(batch.upserts.iter().map(|p| p.file.path.clone()))
            .collect();
        let replacements: BTreeMap<_, _> = batch
            .upserts
            .iter()
            .flat_map(|p| std::iter::once(&p.file).chain(&p.symbols))
            .map(|n| (&n.id, n))
            .collect();
        let mut removed = BTreeSet::new();
        let mut owners = BTreeMap::new();
        for node in nodes {
            owners.insert(node.id.clone(), node.path.clone());
            if touched.contains(&node.path)
                && (explicit.contains(&node.path)
                    || replacements
                        .get(&node.id)
                        .is_none_or(|new| new.path != node.path || new.kind != node.kind))
            {
                removed.insert(node.id.clone());
            }
        }
        Self {
            removed,
            touched,
            owners,
        }
    }

    /// Replace source-owned edges and discard references to deleted endpoints.
    #[must_use]
    pub fn removes_edge(&self, edge: &Edge) -> bool {
        self.removed.contains(&edge.from)
            || edge.to.as_ref().is_some_and(|id| self.removed.contains(id))
            || edge
                .path
                .as_ref()
                .or_else(|| self.owners.get(&edge.from))
                .is_some_and(|path| self.touched.contains(path))
    }
}
