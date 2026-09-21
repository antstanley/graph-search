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
