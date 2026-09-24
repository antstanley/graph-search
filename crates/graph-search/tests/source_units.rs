//! Native source facts share graph publication and incremental lifecycle.
#![allow(clippy::unwrap_used)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_engine::{NativeStore, StoreOptions};
use graph_search_types::source::{SourceFileUnits, SourceUnitKind};
use std::collections::BTreeMap;

fn open(root: &std::path::Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        reconcile: Reconcile::Never,
        ..OpenOptions::default()
    })
    .unwrap()
}
fn facts(index: &Index) -> BTreeMap<String, SourceFileUnits> {
    let store = NativeStore::open(index.store_dir(), &StoreOptions::default()).unwrap();
    store.snapshot().unwrap().source_files().unwrap().clone()
}

#[test]
fn parser_and_nonparser_text_publish_hash_bound_source_regions() {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("a.rs", "fn original() { /* cache invalidation */ }\n"),
        ("settings.json", "{\"cache_key\":\"secret_missing\"}\n"),
        ("guide.md", "# Cache\nHandle invalidation carefully.\n"),
        ("notes.txt", "élève cache\r\n"),
    ] {
        std::fs::write(root.path().join(path), text).unwrap();
    }
    let index = open(root.path());
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.source_indexed_files, 4);
    let facts = facts(&index);
    for (path, source) in &facts {
        let bytes = std::fs::read(root.path().join(path)).unwrap();
        assert_eq!(
            source.source_hash,
            graph_search_core::hash::content_hash(&bytes)
        );
        assert!(!source.truncated);
        assert!(!source.units.is_empty());
    }
    assert_eq!(
        facts["settings.json"].units[0].kind,
        SourceUnitKind::Configuration
    );
    assert_eq!(facts["guide.md"].units[0].kind, SourceUnitKind::Markdown);
    assert!(
        facts["a.rs"]
            .units
            .iter()
            .any(|unit| unit.terms.contains_key("invalidation"))
    );
    assert_eq!(
        index
            .search()
            .status()
            .unwrap()
            .coverage
            .source_indexed_files,
        4
    );
}

#[test]
fn changed_removed_renamed_and_rebound_source_facts_match_a_clean_build() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn caller(){ target(); }\n").unwrap();
    std::fs::write(root.path().join("b.rs"), "fn target(){}\n").unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    let original = facts(&index);
    std::fs::write(
        root.path().join("b.rs"),
        "fn replacement(){let marker=\"persisted_body\";}\n",
    )
    .unwrap();
    index.sync().unwrap();
    let changed = facts(&index);
    assert_eq!(changed["a.rs"], original["a.rs"]);
    assert!(
        changed["b.rs"]
            .units
            .iter()
            .any(|unit| unit.terms.contains_key("persisted"))
    );
    std::fs::remove_file(root.path().join("b.rs")).unwrap();
    std::fs::rename(root.path().join("a.rs"), root.path().join("renamed.rs")).unwrap();
    index.sync().unwrap();
    let current = facts(&index);
    assert_eq!(current.len(), 1);
    assert!(current.contains_key("renamed.rs"));
    let clean_root = tempfile::tempdir().unwrap();
    std::fs::copy(
        root.path().join("renamed.rs"),
        clean_root.path().join("renamed.rs"),
    )
    .unwrap();
    let clean = open(clean_root.path());
    clean.reindex().unwrap();
    assert_eq!(current, facts(&clean));
}

#[test]
fn a_legacy_generation_rebuilds_source_facts_before_automatic_queries() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn target(){}\n").unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    let store_dir = index.store_dir().to_path_buf();
    drop(index);
    // A generation from before format 10 is never migrated: the store opens
    // unpublished and the first automatic query rebuilds it.
    let pointer_path = store_dir.join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    pointer["format"] = serde_json::json!(9);
    std::fs::write(pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    let result = index
        .search()
        .symbol(&graph_search_types::query::SymbolQuery::new("target"))
        .unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.context.coverage.source_indexed_files, 1);
    assert_eq!(facts(&index).len(), 1);
}

#[test]
fn reopening_rejects_hash_consistent_facts_with_a_foreign_owner() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "fn target(){}\n").unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    let store_dir = index.store_dir().to_path_buf();
    drop(index);
    let pointer_path = store_dir.join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let source_path = store_dir
        .join("generations")
        .join(pointer["id"].as_str().unwrap())
        .join("source-units.json");
    let mut sources: BTreeMap<String, SourceFileUnits> =
        graph_search_engine::sidecar::load_sources(source_path.parent().unwrap()).unwrap();
    sources.get_mut("a.rs").unwrap().units[0].owner =
        Some(graph_search_types::NodeId::new("absent-owner"));
    graph_search_engine::sidecar::save_sources(source_path.parent().unwrap(), &sources).unwrap();
    // Recompute the artifact checksum to exercise semantic validation itself.
    let bytes = std::fs::read(&source_path).unwrap();
    pointer["files"]["source-units.json"] =
        serde_json::json!(graph_search_core::hash::content_hash(&bytes));
    std::fs::write(pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    // Source facts are verified when first read, so opening succeeds and the
    // first reader of those facts fails loudly.
    let store = NativeStore::open(&store_dir, &StoreOptions::default()).unwrap();
    let snapshot = store.snapshot().unwrap();
    let result = snapshot.source_files();
    assert!(matches!(result, Err(error) if error.to_string().contains("source retrieval facts")));
    assert!(
        snapshot.source_files().is_err(),
        "a failed load is not cached"
    );
}

#[test]
fn legacy_identifier_facts_upgrade_without_blocking_reopen_or_using_old_analysis() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "fn inspect() { let key = \"getHTTPResponse\"; }\n",
    )
    .unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    let store_dir = index.store_dir().to_path_buf();
    drop(index);
    let pointer_path = store_dir.join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let generation = store_dir
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    let mut sources = graph_search_engine::sidecar::load_sources(&generation).unwrap();
    for source in sources.values_mut() {
        source.version = 1;
        for unit in &mut source.units {
            unit.identifiers.clear();
        }
    }
    graph_search_engine::sidecar::save_sources(&generation, &sources).unwrap();
    let bytes = std::fs::read(generation.join("source-units.json")).unwrap();
    pointer["files"]["source-units.json"] =
        serde_json::json!(graph_search_core::hash::content_hash(&bytes));
    let manifest_path = generation.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["source_version"] = serde_json::json!(1);
    manifest.as_object_mut().unwrap().remove("analyzer_version");
    manifest
        .as_object_mut()
        .unwrap()
        .remove("analyzer_unicode_version");
    let bytes = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(&manifest_path, &bytes).unwrap();
    pointer["files"]["manifest.json"] =
        serde_json::json!(graph_search_core::hash::content_hash(&bytes));
    std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    let legacy = open(root.path());
    let mut query = graph_search_types::ExploreQuery::new("where gethttpresponse");
    query.retrieval.analysis = graph_search_types::AnalysisMode::Identifiers;
    query.retrieval.ranking = graph_search_types::RankingStrategy::Body;
    let result = legacy.search().explore(&query).unwrap();
    assert!(!result.items.is_empty());
    assert!(
        result
            .items
            .iter()
            .any(|item| item.evidence.as_ref().is_some_and(|e| e.live))
    );
    assert_eq!(
        facts(&legacy)["a.rs"].version,
        1,
        "never-reconcile must not rewrite the generation"
    );
    drop(legacy);
    let automatic = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    let result = automatic.search().explore(&query).unwrap();
    assert!(result.items.iter().any(|item| item.node.name == "inspect"));
    let current = facts(&automatic);
    assert_eq!(
        current["a.rs"].version,
        graph_search_types::limits::SOURCE_INDEX_VERSION
    );
    assert!(
        current["a.rs"]
            .units
            .iter()
            .any(|unit| unit.identifiers.contains_key("getHTTPResponse"))
    );
    let store = NativeStore::open(&store_dir, &StoreOptions::default()).unwrap();
    let manifest = store.manifest().unwrap().unwrap();
    assert_eq!(
        manifest.analyzer_version,
        graph_search_types::limits::ANALYZER_VERSION
    );
    assert_eq!(
        manifest.analyzer_unicode_version,
        graph_search_types::limits::ANALYZER_UNICODE_VERSION
    );
}

#[test]
fn representation_drift_is_reported_and_cannot_reuse_same_hash_body_facts() {
    use graph_search_types::{ExploreQuery, RankingStrategy};
    for field in [
        "chunker_version",
        "analyzer_version",
        "analyzer_unicode_version",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("a.rs"),
            "fn inspect() { let marker = \"cache\"; }\n",
        )
        .unwrap();
        let index = open(root.path());
        index.reindex().unwrap();
        let store_dir = index.store_dir().to_path_buf();
        drop(index);
        let old_generation = change_manifest_revision(&store_dir, field);

        let never = open(root.path());
        let mut query = ExploreQuery::new("where cache");
        query.retrieval.ranking = RankingStrategy::Body;
        let result = never.search().explore(&query).unwrap();
        assert_eq!(
            result.context.generation.as_deref(),
            Some(old_generation.as_str())
        );
        assert_eq!(result.context.staleness.changed, 1);
        assert!(
            !result
                .context
                .indexed_versions
                .unwrap()
                .retrieval_is_current(),
            "{field}"
        );
        assert_eq!(
            result.context.runtime_versions,
            Some(graph_search_types::context::RuntimeVersions::current())
        );
        assert!(!result.items.is_empty());
        assert!(
            result
                .items
                .iter()
                .all(|item| item.evidence.as_ref().is_some_and(|e| e.live)),
            "same-hash incompatible facts were reused: {field}"
        );
        drop(never);

        let automatic = Index::open(OpenOptions {
            root: root.path().into(),
            ..OpenOptions::default()
        })
        .unwrap();
        let result = automatic.search().explore(&query).unwrap();
        assert_ne!(
            result.context.generation.as_deref(),
            Some(old_generation.as_str())
        );
        assert!(
            result
                .context
                .indexed_versions
                .unwrap()
                .retrieval_is_current()
        );
        assert_eq!(result.context.staleness.changed, 0);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.evidence.as_ref().is_some_and(|e| !e.live))
        );
        let current_generation = result.context.generation;
        automatic.sync().unwrap();
        assert_eq!(
            automatic
                .search()
                .explore(&query)
                .unwrap()
                .context
                .generation,
            current_generation
        );
    }
}

fn change_manifest_revision(store_dir: &std::path::Path, field: &str) -> String {
    let pointer_path = store_dir.join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let old_generation = pointer["id"].as_str().unwrap().to_owned();
    let manifest_path = store_dir
        .join("generations")
        .join(&old_generation)
        .join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    value[field] = if field == "analyzer_unicode_version" {
        serde_json::json!([0, 0, 0])
    } else {
        serde_json::json!(0)
    };
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(manifest_path, &bytes).unwrap();
    pointer["files"]["manifest.json"] =
        serde_json::json!(graph_search_core::hash::content_hash(&bytes));
    std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    old_generation
}

#[test]
fn noop_sync_needs_only_the_header_and_timestamp_refresh_keeps_raw_facts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.rs");
    std::fs::write(&path, "fn target() {}\nfn caller() { target(); }\n").unwrap();
    let index = open(root.path());
    index.reindex().unwrap();
    let pointer_path = index.store_dir().join("CURRENT");
    let pointer_bytes = std::fs::read(&pointer_path).unwrap();
    let pointer: serde_json::Value = serde_json::from_slice(&pointer_bytes).unwrap();
    let generation = index
        .store_dir()
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    let before = graph_search_engine::sidecar::load_manifest(&generation)
        .unwrap()
        .unwrap();
    assert!(before.entries["a.rs"].extraction.is_some());
    let pack = std::fs::read_dir(generation.join("extraction-records"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let pack_bytes = std::fs::read(&pack).unwrap();
    std::fs::write(&pack, b"unavailable cold facts").unwrap();
    let noop = index.sync().unwrap();
    assert!(noop.added.is_empty() && noop.modified.is_empty() && noop.removed.is_empty());
    assert_eq!(std::fs::read(&pointer_path).unwrap(), pointer_bytes);
    std::fs::write(pack, pack_bytes).unwrap();

    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified + std::time::Duration::from_secs(2))
        .unwrap();
    let refreshed = index.sync().unwrap();
    assert!(
        refreshed.added.is_empty() && refreshed.modified.is_empty() && refreshed.removed.is_empty()
    );
    let pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let generation = index
        .store_dir()
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    let after = graph_search_engine::sidecar::load_manifest(&generation)
        .unwrap()
        .unwrap();
    assert_eq!(
        after.entries["a.rs"].extraction,
        before.entries["a.rs"].extraction
    );
    assert_ne!(
        after.entries["a.rs"].mtime_ns,
        before.entries["a.rs"].mtime_ns
    );
    drop(index);
    let reopened = open(root.path());
    assert!(reopened.sync().unwrap().modified.is_empty());
}
