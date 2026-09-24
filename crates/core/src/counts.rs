//! Exact graph summaries prepared once with their immutable generation.

use graph_search_types::result::StoreCounts;
use graph_search_types::{Edge, Language, Node};

/// Counts graph facts without cloning their properties or retaining them.
#[must_use]
pub fn summarize<'a>(
    nodes: impl IntoIterator<Item = &'a Node>,
    edges: impl IntoIterator<Item = &'a Edge>,
) -> StoreCounts {
    let mut counts = StoreCounts::default();
    for node in nodes {
        let count = counts.nodes.entry(node.kind).or_default();
        *count = count.saturating_add(1);
        counts.total_nodes = counts.total_nodes.saturating_add(1);
        if node.is_file() {
            let count = counts
                .files_by_language
                .entry(node.language.unwrap_or(Language::Unknown))
                .or_default();
            *count = count.saturating_add(1);
        }
    }
    for edge in edges {
        let count = counts.edges.entry(edge.kind).or_default();
        *count = count.saturating_add(1);
        counts.total_edges = counts.total_edges.saturating_add(1);
    }
    counts
}

/// Adds `other` to `counts`: the counts of a union of disjoint fact sets.
pub fn add(counts: &mut StoreCounts, other: &StoreCounts) {
    combine(counts, other, u64::saturating_add);
}

/// Removes `other` from `counts`: `other` must count a subset of the facts
/// `counts` counts. Kinds whose count reaches zero are dropped, so the result
/// equals counting the remaining facts afresh.
pub fn subtract(counts: &mut StoreCounts, other: &StoreCounts) {
    combine(counts, other, u64::saturating_sub);
}

fn combine(counts: &mut StoreCounts, other: &StoreCounts, op: fn(u64, u64) -> u64) {
    fn merge<K: Ord + Copy>(
        into: &mut std::collections::BTreeMap<K, u64>,
        from: &std::collections::BTreeMap<K, u64>,
        op: fn(u64, u64) -> u64,
    ) {
        for (key, count) in from {
            let entry = into.entry(*key).or_default();
            *entry = op(*entry, *count);
        }
        into.retain(|_, count| *count > 0);
    }
    merge(&mut counts.nodes, &other.nodes, op);
    merge(&mut counts.edges, &other.edges, op);
    merge(&mut counts.files_by_language, &other.files_by_language, op);
    counts.total_nodes = op(counts.total_nodes, other.total_nodes);
    counts.total_edges = op(counts.total_edges, other.total_edges);
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::NodeId;
    use graph_search_types::kind::EdgeKind;

    #[test]
    fn subtracting_a_part_equals_counting_the_rest() {
        let a = Node::file("a.rs", Language::Rust, 1, 1, "h", 1);
        let b = Node::file("b.py", Language::Python, 1, 1, "h", 1);
        let edge = Edge::dangling(&NodeId::file("a.rs"), EdgeKind::Calls, "x", None, None);
        let mut all = summarize([&a, &b], [&edge]);
        subtract(&mut all, &summarize([&b], []));
        assert_eq!(all, summarize([&a], [&edge]));
        add(&mut all, &summarize([&b], []));
        assert_eq!(all, summarize([&a, &b], [&edge]));
    }
}
