//! Read-only corpus probe for strict explore source capture; stores live in temp.
use graph_search::{Index, OpenOptions, Verification};
use graph_search_types::{ExploreQuery, Stats};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: source_capture_probe ROOT QUERIES_JSON".into());
    }
    let root = PathBuf::from(&args[1]);
    let queries: Vec<String> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let temporary = tempfile::tempdir()?;
    let index = Index::open(OpenOptions {
        root: root.clone(),
        store: Some(temporary.path().join("index")),
        verification: Verification::Content,
        ..Default::default()
    })?;
    index.reindex()?;
    let mut rows = Vec::new();
    for repeat in 0..3 {
        for query in &queries {
            let mut request = ExploreQuery::new(query);
            request.k = 8;
            request.hops = 1;
            request.max_bytes = 16_384;
            let mut result = index.search().explore(&request)?;
            let stats = result.stats;
            let mut raw = BTreeMap::new();
            let mut delivered = BTreeSet::new();
            for item in &result.items {
                if item.snippet.is_none() && item.excerpts.is_empty() {
                    continue;
                }
                let path = &item.node.path;
                if !raw.contains_key(path) {
                    raw.insert(path.clone(), std::fs::read(root.join(path))?);
                }
                let source = &raw[path];
                let hash = graph_search_core::hash::content_hash(source);
                let lines: Vec<_> = std::str::from_utf8(source)?.lines().collect();
                for snippet in item
                    .snippet
                    .iter()
                    .chain(item.excerpts.iter().map(|e| &e.snippet))
                {
                    assert_eq!(snippet.source_hash, hash);
                    for (offset, text) in snippet.lines.iter().enumerate() {
                        let line = snippet.start_line as usize + offset;
                        assert_eq!(lines.get(line - 1), Some(&text.as_str()));
                        assert!(
                            delivered.insert((path.clone(), hash.clone(), line)),
                            "duplicate source line"
                        );
                    }
                }
            }
            result.stats = Stats::default();
            result.context.generation = None;
            rows.push(json!({"query": query, "repeat": repeat, "stats": stats,
                "source_valid": true, "delivered": delivered,
                "result_sha256_without_stats_generation": graph_search_core::hash::content_hash(&serde_json::to_vec(&result)?)}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
