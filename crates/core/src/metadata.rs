//! Generation-owned exact-name tables and native lexical retrieval.

use crate::{Result, lexical::LexicalIndex, work::WorkBudget};
use graph_search_types::{Node, NodeKind, Scored, query::GraphFilters};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Query filters compiled once and reused by all retrieval stages.
#[derive(Default)]
pub struct CompiledFilters {
    language: Option<graph_search_types::Language>,
    path: Option<globset::GlobSet>,
}
impl CompiledFilters {
    /// Compiles a request's path predicate.
    /// # Errors
    /// When the glob is invalid.
    pub fn new(filters: &GraphFilters) -> Result<Self> {
        Ok(Self {
            language: filters.lang,
            path: filters
                .path_glob
                .as_deref()
                .map(crate::files_search::compile_anchored_glob)
                .transpose()?,
        })
    }
    /// Whether matching needs a node's language, which only the metadata
    /// index knows.
    #[must_use]
    pub const fn needs_language(&self) -> bool {
        self.language.is_some()
    }

    /// Matches cached language metadata and a workspace-relative path.
    #[must_use]
    pub fn matches(&self, path: &str, language: Option<graph_search_types::Language>) -> bool {
        self.language.is_none_or(|wanted| language == Some(wanted))
            && self.path.as_ref().is_none_or(|glob| glob.is_match(path))
    }
}

/// Ranked metadata pool, with the candidate count before top-k selection.
pub struct MetadataCandidates {
    /// Best candidates in deterministic score/path/line/id order.
    pub hits: Vec<Scored<Node>>,
    /// Positive-score candidates observed before selection; may be partial if a work cap fired.
    pub matched: usize,
}

#[derive(Clone, Copy)]
struct RankedOrdinal {
    document: usize,
    score: f32,
}
impl PartialEq for RankedOrdinal {
    fn eq(&self, other: &Self) -> bool {
        self.document == other.document && self.score.to_bits() == other.score.to_bits()
    }
}
impl Eq for RankedOrdinal {}
impl PartialOrd for RankedOrdinal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RankedOrdinal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Worse scores and later ordinals rise to the max-heap root.
        // Ordinals already follow deterministic path/line/id order.
        other
            .score
            .total_cmp(&self.score)
            .then(self.document.cmp(&other.document))
    }
}

/// Immutable metadata prepared with a graph generation and shared by snapshots.
pub struct MetadataIndex {
    nodes: Vec<Node>,
    languages: Vec<Option<graph_search_types::Language>>,
    files: BTreeMap<String, Option<graph_search_types::Language>>,
    bare: BTreeMap<String, Vec<usize>>,
    qualified: BTreeMap<String, Vec<usize>>,
    folded: BTreeMap<String, Vec<usize>>,
    symbols: Vec<usize>,
    file_ordinals: Vec<usize>,
    /// Ranked-search postings, built on first ranked search: exact-name,
    /// prefix and path lookups never need them.
    lexical: std::sync::OnceLock<LexicalIndex>,
    identifiers: std::sync::OnceLock<LexicalIndex>,
}
impl Default for MetadataIndex {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
impl MetadataIndex {
    /// Builds exact tables and postings in deterministic path/line/id order.
    #[must_use]
    pub fn new(nodes: Vec<Node>) -> Self {
        Self::build(nodes, None)
    }

    /// Prepares a new generation, reanalyzing only changed metadata fields and
    /// sharing unchanged posting lists. The current generation is never mutated.
    #[must_use]
    pub fn updated(&self, nodes: Vec<Node>) -> Self {
        Self::build(nodes, Some(self))
    }

    #[allow(clippy::too_many_lines)] // one pass over ordered documents
    fn build(mut nodes: Vec<Node>, previous: Option<&Self>) -> Self {
        // Reexport members resolve module paths but are not competing search
        // results; excluding them keeps candidate ranking on real definitions.
        nodes.retain(|node| node.attribute("rust_reexport").is_none());
        nodes.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(
                    a.span
                        .map(|s| s.start_line)
                        .cmp(&b.span.map(|s| s.start_line)),
                )
                .then(a.id.cmp(&b.id))
        });
        let mut bare: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut qualified: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            if let Some(name) = &node.name {
                bare.entry(name.clone()).or_default().push(position);
            }
            if let Some(name) = &node.qualified_name
                && node.name.as_ref() != Some(name)
            {
                qualified.entry(name.clone()).or_default().push(position);
            }
        }
        let files: BTreeMap<_, _> = nodes
            .iter()
            .filter(|n| n.is_file())
            .map(|n| (n.path.clone(), n.language))
            .collect();
        let languages = nodes
            .iter()
            .map(|n| files.get(n.path.as_str()).copied().flatten())
            .collect();
        let symbols: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_file())
            .map(|(i, _)| i)
            .collect();
        let symbol_nodes: Vec<_> = symbols.iter().map(|&i| nodes[i].clone()).collect();
        let mut folded: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (document, node) in symbol_nodes.iter().enumerate() {
            for name in [node.name.as_ref(), node.qualified_name.as_ref()]
                .into_iter()
                .flatten()
            {
                let positions = folded.entry(name.to_lowercase()).or_default();
                if positions.last() != Some(&document) {
                    positions.push(document);
                }
            }
        }
        let (lexical, identifiers) = match previous {
            Some(previous)
                if previous.lexical.get().is_some() || previous.identifiers.get().is_some() =>
            {
                Self::updated_postings(previous, &symbol_nodes)
            }
            _ => (std::sync::OnceLock::new(), std::sync::OnceLock::new()),
        };
        let file_ordinals = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.is_file())
            .map(|(i, _)| i)
            .collect();
        Self {
            nodes,
            languages,
            files,
            bare,
            qualified,
            folded,
            symbols,
            file_ordinals,
            lexical,
            identifiers,
        }
    }

    /// Shares unchanged posting lists with an earlier generation's postings,
    /// for whichever of them that generation had built.
    fn updated_postings(
        previous: &Self,
        symbol_nodes: &[Node],
    ) -> (
        std::sync::OnceLock<LexicalIndex>,
        std::sync::OnceLock<LexicalIndex>,
    ) {
        let old: BTreeMap<_, _> = previous
            .symbols
            .iter()
            .enumerate()
            .map(|(ordinal, &i)| (&previous.nodes[i].id, (ordinal, &previous.nodes[i])))
            .collect();
        let mut claimed = BTreeSet::new();
        let reuse: Vec<_> = symbol_nodes
            .iter()
            .map(|node| {
                old.get(&node.id)
                    .filter(|(_, before)| {
                        node.path == before.path
                            && node.name == before.name
                            && node.qualified_name == before.qualified_name
                            && node.signature == before.signature
                    })
                    .filter(|(ordinal, _)| claimed.insert(*ordinal))
                    .map(|(ordinal, _)| *ordinal)
            })
            .collect();
        let updated = |postings: &std::sync::OnceLock<LexicalIndex>| {
            postings
                .get()
                .map_or_else(std::sync::OnceLock::new, |postings| {
                    std::sync::OnceLock::from(postings.updated(symbol_nodes, &reuse))
                })
        };
        (updated(&previous.lexical), updated(&previous.identifiers))
    }

    fn symbol_nodes(&self) -> Vec<Node> {
        self.symbols
            .iter()
            .map(|&i| self.nodes[i].clone())
            .collect()
    }

    fn lexical(&self) -> &LexicalIndex {
        self.lexical
            .get_or_init(|| LexicalIndex::new(&self.symbol_nodes()))
    }

    fn identifiers(&self) -> &LexicalIndex {
        self.identifiers
            .get_or_init(|| LexicalIndex::with_fields(&self.symbol_nodes(), true, true))
    }

    /// Cached source-file language without graph property reads.
    #[must_use]
    pub fn language(&self, path: &str) -> Option<graph_search_types::Language> {
        self.files.get(path).copied().flatten()
    }

    fn matching<'a>(&'a self, name: &'a str) -> impl Iterator<Item = (usize, f32)> + 'a {
        self.bare
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| (i, 1.0))
            .chain(
                self.qualified
                    .get(name)
                    .into_iter()
                    .flatten()
                    .map(|&i| (i, 0.9)),
            )
    }

    /// Exact bare names precede qualified names. Stops after `k` admitted nodes.
    #[must_use]
    pub fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Vec<Scored<Node>> {
        self.matching(name)
            .filter(|&(i, _)| kinds.is_empty() || kinds.contains(&self.nodes[i].kind))
            .take(k)
            .map(|(i, score)| Scored::new(self.nodes[i].clone(), score))
            .collect()
    }

    /// Exact lookup with pre-admission filters and request work accounting.
    /// # Errors
    /// On cancellation or deadline.
    pub fn find_filtered(
        &self,
        name: &str,
        kinds: &[NodeKind],
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
    ) -> Result<Vec<Scored<Node>>> {
        let mut out = Vec::new();
        for (i, score) in self.matching(name) {
            if !budget.metadata_entry()? {
                break;
            }
            if (!kinds.is_empty() && !kinds.contains(&self.nodes[i].kind))
                || !filters.matches(&self.nodes[i].path, self.languages[i])
            {
                continue;
            }
            if !budget.candidate()? {
                break;
            }
            out.push(Scored::new(self.nodes[i].clone(), score));
        }
        Ok(out)
    }

    /// Case-sensitive whole-name prefixes, bare dictionary before qualified.
    /// Names are not split, normalized, globbed or fuzzily corrected. Duplicate
    /// entities are admitted once; each examined dictionary/posting entry is charged.
    /// # Errors
    /// On an empty prefix, cancellation or deadline.
    pub fn find_prefix(
        &self,
        prefix: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
    ) -> Result<Vec<Scored<Node>>> {
        if prefix.is_empty() {
            return Err(crate::Error::InvalidQuery(
                "name prefix must not be empty".into(),
            ));
        }
        let mut admitted = BTreeSet::new();
        'dictionaries: for dictionary in [&self.bare, &self.qualified] {
            for (_, positions) in dictionary
                .range(prefix.to_owned()..)
                .take_while(|(name, _)| name.starts_with(prefix))
            {
                if !budget.dictionary_entry()? {
                    break 'dictionaries;
                }
                for &ordinal in positions {
                    if !budget.posting()? {
                        break 'dictionaries;
                    }
                    let node = &self.nodes[ordinal];
                    if node.is_file()
                        || admitted.contains(&ordinal)
                        || !filters.matches(&node.path, self.languages[ordinal])
                    {
                        continue;
                    }
                    if !budget.candidate()? {
                        break 'dictionaries;
                    }
                    admitted.insert(ordinal);
                }
            }
        }
        Ok(admitted
            .into_iter()
            .map(|i| Scored::new(self.nodes[i].clone(), 1.0))
            .collect())
    }

    /// Cached path navigation with language/path filters before candidate admission.
    /// # Errors
    /// On an invalid glob, cancellation, or deadline.
    pub fn find_paths(
        &self,
        glob: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
    ) -> Result<Vec<Scored<Node>>> {
        let glob = crate::files_search::compile_anchored_glob(glob)?;
        let mut hits = Vec::new();
        for &ordinal in &self.file_ordinals {
            if !budget.metadata_entry()? {
                break;
            }
            let node = &self.nodes[ordinal];
            if glob.is_match(&node.path) && filters.matches(&node.path, node.language) {
                if !budget.candidate()? {
                    break;
                }
                hits.push(Scored::new(node.clone(), 2.0));
            }
        }
        Ok(hits)
    }

    /// Retrieves exact-name and native postings candidates with one compiled filter.
    /// Existing ranker behavior is retained: exact 2.0, otherwise BM25/(1+BM25).
    /// # Errors
    /// On cancellation or deadline.
    pub fn search(
        &self,
        terms: &[String],
        exact: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
    ) -> Result<Vec<Scored<Node>>> {
        Ok(self
            .search_top(terms, exact, filters, budget, usize::MAX)?
            .hits)
    }

    /// Scores the admitted posting union, retaining only the best `limit` ordinals.
    /// Uses O(C log k) selection, clones nodes only for winners, and does not prune scores.
    /// # Errors
    /// On cancellation or deadline.
    pub fn search_top(
        &self,
        terms: &[String],
        exact: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
        limit: usize,
    ) -> Result<MetadataCandidates> {
        self.search_top_matching(terms, exact, filters, budget, limit, 1)
    }

    /// Ranked metadata with an explicit minimum distinct-term requirement.
    /// # Errors
    /// On cancellation or deadline.
    #[allow(clippy::too_many_arguments)]
    pub fn search_top_matching(
        &self,
        terms: &[String],
        exact: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
        limit: usize,
        minimum: usize,
    ) -> Result<MetadataCandidates> {
        self.search_top_with_analysis(
            terms,
            exact,
            filters,
            budget,
            limit,
            minimum,
            graph_search_types::AnalysisMode::Split,
        )
    }

    /// Uses the selected native field representation with the same ranking/work contract.
    /// # Errors
    /// On cancellation or deadline.
    #[allow(clippy::too_many_arguments)]
    pub fn search_top_with_analysis(
        &self,
        terms: &[String],
        exact: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
        limit: usize,
        minimum: usize,
        analysis: graph_search_types::AnalysisMode,
    ) -> Result<MetadataCandidates> {
        self.search_top_with_policy(
            terms,
            exact,
            filters,
            budget,
            limit,
            minimum,
            analysis,
            graph_search_types::FieldNormalization::Combined,
        )
    }

    /// Explicit metadata normalization over the same generation-owned postings.
    /// # Errors
    /// On cancellation, deadline, or invalid query limits.
    #[allow(clippy::too_many_arguments)]
    pub fn search_top_with_policy(
        &self,
        terms: &[String],
        exact: &str,
        filters: &CompiledFilters,
        budget: &mut WorkBudget,
        limit: usize,
        minimum: usize,
        analysis: graph_search_types::AnalysisMode,
        normalization: graph_search_types::FieldNormalization,
    ) -> Result<MetadataCandidates> {
        let lexical = match analysis {
            graph_search_types::AnalysisMode::Split => self.lexical(),
            graph_search_types::AnalysisMode::Identifiers => self.identifiers(),
        };

        let accept = |document: usize| {
            let i = self.symbols[document];
            filters.matches(&self.nodes[i].path, self.languages[i])
        };
        let mut scores = BTreeMap::new();
        let mut exact_documents = BTreeSet::new();
        for &document in self.folded.get(exact).into_iter().flatten() {
            if !budget.metadata_entry()? {
                break;
            }
            if !accept(document) {
                continue;
            }
            if !budget.candidate()? {
                break;
            }
            scores.insert(document, 0.0);
            exact_documents.insert(document);
        }
        let mut coverage = BTreeMap::new();
        lexical.accumulate_ranked(
            terms,
            &mut scores,
            accept,
            budget,
            &mut coverage,
            minimum,
            normalization,
        )?;
        let mut best = BinaryHeap::new();
        let mut matched = 0usize;
        for (document, bm25) in scores {
            budget.check()?;
            if minimum > 1
                && !exact_documents.contains(&document)
                && coverage.get(&document).copied().unwrap_or(0) < minimum
            {
                continue;
            }
            let score = if exact_documents.contains(&document) {
                2.0
            } else {
                bm25 / (1.0 + bm25)
            };
            if score <= 0.0 {
                continue;
            }
            matched = matched.saturating_add(1);
            let candidate = RankedOrdinal { document, score };
            if best.len() < limit {
                best.push(candidate);
            } else if best.peek().is_some_and(|worst| candidate < *worst) {
                best.pop();
                best.push(candidate);
            }
        }
        let hits = best
            .into_sorted_vec()
            .into_iter()
            .map(|candidate| {
                Scored::new(
                    self.nodes[self.symbols[candidate.document]].clone(),
                    candidate.score,
                )
            })
            .collect();
        Ok(MetadataCandidates { hits, matched })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::WorkLimits;
    use graph_search_types::{Language, NodeId};

    fn same_generation(actual: &MetadataIndex, expected: &MetadataIndex) {
        use graph_search_types::{AnalysisMode, FieldNormalization};
        assert_eq!(actual.bare, expected.bare);
        assert_eq!(actual.qualified, expected.qualified);
        assert_eq!(actual.folded, expected.folded);
        assert_eq!(actual.languages, expected.languages);
        assert_eq!(actual.files, expected.files);
        for query in [
            "common",
            "common rare",
            "HTTPServer",
            "replacement",
            "scope",
            "absent",
            "is",
        ] {
            for analysis in [AnalysisMode::Split, AnalysisMode::Identifiers] {
                for normalization in [FieldNormalization::Combined, FieldNormalization::Bm25f] {
                    for minimum in [1, 2] {
                        let terms = crate::lexical::query_terms(query);
                        let run = |index: &MetadataIndex| {
                            let mut work = WorkBudget::new(WorkLimits::default());
                            let candidates = index
                                .search_top_with_policy(
                                    &terms,
                                    query,
                                    &CompiledFilters::default(),
                                    &mut work,
                                    7,
                                    minimum,
                                    analysis,
                                    normalization,
                                )
                                .unwrap();
                            assert!(work.report().2.is_empty());
                            (
                                candidates.matched,
                                candidates
                                    .hits
                                    .into_iter()
                                    .map(|hit| {
                                        (
                                            hit.item.id,
                                            hit.item.path,
                                            hit.item.name,
                                            hit.score.to_bits(),
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        };
                        assert_eq!(
                            run(actual),
                            run(expected),
                            "{query}/{analysis:?}/{normalization:?}/{minimum}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn metadata_deltas_match_rebuilds_through_mutations_and_duplicate_ids() {
        let mut nodes: Vec<_> = (0..24)
            .map(|i| {
                let mut node = symbol(
                    &format!("src/{}.rs", i % 6),
                    &format!("common{i}"),
                    &format!("scope::{i}"),
                );
                node.signature = Some("fn HTTPServer(common: Rare)".into());
                node
            })
            .collect();
        nodes.extend(
            (0..6).map(|i| Node::file(&format!("src/{i}.rs"), Language::Rust, 0, 1, "hash", 1)),
        );
        let mut current = MetadataIndex::new(nodes.clone());
        for step in 0..48 {
            let before = nodes.clone();
            let position = step % 20;
            match step % 6 {
                0 => {
                    nodes[position].signature =
                        Some(format!("fn replacement(common: Value{step})"));
                }
                1 => {
                    nodes[position].name = Some(format!("replacement{step}"));
                    nodes[position].qualified_name = Some(format!("scope::replacement{step}"));
                }
                2 => nodes.push(symbol(
                    &format!("early/{step}.rs"),
                    "common",
                    "scope::common",
                )),
                3 => {
                    nodes.pop();
                }
                4 => nodes[position].span = Some(graph_search_types::Span::new(100, 101, 1, 3)),
                _ => nodes.push(nodes[position].clone()), // Public construction need not deduplicate IDs.
            }
            let next = current.updated(nodes.clone());
            same_generation(&next, &MetadataIndex::new(nodes.clone()));
            same_generation(&current, &MetadataIndex::new(before));
            current = next;
        }
        same_generation(&current.updated(Vec::new()), &MetadataIndex::default());
        same_generation(
            &MetadataIndex::default().updated(nodes.clone()),
            &MetadataIndex::new(nodes),
        );
    }

    fn symbol(path: &str, name: &str, qualified: &str) -> Node {
        Node {
            id: NodeId::symbol(path, NodeKind::Function, qualified, None),
            path: path.into(),
            kind: NodeKind::Function,
            name: Some(name.into()),
            qualified_name: Some(qualified.into()),
            ..Node::default()
        }
    }
    #[test]
    fn rejected_metadata_records_consume_work_before_candidate_admission() {
        let paths: Vec<_> = (0..8).map(|i| format!("{i}.rs")).collect();
        let index = MetadataIndex::new(
            paths
                .iter()
                .map(|p| symbol(p, "shared", "shared"))
                .collect(),
        );
        let files = MetadataIndex::new(
            paths
                .iter()
                .map(|p| Node::file(p, Language::Rust, 0, 1, "hash", 1))
                .collect(),
        );
        let filters = CompiledFilters::new(&GraphFilters {
            path_glob: Some("7.rs".into()),
            ..Default::default()
        })
        .unwrap();
        for cap in [0, 3, 7, 8] {
            for route in 0..3 {
                let mut work = WorkBudget::new(WorkLimits {
                    metadata_entries: cap,
                    postings: 0,
                    ..Default::default()
                });
                let hits = match route {
                    0 => index
                        .find_filtered("shared", &[], &filters, &mut work)
                        .unwrap(),
                    1 => files.find_paths("*.rs", &filters, &mut work).unwrap(),
                    _ => {
                        index
                            .search_top(&[], "shared", &filters, &mut work, 10)
                            .unwrap()
                            .hits
                    }
                };
                assert_eq!(hits.len(), usize::from(cap == 8));
                assert_eq!(work.metadata_entries_examined(), cap as u64);
                assert_eq!(work.lexical_report(), (u64::from(cap == 8), 0));
                assert_eq!(
                    work.report()
                        .2
                        .iter()
                        .any(|t| t.kind == graph_search_types::TruncationKind::MetadataEntries),
                    cap < 8
                );
            }
        }
        let mut work = WorkBudget::new(WorkLimits {
            metadata_entries: 8,
            ..Default::default()
        });
        let hits = index
            .find_filtered("shared", &[], &CompiledFilters::default(), &mut work)
            .unwrap();
        assert_eq!(hits.len(), 8);
        assert!(
            work.report().2.is_empty(),
            "duplicate qualified names require no hidden second scan"
        );
    }

    #[test]
    fn metadata_allowance_is_shared_across_lexical_lanes() {
        let mut work = WorkBudget::new(WorkLimits {
            metadata_entries: 5,
            ..Default::default()
        });
        assert!(work.metadata_entry().unwrap());
        let mut first = work.lexical_lane(2);
        for _ in 0..3 {
            assert!(first.metadata_entry().unwrap());
        }
        work.absorb_lexical(first);
        assert_eq!(work.metadata_entries_examined(), 4);
        let mut second = work.lexical_lane(2);
        assert!(second.metadata_entry().unwrap());
        assert!(!second.metadata_entry().unwrap());
        work.absorb_lexical(second);
        assert_eq!(work.metadata_entries_examined(), 5);
        assert!(!work.metadata_entry().unwrap());
        assert_eq!(work.report().2.len(), 1);
        assert_eq!(work.report().2[0].cap, 5);
    }

    #[test]
    fn conjunction_does_not_spend_candidate_admissions_on_partial_matches() {
        let index = MetadataIndex::new(
            (0..1000)
                .map(|i| {
                    symbol(
                        &format!("{i:04}.rs"),
                        if i == 999 { "commonRare" } else { "common" },
                        "Scope",
                    )
                })
                .collect(),
        );
        let mut budget = WorkBudget::new(WorkLimits {
            candidates: 1,
            postings: 64,
            ..WorkLimits::default()
        });
        let result = index
            .search_top_matching(
                &["common".into(), "rare".into()],
                "absent",
                &CompiledFilters::default(),
                &mut budget,
                10,
                2,
            )
            .unwrap();
        assert_eq!(result.matched, 1);
        assert_eq!(result.hits[0].item.path, "0999.rs");
        assert_eq!(budget.lexical_report().0, 1);
        assert!(budget.lexical_report().1 < 64);
        assert!(budget.report().2.is_empty());
    }

    #[test]
    fn prefix_dictionary_matches_exhaustive_whole_names() {
        let names = [
            "Cache",
            "CacheHTTP2",
            "cache",
            "cache_key",
            "école",
            "e\u{301}cole",
        ];
        let nodes: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, name)| symbol(&format!("{i}.rs"), name, &format!("Scope::{name}")))
            .collect();
        let index = MetadataIndex::new(nodes.clone());
        for prefix in [
            "C",
            "CacheHTTP",
            "cache_",
            "Scope::C",
            "é",
            "e\u{301}",
            "HTTP",
            "*",
            "missing",
        ] {
            let mut budget = WorkBudget::new(WorkLimits::default());
            let hits = index
                .find_prefix(prefix, &CompiledFilters::default(), &mut budget)
                .unwrap();
            let expected: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    n.name.as_deref().is_some_and(|n| n.starts_with(prefix))
                        || n.qualified_name
                            .as_deref()
                            .is_some_and(|n| n.starts_with(prefix))
                })
                .map(|n| n.id.clone())
                .collect();
            assert_eq!(
                hits.into_iter().map(|h| h.item.id).collect::<Vec<_>>(),
                expected
            );
            assert!(budget.report().2.is_empty());
        }
    }

    #[test]
    fn prefix_expansion_postings_and_admissions_have_independent_caps() {
        let index = MetadataIndex::new(vec![
            symbol("a.rs", "cacheA", "Scope::cacheA"),
            symbol("b.rs", "cacheB", "Scope::cacheB"),
            symbol("c.rs", "cacheC", "Scope::cacheC"),
        ]);
        let filters = CompiledFilters::new(&GraphFilters {
            path_glob: Some("c.rs".into()),
            ..GraphFilters::default()
        })
        .unwrap();
        let mut budget = WorkBudget::new(WorkLimits {
            candidates: 1,
            ..WorkLimits::default()
        });
        let hits = index.find_prefix("cache", &filters, &mut budget).unwrap();
        assert_eq!(hits[0].item.path, "c.rs");
        assert_eq!(budget.lexical_report(), (1, 3));
        assert_eq!(budget.dictionary_entries_examined(), 3);
        assert!(budget.report().2.is_empty());
        for (limits, expected) in [
            (
                WorkLimits {
                    dictionary_entries: 0,
                    ..WorkLimits::default()
                },
                graph_search_types::TruncationKind::DictionaryEntries,
            ),
            (
                WorkLimits {
                    postings: 0,
                    ..WorkLimits::default()
                },
                graph_search_types::TruncationKind::Postings,
            ),
            (
                WorkLimits {
                    candidates: 0,
                    ..WorkLimits::default()
                },
                graph_search_types::TruncationKind::Candidates,
            ),
        ] {
            let mut budget = WorkBudget::new(limits);
            assert!(
                index
                    .find_prefix("cache", &CompiledFilters::default(), &mut budget)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(budget.report().2[0].kind, expected);
        }
        let mut budget = WorkBudget::new(WorkLimits {
            dictionary_entries: 3,
            ..WorkLimits::default()
        });
        assert_eq!(
            index
                .find_prefix("cache", &CompiledFilters::default(), &mut budget)
                .unwrap()
                .len(),
            3
        );
        assert!(
            budget.report().2.is_empty(),
            "exactly exhausted is not omitted work"
        );
    }

    #[test]
    fn bare_and_qualified_names_are_cached_with_stable_precedence() {
        let index = MetadataIndex::new(vec![
            symbol("a.rs", "other", "target"),
            symbol("z.rs", "target", "scope::target"),
        ]);
        let hits = index.find_by_name("target", &[], 2);
        assert_eq!(hits[0].item.path, "z.rs");
        assert_eq!(hits[1].item.path, "a.rs");
        assert_eq!(
            index.find_by_name("scope::target", &[], 1)[0].item.path,
            "z.rs"
        );
        assert!(index.find_by_name("TARGET", &[], 2).is_empty());
    }
    #[test]
    fn filters_precede_candidate_admission_and_exact_survives_posting_exhaustion() {
        let mut nodes: Vec<_> = (0..100)
            .map(|i| symbol(&format!("a{i}.rs"), "common", "common"))
            .collect();
        nodes.push(symbol("z.rs", "common", "common"));
        nodes.push(Node::file("z.rs", Language::Rust, 0, 0, "hash", 4));
        let index = MetadataIndex::new(nodes);
        let filters = CompiledFilters::new(&GraphFilters {
            lang: Some(Language::Rust),
            path_glob: Some("z.rs".into()),
        })
        .unwrap();
        let mut budget = WorkBudget::new(WorkLimits {
            candidates: 1,
            postings: 0,
            ..WorkLimits::default()
        });
        let hits = index
            .search(
                &crate::lexical::query_terms("common"),
                "common",
                &filters,
                &mut budget,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.path, "z.rs");
        assert_eq!(hits[0].score.to_bits(), 2.0f32.to_bits());
        assert_eq!(budget.lexical_report(), (1, 0));
        assert_eq!(budget.report().2.len(), 1);
    }
    #[test]
    fn bounded_heap_matches_full_sort_across_ties_and_cutoffs() {
        let nodes: Vec<_> = (0..257)
            .map(|i| {
                let name = if i % 3 == 0 { "common" } else { "commonTopic" };
                let mut node = symbol(
                    &format!("src/file{i}.rs"),
                    name,
                    &format!("scope{i}::{name}"),
                );
                node.signature = Some("common ".repeat(i % 7));
                node
            })
            .collect();
        let index = MetadataIndex::new(nodes);
        for query in ["common", "topic", "absent", "common topic"] {
            let terms = crate::lexical::query_terms(query);
            let exact = crate::lexical::exact_query(query);
            let mut expected: Vec<_> = index
                .symbols
                .iter()
                .enumerate()
                .filter_map(|(document, &i)| {
                    let node = &index.nodes[i];
                    let raw = index.lexical().score(document, &terms);
                    let is_exact = [node.name.as_ref(), node.qualified_name.as_ref()]
                        .into_iter()
                        .flatten()
                        .any(|name| name.to_lowercase() == exact);
                    let score = if is_exact { 2.0 } else { raw / (1.0 + raw) };
                    (score > 0.0).then_some((node, score))
                })
                .collect();
            expected.sort_by(|(a, sa), (b, sb)| {
                sb.total_cmp(sa)
                    .then(a.path.cmp(&b.path))
                    .then(
                        a.span
                            .map(|s| s.start_line)
                            .cmp(&b.span.map(|s| s.start_line)),
                    )
                    .then(a.id.cmp(&b.id))
            });
            for k in [0, 1, 7, 64, 500] {
                let mut budget = WorkBudget::new(WorkLimits::default());
                let selected = index
                    .search_top(&terms, &exact, &CompiledFilters::default(), &mut budget, k)
                    .unwrap();
                assert_eq!(selected.matched, expected.len());
                let actual: Vec<_> = selected
                    .hits
                    .iter()
                    .map(|hit| (&hit.item.id, hit.score.to_bits()))
                    .collect();
                let expected: Vec<_> = expected
                    .iter()
                    .take(k)
                    .map(|(node, score)| (&node.id, score.to_bits()))
                    .collect();
                assert_eq!(actual, expected, "{query}/{k}");
                assert!(budget.report().2.is_empty());
            }
        }
    }
}
