//! Identifier-aware metadata retrieval, independent of the graph storage adapter.
//!
//! BM25 uses name/path/signature weights 8/2/1 and k1=1.2, b=0.75,
//! matching the research FTS5 metadata experiment. Exact names have a separate
//! priority lane. The index is rebuilt from the current snapshot, so it cannot
//! become stale independently of graph queries.
#![allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects)]
use graph_search_types::Node;
use std::collections::{BTreeMap, BTreeSet};

const STOPWORDS: &[&str] = &[
    "how", "does", "the", "a", "an", "is", "are", "in", "to", "of", "and", "where", "what", "for",
    "with",
];

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

struct Document {
    frequency: BTreeMap<String, f32>,
    length: usize,
}

/// Snapshot-local lexical statistics. Document order matches the input nodes.
pub struct LexicalIndex {
    documents: Vec<Document>,
    frequency: BTreeMap<String, usize>,
    average_length: f32,
}

impl LexicalIndex {
    /// Build a metadata index over symbol nodes.
    #[must_use]
    pub fn new(nodes: &[Node]) -> Self {
        let mut frequency = BTreeMap::new();
        let documents: Vec<_> = nodes
            .iter()
            .map(|node| {
                let mut doc = Document {
                    frequency: BTreeMap::new(),
                    length: 0,
                };
                for (field, weight) in [
                    (node.name.as_deref().unwrap_or_default(), 8.0),
                    (node.path.as_str(), 2.0),
                    (node.signature.as_deref().unwrap_or_default(), 1.0),
                ] {
                    for term in tokens(field) {
                        if STOPWORDS.contains(&term.as_str()) {
                            continue;
                        }
                        doc.length += 1;
                        *doc.frequency.entry(term).or_insert(0.0) += weight;
                    }
                }
                for term in doc.frequency.keys() {
                    *frequency.entry(term.clone()).or_insert(0) += 1;
                }
                doc
            })
            .collect();
        let average_length = (documents.iter().map(|d| d.length).sum::<usize>() as f32
            / documents.len().max(1) as f32)
            .max(1.0);
        Self {
            documents,
            frequency,
            average_length,
        }
    }

    /// Nonnegative BM25 relevance. Zero means no indexed token matched.
    #[must_use]
    pub fn score(&self, document: usize, terms: &[String]) -> f32 {
        let doc = &self.documents[document];
        terms
            .iter()
            .map(|term| {
                let Some(&tf) = doc.frequency.get(term) else {
                    return 0.0;
                };
                let df = self.frequency[term] as f32;
                let count = self.documents.len() as f32;
                let idf = ((count - df + 0.5) / (df + 0.5)).ln().max(0.000_001);
                idf * tf * 2.2
                    / (tf + 1.2 * (0.25 + 0.75 * doc.length as f32 / self.average_length))
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
