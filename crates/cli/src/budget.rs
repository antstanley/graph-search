//! Final graph JSON accounting, including the transport envelope. Depth totals
//! and provenance are required metadata; only delivered evidence may be cut.
use graph_search_types::{
    Envelope,
    result::{DepthCount, SymbolHit, Truncation, TruncationKind},
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct ImpactPayload {
    pub(crate) by_depth: Vec<DepthCount>,
    pub(crate) top: Vec<SymbolHit>,
}
trait Nodes: Serialize {
    fn nodes(&self) -> &[SymbolHit];
    fn nodes_mut(&mut self) -> &mut Vec<SymbolHit>;
}
impl Nodes for Vec<SymbolHit> {
    fn nodes(&self) -> &[SymbolHit] {
        self
    }
    fn nodes_mut(&mut self) -> &mut Vec<SymbolHit> {
        self
    }
}
impl Nodes for ImpactPayload {
    fn nodes(&self) -> &[SymbolHit] {
        &self.top
    }
    fn nodes_mut(&mut self) -> &mut Vec<SymbolHit> {
        &mut self.top
    }
}

pub(crate) fn graph(envelope: &mut Envelope<Vec<SymbolHit>>) -> graph_search::Result<String> {
    fit(envelope)
}
pub(crate) fn impact(envelope: &mut Envelope<ImpactPayload>) -> graph_search::Result<String> {
    fit(envelope)
}
fn encoded(value: &impl Serialize) -> graph_search::Result<String> {
    serde_json::to_string(value).map_err(|error| {
        graph_search::Error::Core(graph_search::core::Error::Store(format!(
            "serialize graph envelope: {error}"
        )))
    })
}

pub(crate) fn scan<T: Serialize>(
    envelope: &mut Envelope<Vec<T>>,
    path: fn(&T) -> &str,
) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        if let Some(context) = &mut envelope.context {
            let paths: std::collections::BTreeSet<_> = envelope.results.iter().map(path).collect();
            context
                .sources
                .retain(|path, _| paths.contains(path.as_str()));
        }
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !envelope
            .truncations
            .iter()
            .any(|item| item.kind == TruncationKind::Bytes)
        {
            envelope.truncations.push(Truncation::new(
                TruncationKind::Bytes,
                cap as u64,
                "serialized scan envelope exceeded its byte cap; matches are partial",
            ));
            continue;
        }
        let empty_sources = std::collections::BTreeMap::new();
        let sources = envelope
            .context
            .as_ref()
            .map_or(&empty_sources, |context| &context.sources);
        if !graph_search::core::payload::drop_scan_tail(
            &mut envelope.results,
            sources,
            json.len().saturating_sub(cap),
            path,
        )
        .map_err(graph_search::Error::Core)?
        {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}
fn drop_tail<T: Serialize>(items: &mut Vec<T>, excess: usize) -> graph_search::Result<bool> {
    let mut removed = 0usize;
    let mut keep = items.len();
    while keep > 0 && removed < excess {
        keep = keep.saturating_sub(1);
        removed = removed.saturating_add(encoded(&items[keep])?.len().saturating_add(1));
    }
    let changed = keep < items.len();
    items.truncate(keep);
    Ok(changed)
}
fn fit<T: Nodes>(envelope: &mut Envelope<T>) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        if let Some(context) = &mut envelope.context {
            context.sources.retain(|path, _| {
                envelope
                    .results
                    .nodes()
                    .iter()
                    .any(|node| &node.path == path)
                    || envelope
                        .edges
                        .iter()
                        .any(|edge| edge.path.as_ref() == Some(path))
            });
        }
        if let Some(approximation) = &mut envelope.approximation {
            approximation.resolved =
                envelope.edges.iter().filter(|edge| edge.resolved).count() as u64;
            approximation.unresolved =
                (envelope.edges.len() as u64).saturating_sub(approximation.resolved);
        }
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !envelope
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes && t.cap == cap as u64)
        {
            envelope.truncations.push(Truncation::new(
                TruncationKind::Bytes,
                cap as u64,
                "serialized graph envelope exceeded its byte cap; evidence is partial",
            ));
            continue;
        }
        let excess = json.len().saturating_sub(cap);
        if drop_tail(&mut envelope.edges, excess)?
            || drop_tail(envelope.results.nodes_mut(), excess)?
        {
            continue;
        }
        return Err(graph_search::Error::Core(
            graph_search::core::Error::ResultBudget(cap),
        ));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn required_envelope_metadata_is_not_discarded_to_claim_a_fitting_result() {
        let mut envelope = Envelope::new(
            3,
            "search.graph.symbol",
            "root",
            serde_json::json!({"target": "x".repeat(70_000)}),
            Vec::new(),
        );
        assert!(matches!(
            graph(&mut envelope),
            Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(65_536)
            ))
        ));
        assert_eq!(envelope.query["target"].as_str().unwrap().len(), 70_000);
    }
}

#[derive(Serialize)]
pub(crate) struct OccurrencePayload {
    pub(crate) items: Vec<graph_search_types::occurrence::OccurrenceHit>,
    pub(crate) indexed_files: usize,
    pub(crate) extracted_files: usize,
}

pub(crate) fn occurrences(
    envelope: &mut Envelope<OccurrencePayload>,
) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        if let Some(context) = &mut envelope.context {
            context
                .sources
                .retain(|path, _| envelope.results.items.iter().any(|item| &item.path == path));
        }
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !envelope
            .truncations
            .iter()
            .any(|item| item.kind == TruncationKind::Bytes)
        {
            envelope.truncations.push(Truncation::new(
                TruncationKind::Bytes,
                cap as u64,
                "serialized occurrence envelope exceeded its byte cap; reference sites are partial",
            ));
            continue;
        }
        if !drop_tail(&mut envelope.results.items, json.len().saturating_sub(cap))? {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}

pub(crate) fn status(
    envelope: &mut Envelope<graph_search_types::result::IndexStatus>,
) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !graph_search::core::payload::trim_status_paths(
            &mut envelope.results,
            json.len().saturating_sub(cap),
        )
        .map_err(graph_search::Error::Core)?
        {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}

pub(crate) fn sync(
    envelope: &mut Envelope<graph_search_types::result::SyncReport>,
) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !graph_search::core::payload::trim_sync_details(
            &mut envelope.results,
            json.len().saturating_sub(cap),
        )
        .map_err(graph_search::Error::Core)?
        {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}

pub(crate) fn stale(envelope: &mut Envelope<Vec<String>>) -> graph_search::Result<String> {
    let cap = graph_search_types::limits::MAX_TOTAL_BYTES;
    loop {
        envelope.stale_paths = Some(envelope.results.clone());
        if let Some(context) = &mut envelope.context {
            context
                .staleness
                .changed_paths
                .clone_from(&envelope.results);
        }
        let json = encoded(envelope)?;
        if json.len() <= cap {
            return Ok(json);
        }
        if !envelope
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
        {
            envelope.truncations.push(Truncation::new(TruncationKind::Bytes, cap as u64,
                "serialized stale notice exceeded its byte cap; changed path details are partial, total remains exact"));
            continue;
        }
        // Each retained path appears in results, stale_paths and context.
        if !drop_tail(
            &mut envelope.results,
            json.len().saturating_sub(cap).div_ceil(3),
        )? {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}
