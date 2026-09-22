//! Finding E2: `explore` carries a compact detail level so the one-call
//! payload stays below the `text` search it replaces.
//!
//! The library keeps rich evidence by default; a caller that pays per byte
//! selects `ExploreDetail::Compact`, keeping the definition, one bounded primary
//! snippet and the impact summary, and dropping labelled excerpts and
//! matched-body evidence.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use graph_search::{Index, OpenOptions};
use graph_search_types::ExploreDetail;
use graph_search_types::query::ExploreQuery;

fn fixture() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.rs"),
        "/// alpha beta gamma\npub fn alpha_target() -> usize {\n    let beta = 1;\n    let gamma = beta;\n    gamma\n}\n\npub fn caller() -> usize { alpha_target() }\n",
    )
    .unwrap();
    let index = Index::open(OpenOptions {
        root: dir.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    (dir, index)
}

fn bytes(result: &graph_search_types::ExploreResult) -> usize {
    serde_json::to_vec(result).unwrap().len()
}

#[test]
fn compact_keeps_the_seed_and_drops_excerpts_and_evidence() {
    let (_dir, index) = fixture();
    let query = ExploreQuery::new("alpha beta gamma");
    let full = index.search().explore(&query).unwrap();
    let compact = index
        .search()
        .explore(&query.clone().with_detail(ExploreDetail::Compact))
        .unwrap();

    assert!(
        full.items
            .iter()
            .any(|item| item.evidence.is_some() || !item.excerpts.is_empty()),
        "the full default must still deliver evidence: {full:?}"
    );
    assert!(
        compact
            .items
            .iter()
            .all(|item| item.evidence.is_none() && item.excerpts.is_empty()),
        "compact must not publish excerpts or evidence: {compact:?}"
    );
    assert_eq!(
        compact.items.len(),
        full.items.len(),
        "detail must not change which seeds are returned"
    );
    assert!(
        compact.items.iter().all(|item| item.snippet.is_some()),
        "compact still carries the primary snippet"
    );
    assert!(
        bytes(&compact) < bytes(&full),
        "compact payload {} must be smaller than full {}",
        bytes(&compact),
        bytes(&full)
    );
}

#[test]
fn detail_round_trips_through_json_for_old_payloads() {
    let query = ExploreQuery::new("alpha");
    let mut value = serde_json::to_value(&query).unwrap();
    // A query serialized before the field existed still deserializes.
    value.as_object_mut().unwrap().remove("detail");
    let decoded: ExploreQuery = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.detail, ExploreDetail::Full);
}
