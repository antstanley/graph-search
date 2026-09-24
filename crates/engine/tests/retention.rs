//! Retention is explicit, generation checked, and validated before mutation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search_core::{
    conformance, memory::MemoryStore, ports::GraphStore, retention::FactRetention,
};
use graph_search_engine::{NativeStore, StoreOptions};
use graph_search_types::{WriteBatch, extraction::Extraction, manifest::FileEntry};
use std::collections::BTreeSet;
fn fixture() -> WriteBatch {
    let mut batch = conformance::fixture_batch();
    for file in &batch.upserts {
        batch.manifest.entries.insert(
            file.file.path.clone(),
            FileEntry {
                size: file.file.bytes.unwrap(),
                mtime_ns: 0,
                content_hash: file.file.content_hash.clone().unwrap(),
                parser_version: file.file.parser_version.unwrap(),
                schema_version: batch.manifest.schema_version,
                quarantine: None,
                extraction: Some(Extraction::default().into()),
            },
        );
    }
    batch
}
#[test]
fn invalid_retention_is_atomic_and_absence_still_removes_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut stores: Vec<Box<dyn GraphStore>> = vec![
        Box::new(MemoryStore::new()),
        Box::new(NativeStore::open(root.path(), &StoreOptions::default()).unwrap()),
    ];
    for store in &mut stores {
        store.publish(fixture()).unwrap();
        let before = store.manifest().unwrap().unwrap();
        let header = before.header();
        let retention = FactRetention {
            generation: store.generation().unwrap(),
            previous: header.clone(),
            paths: header.entries.keys().cloned().collect(),
        };
        let mut batch = WriteBatch::with_manifest(header.clone());
        batch
            .manifest
            .entries
            .get_mut("src/a.rs")
            .unwrap()
            .content_hash = "foreign".into();
        assert!(store.publish_retaining(batch, &retention).is_err());
        let mut stale = retention.clone();
        stale.generation = Some("stale".into());
        assert!(
            store
                .publish_retaining(WriteBatch::with_manifest(header.clone()), &stale)
                .is_err()
        );
        let mut missing = retention.clone();
        missing.paths.insert("absent.rs".into());
        assert!(
            store
                .publish_retaining(WriteBatch::with_manifest(header.clone()), &missing)
                .is_err()
        );
        let mut overlap = WriteBatch::with_manifest(header.clone());
        overlap.upserts = fixture().upserts;
        assert!(store.publish_retaining(overlap, &retention).is_err());
        assert_eq!(store.manifest().unwrap().unwrap(), before);
        let mut next = header;
        next.entries.get_mut("src/a.rs").unwrap().mtime_ns = 99;
        store
            .publish_retaining(WriteBatch::with_manifest(next.clone()), &retention)
            .unwrap();
        assert_eq!(
            store.manifest().unwrap().unwrap().entries["src/a.rs"].extraction,
            before.entries["src/a.rs"].extraction
        );
        assert_eq!(store.manifest_header().unwrap().unwrap(), next);
        // Normal publication's None is removal, not implicit reuse.
        store.publish(WriteBatch::with_manifest(next)).unwrap();
        assert!(
            store
                .extraction_facts(&BTreeSet::from(["src/a.rs".into()]))
                .unwrap()
                .is_empty()
        );
        let now = store.manifest_header().unwrap().unwrap();
        let absent = FactRetention {
            generation: store.generation().unwrap(),
            previous: now.clone(),
            paths: BTreeSet::from(["src/a.rs".into()]),
        };
        assert!(
            store
                .publish_retaining(WriteBatch::with_manifest(now), &absent)
                .is_err()
        );
    }
}
