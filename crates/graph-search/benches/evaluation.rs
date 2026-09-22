//! Criterion benchmarks for the independent-evaluation findings
//! (`research/12-independent-evaluation-response.md`).
//!
//! These are deliberately separate from `search.rs` (the release gate's frozen
//! suite): the corpus here exercises the shapes the two independent evaluations
//! probed — a type used as a parameter/return (finding E5), a struct field type
//! (E1), a `#[cfg(test)]` module (finding E3) and a published store re-open
//! (finding P0.1). A change that regresses those findings moves these numbers
//! while leaving the release gate untouched.
//!
//! ```sh
//! cargo bench -p graph-search --bench evaluation
//! ```

// Benchmarks are fixture code; a setup failure must abort the measurement and
// the generated corpus uses plain arithmetic/formatting by design.
#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::cast_possible_truncation
)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use graph_search::{Index, OpenOptions};
use graph_search_types::ExploreDetail;
use graph_search_types::query::{ExploreQuery, RefQuery, TextQuery, TraversalQuery};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Files generated per corpus.
const FILES: usize = 150;

struct Corpus {
    _root: tempfile::TempDir,
    path: PathBuf,
}

fn corpus() -> &'static Corpus {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let root = tempfile::tempdir().expect("benchmark corpus directory");
        write_corpus(root.path());
        Corpus {
            path: root.path().to_path_buf(),
            _root: root,
        }
    })
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("benchmark corpus directory");
    }
    std::fs::write(path, text).expect("benchmark corpus file");
}

/// One file per index: a struct used as a field, parameter and return type, a
/// receiver-variable method call, and a `#[cfg(test)]` module whose functions
/// must not outrank production code in `explore`.
fn write_corpus(root: &Path) {
    for file in 0..FILES {
        let text = format!(
            "/// Handles request {file} for module {file}.\npub struct Thing{file} {{ pub value: usize }}\npub struct Holder{file} {{ pub inner: Thing{file} }}\n\nimpl Thing{file} {{\n    pub fn run(&self) -> usize {{ self.compute() }}\n    fn compute(&self) -> usize {{ self.value }}\n}}\n\npub fn use_thing_{file}(t: &Thing{file}) -> Thing{file} {{\n    let local: Thing{file} = Thing{file} {{ value: t.value }};\n    local\n}}\n\npub fn call_thing_{file}() -> usize {{\n    let t = Thing{file} {{ value: {file} }};\n    t.run()\n}}\n\n#[cfg(test)]\nmod tests {{\n    use super::*;\n    #[test]\n    fn thing_{file}_works() {{ assert_eq!(call_thing_{file}(), {file}); }}\n}}\n",
        );
        write(&root.join(format!("src/module_{file}.rs")), &text);
        write(
            &root.join(format!("docs/guide_{file}.md")),
            &format!("# Guide {file}\n\nThe thing{file} handler runs its request.\n\n"),
        );
    }
}

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..OpenOptions::default()
    })
    .expect("benchmark index opens")
}

fn benches(c: &mut Criterion) {
    let corpus = corpus();
    let root = corpus.path.as_path();

    // A published generation that the open benchmark re-attaches to, the shape
    // the one-shot CLI pays on every invocation (finding P0.1).
    let store = tempfile::tempdir().expect("benchmark store directory");
    let index = open(root, store.path());
    index.reindex().expect("benchmark corpus indexes");
    drop(index);

    let mut open_group = c.benchmark_group("open");
    open_group.sample_size(20);
    open_group.bench_function("published_store", |b| {
        b.iter(|| black_box(open(root, store.path())));
    });
    open_group.finish();

    let index = open(root, store.path());
    let service = index.search();

    // E1/E5: a type used through a field, a parameter and a return value.
    let mut graph = c.benchmark_group("graph");
    graph.bench_function("refs_type", |b| {
        b.iter(|| {
            black_box(
                service
                    .refs(&RefQuery::new("Thing70"))
                    .expect("type references"),
            );
        });
    });
    graph.bench_function("impact_type", |b| {
        b.iter(|| {
            black_box(
                service
                    .impact(&TraversalQuery::new("Thing70", 2))
                    .expect("type impact"),
            );
        });
    });
    graph.finish();

    // E2: compact vs full per-seed evidence, and the `text` search explore
    // replaces. Each benchmark id carries the serialized byte count measured at
    // setup, so the context-cost regression is visible in the output itself; the
    // timing tracks the work that produces those bytes.
    let mut payload = c.benchmark_group("payload");
    let text_query = TextQuery::new("Thing70");
    let full_query = ExploreQuery::new("Thing70 run request handler");
    let compact_query = full_query.clone().with_detail(ExploreDetail::Compact);
    let text_bytes = serde_json::to_vec(&service.text(&text_query).expect("text search"))
        .expect("serialize")
        .len();
    let full_bytes = serde_json::to_vec(&service.explore(&full_query).expect("full explore"))
        .expect("serialize")
        .len();
    let compact_bytes =
        serde_json::to_vec(&service.explore(&compact_query).expect("compact explore"))
            .expect("serialize")
            .len();
    payload.bench_function(
        BenchmarkId::new("explore_compact", format!("{compact_bytes}B")),
        |b| {
            b.iter(|| {
                let result = service.explore(&compact_query).expect("compact explore");
                black_box(serde_json::to_vec(&result).expect("serialize").len());
            });
        },
    );
    payload.bench_function(
        BenchmarkId::new("explore_full", format!("{full_bytes}B")),
        |b| {
            b.iter(|| {
                let result = service.explore(&full_query).expect("full explore");
                black_box(serde_json::to_vec(&result).expect("serialize").len());
            });
        },
    );
    payload.bench_function(
        BenchmarkId::new("text_search", format!("{text_bytes}B")),
        |b| {
            b.iter(|| {
                let result = service.text(&text_query).expect("text search");
                black_box(serde_json::to_vec(&result).expect("serialize").len());
            });
        },
    );
    payload.finish();
}

criterion_group!(evaluation, benches);
criterion_main!(evaluation);
