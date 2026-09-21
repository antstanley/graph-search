//! Public source-occurrence evidence, independent of adjacency and live file changes.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::work::{CancellationToken, WorkLimits};
use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery, ResolutionClass};
use graph_search_types::{EdgeKind, TruncationKind};

fn query(target: &str, by: OccurrenceBy) -> OccurrenceQuery {
    OccurrenceQuery {
        target: target.into(),
        by,
        ..OccurrenceQuery::default()
    }
}
fn open(root: &std::path::Path, reconcile: Reconcile) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        reconcile,
        ..OpenOptions::default()
    })
    .unwrap()
}

#[test]
fn routes_preserve_distinct_unicode_crlf_sites_and_resolution_evidence() {
    let root = tempfile::tempdir().unwrap();
    let source =
        "function send(){}\r\nfunction entry(){const café=1; send(); send(); missing();}\r\n";
    std::fs::write(root.path().join("calls.ts"), source).unwrap();
    let index = open(root.path(), Reconcile::BeforeQuery);
    index.reindex().unwrap();
    drop(index);
    let index = open(root.path(), Reconcile::Never);
    let target = index
        .search()
        .occurrences(&query("send", OccurrenceBy::Target))
        .unwrap();
    assert_eq!(target.items.len(), 2);
    assert_eq!(target.extracted_files, 1);
    assert_ne!(target.items[0].occurrence.id, target.items[1].occurrence.id);
    for item in &target.items {
        let span = item.occurrence.span.unwrap();
        assert_eq!(
            &source[span.start_byte as usize..span.end_byte as usize],
            "send()"
        );
        assert_eq!(
            item.source_hash,
            graph_search_core::hash::content_hash(source.as_bytes())
        );
        assert_eq!(item.occurrence.resolution, ResolutionClass::ExplicitLexical);
    }
    let mut owner_query = query("entry", OccurrenceBy::Owner);
    owner_query.kind = Some(EdgeKind::Calls);
    let owner = index.search().occurrences(&owner_query).unwrap();
    assert_eq!(owner.items.len(), 3);
    let missing = index
        .search()
        .occurrences(&query("missing", OccurrenceBy::Name))
        .unwrap();
    assert_eq!(missing.items.len(), 1);
    assert!(missing.items[0].occurrence.target.is_none());
    assert!(missing.items[0].occurrence.reason.is_some());
    assert!(
        index
            .search()
            .occurrences(&query("Missing", OccurrenceBy::Name))
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn caps_charge_filtered_occurrences_and_observe_cancellation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("calls.ts"),
        "function send(){} function entry(){send();send();send();}",
    )
    .unwrap();
    let index = open(root.path(), Reconcile::BeforeQuery);
    index.reindex().unwrap();
    let mut request = query("send", OccurrenceBy::Name);
    request.limit = 1;
    let result = index.search().occurrences(&request).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.stats.occurrences_examined, 2);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Results)
    );
    request.kind = Some(EdgeKind::Imports);
    let result = index
        .search()
        .with_work_limits(WorkLimits {
            occurrences: 2,
            ..WorkLimits::default()
        })
        .occurrences(&request)
        .unwrap();
    assert!(result.items.is_empty());
    assert_eq!(result.stats.occurrences_examined, 2);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Occurrences)
    );
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        index
            .search()
            .with_work_limits(WorkLimits {
                cancellation: Some(token),
                ..WorkLimits::default()
            })
            .occurrences(&request),
        Err(graph_search::Error::Core(
            graph_search_core::Error::QueryCancelled
        ))
    ));
    request.target = "x".repeat(8193);
    assert!(index.search().occurrences(&request).is_err());
}

#[test]
fn stale_queries_keep_indexed_coordinates_and_rebinding_keeps_source_ids() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("target.ts"), "export function send(){}\n").unwrap();
    std::fs::write(
        root.path().join("caller.ts"),
        "import {send} from './target'; function entry(){send();}\n",
    )
    .unwrap();
    let index = open(root.path(), Reconcile::Never);
    index.reindex().unwrap();
    let mut request = query("send", OccurrenceBy::Name);
    request.kind = Some(EdgeKind::Calls);
    let before = index.search().occurrences(&request).unwrap();
    assert_eq!(before.items.len(), 1);
    assert_eq!(
        before.items[0].occurrence.resolution,
        ResolutionClass::ExplicitImport
    );
    std::fs::remove_file(root.path().join("target.ts")).unwrap();
    let stale = index.search().occurrences(&request).unwrap();
    assert!(stale.context.staleness.changed > 0);
    assert_eq!(stale.items, before.items);
    index.sync().unwrap();
    let rebound = index.search().occurrences(&request).unwrap();
    assert_eq!(
        rebound.items[0].occurrence.id,
        before.items[0].occurrence.id
    );
    assert_eq!(rebound.items[0].source_hash, before.items[0].source_hash);
    assert!(rebound.items[0].occurrence.target.is_none());
}

#[test]
fn serialized_occurrence_payload_drops_whole_records_at_the_byte_cap() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("calls.ts"),
        format!("function entry(){{{}}}", "missing();".repeat(600)),
    )
    .unwrap();
    let index = open(root.path(), Reconcile::BeforeQuery);
    let mut request = query("missing", OccurrenceBy::Name);
    request.limit = u32::MAX;
    let result = index.search().occurrences(&request).unwrap();
    assert!(!result.items.is_empty());
    assert!(result.items.len() < 500);
    assert!(
        result
            .truncations
            .iter()
            .any(|t| t.kind == TruncationKind::Bytes)
    );
    assert!(serde_json::to_vec(&result).unwrap().len() <= 65_536);
    assert!(
        result
            .items
            .iter()
            .all(|item| !item.source_hash.is_empty() && item.occurrence.span.is_some())
    );
}
