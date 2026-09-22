//! Verified source interval selection under the final serialized response budget.
use crate::{Result, source::SourceCache, work::WorkBudget};
use graph_search_types::{
    limits::{MAX_EVIDENCE_INTERVAL_LINES, MAX_EVIDENCE_INTERVALS},
    result::{ExcerptRole, ExploreResult, Snippet, SourceExcerpt, Truncation, TruncationKind},
};
use std::collections::{BTreeMap, BTreeSet};

enum Tier {
    Structural,
    MatchContext,
    MatchLine,
    DocumentContext,
}

/// Internal candidate context, kept out of the serialized result until selected.
#[derive(Default)]
pub struct MatchContext {
    /// Useful matching regions excluded by the per-owner bound.
    pub omitted_regions: usize,
    /// Primary and complementary scored regions, all from one source version.
    pub regions: Vec<MatchRegion>,
}

/// Coordinates and document labels for one bounded scored region.
pub struct MatchRegion {
    /// Original region boundary; matched windows cannot cross it.
    pub span: graph_search_types::Span,
    /// Matched lines in the same source unit as the selected evidence.
    pub lines: crate::analyzer::MatchLines,
    /// Parent headings in that source version; at most six references.
    pub headings: Vec<graph_search_types::source::MarkdownHeading>,
    /// Original fenced block for a fragment in that source version.
    pub fence: Option<graph_search_types::source::MarkdownFence>,
    /// Header context for a bounded table row group.
    pub table: Option<graph_search_types::source::MarkdownTable>,
}

struct Candidate {
    item: usize,
    start: u32,
    end: u32,
    role: ExcerptRole,
    value: u64,
    utility: u64,
    bytes: u64,
}

/// Adds implementation, body and relationship intervals using captured source only.
/// All base result metadata and primary snippets are reserved first. Candidates
/// compete by value per estimated byte; final admission uses exact serialized bytes.
/// # Errors
/// On cancellation or deadline.
pub fn extend(
    result: &mut ExploreResult,
    sources: &SourceCache,
    cap: usize,
    work: &mut WorkBudget,
    snapshot: &dyn crate::ports::GraphSnapshot,
    match_lines: &BTreeMap<graph_search_types::NodeId, MatchContext>,
) -> Result<()> {
    let primary_sources = crate::context_dedup::prepare(result);
    extend_prepared(
        result,
        sources,
        cap,
        work,
        snapshot,
        match_lines,
        &primary_sources.sources,
    )
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // Eligibility is distinct from delivered text.
pub(crate) fn extend_prepared(
    result: &mut ExploreResult,
    sources: &SourceCache,
    cap: usize,
    work: &mut WorkBudget,
    snapshot: &dyn crate::ports::GraphSnapshot,
    match_lines: &BTreeMap<graph_search_types::NodeId, MatchContext>,
    primary_sources: &crate::context_dedup::Sources,
) -> Result<()> {
    if result.items.iter().any(|item| {
        primary_sources.contains_key(&item.node.id)
            && match_lines
                .get(&graph_search_types::NodeId::new(&item.node.id))
                .is_some_and(|context| context.omitted_regions > 0)
    }) {
        result.truncations.push(Truncation::new(
            TruncationKind::Candidates,
            crate::body::MAX_REGIONS_PER_OWNER as u64,
            "matching source regions omitted by the per-owner context bound",
        ));
    }
    let mut covered: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    let mut lines = BTreeMap::new();
    for item in &result.items {
        for snippet in item
            .snippet
            .iter()
            .chain(item.excerpts.iter().map(|extra| &extra.snippet))
        {
            covered
                .entry((item.node.path.clone(), snippet.source_hash.clone()))
                .or_default()
                .extend((0..snippet.lines.len()).map(|offset| {
                    snippet
                        .start_line
                        .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))
                }));
        }
        if primary_sources
            .get(&item.node.id)
            .is_some_and(|hash| sources.hash(&item.node.path) == Some(hash.as_str()))
            && let Some(text) = sources.text(&item.node.path)
        {
            lines
                .entry(item.node.path.clone())
                .or_insert_with(|| text.lines().collect::<Vec<_>>());
        }
    }
    let mut candidates = Vec::new();
    let mut match_candidates = Vec::new();
    let mut match_line_candidates = Vec::new();
    let mut document_candidates = Vec::new();
    let mut by_owner: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for edge in &result.edges {
        by_owner.entry(&edge.from).or_default().push(edge);
    }
    for (position, item) in result.items.iter().enumerate() {
        let Some(source_hash) = primary_sources.get(&item.node.id) else {
            continue;
        };
        if sources.hash(&item.node.path) != Some(source_hash.as_str()) {
            continue;
        }
        let Some(source) = lines.get(&item.node.path) else {
            continue;
        };
        let mut add = |start: u32, end: u32, role, value: u64, tier: Tier| {
            let end = end
                .min(u32::try_from(source.len()).unwrap_or(u32::MAX))
                .min(start.saturating_add(MAX_EVIDENCE_INTERVAL_LINES.saturating_sub(1)));
            if start == 0 || start > end {
                return;
            }
            let candidate = Candidate {
                item: position,
                start,
                end,
                role,
                value: value
                    .saturating_mul(1_000_000)
                    // Use the retrieval lane's damped reciprocal-rank prior.
                    // A small reorder must not halve a selected item's context value.
                    .checked_div((position as u64).saturating_add(61))
                    .unwrap_or(0),
                utility: 0,
                bytes: 0,
            };
            match tier {
                Tier::Structural => candidates.push(candidate),
                Tier::MatchContext => match_candidates.push(candidate),
                Tier::MatchLine => match_line_candidates.push(candidate),
                Tier::DocumentContext => document_candidates.push(candidate),
            }
        };
        let size = item
            .node
            .end_line
            .saturating_sub(item.node.start_line)
            .saturating_add(1);
        let short = size <= MAX_EVIDENCE_INTERVAL_LINES;
        add(
            item.node.start_line,
            if short {
                item.node.end_line
            } else {
                item.node.start_line.saturating_add(2)
            },
            ExcerptRole::Declaration,
            if short { 12 } else { 4 },
            Tier::Structural,
        );
        if let Some(body) = &item.evidence {
            if &body.source_hash != source_hash {
                continue;
            }
            add(
                body.span.start_line,
                body.span.end_line,
                ExcerptRole::Body,
                10,
                Tier::Structural,
            );
            if let Some(context) = match_lines.get(&graph_search_types::NodeId::new(&item.node.id))
            {
                for region in &context.regions {
                    for heading in &region.headings {
                        add(
                            heading.span.start_line,
                            heading.span.end_line,
                            ExcerptRole::DocumentHeading,
                            4,
                            Tier::DocumentContext,
                        );
                    }
                    if let Some(fence) = region.fence {
                        add(
                            fence.span.start_line,
                            fence.span.start_line,
                            ExcerptRole::DocumentFence,
                            4,
                            Tier::DocumentContext,
                        );
                        if fence.closed {
                            add(
                                fence.span.end_line,
                                fence.span.end_line,
                                ExcerptRole::DocumentFence,
                                4,
                                Tier::DocumentContext,
                            );
                        }
                    }
                    if let Some(table) = region.table {
                        add(
                            table.header_span.start_line,
                            table.delimiter_span.end_line,
                            ExcerptRole::DocumentTableHeader,
                            4,
                            Tier::DocumentContext,
                        );
                    }

                    for (&line, _) in region
                        .lines
                        .range(region.span.start_line..=region.span.end_line)
                    {
                        add(
                            line.saturating_sub(2).max(region.span.start_line),
                            line.saturating_add(2).min(region.span.end_line),
                            ExcerptRole::Body,
                            10,
                            Tier::MatchContext,
                        );
                        add(line, line, ExcerptRole::Body, 10, Tier::MatchLine);
                    }
                }
            }
        }
        let mut reference_lines = BTreeSet::new();
        for edge in by_owner.get(item.node.id.as_str()).into_iter().flatten() {
            if edge.path.as_deref() != Some(item.node.path.as_str()) {
                continue;
            }
            let id = graph_search_types::EdgeId::of(
                &graph_search_types::NodeId::new(&edge.from),
                edge.kind,
                edge.to.as_deref().unwrap_or(&edge.to_name),
            );
            let occurrences = snapshot.occurrences()?;
            let positions = occurrences.edge_positions(id.as_str());
            if positions.is_empty() {
                // Legacy/synthetic edges have no raw occurrence facts. Their
                // existing indexed line remains a coarse relationship anchor.
                if let Some(line) = edge.line {
                    reference_lines.insert(line);
                }
            }
            for &position in positions {
                if !work.occurrence()? {
                    break;
                }
                let Some((path, file, record)) =
                    occurrences.record(snapshot.occurrence_files()?, position)
                else {
                    return Err(crate::Error::Store(
                        "occurrence context lookup does not match its generation".into(),
                    ));
                };
                if path == item.node.path && &file.source_hash == source_hash {
                    reference_lines.insert(record.line);
                }
            }
        }
        for line in reference_lines {
            add(
                line.saturating_sub(2).max(1),
                line.saturating_add(2),
                ExcerptRole::Reference,
                10,
                Tier::Structural,
            );
        }
    }
    // Occurrence discovery is complete, but window accounting and elapsed time
    // still grow during allocation. Reserve their full integer widths so final
    // statistics cannot evict an already selected source interval.
    result.stats.occurrences_examined = work.occurrences_examined();
    for notice in work.report().2 {
        if !result
            .truncations
            .iter()
            .any(|prior| prior.kind == notice.kind)
        {
            result.truncations.push(notice);
        }
    }
    let mut final_stats = result.stats;
    final_stats.context_windows_examined = u64::MAX;
    final_stats.elapsed_ms = u64::MAX;
    let stats_reserve = encoded_len(&final_stats).saturating_sub(encoded_len(&result.stats));
    let mut total = encoded_len(result).saturating_add(stats_reserve);
    // Reserve the omission notice before filling the budget with optional context.
    let notice = Truncation::new(
        TruncationKind::Snippet,
        MAX_EVIDENCE_INTERVALS as u64,
        "additional source intervals omitted by the byte or interval budget",
    );
    let reserve = encoded_len(&notice).saturating_add(1);
    let target = cap.saturating_sub(reserve);
    let mut count: usize = result.items.iter().map(|item| item.excerpts.len()).sum();
    let mut omitted = false;
    let mut refresh = true;
    loop {
        if candidates.is_empty() {
            // Preserve structural/relationship evidence, then try context around
            // matches, then the matching lines themselves when neighbors cannot fit.
            candidates = if !match_candidates.is_empty() {
                std::mem::take(&mut match_candidates)
            } else if !match_line_candidates.is_empty() {
                std::mem::take(&mut match_line_candidates)
            } else if !document_candidates.is_empty() {
                std::mem::take(&mut document_candidates)
            } else {
                break;
            };
            refresh = true;
        }
        if refresh {
            if !refresh_costs(
                &mut candidates,
                result,
                &covered,
                &lines,
                match_lines,
                work,
                primary_sources,
            )? {
                break;
            }
            refresh = false;
        }
        let Some(candidate) = candidates.pop() else {
            continue;
        };
        work.check()?;
        let item = &mut result.items[candidate.item];
        let Some(source_hash) = primary_sources.get(&item.node.id) else {
            continue;
        };
        let key = (item.node.path.clone(), source_hash.clone());
        let seen = covered.entry(key).or_default();
        let source = &lines[&item.node.path];
        let mut start = candidate.start;
        while start <= candidate.end {
            if seen.contains(&start) {
                start = start.saturating_add(1);
                continue;
            }
            let mut end = start;
            while end < candidate.end && !seen.contains(&end.saturating_add(1)) {
                end = end.saturating_add(1);
            }
            let raw = &source[start.saturating_sub(1) as usize..end as usize];
            let raw_bytes = raw
                .iter()
                .fold(0usize, |sum, line| sum.saturating_add(line.len()));
            let old = encoded_len(item);
            // Even removing all existing item metadata cannot make this fit.
            if total.saturating_add(raw_bytes) > target.saturating_add(old) {
                omitted = true;
                start = end.saturating_add(1);
                continue;
            }
            let previous = item.excerpts.clone();
            join_excerpt(
                &mut item.excerpts,
                SourceExcerpt {
                    role: candidate.role,
                    snippet: Snippet {
                        source_hash: source_hash.clone(),
                        start_line: start,
                        lines: raw.iter().map(|line| (*line).to_owned()).collect(),
                    },
                },
            );
            let new = encoded_len(item);
            let next_total = total.saturating_sub(old).saturating_add(new);
            let next_count = count
                .saturating_sub(previous.len())
                .saturating_add(item.excerpts.len());
            if next_total <= target && next_count <= MAX_EVIDENCE_INTERVALS {
                total = next_total;
                seen.extend(start..=end);
                count = next_count;
                refresh = true;
            } else {
                item.excerpts = previous;
                omitted = true;
            }
            start = end.saturating_add(1);
        }
    }
    if omitted {
        result.truncations.push(notice);
    }
    Ok(())
}

/// Join only contiguous excerpts of one role/source version, within one item.
/// Primary snippets and gaps remain separate; merging never invents source lines.
fn join_excerpt(excerpts: &mut Vec<SourceExcerpt>, mut added: SourceExcerpt) {
    let mut position = 0;
    let mut insertion = excerpts.len();
    while position < excerpts.len() {
        let other = &excerpts[position];
        let added_end =
            u64::from(added.snippet.start_line).saturating_add(added.snippet.lines.len() as u64);
        let other_end =
            u64::from(other.snippet.start_line).saturating_add(other.snippet.lines.len() as u64);
        let before = other_end == u64::from(added.snippet.start_line);
        let after = added_end == u64::from(other.snippet.start_line);
        if other.role == added.role
            && other.snippet.source_hash == added.snippet.source_hash
            && (before || after)
            && other
                .snippet
                .lines
                .len()
                .saturating_add(added.snippet.lines.len())
                <= MAX_EVIDENCE_INTERVAL_LINES as usize
        {
            let mut other = excerpts.remove(position);
            insertion = insertion.min(position);
            if before {
                other.snippet.lines.append(&mut added.snippet.lines);
                added = other;
            } else {
                added.snippet.lines.append(&mut other.snippet.lines);
            }
            position = 0;
        } else {
            position = position.saturating_add(1);
        }
    }
    excerpts.insert(insertion.min(excerpts.len()), added);
}

type Covered = BTreeMap<(String, String), BTreeSet<u32>>;

#[allow(clippy::too_many_arguments)] // Match facts, delivered coverage and eligibility have distinct lifetimes.
fn refresh_costs(
    candidates: &mut Vec<Candidate>,
    result: &ExploreResult,
    covered: &Covered,
    lines: &BTreeMap<String, Vec<&str>>,
    contexts: &BTreeMap<graph_search_types::NodeId, MatchContext>,
    work: &mut WorkBudget,
    primary_sources: &crate::context_dedup::Sources,
) -> Result<bool> {
    let mut matches = Vec::with_capacity(result.items.len());
    for item in &result.items {
        let mut positions = BTreeMap::new();
        let mut delivered = 0u128;
        if let Some(source_hash) = primary_sources.get(&item.node.id) {
            let key = (item.node.path.clone(), source_hash.clone());
            let seen = covered.get(&key);
            if let Some(context) = contexts.get(&graph_search_types::NodeId::new(&item.node.id)) {
                for region in &context.regions {
                    if !work.context_window()? {
                        return Ok(false);
                    }
                    for (&line, &terms) in &region.lines {
                        *positions.entry(line).or_insert(0u128) |= terms;
                        if seen.is_some_and(|seen| seen.contains(&line)) {
                            delivered |= terms;
                        }
                    }
                }
            }
        }
        matches.push((positions, delivered));
    }
    for candidate in candidates.iter_mut() {
        if !work.context_window()? {
            return Ok(false);
        }
        let item = &result.items[candidate.item];
        let Some(source_hash) = primary_sources.get(&item.node.id) else {
            candidate.bytes = 0;
            continue;
        };
        let key = (item.node.path.clone(), source_hash.clone());
        let seen = covered.get(&key);
        let source = &lines[&item.node.path];
        candidate.bytes = 0;
        let mut new_terms = 0u128;
        let mut new_positions = Vec::new();
        let (positions, delivered) = &matches[candidate.item];
        let mut gap = false;
        for line in candidate.start..=candidate.end {
            if seen.is_some_and(|seen| seen.contains(&line)) {
                gap = false;
                continue;
            }
            let terms = positions.get(&line).copied().unwrap_or(0) & !delivered;
            new_terms |= terms;
            if terms != 0 {
                new_positions.push((line, terms));
            }
            if !gap {
                candidate.bytes = candidate.bytes.saturating_add(160);
            }
            candidate.bytes = candidate
                .bytes
                .saturating_add(source[line.saturating_sub(1) as usize].len() as u64)
                .saturating_add(4);
            gap = true;
        }
        candidate.utility = candidate.value.saturating_mul(
            crate::context_proximity::SCALE
                .saturating_mul(1 + u64::from(new_terms.count_ones()))
                .saturating_add(crate::context_proximity::bonus(&new_positions)),
        );
    }
    candidates.retain(|candidate| candidate.bytes > 0);
    // Lowest priority first: pop() selects the best remaining marginal byte cost.
    candidates.sort_by(|a, b| {
        a.utility
            .saturating_mul(b.bytes)
            .cmp(&b.utility.saturating_mul(a.bytes))
            .then(b.item.cmp(&a.item))
            .then(b.start.cmp(&a.start))
            .then(b.end.cmp(&a.end))
    });
    Ok(true)
}

fn encoded_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{
        Node,
        result::{ExploreItem, SymbolHit},
    };

    #[test]
    fn final_statistics_fit_without_removing_allocated_evidence() {
        use crate::ports::GraphStore;
        let root = tempfile::tempdir().unwrap();
        let text = "café and quoted \"source\"\n".repeat(20);
        std::fs::write(root.path().join("a.rs"), &text).unwrap();
        let mut sources = SourceCache::new(8192, 8192);
        assert!(sources.read(root.path(), "a.rs").is_some());
        let mut node = SymbolHit::of(&Node {
            path: "a.rs".into(),
            ..Node::default()
        });
        node.start_line = 1;
        node.end_line = 20;
        let base = ExploreResult {
            items: vec![ExploreItem {
                node,
                snippet: Some(Snippet {
                    source_hash: sources.hash("a.rs").unwrap().into(),
                    start_line: 1,
                    lines: vec![text.lines().next().unwrap().into()],
                }),
                retrieval: None,
                excerpts: Vec::new(),
                evidence: None,
                impact: None,
            }],
            ..ExploreResult::default()
        };
        let store = crate::memory::MemoryStore::new();
        let snapshot = store.snapshot().unwrap();
        let mut delivered = false;
        for extra in (200..1600).step_by(7) {
            let cap = encoded_len(&base) + extra;
            let mut result = base.clone();
            let mut work = WorkBudget::new(crate::work::WorkLimits::default());
            extend(
                &mut result,
                &sources,
                cap,
                &mut work,
                snapshot.as_ref(),
                &BTreeMap::new(),
            )
            .unwrap();
            delivered |= !result.items[0].excerpts.is_empty();
            result.stats.context_windows_examined = work.context_windows_examined();
            result.stats.elapsed_ms = u64::MAX;
            assert!(encoded_len(&result) <= cap, "cap {cap}");
            assert_eq!(result.items[0].snippet, base.items[0].snippet);
        }
        assert!(
            delivered,
            "the reservation must still permit source evidence"
        );
    }

    #[test]
    fn completing_an_already_delivered_window_uses_only_its_new_line_cost() {
        let item = |path: &str| ExploreItem {
            node: SymbolHit::of(&Node {
                path: path.into(),
                ..Node::default()
            }),
            snippet: Some(Snippet {
                source_hash: "hash".into(),
                start_line: 1,
                lines: vec!["data".into()],
            }),
            retrieval: None,
            excerpts: Vec::new(),
            evidence: None,
            impact: None,
        };
        let result = ExploreResult {
            items: vec![item("a"), item("b")],
            ..ExploreResult::default()
        };
        let mut covered = BTreeMap::from([
            (("a".into(), "hash".into()), (1..10).collect()),
            (("b".into(), "hash".into()), BTreeSet::new()),
        ]);
        let lines = BTreeMap::from([
            ("a".into(), vec!["data"; 10]),
            ("b".into(), vec!["data"; 3]),
        ]);
        let make = |item, end| Candidate {
            item,
            start: 1,
            end,
            role: ExcerptRole::Body,
            value: 100,
            utility: 0,
            bytes: 0,
        };
        let mut candidates = vec![make(0, 10), make(1, 3)];
        let mut work = WorkBudget::new(crate::work::WorkLimits::default());
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &BTreeMap::new(),
                &mut work,
                &crate::context_dedup::sources(&result),
            )
            .unwrap()
        );
        assert_eq!(
            candidates.last().unwrap().item,
            0,
            "one new line beats three despite the larger original window"
        );
        covered
            .get_mut(&("a".into(), "hash".into()))
            .unwrap()
            .insert(10);
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &BTreeMap::new(),
                &mut work,
                &crate::context_dedup::sources(&result),
            )
            .unwrap()
        );
        assert_eq!(
            candidates.len(),
            1,
            "fully delivered windows have no residual cost"
        );
        assert_eq!(candidates[0].item, 1);
        assert_eq!(work.context_windows_examined(), 4);
    }
    #[test]
    fn new_query_coverage_beats_repetition_and_loses_its_bonus_after_delivery() {
        use graph_search_types::{NodeId, Span};
        let item = ExploreItem {
            node: SymbolHit::of(&Node {
                path: "a".into(),
                ..Node::default()
            }),
            snippet: Some(Snippet {
                source_hash: "hash".into(),
                start_line: 1,
                lines: vec!["same".into()],
            }),
            retrieval: None,
            excerpts: Vec::new(),
            evidence: None,
            impact: None,
        };
        let id = NodeId::new(&item.node.id);
        let result = ExploreResult {
            items: vec![item],
            ..ExploreResult::default()
        };
        let contexts = BTreeMap::from([(
            id,
            MatchContext {
                omitted_regions: 0,
                regions: vec![MatchRegion {
                    span: Span::new(1, 4, 0, 20),
                    lines: BTreeMap::from([(1, 1), (2, 1), (3, 2), (4, 2)]),
                    headings: Vec::new(),
                    fence: None,
                    table: None,
                }],
            },
        )]);
        let lines = BTreeMap::from([("a".into(), vec!["same"; 4])]);
        let mut covered = BTreeMap::from([(("a".into(), "hash".into()), BTreeSet::from([1]))]);
        let candidate = |line| Candidate {
            item: 0,
            start: line,
            end: line,
            role: ExcerptRole::Body,
            value: 100,
            utility: 0,
            bytes: 0,
        };
        let mut work = WorkBudget::new(crate::work::WorkLimits::default());
        let mut candidates = vec![candidate(2), candidate(3)];
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &contexts,
                &mut work,
                &crate::context_dedup::sources(&result),
            )
            .unwrap()
        );
        assert_eq!(
            candidates.pop().unwrap().start,
            3,
            "prefer the missing query term"
        );
        covered.values_mut().next().unwrap().insert(3);
        let mut candidates = vec![candidate(2), candidate(4)];
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &contexts,
                &mut work,
                &crate::context_dedup::sources(&result),
            )
            .unwrap()
        );
        assert_eq!(
            candidates.pop().unwrap().start,
            2,
            "once covered, use the ordinary deterministic tie rule"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One fixture verifies ranking before and after delivery.
    fn proximity_prefers_compact_new_terms_without_overriding_extra_coverage() {
        use graph_search_types::{NodeId, Span};
        let item = ExploreItem {
            node: SymbolHit::of(&Node {
                path: "a".into(),
                ..Node::default()
            }),
            snippet: Some(Snippet {
                source_hash: "hash".into(),
                start_line: 20,
                lines: vec!["same".into()],
            }),
            retrieval: None,
            excerpts: Vec::new(),
            evidence: None,
            impact: None,
        };
        let id = NodeId::new(&item.node.id);
        let result = ExploreResult {
            items: vec![item],
            ..ExploreResult::default()
        };
        let contexts = BTreeMap::from([(
            id,
            MatchContext {
                omitted_regions: 0,
                regions: vec![MatchRegion {
                    span: Span::new(1, 20, 0, 100),
                    lines: BTreeMap::from([
                        (1, 7),
                        (2, 1),
                        (6, 2),
                        (9, 1),
                        (10, 2),
                        (14, 1),
                        (16, 2),
                        (18, 4),
                    ]),
                    headings: Vec::new(),
                    fence: None,
                    table: None,
                }],
            },
        )]);
        let lines = BTreeMap::from([("a".into(), vec!["same"; 20])]);
        let mut covered = BTreeMap::from([(("a".into(), "hash".into()), BTreeSet::from([20]))]);
        let candidate = |start| Candidate {
            item: 0,
            start,
            end: start + 4,
            role: ExcerptRole::Body,
            value: 100,
            utility: 0,
            bytes: 0,
        };
        let mut work = WorkBudget::new(crate::work::WorkLimits::default());
        let eligibility = crate::context_dedup::sources(&result);
        let mut candidates = vec![candidate(2), candidate(8)];
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &contexts,
                &mut work,
                &eligibility
            )
            .unwrap()
        );
        assert_eq!(candidates[0].bytes, candidates[1].bytes);
        assert_eq!(
            candidates.pop().unwrap().start,
            8,
            "same coverage and cost, tighter terms"
        );
        let mut candidates = vec![candidate(8), candidate(14)];
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &contexts,
                &mut work,
                &eligibility
            )
            .unwrap()
        );
        assert_eq!(
            candidates.pop().unwrap().start,
            14,
            "an extra new term beats the proximity bonus at equal cost/value"
        );
        covered.values_mut().next().unwrap().insert(1);
        let mut candidates = vec![candidate(2), candidate(8)];
        assert!(
            refresh_costs(
                &mut candidates,
                &result,
                &covered,
                &lines,
                &contexts,
                &mut work,
                &eligibility
            )
            .unwrap()
        );
        assert_eq!(
            candidates.pop().unwrap().start,
            2,
            "already delivered terms regain the deterministic tie rule"
        );
    }

    #[test]
    fn adjacent_excerpts_merge_without_crossing_role_version_gap_or_line_limits() {
        let excerpt = |start, count, role, hash: &str| SourceExcerpt {
            role,
            snippet: Snippet {
                start_line: start,
                source_hash: hash.into(),
                lines: (start..start + count)
                    .map(|n| format!("line {n} café"))
                    .collect(),
            },
        };
        let mut excerpts = vec![
            excerpt(7, 3, ExcerptRole::Body, "hash"),
            excerpt(1, 3, ExcerptRole::Body, "hash"),
        ];
        let bridge = excerpt(4, 3, ExcerptRole::Body, "hash");
        let mut unmerged = excerpts.clone();
        unmerged.push(bridge.clone());
        join_excerpt(&mut excerpts, bridge);
        assert_eq!(excerpts, vec![excerpt(1, 9, ExcerptRole::Body, "hash")]);
        assert!(encoded_len(&excerpts) < encoded_len(&unmerged));
        for separate in [
            excerpt(10, 1, ExcerptRole::Reference, "hash"),
            excerpt(10, 1, ExcerptRole::Body, "other"),
            excerpt(11, 1, ExcerptRole::Body, "hash"),
            excerpt(10, 72, ExcerptRole::Body, "hash"),
        ] {
            let mut trial = excerpts.clone();
            join_excerpt(&mut trial, separate.clone());
            assert_eq!(trial, vec![excerpts[0].clone(), separate]);
        }
        join_excerpt(&mut excerpts, excerpt(10, 71, ExcerptRole::Body, "hash"));
        assert_eq!(excerpts, vec![excerpt(1, 80, ExcerptRole::Body, "hash")]);
    }
    #[test]
    #[allow(clippy::too_many_lines)]
    fn optional_document_context_never_displaces_matched_body_windows() {
        use crate::ports::GraphStore;
        use graph_search_types::{Language, Span, source::SourceEvidence};
        let root = tempfile::tempdir().unwrap();
        let text = format!(
            "# Parent\n~~~rust\n{}~~~\n",
            "body needle with useful implementation\n".repeat(20)
        );
        std::fs::write(root.path().join("a.md"), &text).unwrap();
        let mut sources = SourceCache::new(8192, 8192);
        assert!(sources.read(root.path(), "a.md").is_some());
        let hash = sources.hash("a.md").unwrap().to_owned();
        let facts = crate::units::extract("a.md", &text, &hash, Language::Unknown, &[]);
        let mut node = SymbolHit::of(&Node {
            path: "a.md".into(),
            ..Node::default()
        });
        node.start_line = 10;
        node.end_line = 20;
        let id = graph_search_types::NodeId::new(&node.id);
        let line_offset = |count| {
            text.split_inclusive('\n')
                .take(count)
                .map(str::len)
                .sum::<usize>()
        };
        let base = ExploreResult {
            items: vec![ExploreItem {
                node,
                snippet: Some(Snippet {
                    source_hash: hash.clone(),
                    start_line: 10,
                    lines: vec![text.lines().nth(9).unwrap().into()],
                }),
                retrieval: None,
                excerpts: Vec::new(),
                evidence: Some(SourceEvidence {
                    package: None,
                    package_ref: None,
                    package_scope_incomplete: false,
                    documentation: None,
                    span: Span::new(
                        10,
                        20,
                        u32::try_from(line_offset(9)).unwrap(),
                        u32::try_from(line_offset(20)).unwrap(),
                    ),
                    kind: graph_search_types::source::SourceUnitKind::MarkdownCodeFence,
                    owner: None,
                    match_line: 10,
                    source_hash: hash,
                    live: false,
                }),
                impact: None,
            }],
            ..ExploreResult::default()
        };
        let store = crate::memory::MemoryStore::new();
        let snapshot = store.snapshot().unwrap();
        let mut delivered = false;
        for extra in (200..2200).step_by(113) {
            let cap = encoded_len(&base).saturating_add(extra);
            let mut outputs = Vec::new();
            for include_headings in [false, true] {
                let contexts = BTreeMap::from([(
                    id.clone(),
                    MatchContext {
                        omitted_regions: 0,
                        regions: vec![MatchRegion {
                            span: base.items[0].evidence.as_ref().unwrap().span,
                            lines: BTreeMap::from([(10, 1), (20, 1)]),
                            table: None,
                            fence: if include_headings {
                                facts.units.iter().find_map(|unit| unit.fence)
                            } else {
                                None
                            },
                            headings: if include_headings {
                                facts.units[0].headings.clone()
                            } else {
                                Vec::new()
                            },
                        }],
                    },
                )]);
                let mut result = base.clone();
                let mut work = WorkBudget::new(crate::work::WorkLimits::default());
                extend(
                    &mut result,
                    &sources,
                    cap,
                    &mut work,
                    snapshot.as_ref(),
                    &contexts,
                )
                .unwrap();
                assert!(encoded_len(&result) <= cap);
                outputs.push(result);
            }
            let body = |result: &ExploreResult| {
                result.items[0]
                    .excerpts
                    .iter()
                    .filter(|excerpt| {
                        !matches!(
                            excerpt.role,
                            ExcerptRole::DocumentHeading | ExcerptRole::DocumentFence
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(outputs[0].items[0].snippet, outputs[1].items[0].snippet);
            assert_eq!(body(&outputs[0]), body(&outputs[1]), "cap {cap}");
            delivered |= outputs[1].items[0]
                .excerpts
                .iter()
                .any(|excerpt| excerpt.role == ExcerptRole::DocumentHeading);
        }
        assert!(
            delivered,
            "parent context must remain useful with sufficient budget"
        );
    }
}
