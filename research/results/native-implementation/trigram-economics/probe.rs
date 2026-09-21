//! Native gram-admission economics. A research prototype, not a production index.
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    hint::black_box,
    path::Path,
    time::Instant,
};

#[derive(Default)]
struct Grams {
    lists: BTreeMap<u32, Vec<usize>>,
    by_file: BTreeMap<usize, Vec<u32>>,
}
fn grams(bytes: &[u8]) -> Vec<u32> {
    let mut out: Vec<_> = bytes
        .windows(3)
        .map(|b| u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
impl Grams {
    fn replace(&mut self, id: usize, text: Option<&str>) {
        if let Some(old) = self.by_file.remove(&id) {
            for gram in old {
                let list = self.lists.get_mut(&gram).unwrap();
                let at = list.binary_search(&id).unwrap();
                list.remove(at);
                if list.is_empty() {
                    self.lists.remove(&gram);
                }
            }
        }
        if let Some(text) = text {
            let unique = grams(text.as_bytes());
            for &gram in &unique {
                let list = self.lists.entry(gram).or_default();
                let at = list.binary_search(&id).unwrap_err();
                list.insert(at, id);
            }
            self.by_file.insert(id, unique);
        }
    }
    fn invalidate(&mut self, id: usize) {
        self.replace(id, None);
    }
    fn new(texts: &[String]) -> Self {
        let mut result = Self::default();
        for (id, text) in texts.iter().enumerate() {
            result.replace(id, Some(text));
        }
        result
    }
    fn candidates(&self, pattern: &str, ignore_case: bool, live: &[usize]) -> Vec<usize> {
        if pattern.len() < 3 || ignore_case {
            return live.to_vec();
        }
        let unique = grams(pattern.as_bytes());
        let mut lists: Vec<_> = unique.iter().filter_map(|g| self.lists.get(g)).collect();
        // A live file with no complete facts must scan, including new files.
        let mut found: BTreeSet<_> = live
            .iter()
            .copied()
            .filter(|id| !self.by_file.contains_key(id))
            .collect();
        if lists.len() == unique.len() {
            lists.sort_by_key(|list| list.len());
            if let Some(first) = lists.first() {
                for &id in first.iter() {
                    if lists
                        .iter()
                        .skip(1)
                        .all(|list| list.binary_search(&id).is_ok())
                    {
                        found.insert(id);
                    }
                }
            }
        }
        live.iter()
            .copied()
            .filter(|id| found.contains(id))
            .collect()
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Hits {
    lines: Vec<(usize, usize)>,
    omitted: bool,
}
fn scan(
    texts: &[String],
    candidates: &[usize],
    pattern: &str,
    ignore_case: bool,
    cap: usize,
) -> Hits {
    assert!(!pattern.is_empty() && !pattern.contains(['\n', '\r']));
    let needle = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_owned()
    };
    let mut hits = Hits {
        lines: Vec::new(),
        omitted: false,
    };
    for &id in candidates {
        for (line, text) in texts[id].lines().enumerate() {
            let matched = if ignore_case {
                text.to_lowercase().contains(&needle)
            } else {
                text.contains(&needle)
            };
            if matched {
                if hits.lines.len() == cap {
                    hits.omitted = true;
                    return hits;
                }
                hits.lines.push((id, line + 1));
            }
        }
    }
    hits
}
fn median(mut run: impl FnMut(), repeats: usize) -> f64 {
    run();
    let mut samples = Vec::new();
    for _ in 0..repeats {
        let start = Instant::now();
        run();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}
fn main() {
    let root = std::env::args().nth(1).expect("repository root");
    let root = Path::new(&root);
    let entries =
        graph_search_core::walk::walk(root, &graph_search_core::config::WalkPolicy::default())
            .unwrap();
    let mut files = Vec::new();
    for entry in entries {
        let bytes = std::fs::read(&entry.path).unwrap();
        if bytes.contains(&0) {
            continue;
        }
        if let Ok(text) = String::from_utf8(bytes) {
            files.push((entry.rel, text));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let paths: Vec<_> = files.iter().map(|(path, _)| path.clone()).collect();
    let texts: Vec<_> = files.into_iter().map(|(_, text)| text).collect();
    assert!(!texts.is_empty());
    let live: Vec<_> = (0..texts.len()).collect();
    let build_ms = median(
        || {
            black_box(Grams::new(black_box(&texts)));
        },
        5,
    );
    let index = Grams::new(&texts);
    let mut patterns: Vec<(String, bool)> = [
        "return",
        "const",
        "fn",
        "é",
        "unlikely_literal_78219_not_in_source",
    ]
    .into_iter()
    .map(|q| (q.into(), false))
    .collect();
    patterns.extend([("HTTP".into(), true), ("İ".into(), true)]);
    let mut selected = BTreeSet::new();
    for text in &texts {
        if let Some(word) = text
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .find(|word| (16..=48).contains(&word.len()) && !selected.contains(*word))
        {
            selected.insert(word.to_owned());
            patterns.push((word.to_owned(), false));
        }
        if selected.len() == 8 {
            break;
        }
    }
    let verification_ms = median(
        || {
            for path in &paths {
                black_box(graph_search_core::hash::content_hash(
                    &std::fs::read(root.join(path)).unwrap(),
                ));
            }
        },
        5,
    );
    let mut rows = Vec::new();
    for (pattern, folded) in patterns {
        let candidates = index.candidates(&pattern, folded, &live);
        for cap in [0, 1, 100, usize::MAX] {
            assert_eq!(
                scan(&texts, &live, &pattern, folded, cap),
                scan(&texts, &candidates, &pattern, folded, cap)
            );
        }
        let direct_ms = median(
            || {
                black_box(scan(black_box(&texts), &live, &pattern, folded, 100));
            },
            21,
        );
        let filtered_ms = median(
            || {
                let ids = index.candidates(&pattern, folded, &live);
                black_box(scan(black_box(&texts), &ids, &pattern, folded, 100));
            },
            21,
        );
        let mut native_query = graph_search_types::query::TextQuery::new(&pattern).with_limit(100);
        native_query.ignore_case = folded;
        let policy = graph_search_core::config::WalkPolicy::default();
        let native_direct_ms = median(
            || {
                black_box(
                    graph_search_core::text_search::search_text(root, &native_query, &policy)
                        .unwrap(),
                );
            },
            5,
        );
        let native =
            graph_search_core::text_search::search_text(root, &native_query, &policy).unwrap();
        let saving = direct_ms - filtered_ms;
        rows.push(json!({"pattern":pattern,"ignore_case":folded,"candidate_files":candidates.len(),"candidate_bytes":candidates.iter().map(|&i|texts[i].len()).sum::<usize>(),"production_direct_ms":native_direct_ms,"production_source_bytes":native.stats.source_bytes_read,"production_files_attempted":native.stats.source_files_attempted,"production_hits":native.items.len(),"production_truncations":native.truncations,"direct_resident_ms":direct_ms,"filtered_resident_ms":filtered_ms,"initial_build_break_even_queries":if saving>0.0 {Some((build_ms/saving).ceil())} else {None},"equivalent_at_caps":[0,1,100,"unlimited"]}));
    }
    // Replace one median-size file repeatedly; include remove/add posting maintenance.
    let mut sizes = live.clone();
    sizes.sort_by_key(|&id| texts[id].len());
    let changed = sizes[sizes.len() / 2];
    let replacement = format!("{}\nchanged_native_trigram_probe_48291\n", texts[changed]);
    let mut mutable = Grams::new(&texts);
    let update_roundtrip_ms = median(
        || {
            mutable.replace(changed, Some(&replacement));
            mutable.replace(changed, Some(&texts[changed]));
        },
        9,
    );
    mutable.invalidate(changed);
    assert!(
        mutable
            .candidates("changed_native_trigram_probe_48291", false, &live)
            .contains(&changed)
    );
    let source_bytes: usize = texts.iter().map(String::len).sum();
    let postings: usize = index.lists.values().map(Vec::len).sum();
    println!(
        "{}",
        json!({"scope":"resident immutable UTF-8/NUL-free admitted snapshot; std literal verification, not production memmem timing; no persisted index or end-to-end speed claim","files":texts.len(),"source_bytes":source_bytes,"source_hashes":paths.iter().zip(&texts).map(|(p,t)|(p,graph_search_core::hash::content_hash(t.as_bytes()))).collect::<BTreeMap<_,_>>(),"distinct_grams":index.lists.len(),"posting_entries":postings,"logical_forward_bytes_u32_docs":index.lists.len()*4+postings*4,"logical_reverse_bytes_u32_grams":postings*4,"resident_allocation_overhead_included":false,"build_ms":build_ms,"median_file_bytes":texts[changed].len(),"update_restore_ms":update_roundtrip_ms,"strict_read_hash_all_files_ms":verification_ms,"strict_verification_bytes_per_request":source_bytes,"queries":rows})
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gram_filter_is_necessary_not_sufficient_and_survives_mutations() {
        let mut texts = vec![
            "abcd".into(),
            "abc---bcd".into(),
            "İSTANBUL café".into(),
            "ABC".into(),
        ];
        let mut index = Grams::new(&texts);
        let mut live = vec![0, 1, 2, 3];
        assert_eq!(index.candidates("abcd", false, &live), vec![0, 1]);
        for operation in 0..5 {
            if operation == 1 {
                texts[0] = "new needle".into();
                index.invalidate(0);
            }
            if operation == 2 {
                index.replace(0, Some(&texts[0]));
            }
            if operation == 3 {
                texts[1].clear();
                index.replace(1, None);
            }
            if operation == 4 {
                texts.push("brand_new_needle".into());
                live.push(4);
            }
            for q in [
                "brand_new_needle",
                "abcd",
                "café",
                "İSTANBUL",
                "new needle",
                "a",
                "é",
                "i\u{307}stanbul",
                "ABC",
                "missing",
            ] {
                for folded in [false, true] {
                    for cap in [0, 1, 100] {
                        assert_eq!(
                            scan(&texts, &live, q, folded, cap),
                            scan(&texts, &index.candidates(q, folded, &live), q, folded, cap)
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn exhaustive_small_sources_admit_every_literal_match() {
        let mut texts = Vec::new();
        for code in 0..4096usize {
            let mut text = String::new();
            let mut value = code;
            for _ in 0..6 {
                text.push(['a', 'b', 'C', '\n'][value % 4]);
                value /= 4;
            }
            texts.push(text);
        }
        let index = Grams::new(&texts);
        let live: Vec<_> = (0..texts.len()).collect();
        for code in 0..81usize {
            let mut q = String::new();
            let mut value = code;
            for _ in 0..4 {
                q.push(['a', 'b', 'C'][value % 3]);
                value /= 3;
            }
            let candidates = index.candidates(&q, false, &live);
            assert_eq!(
                scan(&texts, &live, &q, false, usize::MAX),
                scan(&texts, &candidates, &q, false, usize::MAX)
            );
        }
    }
}
