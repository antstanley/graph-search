//! Rust method calls bound through their receiver's stated type (finding P1.2).
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_types::occurrence::{OccurrenceFile, ResolutionClass};
use graph_search_types::{EdgeKind, NodeId};
use std::{collections::BTreeMap, path::Path};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn open(root: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        reconcile: Reconcile::Never,
        ..Default::default()
    })
    .unwrap()
}

fn facts(index: &Index) -> BTreeMap<String, OccurrenceFile> {
    let store = graph_search_engine::NativeStore::open(
        index.store_dir(),
        &graph_search_engine::StoreOptions::default(),
    )
    .unwrap();
    store
        .snapshot()
        .unwrap()
        .occurrence_files()
        .unwrap()
        .clone()
}

/// The target of the one call in `path` spelled `raw` on `line`.
fn call(
    all: &BTreeMap<String, OccurrenceFile>,
    path: &str,
    line: u32,
    raw: &str,
) -> Option<String> {
    let matches: Vec<_> = all[path]
        .records
        .iter()
        .filter(|r| {
            r.kind == EdgeKind::Calls && r.line == line && r.raw_name.as_deref() == Some(raw)
        })
        .collect();
    assert_eq!(matches.len(), 1, "{path}:{line} {raw}: {:?}", all[path]);
    let record = matches[0];
    if record.target.is_some() {
        assert_eq!(record.resolution, ResolutionClass::Receiver, "{record:?}");
    }
    record
        .target
        .as_ref()
        .map(|id| NodeId::as_str(id).to_owned())
}

const DOMAIN: &str = "pub struct Registry;
impl Registry {
    pub fn new() -> Self { Registry }
    pub fn execute(&self) {}
    fn private(&self) {}
}
pub fn load() -> Result<Registry, ()> { Ok(Registry) }
";

const HANDLE: &str = "use core::cell::Ref;
use dom::Registry;
pub struct Handle(core::cell::RefCell<Registry>);
impl Handle {
    pub fn borrow(&self) -> Ref<'_, Registry> { self.0.borrow() }
}
";

const APP: &str = "use dom::{Registry, load};
use crate::handle::Handle;
pub struct Runner { tools: Handle }
pub struct Point { x: u8 }
impl Point { pub fn norm(&self) -> u8 { self.x } }
impl Runner {
    fn run(&self, other: &Handle, items: Vec<u8>) -> Result<(), ()> {
        let direct = Registry::new();
        direct.execute();
        let registry = self.tools.borrow();
        registry.execute();
        other.borrow().execute();
        let loaded = load()?;
        loaded.execute();
        load().unwrap().execute();
        let point = Point { x: 1 };
        point.norm();
        items.iter().for_each(|direct| direct.execute());
        let unknown = mystery();
        unknown.execute();
        items.len();
        direct.private();
        Ok(())
    }
}
";

fn workspace(root: &Path) {
    for (path, text) in [
        (
            "Cargo.toml",
            "[workspace]\nmembers=['dom','app']\n[workspace.package]\nedition='2021'\n",
        ),
        (
            "dom/Cargo.toml",
            "[package]\nname='dom'\nedition.workspace=true\n",
        ),
        ("dom/src/lib.rs", DOMAIN),
        (
            "app/Cargo.toml",
            "[package]\nname='app'\nedition.workspace=true\n",
        ),
        (
            "app/src/lib.rs",
            "mod handle;\nmod run;\npub use handle::Handle;\n",
        ),
        ("app/src/handle.rs", HANDLE),
        ("app/src/run.rs", APP),
    ] {
        write(root, path, text);
    }
}

#[test]
fn receiver_types_bind_method_calls_and_never_guess() {
    let root = tempfile::tempdir().unwrap();
    workspace(root.path());
    let index = open(root.path());
    index.reindex().unwrap();
    let all = facts(&index);
    let run = "app/src/run.rs";
    let execute = Some("sym:dom/src/lib.rs#method:Registry::execute".to_owned());
    // A constructor returning `Self`.
    assert_eq!(call(&all, run, 9, "direct.execute"), execute);
    // A `self` field, then a method whose `Ref<'_, Registry>` return is peeled.
    assert_eq!(
        call(&all, run, 10, "self.tools.borrow"),
        Some("sym:app/src/handle.rs#method:Handle::borrow".to_owned())
    );
    assert_eq!(call(&all, run, 11, "registry.execute"), execute);
    // A typed parameter, chained.
    assert_eq!(call(&all, run, 12, "other.borrow().execute"), execute);
    // `?` and `unwrap()` take the `Result` success type.
    assert_eq!(call(&all, run, 14, "loaded.execute"), execute);
    assert_eq!(call(&all, run, 15, "load().unwrap().execute"), execute);
    // A struct literal names its type.
    assert_eq!(
        call(&all, run, 17, "point.norm"),
        Some("sym:app/src/run.rs#method:Point::norm".to_owned())
    );
    // A closure parameter shadows the typed local; an untyped local and a
    // std receiver stay unbound; another crate's private method is invisible.
    assert_eq!(call(&all, run, 18, "direct.execute"), None);
    assert_eq!(call(&all, run, 20, "unknown.execute"), None);
    assert_eq!(call(&all, run, 21, "items.len"), None);
    assert_eq!(call(&all, run, 22, "direct.private"), None);
}

#[test]
fn a_changed_return_type_rebinds_receiver_calls_like_a_clean_build() {
    let root = tempfile::tempdir().unwrap();
    workspace(root.path());
    let mut index = open(root.path());
    index.reindex().unwrap();
    // `borrow` now returns another type with its own `execute`.
    write(
        root.path(),
        "app/src/handle.rs",
        &HANDLE
            .replace("Ref<'_, Registry>", "Ref<'_, Other>")
            .replace("RefCell<Registry>", "RefCell<Other>")
            .replace(
                "pub struct Handle",
                "pub struct Other;\nimpl Other { pub fn execute(&self) {} }\npub struct Handle",
            ),
    );
    index.sync().unwrap();
    drop(index);
    index = open(root.path());
    let incremental = facts(&index);
    assert_eq!(
        call(&incremental, "app/src/run.rs", 11, "registry.execute"),
        Some("sym:app/src/handle.rs#method:Other::execute".to_owned())
    );
    index.reindex().unwrap();
    assert_eq!(facts(&index), incremental);
}
