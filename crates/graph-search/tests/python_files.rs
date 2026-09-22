//! Python indexing end to end: symbols, calls and relative imports through
//! one [`SearchService`] handle (`SPEC.md` §7.5).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::query::{DepsQuery, NeighborsQuery, SymbolQuery, TraversalQuery};
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

#[test]
fn python_rebound_methods_are_each_contained_by_their_class() {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    write(
        tmp.path(),
        "m.py",
        "class C:\n    @property\n    def x(self):\n        return 1\n\n    @x.setter\n    def x(self, value):\n        pass\n",
    );
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    let found = index
        .search()
        .symbol(&SymbolQuery::new("C.x"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert_eq!(found.nodes.len(), 2, "{found:?}");
    let class = index
        .search()
        .neighbors(&NeighborsQuery::new("C"))
        .unwrap_or_else(|e| panic!("neighbors: {e}"));
    for node in &found.nodes {
        assert!(
            class
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::Contains
                    && edge.from == "sym:m.py#class:C"
                    && edge.to.as_deref() == Some(node.id.as_str())),
            "{} is not contained by C: {class:?}",
            node.id
        );
    }
}

#[test]
fn python_function_local_imports_resolve_to_module_files() {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let root = tmp.path();
    write(root, "pkg/__init__.py", "");
    write(root, "pkg/mod.py", "def run():\n    pass\n");
    write(root, "util.py", "");
    write(root, "lib.rs", "pub mod util {}\n");
    write(
        root,
        "app.py",
        "def main():\n    import pkg.mod\n    import util\n    from pkg import mod\n",
    );
    let index = open(root);
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    let main = index
        .search()
        .neighbors(&NeighborsQuery::new("main"))
        .unwrap_or_else(|e| panic!("neighbors: {e}"));
    let imports: Vec<(&str, Option<&str>)> = main
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Imports)
        .map(|edge| (edge.to_name.as_str(), edge.to.as_deref()))
        .collect();
    // `import util` names `util.py`, not the Rust `mod util`.
    for target in ["pkg/__init__.py", "pkg/mod.py", "util.py"] {
        assert!(
            imports
                .iter()
                .any(|(_, to)| *to == Some(format!("file:{target}").as_str())),
            "{target}: {imports:?}"
        );
    }
    assert!(
        imports
            .iter()
            .all(|(_, to)| to.is_some_and(|to| to.starts_with("file:"))),
        "{imports:?}"
    );
}

#[test]
fn python_submodule_added_later_rebinds_from_import() {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let root = tmp.path();
    write(root, "pkg/__init__.py", "X = 1\n");
    write(root, "app.py", "from pkg import mod\n");
    let index = open(root);
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    // A new `pkg/mod.py` changes what `from pkg import mod` binds, although
    // `pkg` itself still resolves to the same `__init__.py`.
    write(root, "pkg/mod.py", "def run():\n    pass\n");
    let deps = index
        .search()
        .deps(&DepsQuery::new("app.py"))
        .unwrap_or_else(|e| panic!("deps: {e}"));
    assert!(
        deps.edges.iter().any(|edge| edge.kind == EdgeKind::Imports
            && edge.resolved
            && edge.to_name == "pkg/mod.py"),
        "{deps:?}"
    );
    assert!(deps.edges.iter().all(|edge| edge.resolved), "{deps:?}");
}

fn binding_workspace() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let root = tmp.path();
    write(root, "pkg/__init__.py", "");
    write(
        root,
        "pkg/mod.py",
        "from typing import overload\n\n\nclass Job:\n    def run(self):\n        pass\n\n\ndef run():\n    pass\n\n\n@overload\ndef load(x: int) -> int: ...\n@overload\ndef load(x: str) -> str: ...\ndef load(x):\n    return x\n",
    );
    write(root, "pkg/tools.py", "def helper():\n    pass\n");
    // A regular package wins over a same-named module file.
    write(root, "pkg/util.py", "def which():\n    pass\n");
    write(root, "pkg/util/__init__.py", "def which():\n    pass\n");
    write(
        root,
        "app.py",
        "import pkg.tools\nimport pkg.tools as tools\nfrom pkg.mod import run, load, Job\nfrom pkg.util import which\n\n\ndef outer():\n    def inner():\n        pass\n    inner()\n\n\ndef main():\n    run()\n    Job()\n    tools.helper()\n    pkg.tools.helper()\n    inner()\n    later()\n    json.dumps()\n",
    );
    tmp
}

fn callees(index: &Index, symbol: &str) -> Vec<(String, Option<String>)> {
    index
        .search()
        .callees(&TraversalQuery::new(symbol, 1))
        .unwrap_or_else(|e| panic!("callees: {e}"))
        .edges
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
        .map(|edge| (edge.to_name, edge.to))
        .collect()
}

#[test]
fn python_calls_bind_constructors_module_members_and_top_level_imports() {
    let tmp = binding_workspace();
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));

    let imports: Vec<(String, Option<String>)> = index
        .search()
        .deps(&DepsQuery::new("app.py"))
        .unwrap_or_else(|e| panic!("deps: {e}"))
        .edges
        .into_iter()
        .filter(|edge| edge.kind == EdgeKind::Imports)
        .map(|edge| (edge.to_name, edge.to))
        .collect();
    let import = |name: &str| {
        imports
            .iter()
            .find(|(to_name, _)| to_name == name)
            .and_then(|(_, to)| to.clone())
            .unwrap_or_else(|| panic!("{name}: {imports:?}"))
    };
    // `run` is the top-level function, not `Job.run`; `load` is the last of
    // its `@overload` rebindings; `which` comes from the package, not the
    // same-named module file.
    assert_eq!(import("run"), "sym:pkg/mod.py#function:run");
    assert_eq!(import("load"), "sym:pkg/mod.py#function:load@17");
    assert!(
        import("which").starts_with("sym:pkg/util/__init__.py#"),
        "{imports:?}"
    );

    let main = callees(&index, "main");
    let target = |name: &str| {
        main.iter()
            .find(|(to_name, _)| to_name == name)
            .unwrap_or_else(|| panic!("{name}: {main:?}"))
            .1
            .clone()
    };
    // A bare call never reaches a method; calling a class constructs it.
    assert_eq!(
        target("run").as_deref(),
        Some("sym:pkg/mod.py#function:run")
    );
    assert_eq!(target("Job").as_deref(), Some("sym:pkg/mod.py#class:Job"));
    // Both module-qualified calls (`tools.helper()`, `pkg.tools.helper()`)
    // resolve through the module they name: one edge, nothing dangling.
    assert_eq!(
        target("helper").as_deref(),
        Some("sym:pkg/tools.py#function:helper")
    );
    assert!(
        main.iter().all(|(name, _)| !name.ends_with("tools.helper")),
        "{main:?}"
    );
    // An external module call keeps its spelling when it dangles.
    assert_eq!(target("json.dumps"), None);
    // A function-local `def` is invisible outside its function.
    assert_eq!(target("inner"), None);
    assert!(
        callees(&index, "outer")
            .iter()
            .any(|(name, to)| name == "outer.inner" && to.is_some()),
        "{:?}",
        callees(&index, "outer")
    );

    // A body-only edit keeps the binding surface; a new top-level name
    // changes it and binds the waiting consumer.
    write(
        tmp.path(),
        "pkg/tools.py",
        "def helper():\n    return 1\n\n\ndef later():\n    pass\n",
    );
    let main = callees(&index, "main");
    assert!(
        main.iter().any(|(name, to)| name == "later"
            && to.as_deref() == Some("sym:pkg/tools.py#function:later")),
        "{main:?}"
    );
    assert!(
        main.iter()
            .any(|(name, to)| name == "helper" && to.is_some()),
        "{main:?}"
    );
}
