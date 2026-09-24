//! Regression gates for lexical retrieval and cached dependency rebinding.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_core::config::WalkPolicy;
use graph_search_core::ports::{
    GraphStore, LanguageExtractor, ListRegistry, ParseError, SourceFile,
};
use graph_search_core::reconcile::Projector;
use graph_search_engine::{NativeStore, StoreOptions};
use graph_search_types::{ExploreQuery, Language};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn write(root: &Path, path: &str, text: &str) {
    std::fs::write(root.join(path), text).unwrap();
}
fn index(root: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        ..OpenOptions::default()
    })
    .unwrap()
}

#[test]
fn lexical_retrieval_matches_identifiers_signatures_and_exact_names() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "a.rs",
        "fn load_pds_secret() {} fn secret() {} fn parse(value: DistinctivePayload) {} fn is() {} fn HTTPServer() {}",
    );
    let i = index(d.path());
    i.reindex().unwrap();
    for (q, expected) in [
        ("load pds secret", "load_pds_secret"),
        ("secret", "secret"),
        ("DistinctivePayload", "parse"),
        ("is", "is"),
        ("http server", "HTTPServer"),
    ] {
        let r = i.search().explore(&ExploreQuery::new(q)).unwrap();
        assert_eq!(r.items[0].node.name, expected, "{q}: {r:?}");
    }
    let r = i
        .search()
        .explore(&ExploreQuery::new("zzzzunmatchabletoken"))
        .unwrap();
    assert!(r.items.is_empty(), "{r:?}");
    let names = |q| {
        i.search()
            .explore(&ExploreQuery::new(q))
            .unwrap()
            .items
            .into_iter()
            .map(|x| x.node.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("load pds secret"),
        names("load load pds secret secret")
    );
}

struct CountingExtractor {
    inner: Box<dyn LanguageExtractor>,
    parsed: Arc<Mutex<Vec<String>>>,
}
impl LanguageExtractor for CountingExtractor {
    fn language(&self) -> Language {
        self.inner.language()
    }
    fn supports(&self, path: &Path) -> bool {
        self.inner.supports(path)
    }
    fn extract(
        &self,
        file: &SourceFile<'_>,
    ) -> Result<graph_search_core::extraction::Extraction, ParseError> {
        self.parsed
            .lock()
            .unwrap()
            .push(file.path.to_string_lossy().into_owned());
        self.inner.extract(file)
    }
}
fn counting_registry(parsed: &Arc<Mutex<Vec<String>>>) -> ListRegistry {
    ListRegistry::new(
        graph_search_langs::all_extractors()
            .into_iter()
            .map(|inner| {
                Box::new(CountingExtractor {
                    inner,
                    parsed: Arc::clone(parsed),
                }) as Box<dyn LanguageExtractor>
            })
            .collect(),
    )
}
fn graph(store: &dyn GraphStore) -> (Vec<String>, Vec<String>) {
    let s = store.snapshot().unwrap();
    let mut nodes: Vec<_> = s
        .all_nodes()
        .unwrap()
        .into_iter()
        .map(|n| serde_json::to_string(&n).unwrap())
        .collect();
    let mut edges: Vec<_> = s
        .all_edges()
        .unwrap()
        .into_iter()
        .map(|e| serde_json::to_string(&e).unwrap())
        .collect();
    nodes.sort();
    edges.sort();
    edges.dedup();
    (nodes, edges)
}
fn assert_fresh(root: &Path, store: &dyn GraphStore) {
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let p = Projector::new(&registry, &policy);
    let mut fresh = graph_search_core::memory::MemoryStore::new();
    p.reindex(root, &mut fresh).unwrap();
    assert_eq!(
        graph(store),
        graph(&fresh),
        "incremental projection must equal clean reindex"
    );
}

#[test]
fn selective_sync_reuses_persisted_facts_and_tracks_unresolved_and_ambiguous_names() {
    let root = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    write(root.path(), "a.rs", "fn entry() { leaf(); missing(); }");
    write(root.path(), "b.rs", "fn leaf() {}");
    write(root.path(), "c.rs", "fn outer() { entry(); }");
    write(root.path(), "z.rs", "fn independent() {}");
    let parsed = Arc::new(Mutex::new(Vec::new()));
    let registry = counting_registry(&parsed);
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    {
        let mut store = NativeStore::open(db.path(), &StoreOptions::default()).unwrap();
        projector.reindex(root.path(), &mut store).unwrap();
    }
    // Reopening ensures this isn't merely an in-process cache.
    let mut store = NativeStore::open(db.path(), &StoreOptions::default()).unwrap();
    parsed.lock().unwrap().clear();
    write(root.path(), "b.rs", "fn leaf() { /* changed body */ }");
    let report = projector.sync(root.path(), &mut store).unwrap();
    assert_eq!(*parsed.lock().unwrap(), ["b.rs"]);
    assert_eq!(report.modified, ["b.rs"]);
    assert_eq!(report.unchanged, 3);
    assert_fresh(root.path(), &store);
    // Addition resolves an old dangling call and makes the old unique name ambiguous.
    parsed.lock().unwrap().clear();
    write(root.path(), "new.rs", "fn missing() {} fn leaf() {}");
    projector.sync(root.path(), &mut store).unwrap();
    assert_eq!(*parsed.lock().unwrap(), ["new.rs"]);
    assert_fresh(root.path(), &store);
    // Deletion resolves ambiguity and makes missing dangling again, without parsing.
    parsed.lock().unwrap().clear();
    std::fs::remove_file(root.path().join("new.rs")).unwrap();
    projector.sync(root.path(), &mut store).unwrap();
    assert!(parsed.lock().unwrap().is_empty());
    assert_fresh(root.path(), &store);
    // Rename changes stable IDs, then no-op preserves all projections.
    std::fs::rename(root.path().join("b.rs"), root.path().join("renamed.rs")).unwrap();
    projector.sync(root.path(), &mut store).unwrap();
    assert_fresh(root.path(), &store);
    parsed.lock().unwrap().clear();
    let report = projector.sync(root.path(), &mut store).unwrap();
    assert_eq!(report.unchanged, 4);
    assert!(report.modified.is_empty());
    assert!(parsed.lock().unwrap().is_empty());
}

#[test]
fn import_precedence_and_cross_language_changes_match_clean_reindex() {
    let root = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "use.ts",
        "import { leaf } from './target'; function entry() { leaf(); }",
    );
    write(
        root.path(),
        "index.html",
        "<a class=\"active\" href=\"next.html\">Next</a>",
    );
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = NativeStore::open(db.path(), &StoreOptions::default()).unwrap();
    projector.reindex(root.path(), &mut store).unwrap();
    for (path, content) in [
        ("target.js", "export function leaf() {}"),
        ("target.ts", "export function leaf() {}"),
        ("style.css", ".active { color: red; }"),
        ("next.html", "<p id=\"target\">Hello</p>"),
        ("style.css", ".other { color: blue; }"),
    ] {
        write(root.path(), path, content);
        projector.sync(root.path(), &mut store).unwrap();
        assert_fresh(root.path(), &store);
    }
    std::fs::remove_file(root.path().join("target.ts")).unwrap();
    projector.sync(root.path(), &mut store).unwrap();
    assert_fresh(root.path(), &store);
}

#[test]
fn old_manifests_rebuild_and_missing_fact_caches_fall_back_safely() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.rs", "fn entry() { leaf(); }");
    write(root.path(), "b.rs", "fn leaf() {}");
    let parsed = Arc::new(Mutex::new(Vec::new()));
    let registry = counting_registry(&parsed);
    let policy = WalkPolicy::default();
    let projector = Projector::new(&registry, &policy);
    let mut store = graph_search_core::memory::MemoryStore::new();
    projector.reindex(root.path(), &mut store).unwrap();
    let mut old = store.manifest().unwrap().unwrap();
    old.schema_version = 1;
    for entry in old.entries.values_mut() {
        entry.extraction = None;
    }
    store.commit_manifest(old).unwrap();
    parsed.lock().unwrap().clear();
    assert!(
        projector
            .sync(root.path(), &mut store)
            .unwrap()
            .reindexed_all
    );
    assert_eq!(parsed.lock().unwrap().len(), 2);
    assert_fresh(root.path(), &store);
    let mut old = store.manifest().unwrap().unwrap();
    old.entries.get_mut("a.rs").unwrap().extraction = None;
    store.commit_manifest(old).unwrap();
    parsed.lock().unwrap().clear();
    // No binding change seeds repair here. The missing raw cache must still
    // force conservative reconstruction, including parsing its unknown owner.
    write(root.path(), "b.rs", "fn leaf() { /* body only */ }");
    let report = projector.sync(root.path(), &mut store).unwrap();
    let mut reparsed = parsed.lock().unwrap().clone();
    reparsed.sort();
    assert_eq!(reparsed, ["a.rs", "b.rs"]);
    assert_eq!(report.modified, ["a.rs", "b.rs"]);
    assert_fresh(root.path(), &store);
}
