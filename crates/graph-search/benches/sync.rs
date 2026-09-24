//! Criterion benchmark for how sync cost scales with workspace size
//! (`research/16-proportional-sync.md`).
//!
//! The same one-file edit is synced in Rust workspaces of increasing size. Every
//! module has ten functions of three cross-module calls, so the graph grows with
//! the file count. A sync that is proportional to the edit stays flat across
//! sizes, apart from the per-path stat walk.
//!
//! ```sh
//! cargo bench -p graph-search --bench sync
//! ```

// Benchmarks are fixture code: a setup failure must abort the measurement and
// the generated corpus uses plain arithmetic/formatting.
#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::format_push_string,
    clippy::uninlined_format_args
)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use graph_search::{Index, OpenOptions};
use std::hint::black_box;
use std::path::Path;

/// Workspace sizes, in modules.
const SIZES: [usize; 3] = [250, 1_000, 4_000];
/// Functions per module, and calls per function.
const FUNCTIONS: usize = 10;
const CALLS: usize = 3;

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("benchmark corpus directory");
    }
    std::fs::write(path, text).expect("benchmark corpus file");
}

fn module(i: usize, size: usize) -> String {
    let mut text = String::new();
    for function in 0..FUNCTIONS {
        text.push_str(&format!("pub fn f{function}() {{\n"));
        for call in 0..CALLS {
            let target = (i * 7 + function * 3 + call) % size;
            text.push_str(&format!("    crate::m{target}::f{call}();\n"));
        }
        text.push_str("}\n");
    }
    text
}

fn corpus(root: &Path, size: usize) {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = \"bench\"\nversion = \"0.1.0\"\n",
    );
    let mut lib = String::new();
    for i in 0..size {
        lib.push_str(&format!("pub mod m{i};\n"));
        write(&root.join(format!("src/m{i}.rs")), &module(i, size));
    }
    write(&root.join("src/lib.rs"), &lib);
}

fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync_scaling");
    group.sample_size(10);
    for size in SIZES {
        let root = tempfile::tempdir().expect("benchmark corpus");
        corpus(root.path(), size);
        let store = tempfile::tempdir().expect("benchmark store");
        let index = Index::open(OpenOptions {
            root: root.path().into(),
            store: Some(store.path().into()),
            ..OpenOptions::default()
        })
        .expect("benchmark index opens");
        index.reindex().expect("benchmark corpus indexes");

        // A body edit to one module: its symbols keep their names, so only the
        // file itself and whatever binds through it can change.
        let path = root.path().join("src/m1.rs");
        let original = module(1, size);
        let edited = original.replacen("pub fn f0() {\n", "pub fn f0() {\n    let _ = 1;\n", 1);
        let mut toggle = false;
        group.bench_with_input(BenchmarkId::new("one_file_edit", size), &size, |b, _| {
            b.iter(|| {
                toggle = !toggle;
                write(&path, if toggle { &edited } else { &original });
                black_box(index.sync().expect("edit sync"));
            });
        });
        write(&path, &original);
    }
    group.finish();
}

criterion_group!(sync, benches);
criterion_main!(sync);
