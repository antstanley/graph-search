//! Accuracy regressions discovered during the research/search-accuracy investigation.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use graph_search::{Index, OpenOptions};
use graph_search_types::query::*;
use graph_search_types::{EdgeKind, Language, NodeKind};
fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for (p, s) in files {
        let path = dir.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, s).unwrap();
    }
    let index = Index::open(OpenOptions {
        root: dir.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    (dir, index)
}
#[test]
fn js_calls_belong_to_functions_and_nested_receivers_are_visited() {
    let (_d, i) = fixture(&[(
        "a.ts",
        "function leaf() {} function factory() { return {}; } function entry() { leaf(); factory().run(); }",
    )]);
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(r.edges.iter().any(|e| e.to_name == "leaf" && e.resolved));
    assert!(r.edges.iter().any(|e| e.to_name == "factory" && e.resolved));
    assert!(r.edges.iter().all(|e| e.from.contains("function:entry")));
}
#[test]
fn rust_nested_receiver_calls_are_visited() {
    let (_d, i) = fixture(&[("a.rs", "fn factory() {} fn entry() { factory().run(); }")]);
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(r.edges.iter().any(|e| e.to_name == "factory" && e.resolved));
}
#[test]
fn nested_qualified_names_do_not_repeat_ancestors() {
    let (_d, i) = fixture(&[
        ("a.rs", "mod a { mod b { fn leaf() {} } }"),
        (
            "b.ts",
            "function outer() { function inner() { function leaf() {} } }",
        ),
    ]);
    for q in ["a::b::leaf", "outer.inner.leaf"] {
        assert_eq!(
            i.search().symbol(&SymbolQuery::new(q)).unwrap().nodes.len(),
            1,
            "{q}"
        );
    }
}
#[test]
fn symbol_filters_precede_limit_and_ids_are_supported() {
    let (_d, i) = fixture(&[("a.rs", "fn same() {}"), ("z.rs", "fn same() {}")]);
    let mut q = SymbolQuery::new("same").with_limit(1);
    q.filters.path_glob = Some("z.rs".into());
    let r = i.search().symbol(&q).unwrap();
    assert_eq!(r.nodes.len(), 1);
    assert_eq!(r.nodes[0].path, "z.rs");
    assert_eq!(
        i.search()
            .symbol(&SymbolQuery::new(&r.nodes[0].id))
            .unwrap()
            .nodes
            .len(),
        1
    );
}
#[test]
fn ambiguous_targets_are_not_silently_selected() {
    let (_d, i) = fixture(&[("a.rs", "fn same() {}"), ("z.rs", "fn same() {}")]);
    assert!(i.search().callers(&TraversalQuery::new("same", 1)).is_err());
}
#[test]
fn graph_filters_are_applied_and_bad_globs_rejected() {
    let (_d, i) = fixture(&[("a.rs", "fn leaf() {} fn entry() { leaf(); }")]);
    let mut q = TraversalQuery::new("leaf", 1);
    q.filters.path_glob = Some("z.rs".into());
    let r = i.search().callers(&q).unwrap();
    assert!(r.nodes.is_empty());
    assert!(r.edges.is_empty());
    q.filters.path_glob = Some("[".into());
    assert!(i.search().callers(&q).is_err());
}
#[test]
fn explore_body_hits_respect_filters() {
    let (_d, i) = fixture(&[
        ("a.rs", "fn leaf() {}"),
        ("notes.md", "distinctivebodytoken"),
    ]);
    let mut q = ExploreQuery::new("distinctivebodytoken");
    q.filters.lang = Some(Language::Rust);
    q.filters.path_glob = Some("*.rs".into());
    assert!(i.search().explore(&q).unwrap().items.is_empty());
}
#[test]
fn zero_length_path_contains_its_endpoint() {
    let (_d, i) = fixture(&[("a.rs", "fn leaf() {}")]);
    let r = i.search().path(&PathQuery::new("leaf", "leaf")).unwrap();
    assert_eq!(r.nodes.len(), 1);
    assert!(r.edges.is_empty());
}
#[test]
fn graph_limits_are_reported() {
    let (_d, i) = fixture(&[("a.rs", "fn leaf() {} fn entry() { leaf(); }")]);
    let mut q = TraversalQuery::new("leaf", 1);
    q.limit = 1;
    let r = i.search().callers(&q).unwrap();
    assert!(!r.truncations.is_empty());
}
#[test]
fn foreign_qualified_calls_do_not_bind_to_bare_local_names() {
    let (_d, i) = fixture(&[("a.rs", "fn leaf() {} fn entry() { external::leaf(); }")]);
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(
        r.edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .all(|e| !e.resolved)
    );
}
#[test]
fn sync_rebinds_unchanged_callers_after_target_edits() {
    let (d, i) = fixture(&[("a.rs", "fn entry() { leaf(); }"), ("b.rs", "fn leaf() {}")]);
    std::fs::write(d.path().join("b.rs"), "fn replacement() {}\n").unwrap();
    i.sync().unwrap();
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(r.edges.iter().any(|e| e.to_name == "leaf" && !e.resolved));
    std::fs::write(d.path().join("c.rs"), "fn leaf() {}\n").unwrap();
    i.sync().unwrap();
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(r.edges.iter().any(|e| e.to_name == "leaf" && e.resolved));
}
#[test]
fn named_import_aliases_and_js_extension_substitution_resolve() {
    let (_d, i) = fixture(&[
        ("a.ts", "export function shared() {}"),
        ("b.ts", "export function shared() {}"),
        (
            "use.ts",
            "import { shared as local } from './a.js'; function entry() { local(); }",
        ),
    ]);
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(
        r.edges
            .iter()
            .any(|e| e.to.as_deref() == Some("sym:a.ts#function:shared"))
    );
}
#[test]
fn callable_parameters_remain_dynamic() {
    let (_d, i) = fixture(&[
        ("a.rs", "fn leaf() {} fn entry(leaf: fn()) { leaf(); }"),
        (
            "b.ts",
            "function target() {} function shadow(target: () => void) { target(); }",
        ),
    ]);
    for q in ["entry", "shadow"] {
        let r = i.search().callees(&TraversalQuery::new(q, 1)).unwrap();
        assert!(
            r.edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Calls)
                .all(|e| !e.resolved),
            "{r:?}"
        );
    }
}
#[test]
fn self_receiver_uses_lexical_impl_owner() {
    let (_d, i) = fixture(&[(
        "a.rs",
        "struct A; struct B; impl A { fn run(&self) { self.finish(); } fn finish(&self) {} } impl B { fn finish(&self) {} }",
    )]);
    let r = i
        .search()
        .callees(&TraversalQuery::new("A::run", 1))
        .unwrap();
    assert!(
        r.edges
            .iter()
            .any(|e| e.to_name == "A::finish" && e.resolved)
    );
}
#[test]
fn files_and_text_subdirectories_are_resolved_once_and_stay_fresh() {
    let (directory, index) = fixture(&[("src/a.rs", "fn needle() {}")]);
    let files = graph_search_types::FilesQuery::new("*.rs").with_path("src");
    let result = index.search().files(&files).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].path, "a.rs");
    let mut text = graph_search_types::TextQuery::new("needle");
    text.path = Some("src".into());
    assert_eq!(index.search().text(&text).unwrap().items.len(), 1);
    std::fs::write(directory.path().join("src/b.rs"), "fn added() {} ").unwrap();
    assert_eq!(index.search().files(&files).unwrap().items.len(), 2);
}
#[test]
fn invalid_text_include_does_not_broaden_search() {
    let (_d, i) = fixture(&[("a.rs", "needle")]);
    assert!(
        i.search()
            .text(&graph_search_types::TextQuery::new("needle").with_include("["))
            .is_err()
    );
}
#[test]
fn first_graph_query_builds_missing_index() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.rs"), "fn leaf() {}").unwrap();
    let i = Index::open(OpenOptions {
        root: d.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    assert_eq!(
        i.search()
            .symbol(&SymbolQuery::new("leaf"))
            .unwrap()
            .nodes
            .len(),
        1
    );
}
#[test]
fn explore_punctuation_and_body_snippets_work() {
    let (_d, i) = fixture(&[
        ("a.rs", "fn leaf() {}"),
        ("notes.md", "intro\ndistinctivebodytoken\nend"),
    ]);
    assert!(
        i.search()
            .explore(&ExploreQuery::new("leaf()"))
            .unwrap()
            .items
            .iter()
            .any(|n| n.node.name == "leaf")
    );
    let r = i
        .search()
        .explore(&ExploreQuery::new("distinctivebodytoken"))
        .unwrap();
    assert!(r.items[0].snippet.is_some());
}
#[test]
fn deleting_a_target_rebinds_unchanged_callers() {
    let (d, i) = fixture(&[("a.rs", "fn entry() { leaf(); }"), ("b.rs", "fn leaf() {}")]);
    std::fs::remove_file(d.path().join("b.rs")).unwrap();
    i.sync().unwrap();
    let r = i
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(r.edges.iter().any(|e| e.to_name == "leaf" && !e.resolved));
}
#[test]
fn explore_serialized_budget_and_snippet_cap_are_real() {
    let (_d, i) = fixture(&[(
        "a.rs",
        "fn leaf() {\nlet a=1;\nlet b=2;\nlet c=3;\nlet d=4;\nlet e=5;\nlet f=6;\nlet g=7;\nlet h=8;\nlet i=9;\nlet j=10;\n}\nfn entry() { leaf(); }",
    )]);
    let mut q = ExploreQuery::new("leaf").with_context_lines(10);
    // Required source/runtime provenance can exceed a 1,000-byte envelope.
    q.max_bytes = 2048;
    let r = i.search().explore(&q).unwrap();
    assert!(serde_json::to_vec(&r).unwrap().len() <= 2048);
    assert!(!r.items.is_empty());
    assert!(
        r.items
            .iter()
            .all(|x| x.snippet.as_ref().is_none_or(|s| s.lines.len() <= 10))
    );
    q.max_bytes = 1;
    assert!(i.search().explore(&q).is_err());
    q.max_bytes = 1000;
    match i.search().explore(&q) {
        Ok(result) => assert!(serde_json::to_vec(&result).unwrap().len() <= 1000),
        Err(graph_search::Error::Core(graph_search_core::Error::ResultBudget(1000))) => {}
        Err(error) => panic!("unexpected budget failure: {error}"),
    }
}
#[test]
fn impact_keeps_all_converging_edges_and_orders_by_depth() {
    let (_d, i) = fixture(&[(
        "a.rs",
        "fn target() {} fn x() { target(); } fn y() { target(); } fn z() { x(); y(); }",
    )]);
    let r = i
        .search()
        .impact(&TraversalQuery::new("target", 3))
        .unwrap();
    assert_eq!(r.edges.len(), 4);
    assert_eq!(r.by_depth[0].total, 2);
    assert_eq!(r.by_depth[1].total, 1);
    assert_eq!(r.top.last().unwrap().name, "z");
}
#[test]
fn impact_includes_type_uses_so_structs_have_a_blast_radius() {
    // Finding E1: `impact` traversed only Calls/References, so asking what
    // breaks when a struct changes returned nothing even though `type_uses`
    // edges existed. The cone now matches the `refs` reference vocabulary.
    let (_d, i) = fixture(&[(
        "a.rs",
        "pub struct Thing { pub v: usize }\npub struct Holder { pub inner: Thing }\n",
    )]);
    let r = i.search().impact(&TraversalQuery::new("Thing", 2)).unwrap();
    assert!(
        !r.top.is_empty(),
        "a used struct must have a non-empty impact cone: {r:?}"
    );
    assert_eq!(r.by_depth[0].total, 1, "{:?}", r.by_depth);
    assert!(
        r.top.iter().any(|hit| hit.name.as_str() == "Holder"),
        "the user of the struct must be in the cone: {:?}",
        r.top
    );
}

#[test]
fn rust_parameter_and_return_types_are_type_uses() {
    // Finding E5: `type_uses` came only from direct field types, so a type used
    // solely as a parameter or return type was invisible to `refs`/`impact`.
    let (_d, i) = fixture(&[(
        "a.rs",
        "pub struct Thing { pub v: usize }\npub fn take(t: &Thing) -> Thing { *t }\n",
    )]);
    let r = i.search().refs(&RefQuery::new("Thing")).unwrap();
    assert!(
        r.edges
            .iter()
            .any(|e| e.resolved && e.from.contains("take")),
        "the parameter and return type must be recorded as type uses: {:?}",
        r.edges
    );
}

#[test]
fn rust_generic_parameters_and_self_are_not_type_uses() {
    // A generic parameter `T` is a lexical binding, not a reference to a
    // workspace type named `T`; it must not fabricate a type-use edge, while a
    // real parameter type is still recorded.
    let (_d, i) = fixture(&[
        (
            "a.rs",
            "pub fn generic<T: Clone>(x: T, thing: Thing) -> T { x }\n",
        ),
        (
            "b.rs",
            "pub struct T { pub v: usize }\npub struct Thing { pub v: usize }\n",
        ),
    ]);
    let refs_t = i.search().refs(&RefQuery::new("T")).unwrap();
    assert!(
        !refs_t.edges.iter().any(|e| e.from.contains("generic")),
        "the generic parameter T must not reference struct T: {:?}",
        refs_t.edges
    );
    let refs_thing = i.search().refs(&RefQuery::new("Thing")).unwrap();
    assert!(
        refs_thing
            .edges
            .iter()
            .any(|e| e.resolved && e.from.contains("generic")),
        "the real parameter type Thing must remain a type use: {:?}",
        refs_thing.edges
    );
}

#[test]
fn explore_respects_index_exclusions() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("excluded")).unwrap();
    std::fs::write(d.path().join("excluded/note.md"), "distinctivebodytoken").unwrap();
    let i = Index::open(OpenOptions {
        root: d.path().into(),
        excludes: vec!["excluded".into()],
        ..OpenOptions::default()
    })
    .unwrap();
    i.reindex().unwrap();
    assert!(
        i.search()
            .explore(&ExploreQuery::new("distinctivebodytoken"))
            .unwrap()
            .items
            .is_empty()
    );
}
#[test]
fn tsx_uses_the_tsx_grammar() {
    let (_d, i) = fixture(&[(
        "view.tsx",
        "export function View() { return <section>{renderBody()}</section>; } function renderBody() { return 'body'; }",
    )]);
    let r = i.search().callees(&TraversalQuery::new("View", 1)).unwrap();
    assert!(
        r.edges
            .iter()
            .any(|e| e.to_name == "renderBody" && e.resolved)
    );
}
#[test]
fn explore_two_hop_connections_include_intermediate_source() {
    let (_d, i) = fixture(&[(
        "a.rs",
        "fn entry() { middle(); } fn middle() { leaf(); } fn leaf() {}",
    )]);
    let mut q = ExploreQuery::new("entry leaf");
    q.k = 2;
    q.hops = 2;
    let r = i.search().explore(&q).unwrap();
    assert!(r.items.iter().any(|n| n.node.name == "middle"));
    assert!(r.edges.iter().any(|e| e.to_name == "middle"));
    assert!(r.edges.iter().any(|e| e.to_name == "leaf"));
}
#[test]
fn js_initializer_calls_use_enclosing_function_and_destructured_bindings_are_siblings() {
    let (_directory, index) = fixture(&[(
        "a.ts",
        "function leaf() { return {}; } function entry() { const result = leaf(); const { first, second } = leaf(); }",
    )]);
    let result = index
        .search()
        .callees(&TraversalQuery::new("entry", 1))
        .unwrap();
    assert!(
        result
            .edges
            .iter()
            .any(|edge| edge.to_name == "leaf" && edge.resolved)
    );
    for name in ["entry.first", "entry.second"] {
        assert_eq!(
            index
                .search()
                .symbol(&SymbolQuery::new(name))
                .unwrap()
                .nodes
                .len(),
            1,
            "{name}"
        );
    }
}
#[test]
fn three_same_name_symbols_on_one_line_index_with_distinct_ids() {
    // Minified sources repeat a name on one line; a line disambiguator alone
    // collided and the store rejected the whole index.
    let (_directory, index) = fixture(&[
        ("a.rs", "fn f() {} fn f() {} fn f() {}\n"),
        ("b.js", "function g() {} function g() {} function g() {}\n"),
        ("c.css", ".a{--p:1}.b{--p:2}.c{--p:3}\n"),
    ]);
    for (name, path) in [("f", "a.rs"), ("g", "b.js")] {
        let nodes = index
            .search()
            .symbol(&SymbolQuery::new(name))
            .unwrap()
            .nodes;
        let mut ids: Vec<_> = nodes
            .iter()
            .filter(|n| n.path == path)
            .map(|n| n.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3, "{name}: {ids:?}");
    }
}
#[test]
fn every_duplicate_declaration_is_contained_by_its_file() {
    // `#[cfg]` variants share a fact key; each still needs its own `contains`.
    let (_directory, index) = fixture(&[(
        "a.rs",
        "#[cfg(unix)]\nfn detach() {}\n\nfn other() {}\n\n#[cfg(windows)]\nfn detach() {}\n",
    )]);
    let mut query = NeighborsQuery::new("file:a.rs");
    query.rel = Some(EdgeKind::Contains);
    let result = index.search().neighbors(&query).unwrap();
    let mut contained: Vec<_> = result
        .edges
        .iter()
        .filter(|e| e.from == "file:a.rs")
        .filter_map(|e| e.to.clone())
        .collect();
    contained.sort();
    assert_eq!(
        contained,
        [
            "sym:a.rs#function:detach",
            "sym:a.rs#function:detach@7",
            "sym:a.rs#function:other"
        ]
    );
}
#[test]
fn ts_private_and_awaited_generic_this_calls_bind_to_the_class_method() {
    let (_directory, index) = fixture(&[(
        "a.ts",
        "export class A {\n  #k(): number { return 1 }\n  async call<T>(op: string): Promise<T> { return {} as T }\n  async run() {\n    const a = await this.call<{ x?: string }>('a');\n    const b = this.call<number>('b');\n    return this.#k() + (await a) + (await b);\n  }\n}\nexport class B { call() {} #k() {} }\n",
    )]);
    let result = index
        .search()
        .callees(&TraversalQuery::new("A.run", 1))
        .unwrap();
    let mut bound: Vec<_> = result
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| (e.to.as_ref().map(ToString::to_string), e.occurrence_count))
        .collect();
    bound.sort();
    // Both `this.call<..>(..)` forms aggregate into one two-occurrence edge.
    assert_eq!(
        bound,
        [
            (Some("sym:a.ts#method:A.#k".to_owned()), Some(1)),
            (Some("sym:a.ts#method:A.call".to_owned()), Some(2)),
        ]
    );
}
#[test]
fn ts_abstract_methods_are_methods_of_their_class() {
    let (_directory, index) = fixture(&[(
        "a.ts",
        "export abstract class Base {\n  protected abstract baseUrl(): string;\n  run() { return this.baseUrl(); }\n}\n",
    )]);
    let nodes = index
        .search()
        .symbol(&SymbolQuery::new("baseUrl"))
        .unwrap()
        .nodes;
    assert_eq!(
        nodes
            .iter()
            .map(|n| (n.kind, n.qualified_name.as_str(), n.start_line))
            .collect::<Vec<_>>(),
        [(NodeKind::Method, "Base.baseUrl", 2)]
    );
    let callers = index
        .search()
        .callers(&TraversalQuery::new("Base.baseUrl", 1))
        .unwrap();
    assert!(
        callers
            .edges
            .iter()
            .any(|e| e.from.as_str() == "sym:a.ts#method:Base.run")
    );
}
#[test]
fn rust_self_path_calls_bind_to_the_enclosing_impl_when_the_name_is_shared() {
    let (_directory, index) = fixture(&[
        (
            "src/lib.rs",
            "mod other;\npub struct Config;\nimpl Config {\n    pub fn new() -> Self { Config }\n    pub fn from_env() -> Self { Self::new() }\n}\n",
        ),
        (
            "src/other.rs",
            "pub struct Other;\nimpl Other { pub fn new() -> Self { Other } }\n",
        ),
    ]);
    let result = index
        .search()
        .callees(&TraversalQuery::new("Config::from_env", 1))
        .unwrap();
    let targets: Vec<_> = result
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .map(|e| e.to.as_ref().map(ToString::to_string))
        .collect();
    assert_eq!(
        targets,
        [Some("sym:src/lib.rs#method:Config::new".to_owned())]
    );
}
#[test]
fn rust_self_path_calls_bind_in_trait_and_generic_impls() {
    let (_directory, index) = fixture(&[
        (
            "src/lib.rs",
            "mod other;\npub struct Holder<T>(T);\nimpl<T> Holder<T> {\n    pub fn new(t: T) -> Self { Holder(t) }\n}\nimpl<T: Default> Default for Holder<T> {\n    fn default() -> Self { Self::new(T::default()) }\n}\n",
        ),
        (
            "src/other.rs",
            "pub struct Other;\nimpl Other { pub fn new() -> Self { Other } }\n",
        ),
    ]);
    let callees = index
        .search()
        .callees(&TraversalQuery::new(
            "sym:src/lib.rs#method:Holder::default",
            1,
        ))
        .unwrap();
    assert!(
        callees.edges.iter().any(|e| e.kind == EdgeKind::Calls
            && e.to.as_ref().map(ToString::to_string).as_deref()
                == Some("sym:src/lib.rs#method:Holder::new")),
        "{:?}",
        callees.edges
    );
}
#[test]
fn rust_glob_imported_parent_functions_are_called_from_test_modules() {
    let (_directory, index) = fixture(&[
        (
            "src/lib.rs",
            "mod other;\nfn render_arguments(x: u32) -> u32 { x }\npub fn apply() -> u32 { render_arguments(1) }\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn renders() { assert_eq!(render_arguments(2), 2); let y = render_arguments(3); }\n}\n",
        ),
        ("src/other.rs", "pub fn unrelated() {}\n"),
    ]);
    let callers = index
        .search()
        .callers(&TraversalQuery::new("render_arguments", 1))
        .unwrap();
    let mut from: Vec<_> = callers.edges.iter().map(|e| e.from.clone()).collect();
    from.sort();
    assert_eq!(
        from,
        [
            "sym:src/lib.rs#function:apply",
            "sym:src/lib.rs#function:tests::renders"
        ]
    );
}
#[test]
fn rust_super_glob_does_not_bind_names_the_parent_lacks() {
    let (_directory, index) = fixture(&[
        (
            "src/lib.rs",
            "mod other;\nfn local() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n    fn case() { helper(); local(); }\n}\n",
        ),
        ("src/other.rs", "pub fn helper() {}\n"),
    ]);
    let callees = index
        .search()
        .callees(&TraversalQuery::new("tests::case", 1))
        .unwrap();
    let bound: Vec<_> = callees
        .edges
        .iter()
        .map(|e| (e.to_name.as_str(), e.to.as_ref().map(ToString::to_string)))
        .collect();
    assert!(
        bound.contains(&("local", Some("sym:src/lib.rs#function:local".to_owned()))),
        "{bound:?}"
    );
    assert!(
        bound
            .iter()
            .any(|(name, to)| name.ends_with("helper") && to.is_none()),
        "{bound:?}"
    );
}
#[test]
fn rust_calls_in_macro_arguments_are_extracted() {
    let (_directory, index) = fixture(&[
        (
            "src/lib.rs",
            "mod util;\npub struct Stats;\nimpl Stats {\n    fn average(&self) -> u32 { 1 }\n    fn report(&self) -> String {\n        format!(\"avg {}\", self.average())\n    }\n}\nfn width(s: &str) -> usize { s.len() }\nfn parse<T: Default>(s: &str) -> T { T::default() }\npub fn check() {\n    assert_eq!(width(\"a\"), util::double(parse::<usize>(\"1\")));\n    println!(\"{}\", vec![width(\"b\")].len());\n}\nmacro_rules! make { ($n:ident) => { fn $n() {} } }\nmake!(generated);\n",
        ),
        (
            "src/util.rs",
            "pub fn double(x: usize) -> usize { x * 2 }\n",
        ),
    ]);
    let callees = |target: &str| {
        let mut names: Vec<_> = index
            .search()
            .callees(&TraversalQuery::new(target, 1))
            .unwrap()
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Calls && e.resolved)
            .filter_map(|e| e.to.as_ref().map(ToString::to_string))
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        callees("Stats::report"),
        ["sym:src/lib.rs#method:Stats::average"]
    );
    assert_eq!(
        callees("check"),
        [
            "sym:src/lib.rs#function:parse",
            "sym:src/lib.rs#function:width",
            "sym:src/util.rs#function:double",
        ]
    );
    // Macro names are not calls.
    let all: Vec<_> = index
        .search()
        .callees(&TraversalQuery::new("check", 1))
        .unwrap()
        .edges
        .into_iter()
        .map(|e| e.to_name)
        .collect();
    assert!(
        !all.iter()
            .any(|n| n == "vec" || n == "assert_eq" || n == "println"),
        "{all:?}"
    );
}
#[test]
fn rust_calls_through_a_child_module_path_resolve() {
    let (_directory, index) = fixture(&[
        (
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        ),
        (
            "src/lib.rs",
            "mod util;\npub fn check() -> usize { util::double(1) }\n",
        ),
        (
            "src/util.rs",
            "pub fn double(x: usize) -> usize { x * 2 }\nfn double_private() {}\n",
        ),
        ("src/other.rs", "pub fn double(x: usize) -> usize { x }\n"),
    ]);
    let callees = index
        .search()
        .callees(&TraversalQuery::new("check", 1))
        .unwrap();
    let targets: Vec<_> = callees
        .edges
        .iter()
        .map(|e| e.to.as_ref().map(ToString::to_string))
        .collect();
    assert_eq!(
        targets,
        [Some("sym:src/util.rs#function:double".to_owned())]
    );
}
