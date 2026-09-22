//! Criterion benchmarks for the public library path.
//!
//! The corpus is generated deterministically into a temporary directory, so the
//! measurements describe this implementation on a fixed synthetic workload
//! rather than any external repository. `research/scripts/release_gate.py` runs
//! these benchmarks and turns the sampled distributions into p50/p95/p99 gates.
//!
//! ```sh
//! cargo bench -p graph-search --bench search
//! ```

// Benchmarks are fixture code: a setup failure must abort the measurement, the
// generated corpus deliberately uses plain arithmetic/formatting, and the long
// group function is the benchmark definition itself.
#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::cast_possible_truncation
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use graph_search::{Index, OpenOptions};
use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
use graph_search_types::query::GraphFilters;
use graph_search_types::{
    ExploreQuery, FilesQuery, RefQuery, RetrievalOptions, SymbolQuery, TextQuery,
};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Files generated per corpus; sized so one cold index stays a few seconds.
const FILES: usize = 200;
/// Functions emitted per generated file.
const FUNCTIONS: usize = 24;

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

/// Deterministic pseudo-random values without a dependency.
fn noise(seed: u64, len: usize) -> String {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            char::from(b'a' + u8::try_from((state >> 33) % 26).unwrap_or(0))
        })
        .collect()
}

fn write_corpus(root: &Path) {
    for file in 0..FILES {
        let mut text = String::new();
        for function in 0..FUNCTIONS {
            text.push_str(&format!(
                "/// Handles request {function} for module {file}.\npub fn handler_{file}_{function}(input: &str) -> usize {{\n    let needle = \"corpusNeedle{file}_{function}\";\n    if input.contains(needle) {{ return needle.len(); }}\n    auditor_{file}_{}(input)\n}}\n\n",
                function.saturating_sub(1),
            ));
            if function == 0 {
                text.push_str(&format!(
                    "fn auditor_{file}_0(input: &str) -> usize {{ input.len() }}\n\n"
                ));
            }
        }
        write(&root.join(format!("src/module_{file}.rs")), &text);
        write(
            &root.join(format!("docs/guide_{file}.md")),
            &format!(
                "# Guide {file}\n\nThe corpusNeedle{file}_0 marker documents request handling and the auditor path.\n\n{}\n",
                noise(u64::try_from(file).unwrap_or(0), 240)
            ),
        );
    }
    write(
        &root.join("tsconfig.json"),
        "{\"compilerOptions\":{\"baseUrl\":\".\",\"paths\":{\"@lib/*\":[\"src/lib/*\"]},\"moduleResolution\":\"bundler\",\"module\":\"esnext\"}}\n",
    );
    write(
        &root.join("src/lib/client.ts"),
        "export function sendRequest(payload: string): number { return payload.length; }\n",
    );
    write(
        &root.join("src/app.ts"),
        "import { sendRequest } from \"@lib/client\";\nexport function dispatch(payload: string): number { return sendRequest(payload); }\n",
    );
    write(
        &root.join("component.svelte"),
        "<script context=\"module\">\nexport const shared = 1;\n</script>\n<h1>{shared}</h1>\n<script lang=\"ts\">\nexport function componentHandler(input: string): number { return input.length + shared; }\n</script>\n",
    );
}

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..OpenOptions::default()
    })
    .expect("benchmark index opens")
}

fn one_file(root: &Path, store: &Path) -> Index {
    let index = open(root, store);
    index.reindex().expect("benchmark corpus indexes");
    index
}

fn benches(c: &mut Criterion) {
    let corpus = corpus();
    let root = corpus.path.as_path();

    let mut build = c.benchmark_group("build");
    build.sample_size(10);
    build.bench_function("cold_index", |b| {
        b.iter_batched(
            || tempfile::tempdir().expect("benchmark store directory"),
            |store| {
                let index = open(root, store.path());
                black_box(index.reindex().expect("cold index"));
            },
            BatchSize::PerIteration,
        );
    });
    build.finish();

    let mut sync = c.benchmark_group("sync");
    sync.sample_size(20);
    let edit_root = tempfile::tempdir().expect("benchmark edit directory");
    write_corpus(edit_root.path());
    let edit_store = tempfile::tempdir().expect("benchmark edit store");
    let edit_index = one_file(edit_root.path(), edit_store.path());
    let mut generation = 0u64;
    sync.bench_function("one_file_edit", |b| {
        b.iter(|| {
            generation = generation.wrapping_add(1);
            write(
                &edit_root.path().join("src/module_0.rs"),
                &format!(
                    "pub fn handler_0_0(input: &str) -> usize {{ input.len() + {} }}\n",
                    generation
                ),
            );
            black_box(edit_index.sync().expect("benchmark sync"));
        });
    });
    sync.finish();

    let store = tempfile::tempdir().expect("benchmark store directory");
    let index = one_file(root, store.path());
    let service = index.search();

    let mut lookup = c.benchmark_group("lookup");
    lookup.bench_function("exact_symbol", |b| {
        b.iter(|| {
            black_box(
                service
                    .symbol(&SymbolQuery::new("handler_100_10"))
                    .expect("symbol lookup"),
            );
        });
    });
    lookup.bench_function("references", |b| {
        b.iter(|| {
            black_box(
                service
                    .refs(&RefQuery::new("handler_100_10"))
                    .expect("reference lookup"),
            );
        });
    });
    lookup.finish();

    let mut explore = c.benchmark_group("explore");
    explore.bench_function("body_multiword", |b| {
        b.iter(|| {
            black_box(
                service
                    .explore(&ExploreQuery::new("corpusNeedle40_3 auditor request"))
                    .expect("body explore"),
            );
        });
    });
    explore.bench_function("metadata_single_term", |b| {
        let mut query = ExploreQuery::new("handler_100_10");
        query.retrieval = RetrievalOptions {
            ranking: graph_search_types::RankingStrategy::Metadata,
            ..RetrievalOptions::default()
        };
        b.iter(|| {
            black_box(service.explore(&query).expect("metadata explore"));
        });
    });
    explore.bench_function("positional_route", |b| {
        let mut query = ExploreQuery::new("corpusNeedle40_3");
        query.retrieval = RetrievalOptions {
            mode: graph_search_types::ExploreMode::Near,
            near_window: 4,
            ..RetrievalOptions::default()
        };
        b.iter(|| {
            black_box(service.explore(&query).expect("positional explore"));
        });
    });
    explore.finish();

    let mut occurrences = c.benchmark_group("occurrences");
    occurrences.bench_function("by_name", |b| {
        b.iter(|| {
            black_box(
                service
                    .occurrences(&OccurrenceQuery {
                        target: "auditor_40_0".into(),
                        by: OccurrenceBy::Target,
                        ..OccurrenceQuery::default()
                    })
                    .expect("occurrence lookup"),
            );
        });
    });
    occurrences.finish();

    // Unindexed live scans: no generation is built, so these run on the raw tree.
    let live_root = tempfile::tempdir().expect("benchmark scan directory");
    write_corpus(live_root.path());
    let live = open(live_root.path(), &live_root.path().join("unused-store"));
    let live_service = live.search();
    let mut scan = c.benchmark_group("scan");
    scan.bench_function("text_literal", |b| {
        b.iter(|| {
            black_box(
                live_service
                    .text(&TextQuery::new("corpusNeedle150_0"))
                    .expect("literal scan")
                    .items
                    .len(),
            );
        });
    });
    scan.bench_function("files_glob", |b| {
        b.iter(|| {
            black_box(
                live_service
                    .files(&FilesQuery::new("src/*.rs"))
                    .expect("file scan")
                    .items
                    .len(),
            );
        });
    });
    scan.bench_function("filtered_explore", |b| {
        let query = ExploreQuery {
            filters: GraphFilters {
                path_glob: Some("src/*.rs".into()),
                ..GraphFilters::default()
            },
            ..ExploreQuery::new("handler_50_5")
        };
        b.iter(|| {
            black_box(service.explore(&query).expect("filtered explore"));
        });
    });
    scan.finish();
}

criterion_group!(search, benches);
criterion_main!(search);
