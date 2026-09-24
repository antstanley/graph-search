//! Per-file graph shards (format 10, `research/16-proportional-sync.md` §3.1).
//!
//! A shard holds every graph fact one file owns: its nodes, the edges whose
//! owning path it is (resolved and dangling), and its reference occurrences.
//! Shards are records in the content-addressed packs every generation shares,
//! so a publish writes only the shards it replaces. Each shard also derives the
//! posting rows that index it globally.

use crate::record_codec::{DecodeRecord, EncodeRecord};
use crate::segment::Row;
use crate::source_records::Layout;
use graph_search_types::occurrence::OccurrenceFile;
use graph_search_types::{Edge, Node, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

/// The shard index artifact committed by CURRENT.
pub(crate) const FILE: &str = "shards.json";
/// Shards are read one file at a time, so their packs are kept small: a lookup
/// inflates at most one pack of this size.
pub(crate) const LAYOUT: Layout = Layout {
    index: FILE,
    directory: "shard-records",
    pack_bytes: 256 * 1024,
};

/// Posting tables derived from shards.
pub(crate) const NODES: &str = "nodes";
pub(crate) const INCOMING: &str = "incoming";
pub(crate) const FOREIGN: &str = "foreign";
pub(crate) const EDGE_COUNTS: &str = "edge_counts";
/// Every table a shard contributes rows to.
pub(crate) const TABLES: [&str; 4] = [NODES, INCOMING, FOREIGN, EDGE_COUNTS];

/// Every graph fact one file owns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Shard {
    /// Sorted by id; ids are unique.
    pub(crate) nodes: Vec<Node>,
    /// Sorted by source, kind and id; ids are unique.
    pub(crate) edges: Vec<Edge>,
    pub(crate) occurrences: Option<OccurrenceFile>,
}

impl EncodeRecord for &Shard {
    fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()> {
        serde_json::to_writer(out, self).map_err(io::Error::other)
    }
}

impl DecodeRecord for Shard {
    fn decode_record(bytes: &[u8]) -> io::Result<Self> {
        let shard: Self = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        if !shard.is_normal() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "shard facts are not in canonical order",
            ));
        }
        Ok(shard)
    }
}

/// The canonical edge order, which `all_edges` has always returned.
pub(crate) fn edge_order(a: &Edge, b: &Edge) -> std::cmp::Ordering {
    a.from
        .cmp(&b.from)
        .then_with(|| a.kind.cmp(&b.kind))
        .then_with(|| a.id.cmp(&b.id))
}

impl Shard {
    /// Sorts nodes and edges into canonical order. A repeated node id keeps its
    /// last value (an upsert replaces a node); a repeated edge id keeps its first
    /// (as a graph with duplicate edges always reported the first).
    pub(crate) fn normalize(&mut self) {
        let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
        for node in std::mem::take(&mut self.nodes) {
            nodes.insert(node.id.clone(), node);
        }
        self.nodes = nodes.into_values().collect();
        self.edges.sort_by(edge_order);
        self.edges.dedup_by(|a, b| a.id == b.id);
    }

    fn is_normal(&self) -> bool {
        self.nodes.windows(2).all(|pair| pair[0].id < pair[1].id)
            && self
                .edges
                .windows(2)
                .all(|pair| edge_order(&pair[0], &pair[1]).is_lt() && pair[0].id != pair[1].id)
    }

    pub(crate) fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes
            .binary_search_by(|node| node.id.cmp(id))
            .ok()
            .and_then(|index| self.nodes.get(index))
    }

    pub(crate) fn file(&self) -> Option<&Node> {
        self.nodes.iter().find(|node| node.is_file())
    }

    /// The rows this shard, owned by `path`, contributes to each table.
    pub(crate) fn rows(&self, path: &str) -> BTreeMap<&'static str, Vec<Row>> {
        let mut rows: BTreeMap<&'static str, Vec<Row>> =
            TABLES.iter().map(|table| (*table, Vec::new())).collect();
        let mut push = |table: &'static str, row: Row| {
            if let Some(rows) = rows.get_mut(table) {
                rows.push(row);
            }
        };
        for node in &self.nodes {
            push(NODES, Row::new(node.id.as_str(), path, node.kind.as_str()));
        }
        let mut targets: BTreeSet<&str> = BTreeSet::new();
        let mut foreign: BTreeSet<&str> = BTreeSet::new();
        for edge in &self.edges {
            if let Some(to) = &edge.to {
                targets.insert(to.as_str());
            }
            if self.node(&edge.from).is_none() {
                foreign.insert(edge.from.as_str());
            }
        }
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for record in self.occurrences.iter().flat_map(|facts| &facts.records) {
            if let Some(target) = &record.target {
                targets.insert(target.as_str());
            }
            let count = counts
                .entry(record.edge_id().as_str().to_owned())
                .or_default();
            *count = count.saturating_add(1);
        }
        for target in targets {
            push(INCOMING, Row::new(target, path, ""));
        }
        for from in foreign {
            push(FOREIGN, Row::new(from, path, ""));
        }
        for (edge, count) in counts {
            push(EDGE_COUNTS, Row::new(edge, path, count.to_string()));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::kind::{EdgeKind, NodeKind};

    fn node(path: &str, name: &str) -> Node {
        let mut node = Node::file(path, graph_search_types::Language::Rust, 1, 1, "h", 1);
        if !name.is_empty() {
            node.id = NodeId::new(format!("sym:{path}#function:{name}"));
            node.kind = NodeKind::Function;
            node.name = Some(name.to_owned());
        }
        node
    }

    #[test]
    fn shards_normalize_round_trip_and_index_their_edges() {
        let a = node("a.rs", "");
        let f = node("a.rs", "f");
        let g = node("b.rs", "g");
        let mut shard = Shard {
            nodes: vec![f.clone(), a.clone(), f.clone()],
            edges: vec![
                Edge::resolved(&f.id, EdgeKind::Calls, &g.id, "g", Some("a.rs"), Some(2)),
                Edge::dangling(&f.id, EdgeKind::Calls, "h", Some("a.rs"), Some(3)),
                Edge::resolved(
                    &g.id,
                    EdgeKind::References,
                    &f.id,
                    "f",
                    Some("a.rs"),
                    Some(4),
                ),
                Edge::resolved(&f.id, EdgeKind::Calls, &g.id, "g", Some("a.rs"), Some(9)),
            ],
            occurrences: None,
        };
        shard.normalize();
        assert_eq!(shard.nodes.len(), 2);
        assert_eq!(shard.edges.len(), 3);
        // The first of two same-id edges survives.
        assert!(shard.edges.iter().any(|edge| edge.line == Some(2)));
        let mut bytes = Vec::new();
        (&shard).encode_record(&mut bytes).unwrap();
        assert_eq!(Shard::decode_record(&bytes).unwrap(), shard);
        let rows = shard.rows("a.rs");
        let keys = |table: &str| -> Vec<String> {
            rows[table].iter().map(|row| row.key.clone()).collect()
        };
        assert_eq!(keys(NODES), [a.id.as_str(), f.id.as_str()]);
        assert_eq!(keys(INCOMING), [f.id.as_str(), g.id.as_str()]);
        // `g` lives in b.rs, so the edge from it is indexed as foreign.
        assert_eq!(keys(FOREIGN), [g.id.as_str()]);
        // An out-of-order record is refused.
        let mut disordered = shard.clone();
        disordered.nodes.reverse();
        let mut bytes = Vec::new();
        (&disordered).encode_record(&mut bytes).unwrap();
        assert!(Shard::decode_record(&bytes).is_err());
    }
}
