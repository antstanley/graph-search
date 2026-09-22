//! Framework script regions: extraction, coordinates, coverage and persistence.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::GraphStore;
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::source::SourceFileUnits;
use graph_search_types::{ExploreQuery, Language, NodeKind, SymbolQuery};
use std::{collections::BTreeMap, path::Path};

fn facts(store: &Path) -> BTreeMap<String, SourceFileUnits> {
    let store = GrafeoStore::open(store, &StoreOptions::default()).unwrap();
    store.snapshot().unwrap().source_files().unwrap().clone()
}

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn svelte_regions_are_searchable_offset_and_reported() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let source = "<script context=\"module\">\nexport const moduleNeedle = 1;\n</script>\n\
                  <p>templateMarker</p>\n\
                  <script lang=\"ts\">\nexport function instanceNeedle(): number { return 2; }\n</script>\n";
    std::fs::write(root.path().join("Widget.svelte"), source).unwrap();
    let index = open(root.path(), store.path());
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.source_framework_region_files, 1);
    assert_eq!(report.coverage.source_framework_regions, 2);
    assert_eq!(report.coverage.source_framework_unextracted_regions, 0);

    // Both declared domains resolve.
    for name in ["moduleNeedle", "instanceNeedle"] {
        let result = index.search().symbol(&SymbolQuery::new(name)).unwrap();
        assert!(
            result.nodes.iter().any(|node| node.path == "Widget.svelte"),
            "{name} was not extracted from a declared script region"
        );
    }
    // The file node reports the framework language, not `unknown`.
    let store_handle = GrafeoStore::open(store.path(), &StoreOptions::default()).unwrap();
    let snapshot = store_handle.snapshot().unwrap();
    let file = snapshot
        .find_by_name("Widget.svelte", &[NodeKind::File], 1)
        .unwrap();
    assert_eq!(file[0].item.language, Some(Language::Svelte));
    // Template markup stays body-searchable even though it is not modeled.
    let explore = index
        .search()
        .explore(&ExploreQuery::new("templateMarker"))
        .unwrap();
    assert!(
        explore
            .items
            .iter()
            .any(|item| item.node.path == "Widget.svelte")
    );

    // Region facts persist in the generation's source records.
    let persisted = facts(store.path());
    let widget = &persisted["Widget.svelte"];
    assert_eq!(widget.embedded_regions, 2);
    assert_eq!(widget.embedded_unextracted_regions, 0);
    assert!(!widget.embedded_truncated);
}

#[test]
fn unmodeled_regions_are_visible_in_coverage_rather_than_lost() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let source = "<script lang=\"coffee\">\nignored = 1\n</script>\n<script setup>\nconst ok = 1;\n</script>\n";
    std::fs::write(root.path().join("Panel.vue"), source).unwrap();
    let index = open(root.path(), store.path());
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.source_framework_regions, 2);
    assert_eq!(report.coverage.source_framework_unextracted_regions, 1);
    assert_eq!(report.coverage.source_framework_truncated_files, 0);
    assert!(
        index
            .search()
            .symbol(&SymbolQuery::new("ok"))
            .unwrap()
            .nodes
            .iter()
            .any(|node| node.path == "Panel.vue")
    );
    assert!(
        index
            .search()
            .symbol(&SymbolQuery::new("ignored"))
            .unwrap()
            .nodes
            .is_empty()
    );
}

#[test]
fn astro_frontmatter_and_incremental_edits_match_a_clean_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let path = root.path().join("page.astro");
    let first = "---\nexport const beforeNeedle = 1;\n---\n<h1>{beforeNeedle}</h1>\n";
    std::fs::write(&path, first).unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let before = facts(store.path());
    assert_eq!(before["page.astro"].embedded_regions, 1);

    let second = "---\nexport const afterNeedle = 2;\n---\n<h1>{afterNeedle}</h1>\n<script>\nconsole.log(afterNeedle);\n</script>\n";
    std::fs::write(&path, second).unwrap();
    index.sync().unwrap();
    let incremental = facts(store.path());
    assert_eq!(incremental["page.astro"].embedded_regions, 2);

    // A clean rebuild of the same source produces identical persisted facts.
    let clean_store = tempfile::tempdir().unwrap();
    let clean = open(root.path(), clean_store.path());
    clean.reindex().unwrap();
    let rebuilt = facts(clean_store.path());
    for (path, units) in &incremental {
        let other = &rebuilt[path];
        assert_eq!(units.embedded_regions, other.embedded_regions);
        assert_eq!(
            units.embedded_unextracted_regions,
            other.embedded_unextracted_regions
        );
        assert_eq!(
            units
                .units
                .iter()
                .map(|unit| unit.span.start_byte)
                .collect::<Vec<_>>(),
            other
                .units
                .iter()
                .map(|unit| unit.span.start_byte)
                .collect::<Vec<_>>()
        );
    }
    assert!(
        index
            .search()
            .symbol(&SymbolQuery::new("afterNeedle"))
            .unwrap()
            .nodes
            .iter()
            .any(|node| node.path == "page.astro")
    );
}
