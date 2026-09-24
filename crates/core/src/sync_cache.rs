//! State a long-lived index keeps between its own syncs
//! (`research/16-proportional-sync.md` phase 1g).
//!
//! Rust module paths are built from every module declaration, every Cargo
//! manifest, the package boundaries and the walked file set. A sync that
//! changes none of those (the common case: an edit inside a function) builds
//! exactly what the previous sync built, so the result is kept and reused.
//! The memo is valid only against the store generation it was published
//! into: another writer's publication, or a failed one, makes the next sync
//! rebuild.

use crate::resolve::SymbolTable;
use graph_search_types::Node;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// Between-sync state, owned by the caller and lent to each sync.
#[derive(Default)]
pub struct SyncCache {
    rust: Mutex<Option<RustModules>>,
    /// How many times Rust module paths were built rather than reused.
    #[cfg(test)]
    builds: std::sync::atomic::AtomicUsize,
}

/// Rust module paths and the inputs they were built from.
struct RustModules {
    /// The generation these paths describe; `None` until their sync publishes.
    generation: Option<String>,
    known: BTreeSet<String>,
    boundaries: BTreeSet<String>,
    manifests: BTreeMap<String, Node>,
    /// Each file's module declarations, sorted by id.
    modules: BTreeMap<String, Vec<Node>>,
    catalog: crate::rust_modules::Catalog,
    paths: crate::rust_paths::Paths,
}

impl SyncCache {
    /// Prepares `table`'s Rust module paths: reused when the store is still at
    /// `generation`, the file set, boundaries and manifests are unchanged and
    /// every file in `replaced` declares exactly the modules it did; built
    /// otherwise.
    pub(crate) fn prepare_rust_modules(
        &self,
        table: &mut SymbolTable<'_>,
        generation: Option<&str>,
        known: &BTreeSet<String>,
        boundaries: &BTreeSet<String>,
        replaced: &BTreeSet<String>,
    ) {
        let Ok(mut slot) = self.rust.lock() else {
            table.prepare_rust_modules(known, boundaries);
            return;
        };
        if let Some(memo) = slot.as_mut()
            && generation.is_some()
            && memo.generation.as_deref() == generation
            && memo.known == *known
            && memo.boundaries == *boundaries
            && memo.manifests == *table.manifests()
            && replaced.iter().all(|path| {
                memo.modules.get(path).map_or(&[][..], Vec::as_slice)
                    == table.added_modules(path).as_slice()
            })
        {
            table.install_rust_modules(memo.catalog.clone(), memo.paths.without_members());
            memo.generation = None;
            return;
        }
        #[cfg(test)]
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let built = table.build_rust_modules(known, boundaries);
        let mut modules: BTreeMap<String, Vec<Node>> = BTreeMap::new();
        for node in built.iter() {
            modules
                .entry(node.path.clone())
                .or_default()
                .push(Node::clone(node));
        }
        *slot = Some(RustModules {
            generation: None,
            known: known.clone(),
            boundaries: boundaries.clone(),
            manifests: table.manifests().clone(),
            modules,
            catalog: table.rust_roots.clone(),
            paths: table.rust_paths.without_members(),
        });
    }

    /// Records that the sync which last prepared Rust module paths published
    /// `generation`.
    pub(crate) fn published(&self, generation: Option<String>) {
        if let Ok(mut slot) = self.rust.lock()
            && let Some(memo) = slot.as_mut()
        {
            memo.generation = generation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::{NodeId, NodeKind};
    use std::sync::atomic::Ordering;

    fn module(path: &str, name: &str) -> Node {
        let mut node = Node {
            id: NodeId::symbol(path, NodeKind::Module, name, None),
            kind: NodeKind::Module,
            path: path.to_owned(),
            name: Some(name.to_owned()),
            qualified_name: Some(name.to_owned()),
            ..Node::default()
        };
        node.attributes
            .insert("rust_module_form".into(), "external".into());
        node
    }

    fn table(modules: &[Node]) -> SymbolTable<'static> {
        let mut table = SymbolTable::new();
        for node in modules {
            table.add(node);
        }
        table
    }

    #[test]
    fn module_paths_are_reused_only_at_their_generation_with_unchanged_declarations() {
        let cache = SyncCache::default();
        let known: BTreeSet<String> = ["src/lib.rs", "src/a.rs"].map(String::from).into();
        let boundaries = BTreeSet::new();
        let lib: BTreeSet<String> = BTreeSet::from(["src/lib.rs".to_owned()]);
        let body: BTreeSet<String> = BTreeSet::from(["src/a.rs".to_owned()]);
        let prepare = |modules: &[Node], generation: Option<&str>, replaced: &BTreeSet<String>| {
            cache.prepare_rust_modules(
                &mut table(modules),
                generation,
                &known,
                &boundaries,
                replaced,
            );
        };
        let declared = [module("src/lib.rs", "a")];
        prepare(&declared, Some("g1"), &lib);
        cache.published(Some("g2".into()));
        // At the published generation, an edit that declares nothing reuses.
        prepare(&declared, Some("g2"), &body);
        assert_eq!(cache.builds.load(Ordering::Relaxed), 1);
        cache.published(Some("g3".into()));
        // A changed declaration rebuilds.
        prepare(&[module("src/lib.rs", "b")], Some("g3"), &lib);
        assert_eq!(cache.builds.load(Ordering::Relaxed), 2);
        cache.published(Some("g4".into()));
        // Another writer's generation rebuilds.
        prepare(&declared, Some("other"), &body);
        assert_eq!(cache.builds.load(Ordering::Relaxed), 3);
        // An unpublished sync leaves nothing to reuse.
        prepare(&declared, None, &body);
        assert_eq!(cache.builds.load(Ordering::Relaxed), 4);
    }
}
