//! Identifier-aware metadata and bounded body retrieval, independent of storage.
//!
//! BM25 uses name/path/signature weights 8/2/1 and k1=1.2, b=0.75,
//! matching the research FTS5 metadata experiment. Bounded function/method body
//! term counts use a separate BM25 length normalization at weight 1. Exact names
//! have a separate priority lane. Both fields are built from the same snapshot;
//! body counts are cached with extraction facts and never read from stale offsets.
#![allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects)]
use graph_search_types::{Node, NodeKind};
use serde::{Deserialize, Serialize};
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

/// Reserved, versioned attribute holding normalized body terms, never source text.
pub const BODY_TERMS_ATTRIBUTE: &str = "graph_search.body_terms.v1";
/// Maximum number of function/method source lines indexed for body relevance.
pub const BODY_LINE_CAP: usize = 64;
/// Maximum number of Unicode scalar values indexed from one function/method body.
pub const BODY_CHAR_CAP: usize = 4096;

#[derive(Default, Serialize, Deserialize)]
struct BodyTerms {
    terms: BTreeMap<String, u32>,
    truncated: bool,
}

/// Attach bounded body term frequencies to extracted facts before they are cached.
/// Invalid spans are ignored; raw source bodies are not persisted.
pub fn attach_body_terms(extraction: &mut crate::extraction::Extraction, source: &str) {
    for fact in &mut extraction.symbols {
        if !matches!(fact.kind, NodeKind::Function | NodeKind::Method) {
            continue;
        }
        let Some(body) = source.get(fact.span.start_byte as usize..fact.span.end_byte as usize)
        else {
            continue;
        };
        let mut characters = body
            .lines()
            .take(BODY_LINE_CAP)
            .flat_map(|line| line.chars().chain(std::iter::once('\n')));
        let bounded: String = characters.by_ref().take(BODY_CHAR_CAP).collect();
        let truncated = characters.next().is_some() || body.lines().nth(BODY_LINE_CAP).is_some();
        let mut terms = BTreeMap::new();
        for term in tokens(&bounded) {
            if !STOPWORDS.contains(&term.as_str()) {
                let count = terms.entry(term).or_insert(0u32);
                *count = count.saturating_add(1);
            }
        }
        if let Ok(encoded) = serde_json::to_string(&BodyTerms { terms, truncated }) {
            fact.attributes
                .insert(BODY_TERMS_ATTRIBUTE.to_owned(), encoded);
        }
    }
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
    bodies: Option<Box<Self>>,
}

impl LexicalIndex {
    /// Build metadata and bounded-body indexes over the same symbol nodes.
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
        let bodies = Self::body_index(nodes);
        Self {
            documents,
            frequency,
            average_length,
            bodies: Some(Box::new(bodies)),
        }
    }

    fn body_index(nodes: &[Node]) -> Self {
        let mut frequency = BTreeMap::new();
        let documents: Vec<_> = nodes
            .iter()
            .map(|node| {
                let body = node
                    .attribute(BODY_TERMS_ATTRIBUTE)
                    .and_then(|encoded| serde_json::from_str::<BodyTerms>(encoded).ok())
                    .unwrap_or_default();
                let length = body.terms.values().map(|n| *n as usize).sum();
                let terms = body
                    .terms
                    .into_iter()
                    .map(|(term, count)| {
                        *frequency.entry(term.clone()).or_insert(0) += 1;
                        (term, count as f32)
                    })
                    .collect();
                Document {
                    frequency: terms,
                    length,
                }
            })
            .collect();
        let average_length = (documents.iter().map(|d| d.length).sum::<usize>() as f32
            / documents.len().max(1) as f32)
            .max(1.0);
        Self {
            documents,
            frequency,
            average_length,
            bodies: None,
        }
    }

    /// Nonnegative BM25 relevance. Zero means no indexed token matched.
    #[must_use]
    pub fn score(&self, document: usize, terms: &[String]) -> f32 {
        self.metadata_score(document, terms)
            + self
                .bodies
                .as_ref()
                .map_or(0.0, |bodies| bodies.metadata_score(document, terms))
    }

    fn metadata_score(&self, document: usize, terms: &[String]) -> f32 {
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
