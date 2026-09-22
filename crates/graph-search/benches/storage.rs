//! Criterion benchmarks for the on-disk generation: how long it takes to publish,
//! re-open and incrementally update a store, and how many bytes it occupies.
//!
//! The corpus mirrors the shape that dominates real indexes of this repository:
//! Rust sources, Markdown prose, and large configuration/JSON files full of
//! hashes and numbers (whose per-region term maps are most of the source-record
//! bytes). Store size is not a timing, so it is printed once at setup; ids stay
//! stable so `--baseline` comparisons survive a size change.
//!
//! ```sh
//! cargo bench -p graph-search --bench storage
//! ```

// Benchmarks are fixture code: a setup failure must abort the measurement, the
// generated corpus uses plain arithmetic/formatting, and the size report goes to
// stderr beside Criterion's own output.
#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::cast_possible_truncation,
    clippy::print_stderr,
    clippy::integer_division
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use graph_search::{Index, OpenOptions};
use graph_search_types::query::TextQuery;
use graph_search_types::{ExploreQuery, SymbolQuery};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const RUST_FILES: usize = 120;
const MARKDOWN_FILES: usize = 40;
const JSON_FILES: usize = 40;
/// Records per JSON file; each carries two hashes and several numbers.
const JSON_RECORDS: usize = 300;

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

/// Deterministic 64-hex-digit pseudo-hash without a dependency.
fn fake_hash(seed: u64) -> String {
    let mut state = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let mut out = String::with_capacity(64);
    for _ in 0..4 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        out.push_str(&format!("{state:016x}"));
    }
    out
}

fn rust_file(file: usize) -> String {
    let mut text = format!("//! Module {file}: request handling for the storage benchmark.\n\n");
    text.push_str(&format!(
        "/// Configuration for worker {file}.\npub struct Worker{file} {{\n    pub capacity: usize,\n    pub label: String,\n}}\n\n"
    ));
    for function in 0..16 {
        text.push_str(&format!(
            "/// Processes batch {function} for worker {file}.\npub fn process_{file}_{function}(worker: &Worker{file}, input: &str) -> usize {{\n    let budget = worker.capacity.saturating_sub({function});\n    if input.contains(\"needle_{file}_{function}\") {{\n        return budget;\n    }}\n    helper_{file}(input) + budget\n}}\n\n"
        ));
    }
    text.push_str(&format!(
        "fn helper_{file}(input: &str) -> usize {{\n    input.len()\n}}\n"
    ));
    text
}

fn markdown_file(file: usize) -> String {
    let mut text = format!("# Design note {file}\n\n");
    for section in 0..12 {
        text.push_str(&format!(
            "## Section {section}\n\nWorker{file} processes batches under a byte budget; see `process_{file}_{section}`. The reconcile pass records generation {section} and verifies content hashes before publication.\n\n- capacity: {}\n- label: note-{file}-{section}\n\n",
            file * 31 + section
        ));
    }
    text
}

fn json_file(file: usize) -> String {
    let mut text = String::from("{\n  \"trials\": [\n");
    for record in 0..JSON_RECORDS {
        let seed = (file * JSON_RECORDS + record) as u64;
        text.push_str(&format!(
            "    {{\"id\": \"trial-{file}-{record}\", \"source_hash\": \"{}\", \"result_hash\": \"{}\", \"elapsed_ms\": {}.{:03}, \"bytes\": {}, \"ok\": {}}}{}\n",
            fake_hash(seed),
            fake_hash(seed ^ 0x9e37_79b9),
            seed % 997,
            seed % 1000,
            seed * 7919 % 65_536,
            record % 3 != 0,
            if record + 1 == JSON_RECORDS { "" } else { "," }
        ));
    }
    text.push_str("  ]\n}\n");
    text
}

fn write_corpus(root: &Path) {
    for file in 0..RUST_FILES {
        write(
            &root.join(format!("src/module_{file}.rs")),
            &rust_file(file),
        );
    }
    for file in 0..MARKDOWN_FILES {
        write(
            &root.join(format!("docs/note_{file}.md")),
            &markdown_file(file),
        );
    }
    for file in 0..JSON_FILES {
        write(
            &root.join(format!("results/trial_{file}.json")),
            &json_file(file),
        );
    }
}

fn open(root: &Path, store: &Path, read_only: bool) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        read_only,
        ..OpenOptions::default()
    })
    .expect("benchmark index opens")
}

fn disk_bytes(dir: &Path) -> u64 {
    // Unique inodes only: generations hard-link shared packs.
    use std::os::unix::fs::MetadataExt;
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("store directory").flatten() {
            let meta = entry.metadata().expect("store entry");
            if meta.is_dir() {
                stack.push(entry.path());
            } else if seen.insert(meta.ino()) {
                total += meta.len();
            }
        }
    }
    total
}

fn benches(c: &mut Criterion) {
    let corpus = corpus();
    let root = corpus.path.as_path();

    let store = tempfile::tempdir().expect("benchmark store directory");
    open(root, store.path(), false)
        .reindex()
        .expect("benchmark corpus indexes");
    let bytes = disk_bytes(store.path());
    let size = format!("{}KiB", bytes / 1024);
    eprintln!("storage: published store occupies {bytes} bytes ({size})");

    // Publication: parse, project, encode, hash and fsync a whole generation.
    let mut publish = c.benchmark_group("storage_publish");
    publish.sample_size(10);
    publish.bench_function("reindex_full", |b| {
        b.iter_batched(
            || tempfile::tempdir().expect("fresh store"),
            |fresh| {
                let index = open(root, fresh.path(), false);
                black_box(index.reindex().expect("reindex"));
                fresh
            },
            BatchSize::PerIteration,
        );
    });
    publish.finish();

    // Re-open: what every one-shot CLI invocation pays before answering.
    let mut reopen = c.benchmark_group("storage_open");
    reopen.sample_size(20);
    reopen.bench_function("published_read_only", |b| {
        b.iter(|| black_box(open(root, store.path(), true)));
    });
    reopen.bench_function("open_then_symbol", |b| {
        b.iter(|| {
            let index = open(root, store.path(), true);
            black_box(
                index
                    .search()
                    .symbol(&SymbolQuery::new("process_7_3"))
                    .expect("symbol"),
            );
        });
    });
    reopen.bench_function("open_then_explore", |b| {
        b.iter(|| {
            let index = open(root, store.path(), true);
            black_box(
                index
                    .search()
                    .explore(&ExploreQuery::new("worker byte budget reconcile"))
                    .expect("explore"),
            );
        });
    });
    reopen.bench_function("open_then_text", |b| {
        b.iter(|| {
            let index = open(root, store.path(), true);
            black_box(
                index
                    .search()
                    .text(&TextQuery::new("needle_42_7"))
                    .expect("text"),
            );
        });
    });
    reopen.finish();

    // Incremental maintenance of an existing generation.
    let index = open(root, store.path(), false);
    let mut maintain = c.benchmark_group("storage_sync");
    maintain.sample_size(10);
    maintain.bench_function("noop", |b| {
        b.iter(|| black_box(index.sync().expect("no-op sync")));
    });
    let edited = root.join("src/module_7.rs");
    let json_edited = root.join("results/trial_3.json");
    let original = rust_file(7);
    let json_original = json_file(3);
    let mut toggle = false;
    maintain.bench_function("rust_body_edit", |b| {
        b.iter(|| {
            toggle = !toggle;
            let text = if toggle {
                original.replace("helper_7(input) + budget", "helper_7(input) + budget + 1")
            } else {
                original.clone()
            };
            write(&edited, &text);
            black_box(index.sync().expect("edit sync"));
        });
    });
    maintain.bench_function("json_edit", |b| {
        b.iter(|| {
            toggle = !toggle;
            let text = if toggle {
                json_original.replace("trial-3-10\"", "trial-3-10-edited\"")
            } else {
                json_original.clone()
            };
            write(&json_edited, &text);
            black_box(index.sync().expect("edit sync"));
        });
    });
    maintain.finish();
    write(&edited, &original);
    write(&json_edited, &json_original);
}

criterion_group!(storage, benches);
criterion_main!(storage);
