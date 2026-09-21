//! Parser-owned documentation facts share the immutable extraction generation.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions};
use graph_search_core::ports::GraphStore;

#[test]
fn documentation_facts_survive_packs_reopen_and_independent_file_edits() {
    let root = tempfile::tempdir().unwrap();
    let source = "/// original café\r\nfn documented() {}\r\n";
    std::fs::write(root.path().join("a.rs"), source).unwrap();
    std::fs::write(root.path().join("b.rs"), source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    let store_path = index.store_dir().to_path_buf();
    drop(index);
    let facts = || {
        let store = graph_search_engine::GrafeoStore::open(
            &store_path,
            &graph_search_engine::StoreOptions::default(),
        )
        .unwrap();
        let manifest = store.manifest().unwrap().unwrap();
        manifest
            .entries
            .into_iter()
            .map(|(path, entry)| (path, entry.extraction.unwrap()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before = facts();
    assert_eq!(before["a.rs"].doc_comments, before["b.rs"].doc_comments);
    assert_eq!(before["a.rs"].doc_comments.len(), 1);
    let comment = &before["a.rs"].doc_comments[0];
    assert_eq!(
        source[comment.span.start_byte as usize..comment.span.end_byte as usize].trim_end(),
        "/// original café"
    );
    assert!(
        before["a.rs"]
            .symbols
            .iter()
            .any(|symbol| Some(&symbol.key) == comment.owner_key.as_ref())
    );
    let index = Index::open(options()).unwrap();
    std::fs::write(
        root.path().join("a.rs"),
        "/** replacement */\nfn renamed() {}\n",
    )
    .unwrap();
    index.sync().unwrap();
    drop(index);
    let after = facts();
    assert_eq!(after["b.rs"], before["b.rs"]);
    assert_ne!(after["a.rs"].doc_comments, before["a.rs"].doc_comments);
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    assert_eq!(facts(), after);
}

#[test]
fn documentation_retrieves_the_documented_declaration_without_claiming_containment() {
    use graph_search_types::{query::ExploreQuery, source::SourceUnitKind};
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "a.rs",
            "/// zirconium café\r\n/// opal\r\nfn operation() {}\r\n",
        ),
        ("b.ts", "/** tourmaline */\nexport function execute() {}\n"),
        ("c.rs", "mod service {\n//! aquamarine\nfn child() {}\n}\n"),
    ] {
        std::fs::write(root.path().join(path), text).unwrap();
    }
    let options = || OpenOptions {
        root: root.path().into(),
        reconcile: graph_search::Reconcile::Never,
        ..OpenOptions::default()
    };
    let index = Index::open(options()).unwrap();
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.source_documentation_truncated_files, 0);
    drop(index);
    let index = Index::open(options()).unwrap();
    for (query, name, inner) in [
        ("zirconium", "operation", false),
        ("tourmaline", "execute", false),
        ("aquamarine", "service", true),
    ] {
        let result = index.search().explore(&ExploreQuery::new(query)).unwrap();
        let hit = result
            .items
            .iter()
            .find(|hit| hit.node.name == name)
            .unwrap();
        let evidence = hit.evidence.as_ref().unwrap();
        assert_eq!(evidence.kind, SourceUnitKind::DocumentationComment);
        let document = evidence.documentation.as_ref().unwrap();
        assert_eq!(
            document.documented_symbol.as_ref().unwrap().as_str(),
            hit.node.id
        );
        assert_eq!(document.inner, inner);
        assert_eq!(evidence.owner.is_some(), inner);
        assert!(
            hit.snippet
                .as_ref()
                .unwrap()
                .lines
                .iter()
                .any(|line| line.contains(query))
        );
        assert!(!evidence.live);
    }
    let mut conjunction = ExploreQuery::new("zirconium opal");
    conjunction.retrieval.mode = graph_search_types::ExploreMode::Terms;
    conjunction.retrieval.term_match = graph_search_types::TermMatch::All;
    let result = index.search().explore(&conjunction).unwrap();
    let hit = result
        .items
        .iter()
        .find(|hit| hit.node.name == "operation")
        .unwrap();
    let comment = hit
        .evidence
        .as_ref()
        .unwrap()
        .documentation
        .as_ref()
        .unwrap();
    assert_eq!((comment.span.start_line, comment.span.end_line), (1, 2));
    let mut phrase = ExploreQuery::new("zirconium café");
    phrase.retrieval.mode = graph_search_types::ExploreMode::Phrase;
    let result = index.search().explore(&phrase).unwrap();
    let hit = result
        .items
        .iter()
        .find(|hit| hit.node.name == "operation")
        .unwrap();
    assert!(hit.evidence.as_ref().unwrap().documentation.is_some());
    assert!(hit.evidence.as_ref().unwrap().owner.is_none());
    verify_live_documentation_transition(&index, root.path());
}

fn verify_live_documentation_transition(index: &Index, root: &std::path::Path) {
    use graph_search_types::query::ExploreQuery;
    std::fs::write(
        root.join("a.rs"),
        "/// replacementgem\nfn replacement() {}\n",
    )
    .unwrap();
    let stale = index
        .search()
        .explore(&ExploreQuery::new("replacementgem"))
        .unwrap();
    let evidence = stale
        .items
        .iter()
        .find_map(|hit| hit.evidence.as_ref())
        .unwrap();
    assert!(evidence.live);
    assert!(evidence.documentation.is_none());
    index.sync().unwrap();
    let changed = index
        .search()
        .explore(&ExploreQuery::new("replacementgem"))
        .unwrap();
    assert!(changed.items.iter().any(|hit| {
        hit.node.name == "replacement"
            && hit
                .evidence
                .as_ref()
                .is_some_and(|e| e.documentation.is_some())
    }));
    let source_facts = || {
        let store = graph_search_engine::GrafeoStore::open(
            index.store_dir(),
            &graph_search_engine::StoreOptions::default(),
        )
        .unwrap();
        store.snapshot().unwrap().source_files().clone()
    };
    let incremental = source_facts();
    index.reindex().unwrap();
    assert_eq!(source_facts(), incremental);
}
