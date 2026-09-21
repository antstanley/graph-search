//! Reproduce target-only replacement versus source-owned reference facts.
use graph_search_core::{
    config::WalkPolicy,
    memory::MemoryStore,
    ports::{GraphStore, ListRegistry},
    reconcile::Projector,
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{EdgeKind, FileProjection, NodeId, WriteBatch};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

struct CaptureStore {
    inner: Box<dyn GraphStore>,
    batches: Vec<Vec<String>>,
}
impl CaptureStore {
    fn capture(&mut self, batch: &WriteBatch) {
        let mut paths: Vec<_> = batch.upserts.iter().map(|p| p.file.path.clone()).collect();
        paths.sort();
        self.batches.push(paths);
    }
}
impl GraphStore for CaptureStore {
    fn apply(
        &mut self,
        batch: WriteBatch,
    ) -> graph_search_core::Result<graph_search_types::ApplyOutcome> {
        self.capture(&batch);
        self.inner.apply(batch)
    }
    fn publish(
        &mut self,
        batch: WriteBatch,
    ) -> graph_search_core::Result<graph_search_types::ApplyOutcome> {
        self.capture(&batch);
        self.inner.publish(batch)
    }
    fn snapshot(
        &self,
    ) -> graph_search_core::Result<Box<dyn graph_search_core::ports::GraphSnapshot + '_>> {
        self.inner.snapshot()
    }
    fn manifest(&self) -> graph_search_core::Result<Option<graph_search_types::Manifest>> {
        self.inner.manifest()
    }
    fn publish_retaining(&mut self, batch: WriteBatch, retention: &graph_search_core::retention::FactRetention) -> graph_search_core::Result<graph_search_types::ApplyOutcome> {
        self.capture(&batch);
        self.inner.publish_retaining(batch, retention)
    }
    fn generation(&self) -> graph_search_core::Result<Option<String>> { self.inner.generation() }
    fn extraction_facts(&self, paths: &std::collections::BTreeSet<String>) -> graph_search_core::Result<graph_search_core::ports::ExtractionFacts> { self.inner.extraction_facts(paths) }
    fn dependency_index(&self) -> graph_search_core::Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.inner.dependency_index()
    }
    fn manifest_header(&self) -> graph_search_core::Result<Option<graph_search_types::Manifest>> { self.inner.manifest_header() }
    fn commit_manifest(
        &mut self,
        manifest: graph_search_types::Manifest,
    ) -> graph_search_core::Result<()> {
        self.inner.commit_manifest(manifest)
    }
}

fn repair_chain(count: usize, persistent: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("n0.js"),
        "export function n0() { return 1; }\n",
    )?;
    for n in 1..count {
        std::fs::write(
            root.path().join(format!("n{n}.js")),
            format!(
                "import {{n{previous}}} from './n{previous}.js';\nexport function n{n}() {{ return n{previous}(); }}\n",
                previous = n - 1
            ),
        )?;
    }
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let directory = tempfile::tempdir()?;
    let mut store = CaptureStore {
        inner: if persistent {
            Box::new(GrafeoStore::open(
                directory.path(),
                &StoreOptions::default(),
            )?)
        } else {
            Box::new(MemoryStore::new())
        },
        batches: Vec::new(),
    };
    projector.reindex(root.path(), &mut store)?;
    store.batches.clear();
    let source_hashes = (0..count)
        .map(|n| {
            std::fs::read(root.path().join(format!("n{n}.js")))
                .map(|bytes| graph_search_core::hash::content_hash(&bytes))
        })
        .collect::<Result<Vec<_>, _>>()?;
    std::fs::write(
        root.path().join("n0.js"),
        "export function n0() { return 10; }\n",
    )?;
    let report = projector.sync(root.path(), &mut store)?;
    let mut changed_sources = Vec::new();
    for (n, before) in source_hashes.iter().enumerate() {
        let path = format!("n{n}.js");
        let after = graph_search_core::hash::content_hash(&std::fs::read(root.path().join(&path))?);
        if before != &after {
            changed_sources.push(path);
        }
    }
    assert_eq!(changed_sources, ["n0.js"]);
    assert_eq!(store.batches.len(), 1);
    assert_eq!(store.batches[0], ["n0.js"]);
    let mut clean = MemoryStore::new();
    projector.reindex(root.path(), &mut clean)?;
    if persistent {
        store.inner = Box::new(GrafeoStore::open(
            directory.path(),
            &StoreOptions::default(),
        )?);
    }
    let actual = store.snapshot()?;
    let expected = clean.snapshot()?;
    assert_eq!(actual.all_nodes()?, expected.all_nodes()?);
    assert_eq!(actual.all_edges()?, expected.all_edges()?);
    assert_eq!(actual.occurrence_files(), expected.occurrence_files());
    assert_eq!(actual.source_files(), expected.source_files());
    Ok(
        json!({"adapter":if persistent {"grafeo_reopened"} else {"memory"},"files":count,"changed_sources":changed_sources,"reported_modified":report.modified,"upserted":store.batches[0],
        "matches_clean_rebuild":true,"calls":actual.all_edges()?.iter().filter(|e|e.kind==EdgeKind::Calls).count()}),
    )
}

fn state(store: &dyn GraphStore, target: &NodeId) -> Result<Value, Box<dyn std::error::Error>> {
    let snapshot = store.snapshot()?;
    let incoming: Vec<_> = snapshot
        .all_edges()?
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Calls && edge.to.as_ref() == Some(target))
        .collect();
    let occurrences: Vec<_> = snapshot
        .occurrence_files()
        .iter()
        .flat_map(|(path, file)| {
            file.records
                .iter()
                .filter(move |r| r.target.as_ref() == Some(target))
                .map(move |record| json!({"path":path,"record":record}))
        })
        .collect();
    Ok(json!({"target":snapshot.node_by_id(target)?,"incoming":incoming,"occurrences":occurrences}))
}

fn run(root: &Path, store: &mut dyn GraphStore) -> Result<Value, Box<dyn std::error::Error>> {
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    Projector::new(&registry, &policy).reindex(root, store)?;
    let snapshot = store.snapshot()?;
    let nodes = snapshot.all_nodes()?;
    let target = nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("leaf"))
        .ok_or("missing leaf")?
        .id
        .clone();
    let owners: BTreeMap<_, _> = nodes
        .iter()
        .map(|node| (&node.id, node.path.as_str()))
        .collect();
    let projection = FileProjection {
        file: nodes
            .iter()
            .find(|node| node.path == "leaf.js" && node.is_file())
            .ok_or("missing file")?
            .clone(),
        symbols: nodes
            .iter()
            .filter(|node| node.path == "leaf.js" && !node.is_file())
            .cloned()
            .collect(),
        source: snapshot.source_files().get("leaf.js").cloned(),
        occurrences: snapshot.occurrence_files().get("leaf.js").cloned(),
        edges: snapshot
            .all_edges()?
            .into_iter()
            .filter(|edge| {
                edge.path
                    .as_deref()
                    .or_else(|| owners.get(&edge.from).copied())
                    == Some("leaf.js")
            })
            .collect(),
        ..FileProjection::default()
    };
    drop(snapshot);
    let before = state(store, &target)?;
    assert!(
        !before["incoming"]
            .as_array()
            .ok_or("incoming array")?
            .is_empty(),
        "fixture must resolve a caller"
    );
    assert!(
        !before["occurrences"]
            .as_array()
            .ok_or("occurrence array")?
            .is_empty(),
        "fixture must bind occurrences"
    );
    let outcome = store.apply(WriteBatch {
        upserts: vec![projection],
        ..WriteBatch::default()
    })?;
    let after = state(store, &target)?;
    assert_eq!(before["target"], after["target"]);
    assert_eq!(before["occurrences"], after["occurrences"]);
    assert_eq!(before["incoming"], after["incoming"]);
    Ok(
        json!({"before":before,"after_identical_target_replacement":after,"outcome":outcome,"target":target}),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    for (path, text) in [
        ("leaf.js", "export function leaf() { return 1; }\n"),
        (
            "caller.js",
            "import {leaf} from './leaf.js';\nexport function caller() { return leaf(); }\n",
        ),
    ] {
        std::fs::write(root.path().join(path), text)?;
    }
    let memory = run(root.path(), &mut MemoryStore::new())?;
    let directory = tempfile::tempdir()?;
    let mut store = GrafeoStore::open(directory.path(), &StoreOptions::default())?;
    let mut persistent = run(root.path(), &mut store)?;
    drop(store);
    let reopened = GrafeoStore::open(directory.path(), &StoreOptions::default())?;
    let target: NodeId = serde_json::from_value(persistent["target"].clone())?;
    let after_reopen = state(&reopened, &target)?;
    assert_eq!(
        after_reopen,
        persistent["after_identical_target_replacement"]
    );
    persistent["after_reopen"] = after_reopen;
    let repairs = [2, 8, 32]
        .into_iter()
        .flat_map(|count| [false, true].map(move |persistent| (count, persistent)))
        .map(|(count, persistent)| repair_chain(count, persistent))
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"memory":memory,"persistent":persistent,"repair_chains":repairs})
        )?
    );
    Ok(())
}
