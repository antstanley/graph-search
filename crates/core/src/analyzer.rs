//! Native identifier analysis. Whole lexemes and split terms are separate
//! representations; aliases must not become extra sequential phrase positions.
//!
//! Whole lexemes end at Unicode whitespace or ASCII punctuation other than `_`.
//! Non-ASCII spelling (including combining marks) is preserved without Unicode
//! normalization or confusable substitution. This is a retrieval boundary rule,
//! not a programming-language identifier validator. Lowercasing uses Rust's
//! Unicode lowercase mapping; it is not full case folding.

/// One original whole lexeme, with half-open original UTF-8 byte coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lexeme<'a> {
    /// Original source spelling.
    pub text: &'a str,
    /// Inclusive original byte offset.
    pub start: usize,
    /// Exclusive original byte offset.
    pub end: usize,
}

/// Whole words suitable for identifier evidence, retaining original offsets.
#[must_use]
pub fn lexemes(text: &str) -> Vec<Lexeme<'_>> {
    let mut out = Vec::new();
    let mut start = None;
    for (offset, c) in text.char_indices() {
        let delimiter = c.is_whitespace() || (c.is_ascii_punctuation() && c != '_');
        if delimiter {
            if let Some(from) = start.take() {
                out.push(Lexeme {
                    text: &text[from..offset],
                    start: from,
                    end: offset,
                });
            }
        } else if start.is_none() {
            start = Some(offset);
        }
    }
    if let Some(from) = start {
        out.push(Lexeme {
            text: &text[from..],
            start: from,
            end: text.len(),
        });
    }
    out
}

/// Lowercased whole lexemes, preserving underscores and combining sequences.
/// Original spelling and offsets remain available from [`lexemes`].
#[must_use]
pub fn whole_terms(text: &str) -> Vec<String> {
    lexemes(text)
        .into_iter()
        .map(|word| word.text.to_lowercase())
        .collect()
}

/// Distinct whole query lexemes, with sentence function words removed only
/// from multiword discovery. Single-word names such as `is` remain searchable.
#[must_use]
pub fn query_terms(query: &str) -> Vec<String> {
    let sentence = query.split_whitespace().count() > 1;
    whole_terms(query)
        .into_iter()
        .filter(|word| !sentence || !crate::lexical::is_stopword(word))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Query-local membership masks; one bit per distinct analyzed query term.
/// This matches the public 128-term query bound and is never persisted.
pub(crate) type MatchLines = std::collections::BTreeMap<u32, u128>;

pub(crate) fn matching_lines(
    unit: &graph_search_types::source::SourceUnit,
    terms: &[String],
    whole: bool,
) -> crate::Result<MatchLines> {
    use std::collections::BTreeMap;
    if terms.len() > u128::BITS as usize {
        return Err(crate::Error::InvalidQuery(
            "source match analysis supports at most 128 query terms".into(),
        ));
    }
    let masks: BTreeMap<_, _> = terms
        .iter()
        .zip(0..u128::BITS)
        .map(|(term, bit)| (term.as_str(), 1u128.checked_shl(bit).unwrap_or(0)))
        .collect();
    let mut matched = MatchLines::new();
    for (&term, &mask) in &masks {
        for &line in unit.terms.get(term).into_iter().flatten() {
            *matched.entry(line).or_default() |= mask;
        }
    }
    if whole {
        for (spelling, lines) in &unit.identifiers {
            if let Some(&mask) = masks.get(spelling.to_lowercase().as_str()) {
                for &line in lines {
                    *matched.entry(line).or_default() |= mask;
                }
            }
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_spelling_and_original_offsets_survive_analysis() {
        let source = "HTTP2Response::get_HTTPResponse --cache-key e\u{301}cole ÉCOLE İß ΑΒΓ\r\n";
        let words = lexemes(source);
        assert_eq!(
            words.iter().map(|w| w.text).collect::<Vec<_>>(),
            [
                "HTTP2Response",
                "get_HTTPResponse",
                "cache",
                "key",
                "e\u{301}cole",
                "ÉCOLE",
                "İß",
                "ΑΒΓ"
            ]
        );
        for word in &words {
            assert_eq!(&source[word.start..word.end], word.text);
        }
        assert_eq!(
            whole_terms(source),
            [
                "http2response",
                "get_httpresponse",
                "cache",
                "key",
                "e\u{301}cole",
                "école",
                "i\u{307}ß",
                "αβγ"
            ]
        );
        assert_ne!(whole_terms("école"), whole_terms("e\u{301}cole"));
        assert_ne!(whole_terms("ß"), whole_terms("ss"));
        assert_ne!(whole_terms("a"), whole_terms("а")); // Latin vs Cyrillic.
    }

    #[test]
    fn long_names_and_non_ascii_boundaries_are_explicit() {
        let name = "Δ_name9".repeat(1000);
        let words = lexemes(&name);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, name);
        assert_eq!(
            whole_terms("cache—key cache\u{2003}key"),
            ["cache—key", "cache", "key"]
        );
        assert!(lexemes(" :: --\r\n").is_empty());
    }
    #[test]
    fn match_masks_preserve_distinct_terms_without_counting_aliases_twice() {
        let facts = crate::units::extract(
            "a.txt",
            "cache cache CACHE\r\ncache rebuild\r\ngetHTTPResponse\r\n",
            "hash",
            graph_search_types::Language::Unknown,
            &[],
        );
        let terms = vec!["cache".into(), "rebuild".into(), "gethttpresponse".into()];
        let split = matching_lines(&facts.units[0], &terms, false).unwrap();
        assert_eq!(split, std::collections::BTreeMap::from([(1, 1), (2, 3)]));
        let whole = matching_lines(&facts.units[0], &terms, true).unwrap();
        assert_eq!(
            whole,
            std::collections::BTreeMap::from([(1, 1), (2, 3), (3, 4)])
        );
        assert_eq!(whole[&1].count_ones(), 1);
        assert_eq!(whole[&2].count_ones(), 2);
    }

    #[test]
    fn match_masks_cover_the_entire_query_bound_and_reject_overflow() {
        let mut terms: Vec<_> = (0..128).map(|n| format!("term{n}")).collect();
        let facts = crate::units::extract(
            "a.txt",
            &terms.join(" "),
            "hash",
            graph_search_types::Language::Unknown,
            &[],
        );
        assert_eq!(
            matching_lines(&facts.units[0], &terms, true).unwrap()[&1],
            u128::MAX
        );
        terms.push("overflow".into());
        assert!(matches!(
            matching_lines(&facts.units[0], &terms, true),
            Err(crate::Error::InvalidQuery(_))
        ));
    }
}
