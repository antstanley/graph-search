//! Native positional-route workload characterization, not relevance/task-success evaluation.
//! All stores are temporary; source repositories and CodeGraph indexes are read-only.
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::{config::WalkPolicy, hash::content_hash, walk::walk};
use graph_search_types::{ExploreMode, ExploreQuery};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

// Independent batch tokenization and bounded interval oracle. No production
// analyzer, positional verifier, posting filter, or owner mapping is called here.
fn tokens(text: &str) -> Vec<(String, usize, usize)> {
    let mut result = Vec::new();
    let mut start = None;
    for (at, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_whitespace() || (ch.is_ascii_punctuation() && ch != '_') {
            if let Some(from) = start.take() {
                result.push((text[from..at].to_lowercase(), from, at));
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    result
}
fn oracle(words: &[(String, usize, usize)], query: &str, near: bool) -> BTreeSet<(usize, usize)> {
    let required: Vec<_> = tokens(query).into_iter().map(|(word, _, _)| word).collect();
    let mut answer = BTreeSet::new();
    for end in 0..words.len() {
        if near {
            // Latest possible start gives the shortest witness for this end.
            for start in (end.saturating_sub(7)..=end).rev() {
                let mut remaining = required.clone();
                for (word, _, _) in &words[start..=end] {
                    if let Some(at) = remaining.iter().position(|term| term == word) {
                        remaining.remove(at);
                    }
                }
                if remaining.is_empty() {
                    // A witness ends at a required term; trailing unrelated
                    // tokens do not create additional production witnesses.
                    if required.contains(&words[end].0) {
                        answer.insert((words[start].1, words[end].2));
                    }
                    break;
                }
            }
        } else if end + 1 >= required.len() {
            let start = end + 1 - required.len();
            if words[start..=end]
                .iter()
                .map(|(word, _, _)| word)
                .eq(required.iter())
            {
                answer.insert((words[start].1, words[end].2));
            }
        }
    }
    answer
}
fn main() {
    let root_arg = std::env::args().nth(1).expect("repository root");
    let root = Path::new(&root_arg).canonicalize().unwrap();
    assert!(
        !root.join(".graph-search/config.toml").exists(),
        "probe requires default policy"
    );
    let queries = [
        ("return None", false),
        ("not found", false),
        ("pub fn", false),
        ("throw new Error", false),
        ("export async function", false),
        ("request response", true),
    ];
    let store = tempfile::tempdir().unwrap();
    let index = Index::open(OpenOptions {
        root: root.clone(),
        store: Some(store.path().join("native-index")),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap();
    let build = Instant::now();
    let sync = index.reindex().unwrap();
    let build_ms = build.elapsed().as_secs_f64() * 1000.0;
    let entries = walk(&root, &WalkPolicy::default()).unwrap();
    let mut hashes = BTreeMap::new();
    let mut truths = vec![BTreeMap::new(); queries.len()];
    let mut source_bytes = 0usize;
    let mut total_tokens = 0usize;
    let mut skipped = 0usize;
    for entry in entries {
        let bytes = std::fs::read(&entry.path).unwrap();
        if bytes.contains(&0) {
            skipped += 1;
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            skipped += 1;
            continue;
        };
        source_bytes += bytes.len();
        hashes.insert(entry.rel.clone(), content_hash(&bytes));
        let words = tokens(text);
        total_tokens += words.len();
        for (i, (query, near)) in queries.iter().enumerate() {
            let witnesses = oracle(&words, query, *near);
            if !witnesses.is_empty() {
                truths[i].insert(entry.rel.clone(), witnesses);
            }
        }
    }
    let mut rows = Vec::new();
    for (i, (text, near)) in queries.iter().enumerate() {
        let mut query = ExploreQuery::new(*text).with_context_lines(0);
        query.k = 500;
        query.retrieval.mode = if *near {
            ExploreMode::Near
        } else {
            ExploreMode::Phrase
        };
        let truth = &truths[i];
        index.search().explore(&query).unwrap(); // warmup
        let mut times = Vec::new();
        let mut stable = None;
        let mut last = None;
        for _ in 0..5 {
            let started = Instant::now();
            let result = index.search().explore(&query).unwrap();
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            let mut signature = Vec::new();
            for hit in &result.items {
                let Some(evidence) = &hit.evidence else {
                    continue;
                };
                assert_eq!(Some(&evidence.source_hash), hashes.get(&hit.node.path));
                assert!(
                    truth
                        .get(&hit.node.path)
                        .is_some_and(|spans| spans.contains(&(
                            evidence.span.start_byte as usize,
                            evidence.span.end_byte as usize
                        ))),
                    "unverified witness: {} {} {:?}",
                    text,
                    hit.node.path,
                    evidence.span
                );
                signature.push((hit.node.id.clone(), evidence.span));
            }
            if let Some(previous) = &stable {
                assert_eq!(previous, &signature);
            }
            stable = Some(signature);
            last = Some(result);
        }
        let result = last.unwrap();
        let returned: BTreeSet<_> = result
            .items
            .iter()
            .filter(|hit| hit.evidence.is_some())
            .map(|hit| hit.node.path.clone())
            .collect();
        let missing: Vec<_> = truth
            .keys()
            .filter(|path| !returned.contains(*path))
            .cloned()
            .collect();
        if result.truncations.is_empty() {
            assert!(
                missing.is_empty(),
                "unreported omission: {text} {missing:?}"
            );
        }
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        rows.push(
            json!({"query":text,"mode":if *near {"near"} else {"phrase"},
            "near_window":if *near {Some(8)} else {None},"k":500,"context_lines":0,
            "warm_ms":times,"median_ms":sorted[2],"oracle_files":truth.len(),
            "oracle_witnesses":truth.values().map(BTreeSet::len).sum::<usize>(),
            "returned_files":returned.len(),"returned_owners":result.items.len(),
            "verified_evidence":stable.unwrap().len(),"missing_files":missing,
            "truncations":result.truncations,"stats":result.stats}),
        );
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "root":root,"purpose":"correctness and work characterization; not task relevance or model success",
        "policy":"default; no repository graph-search config; temporary native store",
        "build_ms":build_ms,"sync":sync,"utf8_files":hashes.len(),"source_bytes":source_bytes,
        "source_manifest_sha256":content_hash(serde_json::to_string(&hashes).unwrap().as_bytes()),
        "oracle_tokens":total_tokens,"skipped_binary_or_non_utf8":skipped,
        "position_offset_payload_estimate_bytes":total_tokens * 12,
        "estimate_contract":"three u32 values per token (position,start,end); excludes term dictionary, file headers, compression and update costs; not an implemented codec",
        "queries":rows
    })).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_oracle_has_raw_offsets_and_multiset_windows() {
        let text = "İ_NAME::ÉCOLE\r\na a a\n";
        let words = tokens(text);
        assert_eq!(
            oracle(&words, "i\u{307}_name école", false),
            BTreeSet::from([(0, 15)])
        );
        assert_eq!(oracle(&words, "a a", false).len(), 2);
        assert!(oracle(&tokens("a x"), "a a", true).is_empty());
        assert_eq!(
            oracle(&tokens("b x a z"), "a b", true),
            BTreeSet::from([(0, 5)])
        );
        assert!(oracle(&tokens("a x x x x x x x b"), "a b", true).is_empty());
        assert!(oracle(&tokens("cacheInvalidate"), "cache invalidate", false).is_empty());
    }
}
