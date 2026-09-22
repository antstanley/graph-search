//! Python indexing end to end: symbols, calls and relative imports through
//! one [`SearchService`] handle (`SPEC.md` §7.5).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::query::{DepsQuery, SymbolQuery, TraversalQuery};
use graph_search_types::{FilesQuery, Language};

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
    write(root, "pkg/__init__.py", "\"\"\"pkg.\"\"\"\n");
    write(root, "pkg/util.py", "def helper() -> int:\n    return 1\n");
    write(
        root,
        "pkg/greeter.py",
        "from .util import helper\n\n\nclass Greeter:\n    def hello(self) -> str:\n        return self.render()\n\n    def render(self) -> str:\n        return \"hi\"\n\n\ndef greet() -> int:\n    return helper()\n",
    );
    write(
        root,
        "app.py",
        "from pkg.greeter import Greeter\n\n\ndef main() -> None:\n    g = Greeter()\n    g.hello()\n",
    );
    tmp
}

#[test]
fn python_symbols_calls_and_relative_imports_resolve() {
    let tmp = workspace();
    let index = open(tmp.path());
    let report = index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    assert_eq!(report.added.len(), 4, "{report:?}");
    assert!(report.quarantined.is_empty(), "{report:?}");

    // Every file is claimed by the Python adapter, not indexed as unknown.
    let status = index
        .search()
        .status()
        .unwrap_or_else(|e| panic!("status: {e}"));
    let counts = status.counts.expect("counts");
    assert_eq!(counts.files_by_language[&Language::Python], 4, "{counts:?}");

    // `files` answers from the index.
    let py = index
        .search()
        .files(&FilesQuery::new("**/*.py"))
        .unwrap_or_else(|e| panic!("files: {e}"));
    assert_eq!(py.items.len(), 4, "{py:?}");

    // Where is Greeter defined?
    let found = index
        .search()
        .symbol(&SymbolQuery::new("Greeter"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert_eq!(found.nodes.len(), 1, "{found:?}");
    assert_eq!(found.nodes[0].kind, NodeKind::Class);
    assert_eq!(found.nodes[0].path, "pkg/greeter.py");

    // `self.render()` in `hello` resolves to the class's own method.
    let callees = index
        .search()
        .callees(&TraversalQuery::new("Greeter.hello", 1))
        .unwrap_or_else(|e| panic!("callees: {e}"));
    assert!(
        callees.edges.iter().any(|edge| edge.kind == EdgeKind::Calls
            && edge.resolved
            && edge.to_name == "Greeter.render"),
        "{callees:?}"
    );

    // `helper()` resolves through `from .util import helper`.
    let callees = index
        .search()
        .callees(&TraversalQuery::new("greet", 1))
        .unwrap_or_else(|e| panic!("callees: {e}"));
    assert!(
        callees
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Calls && edge.resolved && edge.to_name == "helper"),
        "{callees:?}"
    );

    // The relative import is a file -> file edge in the deps graph.
    let deps = index
        .search()
        .deps(&DepsQuery::new("pkg/greeter.py"))
        .unwrap_or_else(|e| panic!("deps: {e}"));
    assert!(
        deps.edges.iter().any(|edge| edge.kind == EdgeKind::Imports
            && edge.resolved
            && edge.to_name == "pkg/util.py"),
        "{deps:?}"
    );
}
