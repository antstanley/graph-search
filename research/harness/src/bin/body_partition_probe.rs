//! Isolate corpus-statistic effects of Markdown partitioning without tuning production.
use graph_search_core::{
    body::BodyIndex,
    config::WalkPolicy,
    lexical::query_terms,
    memory::MemoryStore,
    metadata::CompiledFilters,
    ports::{GraphStore, ListRegistry},
    reconcile::Projector,
    work::{WorkBudget, WorkLimits},
};
use graph_search_types::{
    Language,
    source::{SourceFileUnits, SourceUnit, SourceUnitKind},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
type Corpus = BTreeMap<String, SourceFileUnits>;
struct Stats {
    n: usize,
    average: f32,
    df: BTreeMap<String, usize>,
}
fn stats(files: &Corpus) -> Stats {
    let mut n = 0;
    let mut total = 0;
    let mut df = BTreeMap::new();
    for file in files.values() {
        for unit in &file.units {
            let length: usize = unit.terms.values().map(Vec::len).sum();
            if length == 0 {
                continue;
            }
            n += 1;
            total += length;
            for term in unit.terms.keys() {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }
    }
    Stats {
        n,
        average: (total as f32 / n.max(1) as f32).max(1.0),
        df,
    }
}
fn score(unit: &SourceUnit, terms: &[String], weights: &Stats, average: f32) -> f32 {
    let length = unit.terms.values().map(Vec::len).sum::<usize>() as f32;
    let mut result = 0.0;
    for term in terms {
        let tf = unit.terms.get(term).map_or(0, Vec::len) as f32;
        if tf == 0.0 {
            continue;
        }
        let df = weights.df.get(term).copied().unwrap_or(0) as f32;
        let idf = (1.0 + (weights.n as f32 - df + 0.5) / (df + 0.5)).ln();
        result += idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / average));
    }
    result
}
fn ranking(
    files: &Corpus,
    query: &[String],
    weights: &Stats,
    average: f32,
) -> Vec<(String, usize, f32)> {
    let physical = stats(files);
    let mut terms = query.to_vec();
    terms.sort();
    terms.dedup();
    terms.sort_by_key(|term| physical.df.get(term).copied().unwrap_or(0));
    let mut best = BTreeMap::new();
    for (path, file) in files {
        for (ordinal, unit) in file.units.iter().enumerate() {
            let value = score(unit, &terms, weights, average);
            if value <= 0.0 {
                continue;
            }
            let entry = best
                .entry((path.clone(), unit.owner.clone()))
                .or_insert((ordinal, value));
            if value > entry.1 {
                *entry = (ordinal, value);
            }
        }
    }
    let mut rows: Vec<_> = best
        .into_iter()
        .map(|((path, _), (unit, score))| (path, unit, score))
        .collect();
    rows.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    rows
}
fn describe(files: &Corpus, rows: &[(String, usize, f32)]) -> Vec<Value> {
    rows.iter()
        .take(20)
        .map(|(path, ordinal, score)| {
            let unit = &files[path].units[*ordinal];
            json!({"path":path,"unit":ordinal,"owner":unit.owner,"span":unit.span,"score":score,
            "term_count":unit.terms.values().map(Vec::len).sum::<usize>()})
        })
        .collect()
}
fn verify(files: &Corpus, terms: &[String], expected: &[(String, usize, f32)]) {
    let mut work = WorkBudget::new(WorkLimits {
        candidates: 100_000,
        postings: 1_000_000,
        ..Default::default()
    });
    let (native, _) = BodyIndex::new(files)
        .search(
            terms,
            &CompiledFilters::default(),
            |_| None,
            &BTreeSet::new(),
            &mut work,
            usize::MAX,
        )
        .unwrap();
    assert!(work.report().2.is_empty());
    assert_eq!(native.len(), expected.len());
    for (native, (path, unit, score)) in native.iter().zip(expected) {
        assert_eq!((&native.path, native.unit), (path, *unit));
        assert!(
            (native.score - score).abs() <= 0.0001,
            "{} vs {}",
            native.score,
            score
        );
    }
}
fn link_diagnostics(root: &Path, files: &Corpus) -> Value {
    let mut records = 0;
    let mut spans = BTreeSet::new();
    let mut linked_files = 0;
    let mut partial_files = 0;
    for (path, file) in files {
        partial_files += usize::from(file.units.iter().any(|unit| unit.links_truncated));
        if !file.units.iter().any(|unit| !unit.links.is_empty()) {
            continue;
        }
        linked_files += 1;
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(
            graph_search_core::hash::content_hash(text.as_bytes()),
            file.source_hash
        );
        for link in file.units.iter().flat_map(|unit| &unit.links) {
            records += 1;
            spans.insert((path, link.span.start_byte, link.span.end_byte));
            for span in [
                Some(link.span),
                Some(link.label_span),
                Some(link.destination_span),
                link.title_span,
            ]
            .into_iter()
            .flatten()
            {
                assert!(
                    text.get(span.start_byte as usize..span.end_byte as usize)
                        .is_some()
                );
                assert_eq!(span.start_line, span.end_line);
            }
        }
    }
    json!({"records":records,"distinct_spans":spans.len(),"files_with_links":linked_files,
        "partial_files":partial_files,"all_spans_address_current_source":true})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let query = &args[2];
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let mut store = MemoryStore::new();
    Projector::new(&registry, &policy)
        .reindex(root, &mut store)
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let current = snapshot.source_files();
    let mut legacy = current.clone();
    for (path, file) in &mut legacy {
        if file.units.iter().any(|unit| {
            matches!(
                unit.kind,
                SourceUnitKind::Markdown
                    | SourceUnitKind::MarkdownCodeFence
                    | SourceUnitKind::MarkdownFrontmatter
                    | SourceUnitKind::MarkdownTable
                    | SourceUnitKind::MarkdownParagraph
                    | SourceUnitKind::MarkdownListItem
                    | SourceUnitKind::MarkdownOpaque
            )
        }) {
            let text = std::fs::read_to_string(root.join(path)).unwrap();
            assert_eq!(
                graph_search_core::hash::content_hash(text.as_bytes()),
                file.source_hash
            );
            *file = graph_search_core::units::extract(
                "legacy.txt",
                &text,
                &file.source_hash,
                Language::Unknown,
                &[],
            );
        }
    }
    let old = stats(&legacy);
    let new = stats(current);
    let terms = query_terms(query);
    let before = ranking(&legacy, &terms, &old, old.average);
    let after = ranking(current, &terms, &new, new.average);
    verify(&legacy, &terms, &before);
    verify(current, &terms, &after);
    println!("{}", serde_json::to_string_pretty(&json!({"query":query,
        "method":"same native facts/extractor; Markdown-only legacy windows; independent exhaustive scorer verified against both native indexes; no production tuning",
        "legacy":{"documents":old.n,"average_length":old.average,"top":describe(&legacy,&before)},
        "structured":{"documents":new.n,"average_length":new.average,"top":describe(current,&after)},
        "link_metadata":link_diagnostics(root,current),
        "structured_old_idf":{"top":describe(current,&ranking(current,&terms,&old,new.average))},
        "structured_old_average":{"top":describe(current,&ranking(current,&terms,&new,old.average))},
        "structured_old_statistics":{"top":describe(current,&ranking(current,&terms,&old,old.average))},
        "query_df":terms.iter().map(|term| json!({"term":term,"legacy":old.df.get(term),"structured":new.df.get(term)})).collect::<Vec<_>>()
    })).unwrap());
}
