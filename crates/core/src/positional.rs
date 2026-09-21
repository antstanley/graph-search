//! Native positional verification over original UTF-8 source, separate from raw
//! substring matching. Whole lexemes occupy one position; punctuation separates
//! them, underscores remain inside a lexeme, and stopwords/repetitions are kept.
//! Lowercase comparison uses the analyzer's Unicode contract (no NFC/full fold).

use crate::{Error, Result};
use std::collections::{BTreeMap, VecDeque};

/// Explicit token-distance semantics, independent of engine-specific slop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Predicate {
    /// Query order, with at most this many intervening tokens in total.
    /// Zero means adjacent whole lexemes (punctuation is not a token).
    Ordered {
        /// Total unmatched positions permitted between the matched endpoints.
        intervening: u16,
    },
    /// Any order, preserving query multiplicities, in at most `tokens` positions.
    Unordered {
        /// Inclusive token-span ceiling, including all intervening positions.
        tokens: u16,
    },
}

/// A verified witness; byte offsets are half-open in the supplied original text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Witness {
    /// First original UTF-8 byte of the first matched lexeme.
    pub start: usize,
    /// Byte after the last matched lexeme.
    pub end: usize,
    /// Zero-based first token position, inclusive.
    pub first_token: usize,
    /// Zero-based last token position, inclusive.
    pub last_token: usize,
}

/// A first witness proves the predicate; absence is distinct from a quota stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Earliest ending witness; shortest span among witnesses ending there.
    Found(Witness),
    /// The complete supplied source was examined without a witness.
    Absent,
    /// A byte or token quota prevented a complete decision.
    Limited,
}

/// Per-call ceilings. Callers may share remaining allowances across regions.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum original source bytes examined, including punctuation/whitespace.
    pub bytes: usize,
    /// Maximum complete lexemes admitted to positional comparison.
    pub tokens: usize,
}

/// The allowance that actually stopped positional verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// Original source scan bytes.
    Bytes,
    /// Complete source lexemes compared.
    Tokens,
    /// Verified witnesses retained.
    Witnesses,
}

/// Counts include work done before an absence, witness, or quota stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verification {
    /// Actual exhausted allowance, if scanning was quota-stopped.
    pub stopped_by: Option<LimitKind>,
    /// Proven witness, proven absence, or incomplete verification.
    pub outcome: Outcome,
    /// Original bytes examined; never exceeds the supplied ceiling.
    pub bytes_examined: usize,
    /// Complete lexemes compared; never exceeds the supplied ceiling.
    pub tokens_examined: usize,
}

/// Bounded witnesses in increasing end-position order. This is not every
/// combinatorial alignment: only the shortest witness for each matching end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matches {
    /// Actual exhausted allowance, if enumeration is incomplete.
    pub stopped_by: Option<LimitKind>,
    /// Original coordinates, including overlapping witnesses.
    pub witnesses: Vec<Witness>,
    /// True only when the entire supplied field was scanned without any cap.
    pub complete: bool,
    /// Original source bytes examined once by the streaming scan.
    pub bytes_examined: usize,
    /// Original complete lexemes admitted to comparison.
    pub tokens_examined: usize,
}

/// Compiled whole-lexeme query. Repeated terms remain separate requirements.
pub struct PositionalQuery {
    terms: Vec<String>,
    predicate: Predicate,
}

#[derive(Clone, Copy)]
struct Start {
    token: usize,
    byte: usize,
}

enum State {
    Ordered(Vec<Option<Start>>),
    Unordered {
        required: BTreeMap<String, usize>,
        counts: BTreeMap<String, usize>,
        queue: VecDeque<(usize, String, usize)>,
    },
}

impl PositionalQuery {
    /// Compile the explicit positional route, or return `None` for other modes.
    /// Positional intent always uses whole lexemes and all query positions;
    /// discovery analyzer/channel/Boolean options do not broaden this predicate.
    /// # Errors
    /// Invalid positional query or distance/window.
    pub fn for_retrieval(
        query: &str,
        options: &graph_search_types::RetrievalOptions,
    ) -> Result<Option<Self>> {
        use graph_search_types::ExploreMode;
        let predicate = match options.mode {
            ExploreMode::Phrase => Predicate::Ordered {
                intervening: options.phrase_gap,
            },
            ExploreMode::Near => Predicate::Unordered {
                tokens: options.near_window,
            },
            _ => return Ok(None),
        };
        Self::new(query, predicate).map(Some)
    }

    /// Compile without dropping stopwords or duplicate query positions.
    /// # Errors
    /// Empty/punctuation-only input, query byte/token overflow, or a distance
    /// over 4096. An unordered window must fit all required query positions.
    pub fn new(query: &str, predicate: Predicate) -> Result<Self> {
        crate::work::validate_query_bytes("positional query", query)?;
        let terms = crate::analyzer::whole_terms(query);
        if terms.is_empty() || terms.len() > graph_search_types::limits::MAX_QUERY_TERMS {
            return Err(Error::InvalidQuery(
                "positional queries require 1..128 whole lexemes".into(),
            ));
        }
        let distance = match predicate {
            Predicate::Ordered { intervening } => intervening,
            Predicate::Unordered { tokens } => {
                if usize::from(tokens) < terms.len() {
                    return Err(Error::InvalidQuery(
                        "unordered window is shorter than the query".into(),
                    ));
                }
                tokens
            }
        };
        if distance > 4096 {
            return Err(Error::InvalidQuery(
                "positional distance exceeds 4096 tokens".into(),
            ));
        }
        Ok(Self { terms, predicate })
    }

    /// Distinct whole terms for a necessary posting filter. Verification still
    /// uses the original ordered/multiset query including repeated positions.
    #[must_use]
    pub fn candidate_terms(&self) -> Vec<String> {
        self.terms
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Verify original text without allocating a full source token stream.
    /// Callers serving indexed evidence must first verify the source hash and
    /// supply only the intended field/region; matching never crosses that slice.
    /// `check` is called initially, at lexeme boundaries, and within 1024 scanned
    /// bytes (plus at most one UTF-8 scalar) for cancellation/deadline checks.
    /// # Errors
    /// Propagates an error from the caller's cooperative check.
    pub fn verify(
        &self,
        text: &str,
        limits: Limits,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Verification> {
        self.scan(text, limits, check, |_| false)
    }

    /// Enumerate shortest witnesses ending at matching source tokens in one
    /// scan, preserving overlaps. The first omitted witness proves truncation;
    /// exactly filling the result cap does not by itself mark an incomplete scan.
    /// # Errors
    /// Witness limits above 10,000, or an error from the cooperative check.
    pub fn verify_all(
        &self,
        text: &str,
        limits: Limits,
        max_witnesses: usize,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Matches> {
        if max_witnesses > 10_000 {
            return Err(Error::InvalidQuery(
                "positional witness limit exceeds 10000".into(),
            ));
        }
        let mut witnesses = Vec::new();
        let report = self.scan(text, limits, check, |witness| {
            if witnesses.len() == max_witnesses {
                return false;
            }
            witnesses.push(witness);
            true
        })?;
        Ok(Matches {
            stopped_by: if matches!(report.outcome, Outcome::Found(_)) {
                Some(LimitKind::Witnesses)
            } else {
                report.stopped_by
            },
            witnesses,
            complete: report.outcome == Outcome::Absent,
            bytes_examined: report.bytes_examined,
            tokens_examined: report.tokens_examined,
        })
    }

    fn scan(
        &self,
        text: &str,
        limits: Limits,
        mut check: impl FnMut() -> Result<()>,
        mut emit: impl FnMut(Witness) -> bool,
    ) -> Result<Verification> {
        check()?;
        let mut state = match self.predicate {
            Predicate::Ordered { .. } => State::Ordered(vec![None; self.terms.len()]),
            Predicate::Unordered { .. } => {
                let mut required: BTreeMap<String, usize> = BTreeMap::new();
                for term in &self.terms {
                    let count = required.entry(term.clone()).or_insert(0);
                    *count = count.saturating_add(1);
                }
                State::Unordered {
                    required,
                    counts: BTreeMap::new(),
                    queue: VecDeque::new(),
                }
            }
        };
        let mut report = Verification {
            stopped_by: None,
            outcome: Outcome::Absent,
            bytes_examined: 0,
            tokens_examined: 0,
        };
        let mut from = None;
        let mut checkpoint = 0usize;
        for (offset, character) in text.char_indices() {
            if character.len_utf8() > limits.bytes.saturating_sub(report.bytes_examined) {
                report.outcome = Outcome::Limited;
                report.stopped_by = Some(LimitKind::Bytes);
                return Ok(report);
            }
            report.bytes_examined = report.bytes_examined.saturating_add(character.len_utf8());
            if report.bytes_examined.saturating_sub(checkpoint) >= 1024 {
                check()?;
                checkpoint = report.bytes_examined;
            }
            if character.is_whitespace() || (character.is_ascii_punctuation() && character != '_') {
                if let Some(start) = from.take() {
                    check()?;
                    if !self.admit(text, start..offset, limits, &mut state, &mut report) {
                        match report.outcome {
                            Outcome::Found(witness) if emit(witness) => {
                                report.outcome = Outcome::Absent;
                            }
                            _ => return Ok(report),
                        }
                    }
                }
            } else {
                from.get_or_insert(offset);
            }
        }
        if let Some(start) = from {
            check()?;
            self.admit(text, start..text.len(), limits, &mut state, &mut report);
            if let Outcome::Found(witness) = report.outcome
                && emit(witness)
            {
                report.outcome = Outcome::Absent;
            }
        }
        Ok(report)
    }

    fn admit(
        &self,
        text: &str,
        span: std::ops::Range<usize>,
        limits: Limits,
        state: &mut State,
        report: &mut Verification,
    ) -> bool {
        let (start, end) = (span.start, span.end);
        if report.tokens_examined >= limits.tokens {
            report.outcome = Outcome::Limited;
            report.stopped_by = Some(LimitKind::Tokens);
            return false;
        }
        let token = report.tokens_examined;
        report.tokens_examined = report.tokens_examined.saturating_add(1);
        let word = text[start..end].to_lowercase();
        let beginning = match state {
            State::Ordered(prefixes) => {
                // Descending updates forbid one occurrence satisfying repeated
                // query positions. A later start dominates an earlier start for
                // every future end under a total intervening-token predicate.
                for position in (0..self.terms.len()).rev() {
                    if self.terms[position] == word {
                        if position == 0 {
                            prefixes[0] = Some(Start { token, byte: start });
                        } else if let Some(previous) = prefixes[position.saturating_sub(1)] {
                            prefixes[position] = Some(previous);
                        }
                    }
                }
                let Predicate::Ordered { intervening } = self.predicate else {
                    unreachable!()
                };
                prefixes.last().copied().flatten().filter(|begin| {
                    self.terms.last() == Some(&word)
                        && token.saturating_sub(begin.token)
                            < self.terms.len().saturating_add(usize::from(intervening))
                })
            }
            State::Unordered {
                required,
                counts,
                queue,
            } => {
                let Predicate::Unordered { tokens } = self.predicate else {
                    unreachable!()
                };
                while queue.front().is_some_and(|(first, _, _)| {
                    token.saturating_sub(*first) >= usize::from(tokens)
                }) {
                    if let Some((_, term, _)) = queue.pop_front()
                        && let Some(count) = counts.get_mut(&term)
                    {
                        *count = count.saturating_sub(1);
                    }
                }
                let required_end = required.contains_key(&word);
                if required_end {
                    let count = counts.entry(word.clone()).or_insert(0);
                    *count = count.saturating_add(1);
                    queue.push_back((token, word, start));
                }
                if required_end
                    && required
                        .iter()
                        .all(|(term, needed)| counts.get(term).copied().unwrap_or(0) >= *needed)
                {
                    while queue
                        .front()
                        .is_some_and(|(_, term, _)| counts[term] > required[term])
                    {
                        if let Some((_, term, _)) = queue.pop_front()
                            && let Some(count) = counts.get_mut(&term)
                        {
                            *count = count.saturating_sub(1);
                        }
                    }
                    queue.front().map(|&(token, _, byte)| Start { token, byte })
                } else {
                    None
                }
            }
        };
        if let Some(begin) = beginning {
            report.outcome = Outcome::Found(Witness {
                start: begin.byte,
                end,
                first_token: begin.token,
                last_token: token,
            });
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;

    fn run(query: &str, predicate: Predicate, text: &str) -> Verification {
        PositionalQuery::new(query, predicate)
            .unwrap()
            .verify(
                text,
                Limits {
                    bytes: text.len(),
                    tokens: usize::MAX,
                },
                || Ok(()),
            )
            .unwrap()
    }

    fn oracle(query: &[&str], predicate: Predicate, words: &[&str]) -> Outcome {
        for last in 0..words.len() {
            for first in (0..=last).rev() {
                let slice = &words[first..=last];
                let matches = match predicate {
                    Predicate::Ordered { intervening } => {
                        let mut position = 0;
                        for word in slice {
                            if query.get(position) == Some(word) {
                                position += 1;
                            }
                        }
                        position == query.len()
                            && slice.len() <= query.len() + usize::from(intervening)
                    }
                    Predicate::Unordered { tokens } => {
                        slice.len() <= usize::from(tokens)
                            && query.iter().all(|term| {
                                slice.iter().filter(|word| *word == term).count()
                                    >= query.iter().filter(|word| *word == term).count()
                            })
                    }
                };
                if matches {
                    return Outcome::Found(Witness {
                        start: first * 2,
                        end: last * 2 + 1,
                        first_token: first,
                        last_token: last,
                    });
                }
            }
        }
        Outcome::Absent
    }

    #[test]
    fn streaming_alignment_matches_exhaustive_interval_oracle() {
        for length in 0..=7u32 {
            for mut value in 0..3usize.pow(length) {
                let mut words = Vec::new();
                for _ in 0..length {
                    words.push(["a", "b", "c"][value % 3]);
                    value /= 3;
                }
                let text = words.join(" ");
                for query in ["a", "a a", "a b", "a b a", "b a b"] {
                    let terms: Vec<_> = query.split_whitespace().collect();
                    for predicate in [
                        Predicate::Ordered { intervening: 0 },
                        Predicate::Ordered { intervening: 1 },
                        Predicate::Ordered { intervening: 3 },
                        Predicate::Unordered { tokens: 3 },
                        Predicate::Unordered { tokens: 5 },
                    ] {
                        assert_eq!(
                            run(query, predicate, &text).outcome,
                            oracle(&terms, predicate, &words),
                            "{query:?} {predicate:?} {text:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn overlapping_enumeration_matches_exhaustive_endpoint_oracle_and_exact_caps() {
        for length in 0..=6u32 {
            for mut value in 0..3usize.pow(length) {
                let mut words = Vec::new();
                for _ in 0..length {
                    words.push(["a", "b", "c"][value % 3]);
                    value /= 3;
                }
                let text = words.join(" ");
                for query in ["a", "a a", "a b", "a b a", "b a b"] {
                    let terms: Vec<_> = query.split_whitespace().collect();
                    for predicate in [
                        Predicate::Ordered { intervening: 0 },
                        Predicate::Ordered { intervening: 2 },
                        Predicate::Unordered { tokens: 4 },
                    ] {
                        let mut expected = Vec::new();
                        for last in 0..words.len() {
                            if !terms.contains(&words[last]) {
                                continue;
                            }
                            for first in (0..=last).rev() {
                                let slice = &words[first..=last];
                                let valid = match predicate {
                                    Predicate::Ordered { intervening } => {
                                        let mut matched = 0;
                                        for word in &slice[..slice.len() - 1] {
                                            if matched + 1 < terms.len() && terms[matched] == *word
                                            {
                                                matched += 1;
                                            }
                                        }
                                        words[last] == terms[terms.len() - 1]
                                            && matched + 1 == terms.len()
                                            && slice.len() <= terms.len() + usize::from(intervening)
                                    }
                                    Predicate::Unordered { tokens } => {
                                        slice.len() <= usize::from(tokens)
                                            && terms.iter().all(|term| {
                                                slice.iter().filter(|word| *word == term).count()
                                                    >= terms
                                                        .iter()
                                                        .filter(|word| *word == term)
                                                        .count()
                                            })
                                    }
                                };
                                if valid {
                                    expected.push(Witness {
                                        start: first * 2,
                                        end: last * 2 + 1,
                                        first_token: first,
                                        last_token: last,
                                    });
                                    break;
                                }
                            }
                        }
                        let compiled = PositionalQuery::new(query, predicate).unwrap();
                        for cap in [0, 1, 2, 100] {
                            let found = compiled
                                .verify_all(
                                    &text,
                                    Limits {
                                        bytes: text.len(),
                                        tokens: words.len(),
                                    },
                                    cap,
                                    || Ok(()),
                                )
                                .unwrap();
                            assert_eq!(
                                found.witnesses,
                                expected.iter().take(cap).copied().collect::<Vec<_>>(),
                                "{query:?} {predicate:?} {text:?} cap {cap}"
                            );
                            assert_eq!(found.complete, expected.len() <= cap);
                            assert!(found.bytes_examined <= text.len());
                            assert!(found.tokens_examined <= words.len());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn enumeration_preserves_confirmed_prefix_on_quota_exhaustion() {
        let query = PositionalQuery::new("a a", Predicate::Ordered { intervening: 0 }).unwrap();
        let result = query
            .verify_all(
                "a a a a",
                Limits {
                    bytes: 7,
                    tokens: 3,
                },
                10,
                || Ok(()),
            )
            .unwrap();
        assert!(!result.complete);
        assert_eq!(result.witnesses.len(), 2);
        assert_eq!(result.witnesses[0].start, 0);
        assert_eq!(result.witnesses[1].start, 2);
        assert!(
            query
                .verify_all(
                    "a a",
                    Limits {
                        bytes: 3,
                        tokens: 2
                    },
                    10_001,
                    || Ok(())
                )
                .is_err()
        );
    }

    #[test]
    fn original_unicode_offsets_and_whole_lexeme_semantics_survive() {
        let adjacent = Predicate::Ordered { intervening: 0 };
        let text = "prefix İ_NAME::ÉCOLE\r\nend";
        let Outcome::Found(witness) = run("i\u{307}_name école", adjacent, text).outcome else {
            panic!("expected a witness")
        };
        assert_eq!(&text[witness.start..witness.end], "İ_NAME::ÉCOLE");
        assert_eq!(
            run("cache invalidate", adjacent, "cacheInvalidate").outcome,
            Outcome::Absent
        );
        assert_eq!(run("a a", adjacent, "a").outcome, Outcome::Absent);
        assert!(matches!(
            run("is a", adjacent, "IS\r\nA").outcome,
            Outcome::Found(_)
        ));
        assert_eq!(run("é", adjacent, "e\u{301}").outcome, Outcome::Absent);
        assert_eq!(run("ss", adjacent, "ß").outcome, Outcome::Absent);
    }

    #[test]
    fn quotas_and_cooperative_checks_never_claim_false_absence() {
        let query = PositionalQuery::new("a b", Predicate::Ordered { intervening: 0 }).unwrap();
        for bytes in 0..=7 {
            for tokens in 0..=4 {
                let report = query
                    .verify("é a b", Limits { bytes, tokens }, || Ok(()))
                    .unwrap();
                assert!(report.bytes_examined <= bytes);
                assert!(report.tokens_examined <= tokens);
                assert_eq!(
                    matches!(report.outcome, Outcome::Found(_)),
                    bytes >= 6 && tokens >= 3
                );
                assert_ne!(report.outcome, Outcome::Absent);
            }
        }
        let mut checks = 0;
        let error = query
            .verify(
                &".".repeat(5000),
                Limits {
                    bytes: 5000,
                    tokens: 10,
                },
                || {
                    checks += 1;
                    if checks == 3 {
                        Err(Error::QueryCancelled)
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
        assert!(matches!(error, Error::QueryCancelled));
        assert_eq!(checks, 3);
        assert_eq!(
            query
                .verify(
                    "",
                    Limits {
                        bytes: 0,
                        tokens: 0
                    },
                    || Ok(())
                )
                .unwrap()
                .outcome,
            Outcome::Absent
        );
    }

    #[test]
    fn distance_and_repetition_boundaries_are_inclusive_and_source_bounded() {
        let text = format!("a {}b", "x ".repeat(4096));
        assert!(matches!(
            run("a b", Predicate::Ordered { intervening: 4096 }, &text).outcome,
            Outcome::Found(_)
        ));
        assert_eq!(
            run("a b", Predicate::Ordered { intervening: 4095 }, &text).outcome,
            Outcome::Absent
        );
        assert_eq!(
            run("a b", Predicate::Unordered { tokens: 4096 }, &text).outcome,
            Outcome::Absent
        );
        let query = "a ".repeat(128);
        for predicate in [
            Predicate::Ordered { intervening: 0 },
            Predicate::Unordered { tokens: 128 },
        ] {
            assert_eq!(
                run(&query, predicate, &"a ".repeat(127)).outcome,
                Outcome::Absent
            );
            assert!(matches!(
                run(&query, predicate, &"a ".repeat(128)).outcome,
                Outcome::Found(_)
            ));
        }
    }

    #[test]
    fn compilation_rejects_unsupported_or_impossible_contracts() {
        assert!(PositionalQuery::new("::", Predicate::Ordered { intervening: 0 }).is_err());
        assert!(PositionalQuery::new("a b", Predicate::Unordered { tokens: 1 }).is_err());
        assert!(PositionalQuery::new("a", Predicate::Ordered { intervening: 4097 }).is_err());
        assert!(
            PositionalQuery::new(&"a ".repeat(129), Predicate::Ordered { intervening: 0 }).is_err()
        );
    }
}
