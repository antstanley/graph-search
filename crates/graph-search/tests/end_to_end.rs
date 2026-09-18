//! The library exercised end to end: open a fixture workspace, index it,
//! and read every mode through [`SearchService`] — the reference shape the
//! evaluation measures (`SPEC.md` §4.7, §11.1 shape 3).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::query::{
    DepsQuery, ExploreQuery, NeighborsQuery, SymbolQuery, TraversalQuery,
};
use graph_search_types::{FilesQuery, TextQuery};

fn write(dir: &std::path::Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new(".")))
        .unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(path, contents).unwrap_or_else(|e| panic!("write: {e}"));
}

fn open(root: &std::path::Path) -> Index {
    Index::open(OpenOptions {
        root: root.to_path_buf(),
        reconcile: Reconcile::BeforeQuery,
        ..OpenOptions::default()
    })
    .unwrap_or_else(|e| panic!("open: {e}"))
}

fn workspace() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let root = tmp.path();
    write(
        root,
        "src/tool.rs",
        "pub struct ToolRegistry;\n\nimpl ToolRegistry {\n    fn execute(&self) { helper(); }\n}\n\nfn helper() {}\n",
    );
    write(
        root,
        "src/main.rs",
        "mod tool;\n\nfn main() {\n    let r = tool::ToolRegistry;\n    drop(r);\n}\n",
    );
    write(
        root,
        "web/page.html",
        "<html><head><link rel=\"stylesheet\" href=\"site.css\"></head><body><div id=\"app\" class=\"container\"></div></body></html>\n",
    );
    write(
        root,
        "web/site.css",
        ".container { color: red; }\n#app { margin: 0; }\n",
    );
    tmp
}

#[test]
fn the_full_surface_answers_through_one_handle() {
    let tmp = workspace();
    let index = open(tmp.path());

    // Not indexed yet: status says so, and graph answers are empty-but-honest.
    let status = index
        .search()
        .status()
        .unwrap_or_else(|e| panic!("status: {e}"));
    assert!(!status.exists);

    // Index it.
    let report = index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    assert_eq!(report.added.len(), 4, "{report:?}");
    assert!(report.quarantined.is_empty(), "{report:?}");

    // symbol: where is ToolRegistry defined?
    let found = index
        .search()
        .symbol(&SymbolQuery::new("ToolRegistry"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert_eq!(found.nodes.len(), 1, "{found:?}");
    assert_eq!(found.nodes[0].kind, NodeKind::Struct);
    assert_eq!(found.nodes[0].path, "src/tool.rs");

    // callers: who calls execute? (helper does not; nothing calls execute,
    // but execute calls helper).
    let callees = index
        .search()
        .callees(&TraversalQuery::new("ToolRegistry::execute", 1))
        .unwrap_or_else(|e| panic!("callees: {e}"));
    let helper_edges: Vec<_> = callees
        .edges
        .iter()
        .filter(|edge| edge.to_name == "helper" && edge.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(helper_edges.len(), 1, "{callees:?}");
    assert!(
        helper_edges[0].resolved,
        "helper is unique in the workspace"
    );

    // impact: the blast radius of helper.
    let impact = index
        .search()
        .impact(&TraversalQuery::new("helper", 2))
        .unwrap_or_else(|e| panic!("impact: {e}"));
    assert_eq!(impact.by_depth[0].total, 1, "{impact:?}");

    // deps: the file graph.
    let deps = index
        .search()
        .deps(&DepsQuery::new("src/main.rs"))
        .unwrap_or_else(|e| panic!("deps: {e}"));
    assert!(
        deps.edges.iter().any(|edge| edge.kind == EdgeKind::Imports
            && edge.resolved
            && edge.to_name == "src/tool.rs"),
        "{deps:?}"
    );

    // neighbors + path.
    let neighbors = index
        .search()
        .neighbors(&NeighborsQuery::new("file:src/main.rs"))
        .unwrap_or_else(|e| panic!("neighbors: {e}"));
    assert!(!neighbors.nodes.is_empty(), "{neighbors:?}");

    // files: fresh index -> index-first answers globs from the store.
    assert!(index.is_fresh());
    let found = index
        .search()
        .files(&FilesQuery::new("**/*.rs"))
        .unwrap_or_else(|e| panic!("files: {e}"));
    let paths: Vec<&str> = found.items.iter().map(|hit| hit.path.as_str()).collect();
    assert_eq!(paths, vec!["src/main.rs", "src/tool.rs"], "{paths:?}");

    // text: always a scan, never the index.
    let found = index
        .search()
        .text(&TextQuery::new("ToolRegistry").with_include("*.rs"))
        .unwrap_or_else(|e| panic!("text: {e}"));
    assert!(found.items.len() >= 2, "{found:?}");
    // Walk order: main.rs sorts before tool.rs.
    assert_eq!(found.items[0].path, "src/main.rs");
    assert_eq!(found.items[0].line, 4);
    assert!(
        found
            .items
            .iter()
            .any(|hit| hit.path == "src/tool.rs" && hit.line == 1)
    );

    // The HTML/CSS cross-edges matched exactly: the page's link element
    // carries the stylesheet relationship.
    let page = index
        .search()
        .neighbors(&NeighborsQuery {
            // Two hops: page -> element -> stylesheet.
            hops: 2,
            ..NeighborsQuery::new("file:web/page.html")
        })
        .unwrap_or_else(|e| panic!("neighbors: {e}"));
    assert!(
        page.edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::LoadsStylesheet
                && edge.to.as_deref() == Some("file:web/site.css")),
        "{page:?}"
    );
    assert!(
        page.edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::UsesClass),
        "{page:?}"
    );

    // explore: seeds assemble with context and honesty.
    let explore = index
        .search()
        .explore(&ExploreQuery::new("tool registry execute"))
        .unwrap_or_else(|e| panic!("explore: {e}"));
    assert!(!explore.items.is_empty(), "{explore:?}");
    assert!(explore.approximation.is_some());

    // Status now reports a healthy index.
    let status = index
        .search()
        .status()
        .unwrap_or_else(|e| panic!("status: {e}"));
    assert!(status.exists);
    let counts = status.counts.expect("counts");
    assert!(counts.total_nodes >= 8, "{counts:?}");
}

#[test]
fn a_stale_index_reconciles_before_answering() {
    let tmp = workspace();
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));

    // Edit a file after indexing: the cheap scan sees drift at the next query.
    write(
        tmp.path(),
        "src/tool.rs",
        "pub struct ToolRegistry;\n\nimpl ToolRegistry {\n    fn execute(&self) { helper(); }\n    fn added(&self) {}\n}\n\nfn helper() {}\n",
    );
    let found = index
        .search()
        .symbol(&SymbolQuery::new("added"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert_eq!(
        found.nodes.len(),
        1,
        "the lazy reconcile picked up the edit: {found:?}"
    );

    // And a second sync is a no-op.
    let report = index.sync().unwrap_or_else(|e| panic!("sync: {e}"));
    assert!(
        report.added.is_empty() && report.modified.is_empty(),
        "{report:?}"
    );
}

#[test]
fn a_reader_never_writes_and_a_read_only_index_refuses() {
    let tmp = workspace();
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    drop(index);

    let read_only = Index::open(OpenOptions {
        root: tmp.path().to_path_buf(),
        read_only: true,
        ..OpenOptions::default()
    })
    .unwrap_or_else(|e| panic!("open: {e}"));
    assert!(read_only.sync().is_err(), "read-only refuses to write");
    // Reads still work.
    let found = read_only
        .search()
        .files(&FilesQuery::new("src/*.rs"))
        .unwrap_or_else(|e| panic!("files: {e}"));
    assert_eq!(found.items.len(), 2);
}

#[test]
fn the_direction_filter_flows_through_deps() {
    let tmp = workspace();
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    let incoming = index
        .search()
        .deps(&DepsQuery {
            target: String::from("src/tool.rs"),
            direction: Direction::In,
            ..DepsQuery::default()
        })
        .unwrap_or_else(|e| panic!("deps: {e}"));
    assert!(
        incoming
            .edges
            .iter()
            .any(|edge| edge.from == "file:src/main.rs"),
        "{incoming:?}"
    );
}
