//! A native sync must not materialize the full raw-fact manifest.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search_core::{
    Result,
    config::WalkPolicy,
    memory::MemoryStore,
    ports::{ExtractionFacts, GraphSnapshot, GraphStore, ListRegistry},
    reconcile::Projector,
    retention::FactRetention,
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{ApplyOutcome, Manifest, WriteBatch};
use std::{cell::RefCell, collections::BTreeSet, path::Path};

struct SelectedOnly {
    inner: Box<dyn GraphStore>,
    reads: RefCell<Vec<BTreeSet<String>>>,
    retained: BTreeSet<String>,
}
impl GraphStore for SelectedOnly {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.inner.apply(batch)
    }
    fn publish(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.inner.publish(batch)
    }
    fn publish_retaining(
        &mut self,
        batch: WriteBatch,
        retention: &FactRetention,
    ) -> Result<ApplyOutcome> {
        self.retained.clone_from(&retention.paths);
        self.inner.publish_retaining(batch, retention)
    }
    fn generation(&self) -> Result<Option<String>> {
        self.inner.generation()
    }
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        self.inner.snapshot()
    }
    fn manifest(&self) -> Result<Option<Manifest>> {
        panic!("native reconciliation requested the full manifest")
    }
    fn manifest_header(&self) -> Result<Option<Manifest>> {
        self.inner.manifest_header()
    }
    fn extraction_facts(&self, paths: &BTreeSet<String>) -> Result<ExtractionFacts> {
        self.reads.borrow_mut().push(paths.clone());
        self.inner.extraction_facts(paths)
    }
    fn dependency_index(
        &self,
    ) -> Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.inner.dependency_index()
    }
    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        self.inner.commit_manifest(manifest)
    }
}
fn paths(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).into()).collect()
}
fn compare(actual: &dyn GraphStore, clean: &dyn GraphStore) {
    let a = actual.snapshot().unwrap();
    let b = clean.snapshot().unwrap();
    assert_eq!(a.all_nodes().unwrap(), b.all_nodes().unwrap());
    assert_eq!(a.all_edges().unwrap(), b.all_edges().unwrap());
    assert_eq!(a.source_files(), b.source_files());
    assert_eq!(a.occurrence_files(), b.occurrence_files());
    assert_eq!(
        actual.manifest().unwrap().unwrap().entries,
        clean.manifest().unwrap().unwrap().entries
    );
}
#[test]
fn body_binding_presence_and_timestamp_updates_load_only_affected_facts() {
    for persistent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let db = tempfile::tempdir().unwrap();
        for (path, text) in [
            ("leaf.js", "export function leaf() { return 1; }"),
            (
                "caller.js",
                "import {leaf} from './leaf.js'; export function caller() { leaf(); }",
            ),
            (
                "outer.js",
                "import {caller} from './caller.js'; function outer() { caller(); }",
            ),
            ("cold.js", "export function unrelated() {}"),
        ] {
            std::fs::write(root.path().join(path), text).unwrap();
        }
        let registry = ListRegistry::new(graph_search_langs::all_extractors());
        let policy = WalkPolicy::default();
        let projector = Projector::new(&registry, &policy);
        let mut store = SelectedOnly {
            inner: if persistent {
                Box::new(GrafeoStore::open(db.path(), &StoreOptions::default()).unwrap())
            } else {
                Box::new(MemoryStore::new())
            },
            reads: RefCell::new(Vec::new()),
            retained: BTreeSet::new(),
        };
        projector.reindex(root.path(), &mut store).unwrap();
        for (text, expected) in [
            ("export function leaf() { return 222; }", BTreeSet::new()),
            (
                "export function renamed() { return 222; }",
                paths(&["caller.js", "outer.js"]),
            ),
        ] {
            store.reads.borrow_mut().clear();
            std::fs::write(root.path().join("leaf.js"), text).unwrap();
            projector.sync(root.path(), &mut store).unwrap();
            assert_eq!(
                store
                    .reads
                    .borrow()
                    .iter()
                    .flatten()
                    .cloned()
                    .collect::<BTreeSet<_>>(),
                expected
            );
            assert!(store.retained.contains("cold.js"));
            let mut clean = MemoryStore::new();
            projector.reindex(root.path(), &mut clean).unwrap();
            compare(store.inner.as_ref(), &clean);
            if persistent {
                store.inner =
                    Box::new(GrafeoStore::open(db.path(), &StoreOptions::default()).unwrap());
                compare(store.inner.as_ref(), &clean);
            }
        }
        std::fs::rename(root.path().join("leaf.js"), root.path().join("moved.js")).unwrap();
        projector.sync(root.path(), &mut store).unwrap();
        let mut clean = MemoryStore::new();
        projector.reindex(root.path(), &mut clean).unwrap();
        compare(store.inner.as_ref(), &clean);
        let cold = root.path().join("cold.js");
        let modified = std::fs::metadata(&cold).unwrap().modified().unwrap();
        std::fs::File::options()
            .write(true)
            .open(&cold)
            .unwrap()
            .set_modified(modified + std::time::Duration::from_secs(2))
            .unwrap();
        store.reads.borrow_mut().clear();
        projector.sync(root.path(), &mut store).unwrap();
        assert!(store.reads.borrow().is_empty());
        projector.reindex(root.path(), &mut clean).unwrap();
        compare(store.inner.as_ref(), &clean);
        if persistent {
            store.inner = Box::new(GrafeoStore::open(db.path(), &StoreOptions::default()).unwrap());
            compare(store.inner.as_ref(), &clean);
        }
        let before = store.inner.manifest().unwrap();
        projector.sync(root.path(), &mut store).unwrap();
        assert_eq!(store.inner.manifest().unwrap(), before);
    }
}

#[test]
fn transient_native_engine_keeps_facts_available_for_selective_sync() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.js"), "function a() {}").unwrap();
    std::fs::write(root.path().join("b.js"), "function b() {}").unwrap();
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = GrafeoStore::open(
        Path::new(""),
        &StoreOptions {
            in_memory: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    projector.reindex(root.path(), &mut store).unwrap();
    std::fs::write(root.path().join("a.js"), "function a() { return 2; }").unwrap();
    projector.sync(root.path(), &mut store).unwrap();
    let mut clean = MemoryStore::new();
    projector.reindex(root.path(), &mut clean).unwrap();
    compare(&store, &clean);
}
