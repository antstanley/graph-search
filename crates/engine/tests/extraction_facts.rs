//! Selective cold-fact reads must describe the handle's committed generation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(unused_imports)]
use graph_search_core::dependencies::DependencyLookup as _;
use graph_search_core::{conformance, memory::MemoryStore, ports::GraphStore};
use graph_search_engine::{NativeStore, StoreOptions};
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
        Box::new(NativeStore::open(directory.path(), &StoreOptions::default()).unwrap()),
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
    let mut writer = NativeStore::open(directory.path(), &options).unwrap();
    let first = fixture("first");
    writer.publish(first.clone()).unwrap();
    let reader = NativeStore::open(directory.path(), &options).unwrap();
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
    let reopened = NativeStore::open(directory.path(), &options).unwrap();
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
fn dependency_records_are_pinned_and_verified_with_the_generation() {
    let directory = tempfile::tempdir().unwrap();
    let options = StoreOptions::default();
    let record = |store: &NativeStore| {
        store
            .dependency_index()
            .unwrap()
            .expect("coherent records")
            .record("src/a.rs")
    };
    let mut writer = NativeStore::open(directory.path(), &options).unwrap();
    writer.publish(fixture("first")).unwrap();
    let first = record(&writer).unwrap();
    assert!(first.is_some());
    let reader = NativeStore::open(directory.path(), &options).unwrap();
    assert_eq!(record(&reader).unwrap(), first);
    writer.publish(fixture("second")).unwrap();
    assert_ne!(record(&writer).unwrap(), first);
    assert_eq!(record(&reader).unwrap(), first);
    // Records are verified by their first reader, not by open.
    let pointer: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("CURRENT")).unwrap()).unwrap();
    let dir = directory
        .path()
        .join("generations")
        .join(pointer["id"].as_str().unwrap());
    std::fs::write(dir.join("dependencies.json"), b"corrupt").unwrap();
    let damaged = NativeStore::open(directory.path(), &options).unwrap();
    assert!(record(&damaged).is_err());
    drop(damaged);
    // Old readers retain their admitted records, independent of later artifacts.
    assert_eq!(record(&reader).unwrap(), first);
}
