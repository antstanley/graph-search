//! Assign each source line to its first selected carrier without changing seeds.
use graph_search_types::{
    limits::MAX_EVIDENCE_INTERVALS,
    result::{ExcerptRole, ExploreResult, Snippet, SourceExcerpt, Truncation, TruncationKind},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type Sources = BTreeMap<String, String>;

pub(crate) struct Prepared {
    pub(crate) sources: Sources,
    shared: BTreeSet<String>,
}

impl Prepared {
    /// A primary removed by payload fitting is not the same as shared text.
    /// Preserve the latter's eligibility without undoing the former's budget decision.
    pub(crate) fn retain(&mut self, result: &ExploreResult) {
        let admitted: BTreeSet<_> = result
            .items
            .iter()
            .filter(|item| item.snippet.is_some() || self.shared.contains(&item.node.id))
            .map(|item| item.node.id.as_str())
            .collect();
        self.sources.retain(|id, _| admitted.contains(id.as_str()));
    }
}

pub(crate) fn sources(result: &ExploreResult) -> Sources {
    result
        .items
        .iter()
        .filter_map(|item| {
            Some((
                item.node.id.clone(),
                item.snippet.as_ref()?.source_hash.clone(),
            ))
        })
        .collect()
}

/// Preserve pre-deduplication eligibility independently of whether an item still
/// carries primary text. An entirely shared primary must not suppress its other
/// matched regions, declaration or relationship context.
pub(crate) fn prepare(result: &mut ExploreResult) -> Prepared {
    let sources = sources(result);
    let mut covered: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    let mut intervals = 0usize;
    let mut omitted = false;
    for item in &mut result.items {
        let primary = item.snippet.take();
        let extras = std::mem::take(&mut item.excerpts);
        let role = if item.evidence.is_some() {
            ExcerptRole::Body
        } else {
            ExcerptRole::Declaration
        };
        let inputs = primary
            .into_iter()
            .map(|snippet| (true, role, snippet))
            .chain(
                extras
                    .into_iter()
                    .map(|extra| (false, extra.role, extra.snippet)),
            );
        for (primary, role, snippet) in inputs {
            let key = (item.node.path.clone(), snippet.source_hash.clone());
            let seen = covered.entry(key).or_default();
            for run in uncovered(snippet, seen) {
                let is_primary = primary && item.snippet.is_none();
                if !is_primary && intervals >= MAX_EVIDENCE_INTERVALS {
                    omitted = true;
                    continue;
                }
                seen.extend((0..run.lines.len()).map(|offset| {
                    run.start_line
                        .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))
                }));
                if is_primary {
                    item.snippet = Some(run);
                } else {
                    item.excerpts.push(SourceExcerpt { role, snippet: run });
                    intervals = intervals.saturating_add(1);
                }
            }
        }
    }
    if omitted {
        result.truncations.push(Truncation::new(
            TruncationKind::Snippet,
            MAX_EVIDENCE_INTERVALS as u64,
            "primary source fragments omitted by the interval cap",
        ));
    }
    let shared = result
        .items
        .iter()
        .filter(|item| item.snippet.is_none() && sources.contains_key(&item.node.id))
        .map(|item| item.node.id.clone())
        .collect();
    Prepared { sources, shared }
}

fn uncovered(snippet: Snippet, seen: &BTreeSet<u32>) -> Vec<Snippet> {
    let mut runs: Vec<Snippet> = Vec::new();
    let mut contiguous = false;
    for (offset, text) in snippet.lines.into_iter().enumerate() {
        let line = snippet
            .start_line
            .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
        if seen.contains(&line) {
            contiguous = false;
            continue;
        }
        if contiguous {
            if let Some(run) = runs.last_mut() {
                run.lines.push(text);
            }
        } else {
            runs.push(Snippet {
                source_hash: snippet.source_hash.clone(),
                start_line: line,
                lines: vec![text],
            });
            contiguous = true;
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{
        Node, NodeId,
        result::{ExploreItem, SymbolHit},
    };

    fn item(id: &str, path: &str, hash: &str, start: u32, end: u32) -> ExploreItem {
        ExploreItem {
            node: SymbolHit::of(&Node {
                id: NodeId::new(id),
                path: path.into(),
                ..Node::default()
            }),
            snippet: Some(Snippet {
                source_hash: hash.into(),
                start_line: start,
                lines: (start..=end).map(|n| format!("line {n}")).collect(),
            }),
            evidence: None,
            retrieval: None,
            impact: None,
            excerpts: Vec::new(),
        }
    }

    #[test]
    fn overlapping_primaries_preserve_union_versions_and_eligibility_without_duplicate_lines() {
        let mut result = ExploreResult {
            items: vec![
                item("first", "a", "v1", 3, 4),
                item("second", "a", "v1", 1, 6),
                item("covered", "a", "v1", 2, 5),
                item("path", "b", "v1", 1, 6),
                item("version", "a", "v2", 1, 6),
            ],
            ..ExploreResult::default()
        };
        let original_nodes: Vec<_> = result.items.iter().map(|x| x.node.clone()).collect();
        let versions = prepare(&mut result);
        assert_eq!(versions.sources.len(), 5);
        assert_eq!(versions.sources["covered"], "v1");
        assert!(result.items[2].snippet.is_none());
        assert!(result.items[2].excerpts.is_empty());
        assert_eq!(result.items[1].snippet.as_ref().unwrap().start_line, 1);
        assert_eq!(result.items[1].excerpts[0].snippet.start_line, 5);
        assert_eq!(
            result
                .items
                .iter()
                .map(|x| x.node.clone())
                .collect::<Vec<_>>(),
            original_nodes
        );
        let mut unique = BTreeSet::new();
        for item in &result.items {
            for snippet in item
                .snippet
                .iter()
                .chain(item.excerpts.iter().map(|x| &x.snippet))
            {
                for (offset, text) in snippet.lines.iter().enumerate() {
                    let line = snippet.start_line + u32::try_from(offset).unwrap();
                    assert_eq!(text, &format!("line {line}"));
                    assert!(unique.insert((
                        item.node.path.clone(),
                        snippet.source_hash.clone(),
                        line
                    )));
                }
            }
        }
        assert_eq!(unique.len(), 18);
        let before = result.clone();
        prepare(&mut result);
        assert_eq!(result, before, "wire representation is idempotent");
    }

    #[test]
    fn payload_eviction_does_not_reenable_an_unshared_primary() {
        let mut result = ExploreResult {
            items: vec![
                item("first", "a", "v1", 1, 2),
                item("shared", "a", "v1", 1, 2),
            ],
            ..ExploreResult::default()
        };
        let mut prepared = prepare(&mut result);
        result.items[0].snippet = None;
        prepared.retain(&result);
        assert!(!prepared.sources.contains_key("first"));
        assert!(prepared.sources.contains_key("shared"));
        result.items.pop();
        prepared.retain(&result);
        assert!(prepared.sources.is_empty());
    }

    #[test]
    fn omitted_fragments_are_bounded_reported_and_do_not_hide_later_carriers() {
        let mut result = ExploreResult::default();
        for n in 0..=MAX_EVIDENCE_INTERVALS {
            let path = format!("path{n}");
            result
                .items
                .push(item(&format!("block{n}"), &path, "hash", 3, 4));
            result
                .items
                .push(item(&format!("wide{n}"), &path, "hash", 1, 6));
        }
        result.items.push(item(
            "rescue",
            &format!("path{MAX_EVIDENCE_INTERVALS}"),
            "hash",
            5,
            6,
        ));
        prepare(&mut result);
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.excerpts.len())
                .sum::<usize>(),
            MAX_EVIDENCE_INTERVALS
        );
        assert_eq!(result.truncations.len(), 1);
        assert_eq!(result.truncations[0].kind, TruncationKind::Snippet);
        let rescue = result.items.last().unwrap().snippet.as_ref().unwrap();
        assert_eq!(rescue.start_line, 5);
        assert_eq!(rescue.lines, ["line 5", "line 6"]);
    }

    #[test]
    fn entirely_shared_primary_still_receives_its_distinct_declaration_context() {
        use crate::ports::GraphStore;
        use std::fmt::Write as _;
        let root = tempfile::tempdir().unwrap();
        let mut text = String::new();
        for line in 1..=20 {
            writeln!(text, "line {line}").unwrap();
        }
        std::fs::write(root.path().join("a"), &text).unwrap();
        let mut cache = crate::source::SourceCache::new(8192, 8192);
        assert!(cache.read(root.path(), "a").is_some());
        let hash = cache.hash("a").unwrap();
        let mut first = item("first", "a", hash, 1, 1);
        first.node.start_line = 1;
        first.node.end_line = 1;
        let mut second = item("second", "a", hash, 1, 1);
        second.node.start_line = 1;
        second.node.end_line = 20;
        let mut result = ExploreResult {
            items: vec![first, second],
            ..ExploreResult::default()
        };
        let versions = prepare(&mut result);
        assert!(result.items[1].snippet.is_none());
        let store = crate::memory::MemoryStore::new();
        let snapshot = store.snapshot().unwrap();
        crate::evidence::extend_prepared(
            &mut result,
            &cache,
            8192,
            &mut crate::work::WorkBudget::new(crate::work::WorkLimits::default()),
            snapshot.as_ref(),
            &BTreeMap::new(),
            &versions.sources,
        )
        .unwrap();
        assert!(result.items[1].snippet.is_none());
        assert_eq!(result.items[1].excerpts.len(), 1);
        let extra = &result.items[1].excerpts[0].snippet;
        assert_eq!(extra.start_line, 2);
        assert_eq!(extra.lines.len(), 19);
        assert_eq!(extra.source_hash, hash);
        assert_eq!(extra.lines.last().unwrap(), "line 20");
    }
}
