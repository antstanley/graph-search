//! Selective cold-fact reads must describe the handle's committed generation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search_core::{conformance, memory::MemoryStore, ports::GraphStore};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{
    EdgeKind, WriteBatch,
    extraction::{Extraction, ReferenceFact},
    manifest::FileEntry,
};
use std::collections::BTreeSet;

fn fixture(name: &str) -> WriteBatch {
    let mut batch = conformance::fixture_batch();
    for file in &batch.upserts {
        batch.manifest.entries.insert(
            file.file.path.clone(),
            FileEntry {
                content_hash: file.file.content_hash.clone().unwrap(),
                extraction: Some(
                    Extraction {
                        references: vec![ReferenceFact::file_level(EdgeKind::Calls, name, 1)],
                        ..Extraction::default()
                    }
                    .into(),
                ),
                size: file.file.bytes.unwrap_or_default(),
                mtime_ns: 0,
                parser_version: graph_search_types::limits::PARSER_VERSION,
                schema_version: graph_search_types::limits::SCHEMA_VERSION,
                quarantine: None,
            },
        );
    }
    batch
}
#[test]
fn native_adapters_select_facts_and_omit_absent_or_missing_caches() {
    let directory = tempfile::tempdir().unwrap();
    let mut stores: Vec<Box<dyn GraphStore>> = vec![
        Box::new(MemoryStore::new()),
        Box::new(GrafeoStore::open(directory.path(), &StoreOptions::default()).unwrap()),
    ];
    for store in &mut stores {
        let mut batch = fixture("initial");
        batch
            .manifest
            .entries
            .get_mut("src/b.rs")
            .unwrap()
            .extraction = None;
        let expected = batch.manifest.entries["src/a.rs"]
            .extraction
            .clone()
            .unwrap();
        store.publish(batch).unwrap();
        assert!(store.extraction_facts(&BTreeSet::new()).unwrap().is_empty());
        assert!(
            store
                .extraction_facts(&BTreeSet::from(["absent".into(), "src/b.rs".into()]))
                .unwrap()
                .is_empty()
        );
        let facts = store
            .extraction_facts(&BTreeSet::from([
                "src/a.rs".into(),
                "src/b.rs".into(),
                "absent".into(),
            ]))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts["src/a.rs"], expected);
    }
}
#[test]
fn selected_reads_remain_pinned_across_publication_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let options = StoreOptions::default();
    let mut writer = GrafeoStore::open(directory.path(), &options).unwrap();
    let first = fixture("first");
    writer.publish(first.clone()).unwrap();
    let reader = GrafeoStore::open(directory.path(), &options).unwrap();
    let paths = BTreeSet::from([String::from("src/a.rs")]);
    for n in 0..4 {
        writer.publish(fixture(&format!("next-{n}"))).unwrap();
    }
    assert_eq!(
        reader.extraction_facts(&paths).unwrap()["src/a.rs"],
        *first.manifest.entries["src/a.rs"]
            .extraction
            .as_ref()
            .unwrap()
    );
    let reopened = GrafeoStore::open(directory.path(), &options).unwrap();
    assert_eq!(
        reopened.extraction_facts(&paths).unwrap(),
        writer.extraction_facts(&paths).unwrap()
    );
    assert_ne!(
        reader.extraction_facts(&paths).unwrap(),
        reopened.extraction_facts(&paths).unwrap()
    );
}
#[test]
fn legacy_embedded_facts_keep_compatible_selected_results() {
    let directory = tempfile::tempdir().unwrap();
    let batch = fixture("legacy");
    graph_search_engine::sidecar::save_manifest(directory.path(), &batch.manifest).unwrap();
    let reader = GrafeoStore::open(directory.path(), &StoreOptions::default()).unwrap();
    let facts = reader
        .extraction_facts(&BTreeSet::from([String::from("src/a.rs")]))
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts["src/a.rs"],
        *batch.manifest.entries["src/a.rs"]
            .extraction
            .as_ref()
            .unwrap()
    );
}

#[test]
fn dependency_records_are_pinned_and_authenticated_with_the_generation() {
    let directory = tempfile::tempdir().unwrap();
    let options = StoreOptions::default();
    let mut writer = GrafeoStore::open(directory.path(), &options).unwrap();
    writer.publish(fixture("first")).unwrap();
    let first = writer.dependency_index().unwrap().unwrap().clone();
    let reader = GrafeoStore::open(directory.path(), &options).unwrap();
    assert_eq!(reader.dependency_index().unwrap(), Some(&first));
    writer.publish(fixture("second")).unwrap();
    assert_ne!(writer.dependency_index().unwrap(), Some(&first));
    assert_eq!(reader.dependency_index().unwrap(), Some(&first));
    let pointer_path = directory.path().join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let dir = directory
        .path()
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    let path = dir.join("dependencies.json");
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(GrafeoStore::open(directory.path(), &options).is_err());
    // Even a rehashed artifact must match the authenticated manifest header.
    let mut malformed: serde_json::Value = serde_json::from_slice(&original).unwrap();
    malformed["records"]["src/a.rs"]["header"]["content_hash"] = "foreign".into();
    let bytes = serde_json::to_vec(&malformed).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    pointer["files"]["dependencies.json"] = graph_search_core::hash::content_hash(&bytes).into();
    std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    assert!(GrafeoStore::open(directory.path(), &options).is_err());
    // Old readers retain the admitted record set, independent of later artifacts.
    assert_eq!(reader.dependency_index().unwrap(), Some(&first));
}

#[test]
fn format_six_uses_fallback_and_next_publication_adds_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let options = StoreOptions::default();
    let mut writer = GrafeoStore::open(directory.path(), &options).unwrap();
    writer.publish(fixture("first")).unwrap();
    let pointer_path = directory.path().join("CURRENT");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&pointer_path).unwrap()).unwrap();
    let dir = directory
        .path()
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    pointer["format"] = 6.into();
    pointer["files"]
        .as_object_mut()
        .unwrap()
        .remove("dependencies.json");
    std::fs::remove_file(dir.join("dependencies.json")).unwrap();
    std::fs::write(&pointer_path, serde_json::to_vec(&pointer).unwrap()).unwrap();
    drop(writer);
    let mut legacy = GrafeoStore::open(directory.path(), &options).unwrap();
    assert!(legacy.dependency_index().unwrap().is_none());
    legacy.publish(fixture("second")).unwrap();
    assert!(legacy.dependency_index().unwrap().is_some());
    let reopened = GrafeoStore::open(directory.path(), &options).unwrap();
    assert_eq!(
        legacy.dependency_index().unwrap(),
        reopened.dependency_index().unwrap()
    );
}
