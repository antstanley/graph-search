//! Incremental binding decisions must agree with complete reprojection.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search_core::{
    Result,
    config::WalkPolicy,
    memory::MemoryStore,
    ports::{GraphSnapshot, GraphStore, ListRegistry},
    reconcile::Projector,
};
use graph_search_engine::{NativeStore, StoreOptions};
use graph_search_types::{ApplyOutcome, Manifest, WriteBatch};
use std::path::Path;

struct Capture {
    inner: Box<dyn GraphStore>,
    upserts: Vec<String>,
}
impl GraphStore for Capture {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.inner.apply(batch)
    }
    fn publish(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.upserts = batch.upserts.iter().map(|p| p.file.path.clone()).collect();
        self.upserts.sort();
        self.inner.publish(batch)
    }
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        self.inner.snapshot()
    }
    fn manifest(&self) -> Result<Option<Manifest>> {
        self.inner.manifest()
    }
    fn manifest_header(&self) -> Result<Option<Manifest>> {
        self.inner.manifest_header()
    }
    fn publish_retaining(
        &mut self,
        batch: WriteBatch,
        retention: &graph_search_core::retention::FactRetention,
    ) -> graph_search_core::Result<graph_search_types::ApplyOutcome> {
        self.upserts = batch.upserts.iter().map(|p| p.file.path.clone()).collect();
        self.upserts.sort();
        self.inner.publish_retaining(batch, retention)
    }
    fn generation(&self) -> graph_search_core::Result<Option<String>> {
        self.inner.generation()
    }
    fn extraction_facts(
        &self,
        paths: &std::collections::BTreeSet<String>,
    ) -> graph_search_core::Result<graph_search_core::ports::ExtractionFacts> {
        self.inner.extraction_facts(paths)
    }
    fn dependency_index(
        &self,
    ) -> graph_search_core::Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.inner.dependency_index()
    }
    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        self.inner.commit_manifest(manifest)
    }
}
struct Case {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    path: &'static str,
    replacement: &'static str,
    upserts: &'static [&'static str],
}
fn write(root: &Path, path: &str, text: &str) {
    std::fs::write(root.join(path), text).unwrap();
}
fn equal(actual: &dyn GraphStore, expected: &dyn GraphStore, case: &str) {
    let a = actual.snapshot().unwrap();
    let b = expected.snapshot().unwrap();
    assert_eq!(
        a.all_nodes().unwrap(),
        b.all_nodes().unwrap(),
        "nodes: {case}"
    );
    assert_eq!(
        a.all_edges().unwrap(),
        b.all_edges().unwrap(),
        "edges: {case}"
    );
    assert_eq!(
        a.source_files().unwrap(),
        b.source_files().unwrap(),
        "source: {case}"
    );
    assert_eq!(
        a.occurrence_files().unwrap(),
        b.occurrence_files().unwrap(),
        "occurrences: {case}"
    );
}
fn run(case: &Case) {
    for persistent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let db = tempfile::tempdir().unwrap();
        for (path, text) in case.files {
            write(root.path(), path, text);
        }
        // Non-parser text must not masquerade as a missing extraction cache.
        write(
            root.path(),
            "README.md",
            "# Project\n\nDocumentation stays unchanged.\n",
        );
        let registry = ListRegistry::new(graph_search_langs::all_extractors());
        let policy = WalkPolicy::default();
        let projector = Projector::new(&registry, &policy);
        let mut store = Capture {
            inner: if persistent {
                Box::new(NativeStore::open(db.path(), &StoreOptions::default()).unwrap())
            } else {
                Box::new(MemoryStore::new())
            },
            upserts: Vec::new(),
        };
        projector.reindex(root.path(), &mut store).unwrap();
        if persistent {
            drop(store.inner);
            store.inner = Box::new(NativeStore::open(db.path(), &StoreOptions::default()).unwrap());
        }
        assert!(
            store.dependency_index().unwrap().is_some(),
            "cached dependencies: {}",
            case.name
        );
        write(root.path(), case.path, case.replacement);
        projector.sync(root.path(), &mut store).unwrap();
        assert_eq!(
            store.upserts, case.upserts,
            "upserts: {} persistent={persistent}",
            case.name
        );
        let mut clean = MemoryStore::new();
        projector.reindex(root.path(), &mut clean).unwrap();
        equal(&store, &clean, case.name);
        if persistent {
            drop(store.inner);
            store.inner = Box::new(NativeStore::open(db.path(), &StoreOptions::default()).unwrap());
            equal(&store, &clean, case.name);
        }
        store.upserts.clear();
        projector.sync(root.path(), &mut store).unwrap();
        assert!(store.upserts.is_empty(), "no-op: {}", case.name);
    }
}
const JS: &[(&str, &str)] = &[
    (
        "leaf.js",
        "export function leaf() { return 1; }\nfunction helper() { return 2; }\n",
    ),
    (
        "caller.js",
        "import {leaf} from './leaf.js';\nexport function caller() { return leaf(); }\n",
    ),
];
const RUST: &[(&str, &str)] = &[
    ("leaf.rs", "pub fn leaf() -> u32 { 1 }\n"),
    ("caller.rs", "fn caller() { leaf(); }\n"),
    ("outer.rs", "fn outer() { caller(); }\n"),
];
#[test]
fn body_and_signature_edits_refresh_only_their_owner() {
    for case in [
        Case {
            name: "js literal width",
            files: JS,
            path: "leaf.js",
            replacement: "export function leaf() { return 12345; }\nfunction helper() { return 2; }\n",
            upserts: &["leaf.js"],
        },
        Case {
            name: "js changed outgoing call and coordinates",
            files: JS,
            path: "leaf.js",
            replacement: "// leading comment\n\nexport function leaf() {\n return helper();\n}\nfunction helper() { return 2; }\n",
            upserts: &["leaf.js"],
        },
        Case {
            name: "js parameters and async",
            files: JS,
            path: "leaf.js",
            replacement: "export async function leaf(value) { return value; }\nfunction helper() { return 2; }\n",
            upserts: &["leaf.js"],
        },
        Case {
            name: "ts signature",
            files: &[
                ("leaf.ts", "export function leaf(): number { return 1; }"),
                (
                    "caller.ts",
                    "import {leaf} from './leaf'; function caller() { leaf(); }",
                ),
            ],
            path: "leaf.ts",
            replacement: "export function leaf(value: string): string { return value; }",
            upserts: &["leaf.ts"],
        },
        Case {
            name: "rust body and documentation",
            files: RUST,
            path: "leaf.rs",
            replacement: "/// New documentation\npub fn leaf() -> u32 {\n 12345\n}\n",
            upserts: &["leaf.rs"],
        },
        Case {
            name: "rust signature",
            files: RUST,
            path: "leaf.rs",
            replacement: "pub fn leaf(value: u64) -> u64 { value }\n",
            upserts: &["leaf.rs"],
        },
    ] {
        run(&case);
    }
}
#[test]
fn binding_changes_still_repair_consumers() {
    for case in [
        Case {
            name: "js export removed",
            files: JS,
            path: "leaf.js",
            replacement: "function leaf() { return 1; }\nfunction helper() { return 2; }\n",
            upserts: &["caller.js", "leaf.js"],
        },
        Case {
            name: "rust visibility",
            files: RUST,
            path: "leaf.rs",
            replacement: "fn leaf() -> u32 { 1 }\n",
            upserts: &["caller.rs", "leaf.rs", "outer.rs"],
        },
        Case {
            name: "rust target renamed",
            files: RUST,
            path: "leaf.rs",
            replacement: "pub fn renamed() -> u32 { 1 }\n",
            upserts: &["caller.rs", "leaf.rs", "outer.rs"],
        },
        Case {
            name: "js reexport retarget",
            files: &[
                ("a.js", "export function leaf() {}"),
                ("b.js", "export function leaf() {}"),
                ("barrel.js", "export {leaf} from './a.js';"),
                (
                    "caller.js",
                    "import {leaf} from './barrel.js'; function caller() { leaf(); }",
                ),
            ],
            path: "barrel.js",
            replacement: "export {leaf} from './b.js';",
            upserts: &["barrel.js", "caller.js"],
        },
        Case {
            name: "js import retarget behind local export",
            files: &[
                ("a.js", "export function leaf() {}"),
                ("b.js", "export function leaf() {}"),
                ("barrel.js", "import {leaf} from './a.js'; export {leaf};"),
                (
                    "caller.js",
                    "import {leaf} from './barrel.js'; function caller() { leaf(); }",
                ),
            ],
            path: "barrel.js",
            replacement: "import {leaf} from './b.js'; export {leaf};",
            upserts: &["barrel.js", "caller.js"],
        },
        Case {
            name: "duplicate identity moved",
            files: &[
                ("leaf.rs", "fn leaf() {}\nfn leaf() {}\n"),
                ("caller.rs", "fn caller() { leaf(); }"),
            ],
            path: "leaf.rs",
            replacement: "// shifted declaration ids\nfn leaf() {}\nfn leaf() {}\n",
            upserts: &["caller.rs", "leaf.rs"],
        },
    ] {
        run(&case);
    }
}
