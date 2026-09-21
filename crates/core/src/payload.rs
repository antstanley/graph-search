//! Serialized result budgets, including provenance added by the library.

use crate::{Error, Result};
use graph_search_types::context::ResultContext;
use graph_search_types::limits::MAX_TOTAL_BYTES;
use graph_search_types::result::{
    Approximation, EdgeHit, GraphResult, ImpactResult, SymbolHit, Truncation, TruncationKind,
};
use serde::Serialize;

/// Bounded collection of scan hits before final provenance and notices are fitted.
#[derive(Default)]
pub(crate) struct ScanBudget(usize);

impl ScanBudget {
    pub(crate) fn admit(&mut self, hit: &impl Serialize) -> Result<bool> {
        let next = self.0.saturating_add(size(hit)?).saturating_add(1);
        if next > MAX_TOTAL_BYTES {
            return Ok(false);
        }
        self.0 = next;
        Ok(true)
    }
}

pub(crate) fn scan_notice(truncations: &mut Vec<Truncation>) {
    if !truncations
        .iter()
        .any(|item| item.kind == TruncationKind::Bytes)
    {
        truncations.push(Truncation::new(
            TruncationKind::Bytes,
            MAX_TOTAL_BYTES as u64,
            "serialized scan result exceeded its byte cap; matches are partial",
        ));
    }
}

macro_rules! fit_scan {
    ($name:ident, $ty:ty) => {
        /// Fits scan hits and their source identities to the compact-JSON ceiling.
        /// # Errors
        /// If required metadata cannot fit or serialization fails.
        pub fn $name(result: &mut $ty) -> Result<()> {
            loop {
                let paths: std::collections::BTreeSet<_> =
                    result.items.iter().map(|item| item.path.as_str()).collect();
                result
                    .context
                    .sources
                    .retain(|path, _| paths.contains(path.as_str()));
                let bytes = size(result)?;
                if bytes <= MAX_TOTAL_BYTES {
                    return Ok(());
                }
                scan_notice(&mut result.truncations);
                if !drop_scan_tail(
                    &mut result.items,
                    &result.context.sources,
                    bytes.saturating_sub(MAX_TOTAL_BYTES),
                    |item| item.path.as_str(),
                )? {
                    return Err(Error::ResultBudget(MAX_TOTAL_BYTES));
                }
            }
        }
    };
}
fit_scan!(fit_files, graph_search_types::result::FilesResult);
fit_scan!(fit_text, graph_search_types::result::TextResult);

/// Drops a scan suffix, counting an identity only when its last hit is removed.
/// # Errors
/// On serialization failure.
pub fn drop_scan_tail<T: Serialize>(
    items: &mut Vec<T>,
    sources: &std::collections::BTreeMap<String, graph_search_types::context::SourceIdentity>,
    excess: usize,
    path: fn(&T) -> &str,
) -> Result<bool> {
    let mut counts = std::collections::BTreeMap::new();
    for item in items.iter() {
        let count = counts.entry(path(item)).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    let mut removed = 0usize;
    let mut keep = items.len();
    while keep > 0 && removed < excess {
        keep = keep.saturating_sub(1);
        let item = &items[keep];
        removed = removed.saturating_add(size(item)?).saturating_add(1);
        let name = path(item);
        if let Some(count) = counts.get_mut(name) {
            *count = count.saturating_sub(1);
            if *count == 0
                && let Some(source) = sources.get(name)
            {
                removed = removed
                    .saturating_add(size(&name)?)
                    .saturating_add(size(source)?)
                    .saturating_add(2);
            }
        }
    }
    let changed = keep < items.len();
    items.truncate(keep);
    Ok(changed)
}

type Parts<'a> = (
    &'a mut Vec<SymbolHit>,
    &'a mut Vec<EdgeHit>,
    &'a mut ResultContext,
    &'a mut Vec<Truncation>,
    &'a mut Option<Approximation>,
);

trait Payload: Serialize {
    fn parts(&mut self) -> Parts<'_>;
}

macro_rules! payload {
    ($ty:ty, $nodes:ident) => {
        impl Payload for $ty {
            fn parts(&mut self) -> Parts<'_> {
                (
                    &mut self.$nodes,
                    &mut self.edges,
                    &mut self.context,
                    &mut self.truncations,
                    &mut self.approximation,
                )
            }
        }
    };
}
payload!(GraphResult, nodes);
payload!(ImpactResult, top);

/// Fits a graph answer to the hard compact-JSON payload ceiling.
/// Lower-priority tail edges are removed before ranked nodes. Counts describe
/// the delivered edges; candidate statistics retain their pre-packing meaning.
///
/// # Errors
/// If required metadata alone cannot fit or serialization fails.
pub fn fit_graph(result: &mut GraphResult) -> Result<()> {
    fit(result, MAX_TOTAL_BYTES)
}

/// Fits individual references without stripping source identity or binding evidence.
/// # Errors
/// When required metadata alone cannot fit or serialization fails.
pub fn fit_occurrences(
    result: &mut graph_search_types::occurrence::OccurrenceResult,
) -> Result<()> {
    loop {
        result
            .context
            .sources
            .retain(|path, _| result.items.iter().any(|item| &item.path == path));
        let bytes = size(result)?;
        if bytes <= MAX_TOTAL_BYTES {
            return Ok(());
        }
        if !result
            .truncations
            .iter()
            .any(|item| item.kind == TruncationKind::Bytes)
        {
            result.truncations.push(Truncation::new(
                TruncationKind::Bytes,
                MAX_TOTAL_BYTES as u64,
                "serialized occurrence result exceeded its byte cap; reference sites are partial",
            ));
            continue;
        }
        if !drop_tail(&mut result.items, bytes.saturating_sub(MAX_TOTAL_BYTES))? {
            return Err(Error::ResultBudget(MAX_TOTAL_BYTES));
        }
    }
}

/// Fits an impact answer, retaining depth counts even when evidence is cut.
///
/// # Errors
/// If required metadata alone cannot fit or serialization fails.
pub fn fit_impact(result: &mut ImpactResult) -> Result<()> {
    fit(result, MAX_TOTAL_BYTES)
}

fn size(value: &impl Serialize) -> Result<usize> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| Error::Store(error.to_string()))
}

// Visit the removed tail once, not repeatedly serializing an entire shrinking
// star graph for each omitted edge. The next whole-result check accounts for
// changes in approximation counts and truncation metadata.
fn drop_tail<T: Serialize>(items: &mut Vec<T>, excess: usize) -> Result<bool> {
    let mut removed = 0usize;
    let mut keep = items.len();
    while keep > 0 && removed < excess {
        keep = keep.saturating_sub(1);
        removed = removed.saturating_add(size(&items[keep])?.saturating_add(1));
    }
    let changed = keep < items.len();
    items.truncate(keep);
    Ok(changed)
}

fn fit(result: &mut impl Payload, cap: usize) -> Result<()> {
    loop {
        let (nodes, edges, context, _, approximation) = result.parts();
        context.sources.retain(|path, _| {
            nodes.iter().any(|node| &node.path == path)
                || edges.iter().any(|edge| edge.path.as_ref() == Some(path))
        });
        if let Some(approximation) = approximation {
            approximation.resolved = edges.iter().filter(|edge| edge.resolved).count() as u64;
            approximation.unresolved = (edges.len() as u64).saturating_sub(approximation.resolved);
        }
        let bytes = size(result)?;
        if bytes <= cap {
            return Ok(());
        }
        let (_, _, _, truncations, _) = result.parts();
        if !truncations
            .iter()
            .any(|item| item.kind == TruncationKind::Bytes)
        {
            truncations.push(Truncation::new(
                TruncationKind::Bytes,
                cap as u64,
                "serialized graph result exceeded its byte cap; evidence is partial",
            ));
            continue;
        }
        let (nodes, edges, _, _, _) = result.parts();
        if drop_tail(edges, bytes.saturating_sub(cap))?
            || drop_tail(nodes, bytes.saturating_sub(cap))?
        {
            continue;
        }
        return Err(Error::ResultBudget(cap));
    }
}

/// Fits status path details while retaining exact counts and required metadata.
/// # Errors
/// If required metadata cannot fit or serialization fails.
pub fn fit_status(result: &mut graph_search_types::result::IndexStatus) -> Result<()> {
    loop {
        let bytes = size(result)?;
        if bytes <= MAX_TOTAL_BYTES {
            return Ok(());
        }
        if !trim_status_paths(result, bytes.saturating_sub(MAX_TOTAL_BYTES))? {
            return Err(Error::ResultBudget(MAX_TOTAL_BYTES));
        }
    }
}

/// Removes a suffix of changed paths to make room in status or its transport envelope.
/// The exact changed count remains intact, and omission is explicitly reported.
/// # Errors
/// When serialization of a path fails.
pub fn trim_status_paths(
    result: &mut graph_search_types::result::IndexStatus,
    excess: usize,
) -> Result<bool> {
    if !result
        .coverage
        .truncations
        .iter()
        .any(|t| t.kind == TruncationKind::Bytes)
    {
        result.coverage.truncations.push(Truncation::new(
            TruncationKind::Bytes, MAX_TOTAL_BYTES as u64,
            "serialized status exceeded its byte cap; changed path details are partial, counts remain exact",
        ));
    }
    let Some(staleness) = &mut result.staleness else {
        return Ok(false);
    };
    let mut removed = 0usize;
    let mut keep = staleness.changed_paths.len();
    while keep > 0 && removed < excess {
        keep = keep.saturating_sub(1);
        removed = removed
            .saturating_add(size(&staleness.changed_paths[keep])?)
            .saturating_add(1);
    }
    let changed = keep < staleness.changed_paths.len();
    staleness.changed_paths.truncate(keep);
    Ok(changed)
}

/// Fits reconciliation detail lists while preserving exact category totals.
/// Call before publication; reserve elapsed counter growth separately.
/// # Errors
/// If required report metadata cannot fit or serialization fails.
pub fn fit_sync(report: &mut graph_search_types::result::SyncReport) -> Result<()> {
    report.counts = Some(report.totals());
    loop {
        let bytes = size(report)?;
        if bytes <= MAX_TOTAL_BYTES {
            return Ok(());
        }
        if !trim_sync_details(report, bytes.saturating_sub(MAX_TOTAL_BYTES))? {
            return Err(Error::ResultBudget(MAX_TOTAL_BYTES));
        }
    }
}

/// Trims report details for the library result or its final transport envelope.
/// Category totals and quarantine coverage remain exact.
/// # Errors
/// When detail serialization fails.
pub fn trim_sync_details(
    report: &mut graph_search_types::result::SyncReport,
    excess: usize,
) -> Result<bool> {
    report.counts = Some(report.totals());
    if !report
        .coverage
        .truncations
        .iter()
        .any(|t| t.kind == TruncationKind::Bytes)
    {
        report.coverage.truncations.push(Truncation::new(
            TruncationKind::Bytes, MAX_TOTAL_BYTES as u64,
            "serialized sync report exceeded its byte cap; details are partial, category totals remain exact",
        ));
    }
    let mut remaining = excess;
    let mut changed = drop_report_tail(&mut report.added, &mut remaining)?;
    changed |= drop_report_tail(&mut report.modified, &mut remaining)?;
    changed |= drop_report_tail(&mut report.removed, &mut remaining)?;
    changed |= drop_report_tail(&mut report.renamed, &mut remaining)?;
    changed |= drop_report_tail(&mut report.quarantined, &mut remaining)?;
    Ok(changed)
}

fn drop_report_tail<T: Serialize>(items: &mut Vec<T>, remaining: &mut usize) -> Result<bool> {
    let mut keep = items.len();
    while keep > 0 && *remaining > 0 {
        keep = keep.saturating_sub(1);
        *remaining = remaining.saturating_sub(size(&items[keep])?.saturating_add(1));
    }
    let changed = keep < items.len();
    items.truncate(keep);
    Ok(changed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use graph_search_types::{Edge, EdgeKind, NodeId};

    #[test]
    fn scan_packing_counts_shared_identities_only_at_the_last_removed_hit() {
        use graph_search_types::context::{SourceIdentity, SourceVerification};
        use graph_search_types::result::{TextHit, TextResult};
        let mut result = TextResult::default();
        for n in 0..1000 {
            let path = format!("file-{n:04}.txt");
            result.context.sources.insert(
                path.clone(),
                SourceIdentity {
                    indexed_hash: None,
                    observed_hash: Some("a".repeat(64)),
                    verification: SourceVerification::Live,
                },
            );
            for line in 1..=2 {
                result.items.push(TextHit {
                    path: path.clone(),
                    line,
                    text: "needle".into(),
                });
            }
        }
        let before = result.items.clone();
        fit_text(&mut result).expect("fit scan");
        assert!(size(&result).expect("bytes") <= MAX_TOTAL_BYTES);
        assert!(!result.items.is_empty() && result.items.len() < before.len());
        assert_eq!(result.items, before[..result.items.len()]);
        assert!(
            result
                .context
                .sources
                .keys()
                .all(|path| result.items.iter().any(|hit| &hit.path == path))
        );
        assert!(
            result
                .items
                .iter()
                .all(|hit| result.context.sources.contains_key(&hit.path))
        );
        let packed = result.clone();
        fit_text(&mut result).expect("idempotent");
        assert_eq!(result, packed);
        result.context.generation = Some("x".repeat(MAX_TOTAL_BYTES));
        assert!(matches!(fit_text(&mut result), Err(Error::ResultBudget(_))));
    }

    #[test]
    fn dense_edges_are_bounded_deterministically_and_counts_follow_evidence() {
        let mut result = GraphResult {
            edges: (0..2000)
                .map(|n| {
                    EdgeHit::from_edge(&Edge::resolved(
                        &NodeId::new("a"),
                        EdgeKind::Calls,
                        &NodeId::new(format!("target{n}")),
                        "target",
                        Some("a.rs"),
                        Some(1),
                    ))
                })
                .collect(),
            approximation: Some(Approximation::default()),
            ..GraphResult::default()
        };
        let original = result.clone();
        fit_graph(&mut result).expect("fit");
        assert!(size(&result).expect("serialize") <= MAX_TOTAL_BYTES);
        assert!(result.edges.len() < 2000);
        assert_eq!(
            result.approximation.as_ref().expect("counts").resolved,
            result.edges.len() as u64
        );
        assert!(
            result
                .truncations
                .iter()
                .any(|item| item.kind == TruncationKind::Bytes)
        );
        let mut repeat = original;
        fit_graph(&mut repeat).expect("repeat");
        assert_eq!(result, repeat);
    }

    #[test]
    fn impact_packing_preserves_depth_counts_and_counts_candidates() {
        let mut result = ImpactResult::default();
        result
            .by_depth
            .push(graph_search_types::result::DepthCount {
                depth: 1,
                total: 2000,
                by_kind: std::collections::BTreeMap::default(),
            });
        result.stats.candidates = 2000;
        result.top = (0..2000)
            .map(|n| {
                SymbolHit::of(&graph_search_types::Node::file(
                    &format!("source{n}.rs"),
                    graph_search_types::Language::Rust,
                    0,
                    0,
                    "hash",
                    4,
                ))
            })
            .collect();
        fit_impact(&mut result).expect("fit impact");
        assert!(size(&result).expect("serialize") <= MAX_TOTAL_BYTES);
        assert!(result.top.len() < 2000);
        assert_eq!(result.by_depth[0].total, 2000);
        assert_eq!(result.stats.candidates, 2000);
        let retained = result.top.len();
        fit_impact(&mut result).expect("idempotent");
        assert_eq!(result.top.len(), retained);
    }

    #[test]
    fn required_metadata_is_never_silently_discarded() {
        let mut result = GraphResult::default();
        result.context.generation = Some("x".repeat(MAX_TOTAL_BYTES));
        assert!(matches!(
            fit_graph(&mut result),
            Err(Error::ResultBudget(_))
        ));
    }
}

#[cfg(test)]
mod status_tests {
    #![allow(clippy::unwrap_used)]
    #[test]
    fn sync_trimming_preserves_all_totals_even_if_no_detail_fits() {
        let mut report = graph_search_types::result::SyncReport {
            added: vec!["a".repeat(70_000)],
            modified: vec!["b".repeat(70_000)],
            removed: vec!["c".repeat(70_000)],
            renamed: vec![graph_search_types::manifest::Rename {
                from: "d".repeat(70_000),
                to: "e".into(),
            }],
            quarantined: vec![graph_search_types::batch::QuarantineRecord::new(
                "f",
                "reason".repeat(20_000),
            )],
            ..Default::default()
        };
        let totals = report.totals();
        let legacy: graph_search_types::result::SyncReport =
            serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
        assert!(legacy.counts.is_none());
        assert_eq!(legacy.totals(), totals);
        fit_sync(&mut report).unwrap();
        assert_eq!(report.totals(), totals);
        assert!(!report.is_empty());
        assert!(report.added.is_empty() && report.modified.is_empty() && report.removed.is_empty());
        assert!(report.renamed.is_empty() && report.quarantined.is_empty());
        let once = report.clone();
        fit_sync(&mut report).unwrap();
        assert_eq!(report, once);
        assert!(serde_json::to_vec(&report).unwrap().len() <= MAX_TOTAL_BYTES);
    }

    use super::*;

    #[test]
    fn status_does_not_discard_required_metadata_to_fit() {
        let mut status = graph_search_types::result::IndexStatus {
            root: "x".repeat(MAX_TOTAL_BYTES + 1),
            ..Default::default()
        };
        assert!(matches!(
            fit_status(&mut status),
            Err(Error::ResultBudget(MAX_TOTAL_BYTES))
        ));
        assert_eq!(status.root.len(), MAX_TOTAL_BYTES + 1);
    }
}
