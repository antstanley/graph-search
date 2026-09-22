//! Generation-owned source-region postings, independent of graph entities.
#![allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects)]

use crate::{Result, metadata::CompiledFilters, work::WorkBudget};
use graph_search_types::{Language, source::SourceFileUnits};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// `str::to_lowercase`, borrowing when that would be the identity (ASCII with
/// no uppercase letters); non-ASCII always takes the full Unicode mapping.
fn lowercase(spelling: &str) -> Cow<'_, str> {
    if spelling.is_ascii() && !spelling.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Borrowed(spelling)
    } else {
        Cow::Owned(spelling.to_lowercase())
    }
}

/// Maximum scored regions retained for one owner before byte-budget assembly.
pub const MAX_REGIONS_PER_OWNER: usize = 4;

#[derive(Default)]
struct Document {
    file: usize,
    entity: usize,
    unit: usize,
    length: usize,
    start_line: u32,
    end_line: u32,
}
#[derive(Clone, Copy)]
struct Posting {
    document: usize,
    frequency: usize,
    line: u32,
}

struct ScoredRegion {
    ordinal: usize,
    score: f32,
    line: u32,
    terms: u128,
}

/// One ranked region; coordinates refer to the source facts used to build the index.
#[derive(Clone, Debug)]
pub struct BodyHit {
    /// Workspace-relative path.
    pub path: String,
    /// Region ordinal within that file's source facts.
    pub unit: usize,
    /// Additional scored regions for this owner, selected after owner ranking.
    pub complementary_units: Vec<usize>,
    /// Matching regions not retained by the per-owner context bound.
    pub omitted_regions: usize,
    /// Earliest matching line; the assembler may select a denser evidence line.
    pub line: u32,
    /// Positive BM25 score within the body lane, not a metadata score.
    pub score: f32,
}

/// Result of checking a candidate against an explicit source predicate.
pub enum RegionDecision {
    /// Proven match, anchored at this original source line.
    Accept(u32),
    /// Proven nonmatch (or unavailable/unverified source, reported by the caller).
    Reject,
    /// Stop verification; unexamined regions cannot become hits.
    Stop,
}

/// Predicate-filtered candidates. Work-budget truncations remain in `WorkBudget`.
pub struct VerifiedBodyHits {
    /// Ranked owners, with only verified complementary regions.
    pub hits: Vec<BodyHit>,
    /// Posting candidates that met the requested term coverage before verification.
    pub candidate_regions: usize,
    /// Regions that passed verification before owner grouping/top-k.
    pub matched_regions: usize,
    /// False when the verifier explicitly stopped; not a claim about postings exhaustion.
    pub verification_complete: bool,
}

type Scores = BTreeMap<usize, (f32, u32, u128)>;

/// Immutable native posting lists prepared when a generation is published/opened.
#[derive(Default)]
pub struct BodyIndex {
    identifiers: Option<Box<Self>>,
    files: Vec<String>,
    documents: Vec<Document>,
    /// Exact-term lookup only (never ordered iteration), so a hash map.
    postings: HashMap<String, Vec<Posting>>,
    average_length: f32,
}

impl BodyIndex {
    /// Builds postings without retaining another copy of source text or term maps.
    #[must_use]
    pub fn new(files: &BTreeMap<String, SourceFileUnits>) -> Self {
        let mut index = Self::build(files, false);
        index.identifiers = Some(Box::new(Self::build(files, true)));
        index
    }

    fn build(files: &BTreeMap<String, SourceFileUnits>, whole: bool) -> Self {
        let mut index = Self::default();
        let mut total = 0usize;
        // Entities are (file, owner) pairs numbered in first-seen order. Files are
        // visited once each, so a per-file owner map with a global counter yields
        // the same numbering without cloning the path for every region.
        let mut entity_count = 0usize;
        for (path, source) in files {
            let file = index.files.len();
            index.files.push(path.clone());
            let mut entities = BTreeMap::new();
            for (unit, region) in source.units.iter().enumerate() {
                // Per lowercased identifier: total occurrences and earliest line.
                // Equivalent to merging and sorting the spellings' line lists,
                // without allocating them: a posting keeps only these two values.
                let mut identifiers: BTreeMap<Cow<'_, str>, (usize, Option<u32>)> = BTreeMap::new();
                if whole {
                    for (spelling, lines) in &region.identifiers {
                        let entry = identifiers.entry(lowercase(spelling)).or_insert((0, None));
                        entry.0 = entry.0.saturating_add(lines.len());
                        if let Some(&low) = lines.iter().min() {
                            entry.1 = Some(entry.1.map_or(low, |line| line.min(low)));
                        }
                    }
                }
                let length = region
                    .terms
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
                    .max(identifiers.values().map(|(count, _)| *count).sum());
                if length == 0 {
                    continue;
                }
                let document = index.documents.len();
                let owner = region
                    .documentation
                    .as_ref()
                    .and_then(|doc| doc.documented_symbol.as_ref())
                    .or(region.owner.as_ref());
                let entity = *entities.entry(owner).or_insert_with(|| {
                    let next = entity_count;
                    entity_count = entity_count.saturating_add(1);
                    next
                });
                index.documents.push(Document {
                    file,
                    entity,
                    unit,
                    length,
                    start_line: region.span.start_line,
                    end_line: region.span.end_line,
                });
                total = total.saturating_add(length);
                // Merge-join of two maps in the same byte order. A merged identifier
                // replaces a term only when it occurs more often.
                let mut merged = identifiers.iter().peekable();
                for (term, lines) in &region.terms {
                    while let Some((spelling, &(count, first))) =
                        merged.next_if(|(spelling, _)| spelling.as_ref() < term.as_str())
                    {
                        index.add_posting(spelling, document, count, first);
                    }
                    let (frequency, first) =
                        match merged.next_if(|(spelling, _)| spelling.as_ref() == term.as_str()) {
                            Some((_, &(count, first))) if count > lines.len() => (count, first),
                            _ => (lines.len(), lines.first().copied()),
                        };
                    index.add_posting(term, document, frequency, first);
                }
                for (spelling, &(count, first)) in merged {
                    index.add_posting(spelling, document, count, first);
                }
            }
        }
        index.average_length = (total as f32 / index.documents.len().max(1) as f32).max(1.0);
        index
    }

    /// Appends one document's posting; clones the term only for a new list.
    fn add_posting(&mut self, term: &str, document: usize, frequency: usize, first: Option<u32>) {
        let Some(line) = first else {
            return;
        };
        let posting = Posting {
            document,
            frequency,
            line,
        };
        if let Some(list) = self.postings.get_mut(term) {
            list.push(posting);
        } else {
            // Capacity 4 is what the first `push` onto an empty Vec allocates. A
            // capacity-1 start reallocates on the second posting; that churn
            // raised peak RSS of a full index by ~200 MB at equal heap size.
            let mut list = Vec::with_capacity(4);
            list.push(posting);
            self.postings.insert(term.to_owned(), list);
        }
    }

    fn idf(&self, frequency: usize) -> f32 {
        let n = self.documents.len() as f32;
        let df = frequency as f32;
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    fn contribution(&self, posting: Posting, idf: f32) -> f32 {
        let tf = posting.frequency as f32;
        let norm = 1.2
            * (0.25 + 0.75 * self.documents[posting.document].length as f32 / self.average_length);
        idf * tf * 2.2 / (tf + norm)
    }

    /// Necessary whole-term filter at file scope, without owner/region top-k.
    /// Terms may occur in different storage windows. Candidates still require
    /// original-source positional verification; split aliases may overselect.
    /// Missing/truncated/stale source facts require the caller's coverage policy.
    /// # Errors
    /// Cancellation/deadline or more than 128 distinct query terms.
    pub fn file_candidates(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
    ) -> Result<Vec<String>> {
        budget.check()?;
        let index = self.identifiers.as_deref().unwrap_or(self);
        let distinct: BTreeSet<_> = terms.iter().collect();
        if distinct.len() > 128 {
            return Err(crate::Error::InvalidQuery(
                "file candidate filter supports at most 128 distinct terms".into(),
            ));
        }
        if distinct.is_empty() {
            return Ok(Vec::new());
        }
        let mut lists = Vec::new();
        for term in &distinct {
            let Some(postings) = index.postings.get(*term) else {
                return Ok(Vec::new());
            };
            lists.push(postings);
        }
        lists.sort_by_key(|postings| postings.len());
        let mut files: BTreeMap<usize, u128> = BTreeMap::new();
        'terms: for (term, postings) in lists.into_iter().enumerate() {
            for posting in postings {
                if !budget.posting()? {
                    break 'terms;
                }
                let file = index.documents[posting.document].file;
                let path = &index.files[file];
                if excluded.contains(path) || !filters.matches(path, language(path)) {
                    continue;
                }
                if !files.contains_key(&file) && !budget.candidate()? {
                    continue;
                }
                *files.entry(file).or_default() |= 1u128 << term;
            }
        }
        Ok(files
            .into_iter()
            .filter(|(_, mask)| mask.count_ones() as usize == distinct.len())
            .map(|(file, _)| index.files[file].clone())
            .collect())
    }

    /// Retrieves the union of all query terms. Filters precede accumulator admission.
    /// `excluded` masks files replaced by the request's live overlay.
    /// # Errors
    /// On cancellation, deadline, or more than 128 distinct query terms.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
        k: usize,
    ) -> Result<(Vec<BodyHit>, usize)> {
        self.search_matching(terms, filters, language, excluded, budget, k, 1)
    }

    /// Selects split-only or whole-identifier source facts without changing query budgets.
    /// # Errors
    /// On cancellation, deadline, or more than 128 distinct query terms.
    #[allow(clippy::too_many_arguments)]
    pub fn search_with_analysis(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
        k: usize,
        minimum: usize,
        analysis: graph_search_types::AnalysisMode,
    ) -> Result<(Vec<BodyHit>, usize)> {
        let index = if analysis == graph_search_types::AnalysisMode::Identifiers {
            self.identifiers.as_deref().unwrap_or(self)
        } else {
            self
        };
        index.search_matching(terms, filters, language, excluded, budget, k, minimum)
    }

    /// Source-region retrieval with an explicit minimum distinct-term requirement.
    /// # Errors
    /// On cancellation, deadline, or more than 128 distinct query terms.
    #[allow(clippy::too_many_arguments)]
    pub fn search_matching(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
        k: usize,
        minimum: usize,
    ) -> Result<(Vec<BodyHit>, usize)> {
        let scores = self.collect_scores(terms, filters, language, excluded, budget, minimum)?;
        Ok(self.rank(scores, k))
    }

    /// Verify every admitted region before owner deduplication or top-k. The
    /// callback must verify the intended source version and charge its own work.
    /// A stop retains only already verified matches and is explicitly reported.
    /// # Errors
    /// Invalid query/anchor, cancellation/deadline, or callback failure.
    #[allow(clippy::too_many_arguments)]
    pub fn search_verified(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
        k: usize,
        minimum: usize,
        analysis: graph_search_types::AnalysisMode,
        mut verify: impl FnMut(&str, usize, u32, &mut WorkBudget) -> Result<RegionDecision>,
    ) -> Result<VerifiedBodyHits> {
        let index = if analysis == graph_search_types::AnalysisMode::Identifiers {
            self.identifiers.as_deref().unwrap_or(self)
        } else {
            self
        };
        let scores = index.collect_scores(terms, filters, language, excluded, budget, minimum)?;
        let candidate_regions = scores.len();
        let mut accepted = Scores::new();
        let mut verification_complete = true;
        for (ordinal, (score, line, coverage)) in scores {
            budget.check()?;
            let doc = &index.documents[ordinal];
            match verify(&index.files[doc.file], doc.unit, line, budget)? {
                RegionDecision::Accept(anchor) => {
                    if !(doc.start_line..=doc.end_line).contains(&anchor) {
                        return Err(crate::Error::InvalidQuery(
                            "verified source anchor is outside its candidate region".into(),
                        ));
                    }
                    accepted.insert(ordinal, (score, anchor, coverage));
                }
                RegionDecision::Reject => {}
                RegionDecision::Stop => {
                    verification_complete = false;
                    break;
                }
            }
        }
        let (hits, matched_regions) = index.rank(accepted, k);
        Ok(VerifiedBodyHits {
            hits,
            candidate_regions,
            matched_regions,
            verification_complete,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_scores(
        &self,
        terms: &[String],
        filters: &CompiledFilters,
        language: impl Fn(&str) -> Option<Language>,
        excluded: &BTreeSet<String>,
        budget: &mut WorkBudget,
        minimum: usize,
    ) -> Result<Scores> {
        let mut scores: BTreeMap<usize, (f32, u32, u128)> = BTreeMap::new();
        // Selective terms get their chance before a common term spends the cap.
        let distinct: BTreeSet<_> = terms.iter().collect();
        if distinct.len() > 128 {
            return Err(crate::Error::InvalidQuery(
                "body retrieval supports at most 128 distinct terms".into(),
            ));
        }
        let mut lists: Vec<_> = distinct
            .iter()
            .filter_map(|term| self.postings.get(*term).map(Vec::as_slice))
            .collect();
        lists.sort_by_key(|list| list.len());
        if minimum > 1 && minimum == distinct.len() {
            if lists.len() != distinct.len() {
                budget.check()?;
                return Ok(Scores::new());
            }
            let weights: Vec<_> = lists.iter().map(|list| self.idf(list.len())).collect();
            crate::intersection::intersect(
                &lists,
                |p| p.document,
                |ordinal| {
                    let path = &self.files[self.documents[ordinal].file];
                    !excluded.contains(path) && filters.matches(path, language(path))
                },
                budget,
                |ordinal, positions, budget| {
                    if !budget.candidate()? {
                        return Ok(false);
                    }
                    let mut score = 0.0;
                    let mut line = u32::MAX;
                    for ((list, &position), &weight) in lists.iter().zip(positions).zip(&weights) {
                        let posting = list[position];
                        score += self.contribution(posting, weight);
                        line = line.min(posting.line);
                    }
                    scores.insert(ordinal, (score, line, u128::MAX >> (128 - distinct.len())));
                    Ok(true)
                },
            )?;
        } else {
            'terms: for (term, list) in lists.into_iter().enumerate() {
                let weight = self.idf(list.len());
                for posting in list {
                    if !budget.posting()? {
                        break 'terms;
                    }
                    let doc = &self.documents[posting.document];
                    let path = &self.files[doc.file];
                    if excluded.contains(path) || !filters.matches(path, language(path)) {
                        continue;
                    }
                    if !scores.contains_key(&posting.document) && !budget.candidate()? {
                        continue;
                    }
                    let (score, line, coverage) =
                        scores
                            .entry(posting.document)
                            .or_insert((0.0, posting.line, 0));
                    *coverage |= 1u128 << term;
                    *score += self.contribution(*posting, weight);
                    *line = (*line).min(posting.line);
                }
            }
        }
        scores.retain(|_, (_, _, coverage)| coverage.count_ones() as usize >= minimum);
        Ok(scores)
    }

    fn rank(&self, scores: BTreeMap<usize, (f32, u32, u128)>, k: usize) -> (Vec<BodyHit>, usize) {
        let matched = scores.len();
        // Owner ranking is determined only by its best region. Extra evidence
        // cannot crowd other owners out of the candidate pool or inflate scores.
        let mut owners: BTreeMap<usize, Vec<ScoredRegion>> = BTreeMap::new();
        for (ordinal, (score, line, mask)) in scores {
            owners
                .entry(self.documents[ordinal].entity)
                .or_default()
                .push(ScoredRegion {
                    ordinal,
                    score,
                    line,
                    terms: mask,
                });
        }
        let mut ranked: Vec<_> = owners.into_values().collect();
        for regions in &mut ranked {
            regions.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.ordinal.cmp(&b.ordinal)));
        }
        ranked.sort_by(|a, b| {
            b[0].score
                .total_cmp(&a[0].score)
                .then(a[0].ordinal.cmp(&b[0].ordinal))
        });
        ranked.truncate(k);
        let hits = ranked
            .into_iter()
            .map(|mut regions| {
                let ScoredRegion {
                    ordinal,
                    score,
                    line,
                    terms: mut covered,
                } = regions.remove(0);
                let doc = &self.documents[ordinal];
                let mut spans = vec![(doc.start_line, doc.end_line)];
                let mut complementary_units = Vec::new();
                while complementary_units.len() < MAX_REGIONS_PER_OWNER.saturating_sub(1) {
                    let choice = regions
                        .iter()
                        .enumerate()
                        .filter(|(_, region)| {
                            region.terms & !covered != 0
                                || !spans
                                    .iter()
                                    .any(|&(start, end)| (start..=end).contains(&region.line))
                        })
                        .max_by(|(_, a), (_, b)| {
                            (a.terms & !covered)
                                .count_ones()
                                .cmp(&(b.terms & !covered).count_ones())
                                .then(a.score.total_cmp(&b.score))
                                .then(b.ordinal.cmp(&a.ordinal))
                        })
                        .map(|(position, _)| position);
                    let Some(position) = choice else {
                        break;
                    };
                    let ScoredRegion {
                        ordinal: extra,
                        terms: mask,
                        ..
                    } = regions.remove(position);
                    let other = &self.documents[extra];
                    complementary_units.push(other.unit);
                    spans.push((other.start_line, other.end_line));
                    covered |= mask;
                }
                BodyHit {
                    path: self.files[doc.file].clone(),
                    unit: doc.unit,
                    line,
                    score,
                    complementary_units,
                    omitted_regions: regions
                        .iter()
                        .filter(|region| {
                            region.terms & !covered != 0
                                || !spans
                                    .iter()
                                    .any(|&(start, end)| (start..=end).contains(&region.line))
                        })
                        .count(),
                }
            })
            .collect();
        (hits, matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::WorkLimits;
    use graph_search_types::{Language, query::GraphFilters};

    fn corpus() -> BTreeMap<String, SourceFileUnits> {
        [
            ("a.txt", "cache cache replacement\n"),
            ("b.txt", "cache\n"),
            ("c.txt", "unrelated replacement replacement\n"),
            ("d.txt", "other\n"),
        ]
        .into_iter()
        .map(|(path, text)| {
            (
                path.into(),
                crate::units::extract(path, text, "hash", Language::Unknown, &[]),
            )
        })
        .collect()
    }

    #[test]
    fn file_candidate_filter_does_not_turn_storage_boundaries_into_phrase_boundaries() {
        use crate::positional::{Limits, PositionalQuery, Predicate};
        let text = format!("alpha\n{}beta", "filler\n".repeat(200));
        let sources = [
            ("whole.txt", text.as_str()),
            ("alpha.txt", "alpha"),
            ("beta.txt", "beta"),
        ];
        let files = sources
            .into_iter()
            .map(|(path, text)| {
                (
                    path.into(),
                    crate::units::extract(path, text, "hash", Language::Unknown, &[]),
                )
            })
            .collect();
        let index = BodyIndex::new(&files);
        let query =
            PositionalQuery::new("alpha beta", Predicate::Ordered { intervening: 200 }).unwrap();
        let terms = query.candidate_terms();
        let candidates = index
            .file_candidates(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut WorkBudget::new(WorkLimits::default()),
            )
            .unwrap();
        assert_eq!(candidates, ["whole.txt"]);
        let witnesses = query
            .verify_all(
                &text,
                Limits {
                    bytes: text.len(),
                    tokens: 202,
                },
                10,
                || Ok(()),
            )
            .unwrap();
        assert!(witnesses.complete);
        assert_eq!(witnesses.witnesses.len(), 1);
        let mut work = WorkBudget::new(WorkLimits {
            postings: 0,
            ..WorkLimits::default()
        });
        assert!(
            index
                .file_candidates(
                    &terms,
                    &CompiledFilters::default(),
                    |_| None,
                    &BTreeSet::new(),
                    &mut work
                )
                .unwrap()
                .is_empty()
        );
        assert!(!work.report().2.is_empty());
        assert!(
            index
                .file_candidates(
                    &terms,
                    &CompiledFilters::default(),
                    |_| None,
                    &BTreeSet::from(["whole.txt".into()]),
                    &mut WorkBudget::new(WorkLimits::default())
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn positional_verification_precedes_best_region_and_owner_top_k() {
        use crate::positional::{Limits, Outcome, PositionalQuery, Predicate};
        let mut lines = vec!["unrelated"; 320];
        for line in lines.iter_mut().take(70) {
            *line = "alpha filler beta";
        }
        lines[250] = "alpha beta";
        let text = lines.join("\n");
        let files = BTreeMap::from([(
            "a.txt".into(),
            crate::units::extract("a.txt", &text, "hash", Language::Unknown, &[]),
        )]);
        let index = BodyIndex::new(&files);
        let query =
            PositionalQuery::new("alpha beta", Predicate::Ordered { intervening: 0 }).unwrap();
        let terms = query.candidate_terms();
        let (ordinary, _) = index
            .search_with_analysis(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut WorkBudget::new(WorkLimits::default()),
                1,
                2,
                graph_search_types::AnalysisMode::Identifiers,
            )
            .unwrap();
        assert!(
            files["a.txt"].units[ordinary[0].unit].span.end_line < 251,
            "fixture must rank a nonphrase region ahead of the actual phrase"
        );
        let mut examined = 0usize;
        let verified = index
            .search_verified(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut WorkBudget::new(WorkLimits::default()),
                1,
                2,
                graph_search_types::AnalysisMode::Identifiers,
                |path, unit, _, work| {
                    examined += 1;
                    let span = files[path].units[unit].span;
                    let region = &text[span.start_byte as usize..span.end_byte as usize];
                    let verification = query.verify(
                        region,
                        Limits {
                            bytes: region.len(),
                            tokens: 4096,
                        },
                        || work.check(),
                    )?;
                    Ok(match verification.outcome {
                        Outcome::Found(witness) => RegionDecision::Accept(
                            span.start_line
                                + u32::try_from(
                                    region[..witness.start]
                                        .bytes()
                                        .filter(|&byte| byte == b'\n')
                                        .count(),
                                )
                                .unwrap(),
                        ),
                        Outcome::Absent => RegionDecision::Reject,
                        Outcome::Limited => RegionDecision::Stop,
                    })
                },
            )
            .unwrap();
        assert_eq!(examined, verified.candidate_regions);
        assert!(verified.verification_complete);
        assert_eq!(verified.hits.len(), 1);
        assert_eq!(verified.hits[0].line, 251);
        assert!(verified.matched_regions > 0);
    }

    #[test]
    fn verifier_stop_filters_and_invalid_anchors_cannot_admit_unverified_regions() {
        let index = BodyIndex::new(&corpus());
        let terms = vec!["cache".into(), "replacement".into()];
        let mut seen = Vec::new();
        let result = index
            .search_verified(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::from(["a.txt".into()]),
                &mut WorkBudget::new(WorkLimits::default()),
                10,
                1,
                graph_search_types::AnalysisMode::Split,
                |path, _, line, _| {
                    seen.push(path.to_owned());
                    Ok(if seen.len() == 1 {
                        RegionDecision::Accept(line)
                    } else {
                        RegionDecision::Stop
                    })
                },
            )
            .unwrap();
        assert_eq!(seen, ["b.txt", "c.txt"]);
        assert!(!result.verification_complete);
        assert_eq!(result.matched_regions, 1);
        assert_eq!(result.hits[0].path, "b.txt");
        assert!(
            index
                .search_verified(
                    &terms,
                    &CompiledFilters::default(),
                    |_| None,
                    &BTreeSet::new(),
                    &mut WorkBudget::new(WorkLimits::default()),
                    10,
                    1,
                    graph_search_types::AnalysisMode::Split,
                    |_, _, _, _| Ok(RegionDecision::Accept(0))
                )
                .is_err()
        );
    }

    #[test]
    fn complementary_regions_preserve_owner_rank_and_region_conjunction() {
        let mut text = vec!["unrelated"; 650];
        for line in [20, 150, 280, 410, 540] {
            text[line] = "cobalt";
        }
        text[600] = "amber";
        let files = BTreeMap::from([
            (
                "a.txt".into(),
                crate::units::extract("a.txt", &text.join("\n"), "hash", Language::Unknown, &[]),
            ),
            (
                "b.txt".into(),
                crate::units::extract("b.txt", "cobalt amber", "hash", Language::Unknown, &[]),
            ),
        ]);
        let index = BodyIndex::new(&files);
        let run = |terms: &[String], minimum| {
            index.search_matching(
                terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut WorkBudget::new(WorkLimits::default()),
                10,
                minimum,
            )
        };
        let terms = vec!["cobalt".into(), "amber".into()];
        let (hits, _) = run(&terms, 1).unwrap();
        assert_eq!(hits.len(), 2);
        let hit = hits.iter().find(|hit| hit.path == "a.txt").unwrap();
        assert_eq!(hit.complementary_units.len(), 3);
        assert!(hit.omitted_regions > 0);
        let selected: Vec<_> = std::iter::once(hit.unit)
            .chain(hit.complementary_units.iter().copied())
            .collect();
        for term in &terms {
            assert!(
                selected
                    .iter()
                    .any(|&unit| files["a.txt"].units[unit].terms.contains_key(term))
            );
        }
        let (all, _) = run(&terms, 2).unwrap();
        assert_eq!(all.len(), 1, "conjunction remains region-local");
        assert_eq!(all[0].path, "b.txt");
        assert!(run(&(0..129).map(|n| format!("term{n}")).collect::<Vec<_>>(), 1).is_err());
    }

    #[test]
    fn conjunction_matches_union_scores_and_admits_only_complete_matches() {
        let files: BTreeMap<_, _> = (0..1000)
            .map(|i| {
                let path = format!("{i:04}.txt");
                let text = if i == 999 { "common rare" } else { "common" };
                let source = crate::units::extract(&path, text, "hash", Language::Unknown, &[]);
                (path, source)
            })
            .collect();
        let index = BodyIndex::new(&files);
        let terms = vec!["common".into(), "rare".into()];
        let (union, _) = index
            .search(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut WorkBudget::new(WorkLimits::default()),
                1000,
            )
            .unwrap();
        let mut budget = WorkBudget::new(WorkLimits {
            candidates: 1,
            postings: 64,
            ..WorkLimits::default()
        });
        let (hits, count) = index
            .search_matching(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::new(),
                &mut budget,
                10,
                2,
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "0999.txt");
        let expected = union.iter().find(|h| h.path == "0999.txt").unwrap();
        assert_eq!(hits[0].score.to_bits(), expected.score.to_bits());
        assert_eq!(budget.lexical_report().0, 1);
        assert!(budget.lexical_report().1 < 64);
        assert!(budget.report().2.is_empty());
        let (masked, count) = index
            .search_matching(
                &terms,
                &CompiledFilters::default(),
                |_| None,
                &BTreeSet::from(["0999.txt".into()]),
                &mut WorkBudget::new(WorkLimits::default()),
                10,
                2,
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(masked.is_empty());
    }

    #[test]
    fn postings_agree_with_independent_exhaustive_region_scoring() {
        let files = corpus();
        let index = BodyIndex::new(&files);
        for query in [
            "cache",
            "replacement",
            "cache replacement",
            "absent",
            "cache absent",
        ] {
            let terms = crate::lexical::query_terms(query);
            let docs: Vec<_> = files
                .iter()
                .flat_map(|(path, f)| f.units.iter().map(move |u| (path, u)))
                .collect();
            let avg = docs
                .iter()
                .map(|(_, u)| u.terms.values().map(Vec::len).sum::<usize>())
                .sum::<usize>() as f32
                / docs.len() as f32;
            let mut expected = Vec::new();
            for (path, unit) in &docs {
                let length = unit.terms.values().map(Vec::len).sum::<usize>() as f32;
                let mut score = 0.0;
                for term in &terms {
                    let df = docs
                        .iter()
                        .filter(|(_, u)| u.terms.contains_key(term))
                        .count() as f32;
                    let tf = unit.terms.get(term).map_or(0, Vec::len) as f32;
                    if tf > 0.0 {
                        score +=
                            (1.0 + (docs.len() as f32 - df + 0.5) / (df + 0.5)).ln() * tf * 2.2
                                / (tf + 1.2 * (0.25 + 0.75 * length / avg));
                    }
                }
                if score > 0.0 {
                    expected.push(((*path).clone(), score));
                }
            }
            expected.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            let (hits, matched) = index
                .search(
                    &terms,
                    &CompiledFilters::default(),
                    |_| None,
                    &BTreeSet::new(),
                    &mut WorkBudget::new(WorkLimits::default()),
                    20,
                )
                .unwrap();
            assert_eq!(matched, expected.len());
            assert_eq!(hits.len(), expected.len());
            for (hit, (path, score)) in hits.iter().zip(expected) {
                assert_eq!(hit.path, path);
                assert!((hit.score - score).abs() < 0.000_001);
            }
        }
    }

    #[test]
    fn filters_masks_and_work_caps_precede_candidate_allocation() {
        let index = BodyIndex::new(&corpus());
        let filter = CompiledFilters::new(&GraphFilters {
            path_glob: Some("c.txt".into()),
            ..GraphFilters::default()
        })
        .unwrap();
        let mut work = WorkBudget::new(WorkLimits {
            candidates: 1,
            ..WorkLimits::default()
        });
        let (hits, _) = index
            .search(
                &["replacement".into()],
                &filter,
                |_| None,
                &BTreeSet::new(),
                &mut work,
                10,
            )
            .unwrap();
        assert_eq!(hits[0].path, "c.txt");
        assert_eq!(work.lexical_report(), (1, 2));
        assert!(work.report().2.is_empty());
        let mut work = WorkBudget::new(WorkLimits {
            postings: 1,
            ..WorkLimits::default()
        });
        let (hits, _) = index
            .search(
                &["replacement".into()],
                &filter,
                |_| None,
                &BTreeSet::new(),
                &mut work,
                10,
            )
            .unwrap();
        assert!(hits.is_empty());
        assert_eq!(work.lexical_report(), (0, 1));
        assert_eq!(
            work.report().2[0].kind,
            graph_search_types::result::TruncationKind::Postings
        );
        let (hits, _) = index
            .search(
                &["replacement".into()],
                &filter,
                |_| None,
                &BTreeSet::from(["c.txt".into()]),
                &mut WorkBudget::new(WorkLimits::default()),
                10,
            )
            .unwrap();
        assert!(hits.is_empty());
    }
}
