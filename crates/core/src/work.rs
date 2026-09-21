//! Request-scoped graph/metadata work limits and cooperative cancellation.

use crate::{Error, Result};
use graph_search_types::limits::{
    GRAPH_WORK_EDGES_CEILING, GRAPH_WORK_EDGES_DEFAULT, GRAPH_WORK_NODES_CEILING,
    GRAPH_WORK_NODES_DEFAULT,
};
use graph_search_types::limits::{
    LEXICAL_POSTINGS_CEILING, LEXICAL_POSTINGS_DEFAULT, METADATA_CANDIDATES_CEILING,
    METADATA_CANDIDATES_DEFAULT,
};
use graph_search_types::{
    NodeId,
    result::{Truncation, TruncationKind},
};
use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

/// Rejects oversized query fields before compiling, copying or reading the tree.
/// # Errors
/// When the UTF-8 byte length exceeds the query-field ceiling.
pub fn validate_query_bytes(field: &str, value: &str) -> Result<()> {
    if value.len() > graph_search_types::limits::MAX_QUERY_BYTES {
        return Err(Error::InvalidQuery(format!(
            "{field} exceeds the {}-byte query field limit",
            graph_search_types::limits::MAX_QUERY_BYTES
        )));
    }
    Ok(())
}

/// A cloneable cancellation signal shared by a host and a running query.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    /// Requests cancellation at the next cooperative query checkpoint.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Limits shared by source, graph and metadata retrieval within one query.
#[derive(Clone, Debug)]
pub struct WorkLimits {
    /// Positional scan bytes across all source fields (ceiling 1 GiB).
    pub positional_bytes: usize,
    /// Positional token comparisons across all fields (ceiling 10,000,000).
    pub positional_tokens: usize,
    /// Verified positional witnesses across all fields (ceiling 10,000).
    pub positional_witnesses: usize,
    /// Visible directory entries processed across query walks (ceiling 1,000,000).
    pub walk_entries: usize,
    /// Maximum source-file open attempts in a content query (ceiling 200,000).
    pub source_files: usize,
    /// Total query source bytes, excluding at most one overflow-detection byte (ceiling 1 GiB).
    pub source_bytes: usize,
    /// Maximum context-window cost evaluations, including re-evaluations (ceiling 100,000).
    pub context_windows: usize,
    /// Maximum raw occurrence entries examined (hard ceiling 100,000).
    pub occurrences: usize,
    /// Maximum distinct graph nodes admitted (hard ceiling 100,000).
    pub nodes: usize,
    /// Maximum adjacency entries examined, including filtered entries (hard ceiling 500,000).
    pub edges: usize,
    /// Maximum delivered relationships, independent of adjacency examination work.
    pub returned_edges: usize,
    /// Unscored metadata records inspected before filtering (hard ceiling 1,000,000).
    pub metadata_entries: usize,
    /// Maximum candidate admissions across native metadata retrieval lanes.
    pub candidates: usize,
    /// Maximum lexical postings examined, including filtered postings.
    pub postings: usize,
    /// Maximum dictionary entries expanded by prefix lookup (hard ceiling 4,096).
    pub dictionary_entries: usize,
    /// Optional monotonic deadline, checked cooperatively.
    pub deadline: Option<Instant>,
    /// Optional host cancellation signal.
    pub cancellation: Option<CancellationToken>,
}
impl Default for WorkLimits {
    fn default() -> Self {
        Self {
            positional_bytes: 64 * 1024 * 1024,
            positional_tokens: 1_000_000,
            positional_witnesses: 1024,
            walk_entries: graph_search_types::limits::MAX_WALK_ENTRIES,
            source_files: 10_000,
            source_bytes: 64 * 1024 * 1024,
            context_windows: 10_000,
            occurrences: 10_000,
            nodes: GRAPH_WORK_NODES_DEFAULT,
            edges: GRAPH_WORK_EDGES_DEFAULT,
            returned_edges: graph_search_types::limits::RETURNED_EDGES_DEFAULT,
            metadata_entries: 100_000,
            candidates: METADATA_CANDIDATES_DEFAULT,
            postings: LEXICAL_POSTINGS_DEFAULT,
            dictionary_entries: graph_search_types::limits::DICTIONARY_ENTRIES_DEFAULT,
            deadline: None,
            cancellation: None,
        }
    }
}

/// Mutable accounting for a single request. A cap fires only on omitted work.
pub struct WorkBudget {
    pub(crate) source_captures: Option<crate::source_capture::Captures>,
    positional_bytes: usize,
    positional_tokens: usize,
    positional_witnesses: usize,
    walk_entries: usize,
    source_files: usize,
    source_bytes: usize,
    context_windows: usize,
    occurrences: usize,
    limits: WorkLimits,
    visited: BTreeSet<NodeId>,
    examined: usize,
    metadata_entries: usize,
    candidates: usize,
    postings: usize,
    dictionary_entries: usize,
    truncations: Vec<Truncation>,
}
impl WorkBudget {
    pub(crate) fn limits(&self) -> WorkLimits {
        self.limits.clone()
    }
    /// Creates an empty budget, clamping caller limits to hard ceilings.
    #[must_use]
    pub fn new(mut limits: WorkLimits) -> Self {
        limits.positional_bytes = limits.positional_bytes.min(1024 * 1024 * 1024);
        limits.positional_tokens = limits.positional_tokens.min(10_000_000);
        limits.positional_witnesses = limits.positional_witnesses.min(10_000);
        limits.walk_entries = limits
            .walk_entries
            .min(graph_search_types::limits::MAX_WALK_ENTRIES);
        limits.source_files = limits.source_files.min(200_000);
        limits.source_bytes = limits.source_bytes.min(1024 * 1024 * 1024);
        limits.context_windows = limits.context_windows.min(100_000);
        limits.occurrences = limits.occurrences.min(100_000);
        limits.dictionary_entries = limits
            .dictionary_entries
            .min(graph_search_types::limits::DICTIONARY_ENTRIES_CEILING);
        limits.returned_edges = limits
            .returned_edges
            .min(graph_search_types::limits::RETURNED_EDGES_CEILING);
        limits.metadata_entries = limits.metadata_entries.min(1_000_000);
        limits.candidates = limits.candidates.min(METADATA_CANDIDATES_CEILING);
        limits.postings = limits.postings.min(LEXICAL_POSTINGS_CEILING);
        limits.nodes = limits.nodes.min(GRAPH_WORK_NODES_CEILING);
        limits.edges = limits.edges.min(GRAPH_WORK_EDGES_CEILING);
        Self {
            source_captures: None,
            positional_bytes: 0,
            positional_tokens: 0,
            positional_witnesses: 0,
            walk_entries: 0,
            source_files: 0,
            source_bytes: 0,
            context_windows: 0,
            occurrences: 0,
            limits,
            visited: BTreeSet::new(),
            examined: 0,
            metadata_entries: 0,
            candidates: 0,
            postings: 0,
            dictionary_entries: 0,
            truncations: Vec::new(),
        }
    }
    /// Reuses raw source bytes across freshness, maintenance and evidence phases.
    /// Retention is bounded by this request's source-byte and open-attempt limits.
    /// Metadata drift on reuse fails the request instead of mixing source versions.
    pub fn enable_source_capture(&mut self) {
        self.source_captures.get_or_insert_with(Default::default);
    }

    /// Checks deadline and cancellation without charging work.
    /// # Errors
    /// When the host cancels or the deadline has elapsed.
    pub fn check(&self) -> Result<()> {
        if self
            .limits
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(Error::QueryCancelled);
        }
        if self
            .limits
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(Error::QueryDeadline);
        }
        Ok(())
    }
    /// Remaining visible-entry allowance, shared by successive query walks.
    #[must_use]
    pub fn remaining_walk_entries(&self) -> usize {
        self.limits.walk_entries.saturating_sub(self.walk_entries)
    }

    pub(crate) fn charge_walk_entries(&mut self, count: u64, exhausted: bool) {
        self.walk_entries = self
            .walk_entries
            .saturating_add(usize::try_from(count).unwrap_or(usize::MAX));
        if exhausted {
            self.record(
                TruncationKind::WalkEntries,
                self.limits.walk_entries,
                "query directory-entry budget reached; enumeration is partial",
            );
        }
    }

    /// Clamped result-edge allowance, independent of work already performed.
    #[must_use]
    pub const fn returned_edge_limit(&self) -> usize {
        self.limits.returned_edges
    }

    /// Admits one source open attempt after path filtering.
    /// # Errors
    /// On cancellation or deadline.
    pub fn source_file(&mut self) -> Result<bool> {
        self.check()?;
        if self.source_files >= self.limits.source_files {
            self.record(
                TruncationKind::SourceFiles,
                self.limits.source_files,
                "source file work cap reached; source evidence is partial",
            );
            return Ok(false);
        }
        if self.source_bytes >= self.limits.source_bytes {
            self.record(
                TruncationKind::SourceBytes,
                self.limits.source_bytes,
                "source byte work cap reached; source evidence is partial",
            );
            return Ok(false);
        }
        self.source_files = self.source_files.saturating_add(1);
        Ok(true)
    }

    /// Remaining bytes plus one bounded probe to distinguish exact EOF from overflow.
    #[must_use]
    pub fn source_read_limit(&self) -> u64 {
        let remaining = self.limits.source_bytes.saturating_sub(self.source_bytes);
        if remaining == 0 {
            0
        } else {
            remaining.saturating_add(1) as u64
        }
    }

    /// Accounts for actual bytes consumed, including failed/invalid files and the probe.
    pub fn charge_source_bytes(&mut self, bytes: usize) {
        self.source_bytes = self.source_bytes.saturating_add(bytes);
        if self.source_over_budget() {
            self.record(
                TruncationKind::SourceBytes,
                self.limits.source_bytes,
                "source byte work cap reached; incomplete source was discarded",
            );
        }
    }

    /// Whether the overflow probe consumed a byte beyond the allowance.
    #[must_use]
    pub const fn source_over_budget(&self) -> bool {
        self.source_bytes > self.limits.source_bytes
    }

    /// Source-file open attempts and actual bytes read (including an overflow probe).
    #[must_use]
    pub const fn source_report(&self) -> (u64, u64) {
        (self.source_files as u64, self.source_bytes as u64)
    }

    /// Charges an occurrence before inspecting or filtering its record.
    /// # Errors
    /// On cancellation or deadline.
    pub fn occurrence(&mut self) -> Result<bool> {
        self.check()?;
        if self.occurrences >= self.limits.occurrences {
            self.record(
                TruncationKind::Occurrences,
                self.limits.occurrences,
                "occurrence work cap reached; reference evidence is partial",
            );
            return Ok(false);
        }
        self.occurrences = self.occurrences.saturating_add(1);
        Ok(true)
    }

    /// Number of source occurrence records examined.
    #[must_use]
    pub const fn occurrences_examined(&self) -> u64 {
        self.occurrences as u64
    }

    /// Verify one captured source field against shared request allowances.
    /// Source reading/hash validation remains the caller's responsibility.
    /// # Errors
    /// Cancellation/deadline or an invalid verifier request.
    pub fn verify_positions(
        &mut self,
        query: &crate::positional::PositionalQuery,
        text: &str,
    ) -> Result<crate::positional::Matches> {
        let limits = crate::positional::Limits {
            bytes: self
                .limits
                .positional_bytes
                .saturating_sub(self.positional_bytes),
            tokens: self
                .limits
                .positional_tokens
                .saturating_sub(self.positional_tokens),
        };
        let remaining = self
            .limits
            .positional_witnesses
            .saturating_sub(self.positional_witnesses);
        let matches = query.verify_all(text, limits, remaining, || self.check())?;
        self.positional_bytes = self.positional_bytes.saturating_add(matches.bytes_examined);
        self.positional_tokens = self
            .positional_tokens
            .saturating_add(matches.tokens_examined);
        self.positional_witnesses = self
            .positional_witnesses
            .saturating_add(matches.witnesses.len());
        if let Some(reason) = matches.stopped_by {
            let (kind, cap) = match reason {
                crate::positional::LimitKind::Bytes => (
                    TruncationKind::PositionalBytes,
                    self.limits.positional_bytes,
                ),
                crate::positional::LimitKind::Tokens => (
                    TruncationKind::PositionalTokens,
                    self.limits.positional_tokens,
                ),
                crate::positional::LimitKind::Witnesses => (
                    TruncationKind::PositionalWitnesses,
                    self.limits.positional_witnesses,
                ),
            };
            self.record(
                kind,
                cap,
                "positional verification allowance exhausted; witnesses are partial",
            );
        }
        Ok(matches)
    }

    /// Actual positional scan bytes, token comparisons, and retained witnesses.
    #[must_use]
    pub const fn positional_report(&self) -> (u64, u64, u64) {
        (
            self.positional_bytes as u64,
            self.positional_tokens as u64,
            self.positional_witnesses as u64,
        )
    }

    /// Charges one bounded context-window cost evaluation before reading its lines.
    /// # Errors
    /// On cancellation or deadline.
    pub fn context_window(&mut self) -> Result<bool> {
        self.check()?;
        if self.context_windows >= self.limits.context_windows {
            self.record(
                TruncationKind::ContextWindows,
                self.limits.context_windows,
                "context window work cap reached; additional source is partial",
            );
            return Ok(false);
        }
        self.context_windows = self.context_windows.saturating_add(1);
        Ok(true)
    }

    /// Number of context-window cost evaluations performed.
    #[must_use]
    pub const fn context_windows_examined(&self) -> u64 {
        self.context_windows as u64
    }

    /// Charges one distinct graph node; repeated visits do not consume capacity.
    /// # Errors
    /// On cancellation or deadline.
    pub fn node(&mut self, id: &NodeId) -> Result<bool> {
        self.check()?;
        if self.visited.contains(id) {
            return Ok(true);
        }
        if self.visited.len() >= self.limits.nodes {
            self.record(
                TruncationKind::GraphNodes,
                self.limits.nodes,
                "graph node work cap reached; counts and evidence are partial",
            );
            return Ok(false);
        }
        self.visited.insert(id.clone());
        Ok(true)
    }
    /// Charges an examined adjacency entry, even when its kind is filtered out.
    /// # Errors
    /// On cancellation or deadline.
    pub fn edge(&mut self) -> Result<bool> {
        self.check()?;
        if self.examined >= self.limits.edges {
            self.record(
                TruncationKind::GraphEdges,
                self.limits.edges,
                "graph edge work cap reached; counts and evidence are partial",
            );
            return Ok(false);
        }
        self.examined = self.examined.saturating_add(1);
        Ok(true)
    }
    /// Charges an unscored metadata record before inspecting or filtering it.
    /// # Errors
    /// On cancellation or deadline.
    pub fn metadata_entry(&mut self) -> Result<bool> {
        self.check()?;
        if self.metadata_entries >= self.limits.metadata_entries {
            self.record(
                TruncationKind::MetadataEntries,
                self.limits.metadata_entries,
                "metadata examination cap reached; retrieval is partial",
            );
            return Ok(false);
        }
        self.metadata_entries = self.metadata_entries.saturating_add(1);
        Ok(true)
    }

    /// Metadata records inspected across exact and path retrieval lanes.
    #[must_use]
    pub const fn metadata_entries_examined(&self) -> u64 {
        self.metadata_entries as u64
    }

    /// Charges one candidate admission before allocating its accumulator.
    /// # Errors
    /// On cancellation or deadline.
    pub fn candidate(&mut self) -> Result<bool> {
        self.check()?;
        if self.candidates >= self.limits.candidates {
            self.record(
                TruncationKind::Candidates,
                self.limits.candidates,
                "retrieval candidate lane cap reached; retrieval is partial",
            );
            return Ok(false);
        }
        self.candidates = self.candidates.saturating_add(1);
        Ok(true)
    }
    /// Charges a posting before filtering or scoring it.
    /// # Errors
    /// On cancellation or deadline.
    pub fn posting(&mut self) -> Result<bool> {
        self.check()?;
        if self.postings >= self.limits.postings {
            self.record(
                TruncationKind::Postings,
                self.limits.postings,
                "lexical posting cap reached; retrieval is partial",
            );
            return Ok(false);
        }
        self.postings = self.postings.saturating_add(1);
        Ok(true)
    }
    /// Charges one prefix dictionary entry before expanding its postings.
    /// # Errors
    /// On cancellation or deadline.
    pub fn dictionary_entry(&mut self) -> Result<bool> {
        self.check()?;
        if self.dictionary_entries >= self.limits.dictionary_entries {
            self.record(
                TruncationKind::DictionaryEntries,
                self.limits.dictionary_entries,
                "prefix dictionary expansion cap reached; retrieval is partial",
            );
            return Ok(false);
        }
        self.dictionary_entries = self.dictionary_entries.saturating_add(1);
        Ok(true)
    }
    /// Actual prefix dictionary entries expanded.
    #[must_use]
    pub const fn dictionary_entries_examined(&self) -> u64 {
        self.dictionary_entries as u64
    }

    /// Candidate admissions and examined postings.
    #[must_use]
    pub const fn lexical_report(&self) -> (u64, u64) {
        (self.candidates as u64, self.postings as u64)
    }

    /// Reserves a fraction of remaining lexical work for an independent retrieval lane.
    /// Counters must be merged with `absorb_lexical` before another lane runs.
    #[must_use]
    pub fn lexical_lane(&self, parts: usize) -> Self {
        let mut limits = self.limits.clone();
        limits.positional_bytes = limits
            .positional_bytes
            .saturating_sub(self.positional_bytes)
            .div_ceil(parts.max(1));
        limits.positional_tokens = limits
            .positional_tokens
            .saturating_sub(self.positional_tokens)
            .div_ceil(parts.max(1));
        limits.positional_witnesses = limits
            .positional_witnesses
            .saturating_sub(self.positional_witnesses)
            .div_ceil(parts.max(1));
        limits.metadata_entries = limits
            .metadata_entries
            .saturating_sub(self.metadata_entries);
        limits.candidates = limits
            .candidates
            .saturating_sub(self.candidates)
            .div_ceil(parts.max(1));
        limits.postings = limits
            .postings
            .saturating_sub(self.postings)
            .div_ceil(parts.max(1));
        limits.dictionary_entries = limits
            .dictionary_entries
            .saturating_sub(self.dictionary_entries)
            .div_ceil(parts.max(1));
        Self::new(limits)
    }

    /// Accounts for a completed lexical lane without charging graph work twice.
    pub fn absorb_lexical(&mut self, lane: Self) {
        self.positional_bytes = self.positional_bytes.saturating_add(lane.positional_bytes);
        self.positional_tokens = self
            .positional_tokens
            .saturating_add(lane.positional_tokens);
        self.positional_witnesses = self
            .positional_witnesses
            .saturating_add(lane.positional_witnesses);
        self.metadata_entries = self.metadata_entries.saturating_add(lane.metadata_entries);
        self.dictionary_entries = self
            .dictionary_entries
            .saturating_add(lane.dictionary_entries);
        self.candidates = self.candidates.saturating_add(lane.candidates);
        self.postings = self.postings.saturating_add(lane.postings);
        for mut item in lane.truncations {
            match item.kind {
                TruncationKind::MetadataEntries => item.cap = self.limits.metadata_entries as u64,
                TruncationKind::PositionalBytes => item.cap = self.limits.positional_bytes as u64,
                TruncationKind::PositionalTokens => item.cap = self.limits.positional_tokens as u64,
                TruncationKind::PositionalWitnesses => {
                    item.cap = self.limits.positional_witnesses as u64;
                }
                _ => {}
            }
            if !self.truncations.iter().any(|old| old.kind == item.kind) {
                self.truncations.push(item);
            }
        }
    }

    fn record(&mut self, kind: TruncationKind, cap: usize, message: &str) {
        if !self.truncations.iter().any(|item| item.kind == kind) {
            self.truncations
                .push(Truncation::new(kind, cap as u64, message));
        }
    }
    /// Observed graph work and fired limits.
    #[must_use]
    pub fn report(&self) -> (u64, u64, Vec<Truncation>) {
        (
            self.visited.len() as u64,
            self.examined as u64,
            self.truncations.clone(),
        )
    }
}

#[cfg(test)]
mod positional_tests {
    use super::*;
    use crate::positional::{LimitKind, PositionalQuery, Predicate};

    fn query() -> PositionalQuery {
        PositionalQuery::new("a", Predicate::Ordered { intervening: 0 }).unwrap()
    }

    #[test]
    fn witness_allowance_is_shared_across_fields_and_exact_fill_is_complete() {
        let mut budget = WorkBudget::new(WorkLimits {
            positional_witnesses: 2,
            ..WorkLimits::default()
        });
        assert!(budget.verify_positions(&query(), "a").unwrap().complete);
        let second = budget.verify_positions(&query(), "a a").unwrap();
        assert_eq!(second.witnesses.len(), 1);
        assert_eq!(second.stopped_by, Some(LimitKind::Witnesses));
        assert_eq!(budget.positional_report(), (4, 3, 2));
        assert_eq!(
            budget.report().2[0].kind,
            TruncationKind::PositionalWitnesses
        );
        assert_eq!(budget.report().2[0].cap, 2);
        assert!(budget.verify_positions(&query(), "b").unwrap().complete);
        let mut fresh = WorkBudget::new(WorkLimits {
            positional_witnesses: 2,
            ..WorkLimits::default()
        });
        assert!(fresh.verify_positions(&query(), "a a").unwrap().complete);
        assert!(fresh.report().2.is_empty());
    }

    #[test]
    fn positional_lane_work_is_reserved_and_merged_into_the_request() {
        let mut request = WorkBudget::new(WorkLimits {
            positional_witnesses: 2,
            ..WorkLimits::default()
        });
        assert!(request.verify_positions(&query(), "a").unwrap().complete);
        let mut lane = request.lexical_lane(2);
        assert_eq!(
            lane.verify_positions(&query(), "a a")
                .unwrap()
                .witnesses
                .len(),
            1
        );
        request.absorb_lexical(lane);
        assert_eq!(request.positional_report(), (4, 3, 2));
        assert_eq!(request.report().2[0].cap, 2);
        assert!(
            request
                .verify_positions(&query(), "a")
                .unwrap()
                .witnesses
                .is_empty()
        );
    }

    #[test]
    fn byte_and_token_exhaustion_report_the_actual_shared_allowance() {
        let mut bytes = WorkBudget::new(WorkLimits {
            positional_bytes: 2,
            ..WorkLimits::default()
        });
        assert!(bytes.verify_positions(&query(), "a").unwrap().complete);
        let stopped = bytes.verify_positions(&query(), "é").unwrap();
        assert_eq!(stopped.stopped_by, Some(LimitKind::Bytes));
        assert_eq!(bytes.positional_report(), (1, 1, 1));
        assert_eq!(bytes.report().2[0].kind, TruncationKind::PositionalBytes);
        let mut tokens = WorkBudget::new(WorkLimits {
            positional_tokens: 2,
            ..WorkLimits::default()
        });
        assert!(tokens.verify_positions(&query(), "a").unwrap().complete);
        let stopped = tokens.verify_positions(&query(), "b a").unwrap();
        assert_eq!(stopped.stopped_by, Some(LimitKind::Tokens));
        assert_eq!(tokens.positional_report(), (4, 2, 1));
        assert_eq!(tokens.report().2[0].kind, TruncationKind::PositionalTokens);
    }

    #[test]
    fn positional_ceiling_zero_and_cancellation_contracts_hold() {
        let mut zero = WorkBudget::new(WorkLimits {
            positional_bytes: 0,
            positional_tokens: 0,
            positional_witnesses: 0,
            ..WorkLimits::default()
        });
        assert!(zero.verify_positions(&query(), "").unwrap().complete);
        assert!(zero.report().2.is_empty());
        assert!(!zero.verify_positions(&query(), "a").unwrap().complete);
        let capped = WorkBudget::new(WorkLimits {
            positional_bytes: usize::MAX,
            positional_tokens: usize::MAX,
            positional_witnesses: usize::MAX,
            ..WorkLimits::default()
        });
        assert_eq!(capped.limits.positional_bytes, 1024 * 1024 * 1024);
        assert_eq!(capped.limits.positional_tokens, 10_000_000);
        assert_eq!(capped.limits.positional_witnesses, 10_000);
        let cancel = CancellationToken::default();
        cancel.cancel();
        let mut stopped = WorkBudget::new(WorkLimits {
            cancellation: Some(cancel),
            ..WorkLimits::default()
        });
        assert!(matches!(
            stopped.verify_positions(&query(), "a"),
            Err(Error::QueryCancelled)
        ));
        assert_eq!(stopped.positional_report(), (0, 0, 0));
    }
}
