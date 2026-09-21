//! Factorial metadata scoring experiment; no production ranking configuration changes.
use graph_search_core::{
    config::WalkPolicy,
    lexical::{LexicalIndex, exact_query, query_terms, tokens},
    memory::MemoryStore,
    ports::{GraphStore, ListRegistry},
    reconcile::Projector,
};
use graph_search_types::Node;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
const STOP: &[&str] = &[
    "how", "does", "the", "a", "an", "is", "are", "in", "to", "of", "and", "where", "what", "for",
    "with",
];
const WEIGHTS: [f32; 4] = [8.0, 2.0, 1.0, 4.0];
struct Document {
    lengths: [usize; 4],
    terms: BTreeMap<String, [u32; 4]>,
}
struct Corpus {
    documents: Vec<Document>,
    df: BTreeMap<String, usize>,
    average: f32,
    averages: [f32; 4],
}
impl Corpus {
    fn new(nodes: &[Node], whole: bool, qualified: bool) -> Self {
        let documents: Vec<_> = nodes
            .iter()
            .map(|node| {
                let mut lengths = [0usize; 4];
                let mut terms: BTreeMap<String, [u32; 4]> = BTreeMap::new();
                for (field, text) in [
                    node.name.as_deref().unwrap_or(""),
                    node.path.as_str(),
                    node.signature.as_deref().unwrap_or(""),
                    node.qualified_name
                        .as_deref()
                        .filter(|name| qualified && Some(*name) != node.name.as_deref())
                        .unwrap_or(""),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut split = BTreeMap::<String, u32>::new();
                    let mut original = BTreeMap::<String, u32>::new();
                    for term in tokens(text)
                        .into_iter()
                        .filter(|term| !STOP.contains(&term.as_str()))
                    {
                        *split.entry(term).or_default() += 1;
                    }
                    if whole {
                        for term in graph_search_core::analyzer::whole_terms(text) {
                            *original.entry(term).or_default() += 1;
                        }
                    }
                    lengths[field] = split
                        .values()
                        .map(|n| *n as usize)
                        .sum::<usize>()
                        .max(original.values().map(|n| *n as usize).sum());
                    for (term, count) in split {
                        terms.entry(term).or_default()[field] = count;
                    }
                    for (term, count) in original {
                        let n = &mut terms.entry(term).or_default()[field];
                        *n = (*n).max(count);
                    }
                }
                Document { lengths, terms }
            })
            .collect();
        let mut df = BTreeMap::new();
        let mut total = [0usize; 4];
        for doc in &documents {
            for term in doc.terms.keys() {
                *df.entry(term.clone()).or_default() += 1;
            }
            for (sum, length) in total.iter_mut().zip(doc.lengths) {
                *sum += length;
            }
        }
        let n = documents.len().max(1) as f32;
        Self {
            average: (total.iter().sum::<usize>() as f32 / n).max(1.0),
            averages: total.map(|nfield| (nfield as f32 / n).max(1.0)),
            documents,
            df,
        }
    }
    fn score(&self, ordinal: usize, terms: &[String], positive: bool, normalization: &str) -> f32 {
        let doc = &self.documents[ordinal];
        terms
            .iter()
            .map(|term| {
                let Some(tf) = doc.terms.get(term) else {
                    return 0.0;
                };
                let df = self.df[term] as f32;
                let ratio = (self.documents.len() as f32 - df + 0.5) / (df + 0.5);
                let idf = if positive {
                    (1.0 + ratio).ln()
                } else {
                    ratio.ln().max(0.000_001)
                };
                let weighted: f32 = tf.iter().zip(WEIGHTS).map(|(&n, w)| n as f32 * w).sum();
                match normalization {
                    "combined" => {
                        idf * weighted * 2.2
                            / (weighted
                                + 1.2
                                    * (0.25
                                        + 0.75 * doc.lengths.iter().sum::<usize>() as f32
                                            / self.average))
                    }
                    "independent" => (0..4)
                        .map(|field| {
                            let f = tf[field] as f32;
                            idf * WEIGHTS[field] * f * 2.2
                                / (f + 1.2
                                    * (0.25
                                        + 0.75 * doc.lengths[field] as f32 / self.averages[field]))
                        })
                        .sum(),
                    "bm25f" => {
                        let normalized: f32 = (0..4)
                            .map(|field| {
                                WEIGHTS[field] * tf[field] as f32
                                    / (0.25
                                        + 0.75 * doc.lengths[field] as f32 / self.averages[field])
                            })
                            .sum();
                        idf * normalized * 2.2 / (normalized + 1.2)
                    }
                    _ => panic!("unknown normalization"),
                }
            })
            .sum()
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    assert!(!root.join(".graph-search/config.toml").exists());
    let tasks: Value = serde_json::from_slice(&std::fs::read(&args[2]).unwrap()).unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(root, &mut store)
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let mut nodes: Vec<_> = snapshot
        .all_nodes()
        .unwrap()
        .into_iter()
        .filter(|node| !node.is_file())
        .collect();
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
    let mut rows = Vec::new();
    let mut checks = 0usize;
    for (whole, qualified) in [(false, false), (true, false), (false, true), (true, true)] {
        let corpus = Corpus::new(&nodes, whole, qualified);
        let native = LexicalIndex::with_fields(&nodes, whole, qualified);
        for task in tasks.as_array().unwrap() {
            let query = task["prompt"].as_str().unwrap();
            let terms = if whole {
                graph_search_core::analyzer::query_terms(query)
            } else {
                query_terms(query)
            };
            let exact = exact_query(query);
            for ordinal in 0..nodes.len() {
                assert_eq!(
                    corpus.score(ordinal, &terms, false, "combined").to_bits(),
                    native.score(ordinal, &terms).to_bits(),
                    "baseline mismatch {ordinal}"
                );
                assert_eq!(
                    corpus.score(ordinal, &terms, false, "bm25f").to_bits(),
                    native
                        .score_with_normalization(
                            ordinal,
                            &terms,
                            graph_search_types::FieldNormalization::Bm25f
                        )
                        .to_bits(),
                    "native BM25F mismatch {ordinal}"
                );
                checks += 2;
            }
            let mut variants = Vec::new();
            for positive in [false, true] {
                for normalization in ["combined", "independent", "bm25f"] {
                    // Representation controls change only the representation.
                    if (whole || qualified) && (positive || normalization != "combined") {
                        continue;
                    }
                    let mut ranked: Vec<_> = nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(ordinal, node)| {
                            let raw = corpus.score(ordinal, &terms, positive, normalization);
                            let is_exact = [node.name.as_deref(), node.qualified_name.as_deref()]
                                .into_iter()
                                .flatten()
                                .any(|name| name.to_lowercase() == exact);
                            let score = if is_exact { 2.0 } else { raw / (1.0 + raw) };
                            (score > 0.0).then_some((ordinal, score, is_exact))
                        })
                        .collect();
                    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                    if !positive
                        && matches!(normalization, "combined" | "bm25f")
                        && whole == qualified
                    {
                        let mut work = graph_search_core::work::WorkBudget::new(
                            graph_search_core::work::WorkLimits {
                                candidates: 100_000,
                                postings: 1_000_000,
                                ..Default::default()
                            },
                        );
                        let production = snapshot
                            .metadata()
                            .search_top_with_policy(
                                &terms,
                                &exact,
                                &graph_search_core::metadata::CompiledFilters::default(),
                                &mut work,
                                50,
                                1,
                                if whole {
                                    graph_search_types::AnalysisMode::Identifiers
                                } else {
                                    graph_search_types::AnalysisMode::Split
                                },
                                if normalization == "bm25f" {
                                    graph_search_types::FieldNormalization::Bm25f
                                } else {
                                    graph_search_types::FieldNormalization::Combined
                                },
                            )
                            .unwrap();
                        assert!(
                            work.report().2.is_empty(),
                            "baseline verification hit work caps"
                        );
                        let expected: Vec<_> = ranked
                            .iter()
                            .take(50)
                            .map(|&(i, s, _)| (nodes[i].id.clone(), s.to_bits()))
                            .collect();
                        let actual: Vec<_> = production
                            .hits
                            .iter()
                            .map(|hit| (hit.item.id.clone(), hit.score.to_bits()))
                            .collect();
                        assert_eq!(actual, expected, "production metadata top-50 mismatch");
                    }
                    variants.push(json!({"whole":whole,"qualified":qualified,"idf":if positive {"positive"} else {"clipped"},
                        "normalization":normalization,"candidates":ranked.len(),"top":ranked.iter().take(50).map(|&(ordinal,score,exact)| {
                            let node = &nodes[ordinal]; json!({"id":node.id,"path":node.path,"span":node.span,"score":score,"exact":exact})
                        }).collect::<Vec<_>>()}));
                }
            }
            rows.push(json!({"task":task["id"],"whole":whole,"qualified":qualified,"terms":terms,
                "average_length":corpus.average,"field_averages":corpus.averages,"variants":variants}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&json!({"root":root,"symbols":nodes.len(),
        "native_bit_exact_checks":checks,"k1":1.2,"b":0.75,"weights":WEIGHTS,
        "field_population":"all symbols including missing fields as zero; means floored at 1; global document DF held fixed within representation",
        "scope":"metadata candidate ordering only; no body fusion, context delivery, latency or model-success claim",
        "rows":rows})).unwrap());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factorial_scores_are_finite_and_baseline_is_native() {
        let nodes: Vec<_> = (0..10)
            .map(|i| Node {
                name: Some(if i < 8 { "common" } else { "rare" }.into()),
                path: format!("src/{i}.rs"),
                signature: Some("fn common(value: Value)".into()),
                ..Node::default()
            })
            .collect();
        let c = Corpus::new(&nodes, false, false);
        let native = LexicalIndex::new(&nodes);
        let terms = query_terms("common value");
        for i in 0..nodes.len() {
            assert_eq!(
                c.score(i, &terms, false, "combined").to_bits(),
                native.score(i, &terms).to_bits()
            );
            assert!(c.score(i, &terms, true, "combined") > c.score(i, &terms, false, "combined"));
            for n in ["combined", "independent", "bm25f"] {
                assert!(c.score(i, &terms, true, n).is_finite());
            }
        }
        let empty = Corpus::new(&[], false, false);
        assert_eq!(empty.average, 1.0);
    }
}
