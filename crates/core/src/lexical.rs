//! Identifier-aware metadata retrieval, independent of the graph storage adapter.
//!
//! BM25 uses name/path/signature weights 8/2/1 and k1=1.2, b=0.75,
//! matching the research FTS5 metadata experiment. Exact names have a separate
//! priority lane. Native postings are prepared with the graph generation.
//! The exhaustive scorer remains available as a differential correctness oracle.
#![allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects)]
use graph_search_types::{FieldNormalization, Node};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[path = "lexical_update.rs"]
mod update;

const STOPWORDS: &[&str] = &[
    "how", "does", "the", "a", "an", "is", "are", "in", "to", "of", "and", "where", "what", "for",
    "with",
];

pub(crate) fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(&word)
}

/// Split `camelCase`, `HTTPServer`, `snake_case` and punctuation into lowercase tokens.
#[must_use]
pub fn tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        let previous = i.checked_sub(1).and_then(|p| chars.get(p));
        let next = chars.get(i + 1);
        let boundary = c.is_uppercase()
            && previous.is_some_and(|p| {
                p.is_lowercase()
                    || p.is_numeric()
                    || (p.is_uppercase() && next.is_some_and(|n| n.is_lowercase()))
            });
        if boundary && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Distinct query terms; keep short literal names, drop sentence function words.
#[must_use]
pub fn query_terms(query: &str) -> Vec<String> {
    let words = tokens(query);
    let sentence = query.split_whitespace().count() > 1;
    words
        .into_iter()
        .filter(|w| !sentence || !STOPWORDS.contains(&w.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Exact symbol spelling after removing display punctuation, not qualification.
#[must_use]
pub fn exact_query(query: &str) -> String {
    query
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
        .to_lowercase()
}

#[derive(Clone, Copy)]
struct Posting {
    document: usize,
    frequency: f32,
    fields: [u32; 4],
    whole_fields: [u32; 4],
}

#[derive(Clone, Copy)]
struct CorpusLengths {
    combined: usize,
    fields: [usize; 4],
}

/// Generation-owned lexical statistics and sorted native postings.
/// Document positions match the input symbol order.
pub struct LexicalIndex {
    lengths: Vec<usize>,
    field_lengths: Vec<[usize; 4]>,
    whole_field_lengths: Vec<[usize; 4]>,
    postings: BTreeMap<String, Arc<Vec<Posting>>>,
    average_length: f32,
    average_field_lengths: [f32; 4],
    fields: (bool, bool),
    totals: CorpusLengths,
}

impl LexicalIndex {
    /// Builds metadata postings, retaining the existing scorer's field weights.
    #[must_use]
    pub fn new(nodes: &[Node]) -> Self {
        Self::with_fields(nodes, false, false)
    }

    /// Builds explicitly selected whole-identifier and qualified-name fields.
    /// Whole/split frequencies and lengths remain separate. Overlapping aliases
    /// use the maximum frequency/length per field rather than counting twice.
    #[must_use]
    pub fn with_fields(nodes: &[Node], whole: bool, qualified: bool) -> Self {
        let mut postings: BTreeMap<String, Vec<Posting>> = BTreeMap::new();
        let mut lengths = Vec::with_capacity(nodes.len());
        let mut field_lengths = Vec::with_capacity(nodes.len());
        let mut whole_field_lengths = Vec::with_capacity(nodes.len());
        for (document, node) in nodes.iter().enumerate() {
            let mut frequency: BTreeMap<String, ([u32; 4], [u32; 4])> = BTreeMap::new();
            let mut split_lengths = [0usize; 4];
            let mut whole_lengths = [0usize; 4];
            for (field_id, field) in [
                node.name.as_deref().unwrap_or_default(),
                node.path.as_str(),
                node.signature.as_deref().unwrap_or_default(),
                node.qualified_name
                    .as_deref()
                    .filter(|name| qualified && Some(*name) != node.name.as_deref())
                    .unwrap_or_default(),
            ]
            .into_iter()
            .enumerate()
            {
                for term in tokens(field) {
                    if STOPWORDS.contains(&term.as_str()) {
                        continue;
                    }
                    split_lengths[field_id] += 1;
                    let (split, _) = frequency.entry(term).or_default();
                    split[field_id] = split[field_id].saturating_add(1);
                }
                if whole {
                    for term in crate::analyzer::whole_terms(field) {
                        whole_lengths[field_id] += 1;
                        let (_, original) = frequency.entry(term).or_default();
                        original[field_id] = original[field_id].saturating_add(1);
                    }
                }
            }
            lengths.push(
                split_lengths
                    .iter()
                    .zip(whole_lengths)
                    .map(|(&split, whole)| split.max(whole))
                    .sum(),
            );
            field_lengths.push(split_lengths);
            whole_field_lengths.push(whole_lengths);
            for (term, (fields, whole_fields)) in frequency {
                let frequency = fields
                    .iter()
                    .zip(whole_fields)
                    .zip([8.0, 2.0, 1.0, 4.0])
                    .map(|((&split, whole), weight)| split.max(whole) as f32 * weight)
                    .sum();
                postings.entry(term).or_default().push(Posting {
                    document,
                    frequency,
                    fields,
                    whole_fields,
                });
            }
        }
        Self::from_parts(
            lengths,
            field_lengths,
            whole_field_lengths,
            postings
                .into_iter()
                .map(|(term, list)| (term, Arc::new(list)))
                .collect(),
            (whole, qualified),
            None,
        )
    }

    fn from_parts(
        lengths: Vec<usize>,
        field_lengths: Vec<[usize; 4]>,
        whole_field_lengths: Vec<[usize; 4]>,
        postings: BTreeMap<String, Arc<Vec<Posting>>>,
        fields: (bool, bool),
        totals: Option<CorpusLengths>,
    ) -> Self {
        let totals = totals.unwrap_or_else(|| CorpusLengths {
            combined: lengths.iter().sum(),
            fields: std::array::from_fn(|field| {
                field_lengths
                    .iter()
                    .zip(&whole_field_lengths)
                    .map(|(split, whole)| split[field].max(whole[field]))
                    .sum()
            }),
        });
        let population = lengths.len().max(1) as f32;
        let average_length = (totals.combined as f32 / population).max(1.0);
        let average_field_lengths = totals
            .fields
            .map(|total| (total as f32 / population).max(1.0));
        Self {
            lengths,
            field_lengths,
            whole_field_lengths,
            postings,
            average_length,
            average_field_lengths,
            fields,
            totals,
        }
    }

    /// Indexed token lengths in name/path/signature order. Kept separately for
    /// field-normalization experiments; the default scorer uses total length.
    #[must_use]
    pub fn field_lengths(&self, document: usize) -> Option<[usize; 3]> {
        self.field_lengths.get(document).map(|v| [v[0], v[1], v[2]])
    }

    /// Unweighted term frequencies in name/path/signature order.
    #[must_use]
    pub fn field_frequencies(&self, document: usize, term: &str) -> Option<[u32; 3]> {
        let postings = self.postings.get(term)?;
        let position = postings
            .binary_search_by_key(&document, |posting| posting.document)
            .ok()?;
        let fields = postings[position].fields;
        Some([fields[0], fields[1], fields[2]])
    }

    /// Whole-spelling lengths in name/path/signature/qualified-name order.
    #[must_use]
    pub fn whole_field_lengths(&self, document: usize) -> Option<[usize; 4]> {
        self.whole_field_lengths.get(document).copied()
    }

    /// Separate whole-spelling frequencies, without split-token overlap.
    #[must_use]
    pub fn whole_field_frequencies(&self, document: usize, term: &str) -> Option<[u32; 4]> {
        let list = self.postings.get(term)?;
        let position = list.binary_search_by_key(&document, |p| p.document).ok()?;
        Some(list[position].whole_fields)
    }

    fn contribution(
        &self,
        posting: Posting,
        frequency: usize,
        normalization: FieldNormalization,
    ) -> f32 {
        let count = self.lengths.len() as f32;
        let df = frequency as f32;
        let idf = ((count - df + 0.5) / (df + 0.5)).ln().max(0.000_001);
        if normalization == FieldNormalization::Bm25f {
            let normalized: f32 = [8.0, 2.0, 1.0, 4.0]
                .into_iter()
                .enumerate()
                .map(|(field, weight)| {
                    let tf = posting.fields[field].max(posting.whole_fields[field]) as f32;
                    let length = self.field_lengths[posting.document][field]
                        .max(self.whole_field_lengths[posting.document][field])
                        as f32;
                    weight * tf / (0.25 + 0.75 * length / self.average_field_lengths[field])
                })
                .sum();
            return idf * normalized * 2.2 / (normalized + 1.2);
        }
        idf * posting.frequency * 2.2
            / (posting.frequency
                + 1.2 * (0.25 + 0.75 * self.lengths[posting.document] as f32 / self.average_length))
    }

    /// Exhaustive random-access scoring for differential validation.
    #[must_use]
    pub fn score(&self, document: usize, terms: &[String]) -> f32 {
        self.score_with_normalization(document, terms, FieldNormalization::Combined)
    }

    /// Exhaustive scoring with an explicit metadata normalization policy.
    #[must_use]
    pub fn score_with_normalization(
        &self,
        document: usize,
        terms: &[String],
        normalization: FieldNormalization,
    ) -> f32 {
        terms
            .iter()
            .map(|term| {
                let Some(postings) = self.postings.get(term) else {
                    return 0.0;
                };
                let Ok(position) = postings.binary_search_by_key(&document, |p| p.document) else {
                    return 0.0;
                };
                self.contribution(postings[position], postings.len(), normalization)
            })
            .sum()
    }

    /// Accumulates only matching postings. Existing zero-score entries reserve
    /// exact-name candidates, preserving that lane before lexical work caps.
    /// `accept` runs before candidate allocation. Every posting examined is charged.
    /// # Errors
    /// On cancellation or deadline.
    pub fn accumulate(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
    ) -> crate::Result<()> {
        self.accumulate_inner(
            terms,
            scores,
            accept,
            budget,
            None,
            FieldNormalization::Combined,
        )
    }

    /// Accumulates scores and observed distinct-term coverage in the same bounded pass.
    /// # Errors
    /// On cancellation or deadline.
    pub fn accumulate_coverage(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
        coverage: &mut BTreeMap<usize, usize>,
    ) -> crate::Result<()> {
        self.accumulate_inner(
            terms,
            scores,
            accept,
            budget,
            Some(coverage),
            FieldNormalization::Combined,
        )
    }

    /// Accumulates only documents present in every requested term list.
    /// Scores preserve input-term order (and repeated-term weights); membership
    /// coverage counts distinct terms. Exact candidates may already be reserved.
    /// # Errors
    /// On cancellation or deadline.
    pub fn accumulate_all(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
        coverage: &mut BTreeMap<usize, usize>,
    ) -> crate::Result<()> {
        self.accumulate_all_inner(
            terms,
            scores,
            accept,
            budget,
            coverage,
            FieldNormalization::Combined,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn accumulate_ranked(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
        coverage: &mut BTreeMap<usize, usize>,
        minimum: usize,
        normalization: FieldNormalization,
    ) -> crate::Result<()> {
        if minimum > 1 && minimum == terms.iter().collect::<BTreeSet<_>>().len() {
            self.accumulate_all_inner(terms, scores, accept, budget, coverage, normalization)
        } else {
            self.accumulate_inner(
                terms,
                scores,
                accept,
                budget,
                (minimum > 1).then_some(coverage),
                normalization,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn accumulate_all_inner(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
        coverage: &mut BTreeMap<usize, usize>,
        normalization: FieldNormalization,
    ) -> crate::Result<()> {
        let Some(lists): Option<Vec<_>> = terms
            .iter()
            .map(|term| self.postings.get(term).map(|list| list.as_slice()))
            .collect()
        else {
            budget.check()?;
            return Ok(());
        };
        let distinct = terms.iter().collect::<BTreeSet<_>>().len();
        crate::intersection::intersect(
            &lists,
            |p| p.document,
            accept,
            budget,
            |ordinal, positions, budget| {
                if !scores.contains_key(&ordinal) && !budget.candidate()? {
                    return Ok(false);
                }
                let score = lists
                    .iter()
                    .zip(positions)
                    .map(|(list, &position)| {
                        self.contribution(list[position], list.len(), normalization)
                    })
                    .sum::<f32>();
                *scores.entry(ordinal).or_default() += score;
                coverage.insert(ordinal, distinct);
                Ok(true)
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn accumulate_inner(
        &self,
        terms: &[String],
        scores: &mut BTreeMap<usize, f32>,
        accept: impl Fn(usize) -> bool,
        budget: &mut crate::work::WorkBudget,
        mut coverage: Option<&mut BTreeMap<usize, usize>>,
        normalization: FieldNormalization,
    ) -> crate::Result<()> {
        budget.check()?;
        let mut seen_terms = BTreeSet::new();
        for term in terms {
            let count_term = coverage.is_some() && seen_terms.insert(term);
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            for &posting in postings.iter() {
                if !budget.posting()? {
                    return Ok(());
                }
                if !accept(posting.document) {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    scores.entry(posting.document)
                {
                    if !budget.candidate()? {
                        continue;
                    }
                    entry.insert(0.0);
                }
                if let Some(score) = scores.get_mut(&posting.document) {
                    *score += self.contribution(posting, postings.len(), normalization);
                    if count_term && let Some(counts) = coverage.as_mut() {
                        let count = counts.entry(posting.document).or_default();
                        *count = count.saturating_add(1);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bm25f_isolates_name_length_from_unmatched_signature_length() {
        let mut nodes = vec![
            Node {
                name: Some("needle".into()),
                path: "a.rs".into(),
                signature: Some("x".into()),
                ..Node::default()
            },
            Node {
                name: Some("other".into()),
                path: "b.rs".into(),
                signature: Some("x".into()),
                ..Node::default()
            },
        ];
        let before = LexicalIndex::new(&nodes);
        nodes[0].signature = Some("x ".repeat(1000));
        let after = LexicalIndex::new(&nodes);
        let terms = vec!["needle".into()];
        assert_eq!(
            before
                .score_with_normalization(0, &terms, FieldNormalization::Bm25f)
                .to_bits(),
            after
                .score_with_normalization(0, &terms, FieldNormalization::Bm25f)
                .to_bits()
        );
        assert_ne!(
            before.score(0, &terms).to_bits(),
            after.score(0, &terms).to_bits()
        );
        // Fixed field mean 1, tf 1, name boost 8, and clipped IDF for N=2, df=1.
        let expected = 0.000_001f32 * 8.0 * 2.2 / (8.0 + 1.2);
        assert_eq!(
            before
                .score_with_normalization(0, &terms, FieldNormalization::Bm25f)
                .to_bits(),
            expected.to_bits()
        );
    }

    #[test]
    fn bm25f_postings_and_conjunctions_match_exhaustive_scoring() {
        let nodes: Vec<_> = (0..257)
            .map(|i| Node {
                name: Some(if i % 13 == 0 { "rareCommon" } else { "common" }.into()),
                path: format!("src/topic{}.rs", i % 7),
                signature: (i % 3 != 0).then(|| "fn value(common: Cache)".into()),
                qualified_name: Some("Module::common".into()),
                ..Node::default()
            })
            .collect();
        for whole in [false, true] {
            let index = LexicalIndex::with_fields(&nodes, whole, whole);
            for terms in [
                vec!["rare".into(), "common".into()],
                vec!["module".into()],
                vec!["absent".into()],
            ] {
                for minimum in [1, terms.len()] {
                    let mut scores = BTreeMap::new();
                    let mut coverage = BTreeMap::new();
                    let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits::default());
                    index
                        .accumulate_ranked(
                            &terms,
                            &mut scores,
                            |i| i % 2 == 0,
                            &mut work,
                            &mut coverage,
                            minimum,
                            FieldNormalization::Bm25f,
                        )
                        .unwrap();
                    let expected: BTreeMap<_, _> = (0..nodes.len())
                        .filter(|i| i % 2 == 0)
                        .filter_map(|i| {
                            let matches = terms
                                .iter()
                                .filter(|term| {
                                    index.score_with_normalization(
                                        i,
                                        std::slice::from_ref(term),
                                        FieldNormalization::Bm25f,
                                    ) > 0.0
                                })
                                .count();
                            (matches >= minimum).then(|| {
                                (
                                    i,
                                    index
                                        .score_with_normalization(
                                            i,
                                            &terms,
                                            FieldNormalization::Bm25f,
                                        )
                                        .to_bits(),
                                )
                            })
                        })
                        .collect();
                    assert_eq!(
                        scores
                            .iter()
                            .map(|(&i, score)| (i, score.to_bits()))
                            .collect::<BTreeMap<_, _>>(),
                        expected
                    );
                    assert!(work.report().2.is_empty());
                    assert_eq!(
                        usize::try_from(work.lexical_report().0).unwrap(),
                        expected.len()
                    );
                }
            }
        }
    }

    #[test]
    fn whole_and_qualified_fields_are_separate_without_double_counting_plain_words() {
        let nodes = [
            Node {
                name: Some("getHTTPResponse".into()),
                path: "a.rs".into(),
                qualified_name: Some("Client::getHTTPResponse".into()),
                signature: Some("fn getHTTPResponse(is: bool)".into()),
                ..Node::default()
            },
            Node {
                name: Some("other".into()),
                path: "b.rs".into(),
                ..Node::default()
            },
        ];
        let legacy = LexicalIndex::new(&nodes);
        let whole = LexicalIndex::with_fields(&nodes, true, false);
        let qualified = LexicalIndex::with_fields(&nodes, false, true);
        let both = LexicalIndex::with_fields(&nodes, true, true);
        assert_eq!(
            legacy.score(0, &["gethttpresponse".into()]).to_bits(),
            0.0f32.to_bits()
        );
        assert!(whole.score(0, &["gethttpresponse".into()]) > 0.0);
        assert_eq!(
            whole.score(0, &["client".into()]).to_bits(),
            0.0f32.to_bits()
        );
        assert!(qualified.score(0, &["client".into()]) > 0.0);
        assert_eq!(
            both.whole_field_frequencies(0, "gethttpresponse"),
            Some([1, 0, 1, 1])
        );
        assert!(both.score(0, &["is".into()]) > 0.0);
        let plain = [Node {
            name: Some("cache".into()),
            path: "a.rs".into(),
            ..Node::default()
        }];
        assert_eq!(
            LexicalIndex::new(&plain)
                .score(0, &["cache".into()])
                .to_bits(),
            LexicalIndex::with_fields(&plain, true, false)
                .score(0, &["cache".into()])
                .to_bits()
        );
    }

    #[test]
    fn field_statistics_keep_unweighted_counts_and_separate_lengths() {
        let index = LexicalIndex::new(&[Node {
            name: Some("cacheCache".into()),
            path: "src/cache.rs".into(),
            signature: Some("cache cache cache".into()),
            ..Node::default()
        }]);
        assert_eq!(index.field_frequencies(0, "cache"), Some([2, 1, 3]));
        assert_eq!(index.field_lengths(0), Some([2, 3, 3]));
        assert_eq!(index.field_frequencies(0, "absent"), None);
        assert_eq!(index.field_lengths(1), None);
    }

    #[test]
    fn identifiers_and_query_terms_are_normalized() {
        assert_eq!(
            tokens("HTTPServer::loadPdsSecret snake_case v2Token"),
            [
                "http", "server", "load", "pds", "secret", "snake", "case", "v2", "token"
            ]
        );
        assert_eq!(
            query_terms("Where is load load secret?"),
            ["load", "secret"]
        );
        assert_eq!(query_terms("is"), ["is"]);
    }
}

#[cfg(test)]
mod differential {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::{WorkBudget, WorkLimits};

    // Independent copy of the pre-postings computation: derive every document
    // term map and corpus frequency from source fields, never from postings.
    fn reference(nodes: &[Node], terms: &[String]) -> Vec<f32> {
        let docs: Vec<_> = nodes
            .iter()
            .map(|node| {
                let mut freq = BTreeMap::new();
                let mut length = 0;
                for (field, weight) in [
                    (node.name.as_deref().unwrap_or_default(), 8.0),
                    (node.path.as_str(), 2.0),
                    (node.signature.as_deref().unwrap_or_default(), 1.0),
                ] {
                    for token in tokens(field)
                        .into_iter()
                        .filter(|t| !STOPWORDS.contains(&t.as_str()))
                    {
                        *freq.entry(token).or_insert(0.0f32) += weight;
                        length += 1;
                    }
                }
                (freq, length)
            })
            .collect();
        let average =
            (docs.iter().map(|(_, n)| n).sum::<usize>() as f32 / docs.len().max(1) as f32).max(1.0);
        let mut df = BTreeMap::new();
        for (terms, _) in &docs {
            for term in terms.keys() {
                *df.entry(term).or_insert(0usize) += 1;
            }
        }
        docs.iter()
            .map(|(freq, length)| {
                terms
                    .iter()
                    .map(|term| {
                        let Some(&tf) = freq.get(term) else {
                            return 0.0;
                        };
                        let frequency = df[term] as f32;
                        let idf = ((docs.len() as f32 - frequency + 0.5) / (frequency + 0.5))
                            .ln()
                            .max(0.000_001);
                        idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * (*length as f32) / average))
                    })
                    .sum()
            })
            .collect()
    }

    #[test]
    fn postings_scores_match_the_original_exhaustive_scorer() {
        let names = ["HTTPServer", "cache_cache", "élève", "common", "is", ""];
        for count in [0, 1, 7, 257] {
            let nodes: Vec<_> = (0..count)
                .map(|i| Node {
                    name: Some(names[i % names.len()].into()),
                    path: format!("src/topic{}/common.rs", i % 11),
                    signature: Some(format!(
                        "fn {}(common: Result<CacheValue>)",
                        names[(i * 7 + 1) % names.len()]
                    )),
                    ..Node::default()
                })
                .collect();
            let index = LexicalIndex::new(&nodes);
            for query in [
                "HTTPServer",
                "cache common",
                "élève",
                "absent",
                "topic3 cache value",
                "is",
                "common common",
            ] {
                let terms = query_terms(query);
                let expected = reference(&nodes, &terms);
                let mut actual = BTreeMap::new();
                let mut budget = WorkBudget::new(WorkLimits::default());
                index
                    .accumulate(&terms, &mut actual, |_| true, &mut budget)
                    .unwrap();
                for (i, expected) in expected.into_iter().enumerate() {
                    assert_eq!(
                        actual.get(&i).copied().unwrap_or_default().to_bits(),
                        expected.to_bits(),
                        "{count}/{query}/{i}"
                    );
                    assert_eq!(index.score(i, &terms).to_bits(), expected.to_bits());
                }
                assert!(budget.report().2.is_empty());
            }
        }
    }

    #[test]
    fn conjunction_scores_match_exhaustive_source_fields_bit_for_bit() {
        let nodes: Vec<_> = (0..257)
            .map(|i| Node {
                name: Some(if i % 13 == 0 { "rareCommon" } else { "common" }.into()),
                path: format!("src/topic{}.rs", i % 7),
                signature: Some("fn value(common: Cache)".into()),
                ..Node::default()
            })
            .collect();
        let index = LexicalIndex::new(&nodes);
        for query in [
            "common rare",
            "cache topic3 common",
            "rare absent",
            "is cache",
            "common common rare",
        ] {
            let terms: Vec<_> = query.split_whitespace().map(str::to_owned).collect();
            let reference = reference(&nodes, &terms);
            let mut expected = BTreeMap::new();
            for (i, node) in nodes.iter().enumerate() {
                let text = format!(
                    "{} {} {}",
                    node.name.as_deref().unwrap(),
                    node.path,
                    node.signature.as_deref().unwrap()
                );
                let words: BTreeSet<_> = tokens(&text)
                    .into_iter()
                    .filter(|t| !STOPWORDS.contains(&t.as_str()))
                    .collect();
                if i % 2 == 0 && terms.iter().all(|term| words.contains(term)) {
                    expected.insert(i, reference[i].to_bits());
                }
            }
            let mut scores = BTreeMap::new();
            let mut coverage = BTreeMap::new();
            let mut budget = WorkBudget::new(WorkLimits::default());
            index
                .accumulate_all(
                    &terms,
                    &mut scores,
                    |i| i % 2 == 0,
                    &mut budget,
                    &mut coverage,
                )
                .unwrap();
            assert_eq!(
                scores
                    .into_iter()
                    .map(|(i, score)| (i, score.to_bits()))
                    .collect::<BTreeMap<_, _>>(),
                expected
            );
            assert!(
                coverage
                    .values()
                    .all(|&n| n == terms.iter().collect::<BTreeSet<_>>().len())
            );
            assert_eq!(
                usize::try_from(budget.lexical_report().0).unwrap(),
                expected.len()
            );
        }
    }

    #[test]
    fn selective_queries_charge_only_matching_postings_and_respect_work_caps() {
        let nodes: Vec<_> = (0..1000)
            .map(|i| Node {
                name: Some(format!("common topic{i}")),
                ..Node::default()
            })
            .collect();
        let index = LexicalIndex::new(&nodes);
        let mut scores = BTreeMap::new();
        let mut budget = WorkBudget::new(WorkLimits::default());
        index
            .accumulate(
                &query_terms("topic731 absent"),
                &mut scores,
                |_| true,
                &mut budget,
            )
            .unwrap();
        assert_eq!(scores.len(), 1);
        assert_eq!(budget.lexical_report(), (1, 1));
        let mut scores = BTreeMap::new();
        let mut budget = WorkBudget::new(WorkLimits {
            postings: 7,
            candidates: 2,
            ..WorkLimits::default()
        });
        index
            .accumulate(&query_terms("common"), &mut scores, |_| true, &mut budget)
            .unwrap();
        assert_eq!(scores.len(), 2);
        assert_eq!(budget.lexical_report(), (2, 7));
        assert_eq!(budget.report().2.len(), 2);
    }
}
