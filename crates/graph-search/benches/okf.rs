//! Criterion benchmarks for Open Knowledge Format bundles: a full index and
//! incremental syncs after edits that do and do not touch OKF link targets.
//!
//! The corpus is one bundle of densely cross-linked concepts (every document
//! has ten sections of three links) beside a small Rust crate, so an unrelated
//! code edit, a concept body edit and a retitle of a heavily linked concept can
//! be told apart. A Rust corpus of the same symbol and edge count is measured
//! alongside to separate OKF rebinding from sync's size-proportional work.
//!
//! ```sh
//! cargo bench -p graph-search --bench okf
//! ```

// Benchmarks are fixture code: a setup failure must abort the measurement and
// the generated corpus uses plain arithmetic/formatting.
#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::format_push_string,
    clippy::uninlined_format_args
)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use graph_search::{Index, OpenOptions};
use std::hint::black_box;
use std::path::Path;

/// Concept documents in the bundle.
const DOCS: usize = 1_000;
/// Sections per document, and links per section.
const SECTIONS: usize = 10;
const LINKS: usize = 3;

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("benchmark corpus directory");
    }
    std::fs::write(path, text).expect("benchmark corpus file");
}

/// Concept `i`: ten sections, each linking three other concepts. Concept 0 is
/// linked from about thirty documents.
fn concept(i: usize, title: &str) -> String {
    let mut text = format!("---\ntype: Metric\ntitle: {title}\n---\n");
    for section in 0..SECTIONS {
        text.push_str(&format!("# Section {section}\n\nSee "));
        for link in 0..LINKS {
            let target = (i * 7 + section * 3 + link) % DOCS;
            text.push_str(&format!("[doc {target}](d{target}.md) "));
        }
        text.push_str("for context.\n\n");
    }
    text
}

/// The Rust module with the same shape: ten functions of three calls each.
fn module(i: usize) -> String {
    let mut text = String::new();
    for function in 0..SECTIONS {
        text.push_str(&format!("pub fn f{function}() {{\n"));
        for call in 0..LINKS {
            let target = (i * 7 + function * 3 + call) % DOCS;
            text.push_str(&format!("    crate::m{target}::f{call}();\n"));
        }
        text.push_str("}\n");
    }
    text
}

fn okf_corpus(root: &Path) {
    write(
        &root.join("kb/index.md"),
        "# Bundle\n\n* [concepts](d0.md)\n",
    );
    for i in 0..DOCS {
        write(
            &root.join(format!("kb/d{i}.md")),
            &concept(i, &format!("Doc {i}")),
        );
    }
    write(&root.join("src/lib.rs"), "pub fn unrelated() {}\n");
}

fn rust_corpus(root: &Path) {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"bench\"\nversion = \"0.1.0\"\n",
    );
    let mut lib = String::from("pub fn unrelated() {}\n");
    for i in 0..DOCS {
        lib.push_str(&format!("pub mod m{i};\n"));
        write(&root.join(format!("src/m{i}.rs")), &module(i));
    }
    write(&root.join("src/lib.rs"), &lib);
    write(&root.join("tools/unrelated.rs"), "pub fn unrelated() {}\n");
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
    let okf = tempfile::tempdir().expect("okf corpus");
    okf_corpus(okf.path());
    let rust = tempfile::tempdir().expect("rust corpus");
    rust_corpus(rust.path());

    let mut build = c.benchmark_group("okf_index");
    build.sample_size(10);
    for (name, root) in [("okf_reindex", okf.path()), ("rust_reindex", rust.path())] {
        build.bench_function(name, |b| {
            b.iter_batched(
                || tempfile::tempdir().expect("fresh store"),
                |fresh| {
                    black_box(open(root, fresh.path()).reindex().expect("reindex"));
                    fresh
                },
                BatchSize::PerIteration,
            );
        });
    }
    build.finish();

    let mut sync = c.benchmark_group("okf_sync");
    sync.sample_size(10);
    // Each edit toggles between two contents so every sample sees a change.
    let mut toggle = false;
    let mut edits = |name: &str, root: &Path, file: &str, a: String, b: String| {
        let store = tempfile::tempdir().expect("benchmark store");
        let index = open(root, store.path());
        index.reindex().expect("benchmark corpus indexes");
        let path = root.join(file);
        sync.bench_function(name, |bench| {
            bench.iter(|| {
                toggle = !toggle;
                write(&path, if toggle { &b } else { &a });
                black_box(index.sync().expect("edit sync"));
            });
        });
        write(&path, &a);
    };
    let unrelated = String::from("pub fn unrelated() {}\n");
    let changed = String::from("pub fn unrelated() { let _ = 1; }\n");
    edits(
        "okf_unrelated_code_edit",
        okf.path(),
        "src/lib.rs",
        unrelated.clone(),
        changed.clone(),
    );
    edits(
        "okf_concept_body_edit",
        okf.path(),
        "kb/d1.md",
        concept(1, "Doc 1"),
        concept(1, "Doc 1") + "\nAn added sentence.\n",
    );
    edits(
        "okf_linked_concept_retitle",
        okf.path(),
        "kb/d0.md",
        concept(0, "Doc 0"),
        concept(0, "Doc Zero"),
    );
    edits(
        "rust_unrelated_code_edit",
        rust.path(),
        "tools/unrelated.rs",
        unrelated,
        changed,
    );
    sync.finish();
}

criterion_group!(okf, benches);
criterion_main!(okf);
