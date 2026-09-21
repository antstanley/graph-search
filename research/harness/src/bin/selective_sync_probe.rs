//! Native sync workload matrix; run only after other builds/tests have stopped.
use graph_search_core::{
    Result as CoreResult,
    config::WalkPolicy,
    ports::{ExtractionFacts, GraphSnapshot, GraphStore, ListRegistry},
    reconcile::Projector,
    retention::FactRetention,
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{ApplyOutcome, Manifest, WriteBatch};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::Path,
    time::Instant,
};

struct Observed {
    inner: GrafeoStore,
    full: Cell<usize>,
    requested: RefCell<BTreeSet<String>>,
    retained: usize,
    upserts: usize,
}
impl GraphStore for Observed {
    fn apply(&mut self, batch: WriteBatch) -> CoreResult<ApplyOutcome> {
        self.inner.apply(batch)
    }
    fn publish(&mut self, batch: WriteBatch) -> CoreResult<ApplyOutcome> {
        self.upserts = batch.upserts.len();
        self.inner.publish(batch)
    }
    fn publish_retaining(
        &mut self,
        batch: WriteBatch,
        r: &FactRetention,
    ) -> CoreResult<ApplyOutcome> {
        self.upserts = batch.upserts.len();
        self.retained = r.paths.len();
        self.inner.publish_retaining(batch, r)
    }
    fn generation(&self) -> CoreResult<Option<String>> {
        self.inner.generation()
    }
    fn snapshot(&self) -> CoreResult<Box<dyn GraphSnapshot + '_>> {
        self.inner.snapshot()
    }
    fn manifest(&self) -> CoreResult<Option<Manifest>> {
        self.full.set(self.full.get() + 1);
        self.inner.manifest()
    }
    fn manifest_header(&self) -> CoreResult<Option<Manifest>> {
        self.inner.manifest_header()
    }
    fn extraction_facts(&self, paths: &BTreeSet<String>) -> CoreResult<ExtractionFacts> {
        self.requested.borrow_mut().extend(paths.iter().cloned());
        self.inner.extraction_facts(paths)
    }
    fn dependency_index(
        &self,
    ) -> CoreResult<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.inner.dependency_index()
    }
    fn commit_manifest(&mut self, m: Manifest) -> CoreResult<()> {
        self.inner.commit_manifest(m)
    }
}
fn write(root: &Path, files: usize) -> std::io::Result<()> {
    std::fs::write(root.join("target.rs"), "pub fn target() -> u32 { 1 }\n")?;
    for n in 1..files {
        let mut source = format!("pub fn consumer_{n}() {{ target(); }}\n");
        for k in 0..16 {
            source.push_str(&format!("pub fn helper_{n}_{k}() -> u32 {{ {k} }}\n"));
        }
        std::fs::write(root.join(format!("consumer_{n}.rs")), source)?;
    }
    Ok(())
}
fn run(files: usize, case: &str, trial: usize) -> Result<Value, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let db = tempfile::tempdir()?;
    write(root.path(), files)?;
    if case == "duplicate_remove" {
        std::fs::write(root.path().join("duplicate.rs"), "pub fn target() {}\n")?;
    }
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = Observed {
        inner: GrafeoStore::open(db.path(), &StoreOptions::default())?,
        full: Cell::new(0),
        requested: RefCell::new(BTreeSet::new()),
        retained: 0,
        upserts: 0,
    };
    projector.reindex(root.path(), &mut store)?;
    match case {
        "noop" => {}
        "body" => std::fs::write(
            root.path().join("target.rs"),
            "pub fn target() -> u32 { 222 }\n",
        )?,
        "public_api" => std::fs::write(
            root.path().join("target.rs"),
            "pub(crate) fn target() -> u32 { 1 }\n",
        )?,
        "rename" => std::fs::rename(root.path().join("target.rs"), root.path().join("moved.rs"))?,
        "delete" => std::fs::remove_file(root.path().join("target.rs"))?,
        "duplicate_add" => {
            std::fs::write(root.path().join("duplicate.rs"), "pub fn target() {}\n")?
        }
        "duplicate_remove" => std::fs::remove_file(root.path().join("duplicate.rs"))?,
        "missing_cache" => {
            let mut manifest = store.inner.manifest()?.ok_or("missing manifest")?;
            manifest
                .entries
                .get_mut("consumer_1.rs")
                .ok_or("missing owner")?
                .extraction = None;
            store.inner.commit_manifest(manifest)?;
            std::fs::write(
                root.path().join("target.rs"),
                "pub fn target() -> u32 { 222 }\n",
            )?;
        }
        _ => return Err("unknown workload".into()),
    }
    store.full.set(0);
    store.requested.borrow_mut().clear();
    store.retained = 0;
    store.upserts = 0;
    let start = Instant::now();
    let report = projector.sync(root.path(), &mut store)?;
    let sync_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        store.full.get(),
        0,
        "native sync hydrated the entire manifest"
    );
    let requested = store.requested.borrow().len();
    let retained = store.retained;
    let upserts = store.upserts;
    store.inner = GrafeoStore::open(db.path(), &StoreOptions::default())?;
    let before = store.inner.manifest()?.ok_or("missing manifest")?;
    let snapshot = store.snapshot()?;
    let nodes = snapshot.all_nodes()?;
    let edges = snapshot.all_edges()?;
    let sources = snapshot.source_files().clone();
    let occurrences = snapshot.occurrence_files().clone();
    drop(snapshot);
    let start = Instant::now();
    projector.reindex(root.path(), &mut store)?;
    let rebuild_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        before.entries,
        store
            .inner
            .manifest()?
            .ok_or("missing rebuilt manifest")?
            .entries
    );
    store.inner = GrafeoStore::open(db.path(), &StoreOptions::default())?;
    let actual = store.snapshot()?;
    assert_eq!(nodes, actual.all_nodes()?);
    assert_eq!(edges, actual.all_edges()?);
    assert_eq!(sources, *actual.source_files());
    assert_eq!(occurrences, *actual.occurrence_files());
    Ok(
        json!({"files_before":files,"case":case,"trial":trial,"sync_ms":sync_ms,"rebuild_ms":rebuild_ms,"requested_unchanged_facts":requested,"retained_records":retained,"upserts":upserts,"reported_modified":report.modified.len(),"matches_rebuild_and_reopen":true,"graph_nodes":nodes.len(),"graph_edges":edges.len()}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = Vec::new();
    for files in [16, 64] {
        for case in [
            "noop",
            "body",
            "public_api",
            "rename",
            "delete",
            "duplicate_add",
            "duplicate_remove",
            "missing_cache",
        ] {
            for trial in 0..3 {
                rows.push(run(files, case, trial)?);
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"protocol":"sequential warm sync then explicit reindex of the same state; synthetic Rust fanout; no competing builds/tests","rows":rows})
        )?
    );
    Ok(())
}
