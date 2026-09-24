//! OKF incremental sync equals a clean rebuild, and rebinds only documents
//! whose link or citation targets changed (`SPEC.md` §7.6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use graph_search_core::{
    Result,
    config::WalkPolicy,
    memory::MemoryStore,
    ports::{ExtractionFacts, GraphSnapshot, GraphStore, ListRegistry},
    reconcile::Projector,
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{ApplyOutcome, Language, Manifest, WriteBatch};
use std::{cell::RefCell, collections::BTreeSet, path::Path};

/// Records which raw facts a sync loads to rebind unchanged files.
struct Observed {
    inner: Box<dyn GraphStore>,
    reads: RefCell<BTreeSet<String>>,
}
impl GraphStore for Observed {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.inner.apply(batch)
    }
    fn publish(&mut self, batch: WriteBatch) -> Result<ApplyOutcome> {
        self.inner.publish(batch)
    }
    // Forwarded so retained (not rebound) facts never count as reads.
    fn publish_retaining(
        &mut self,
        batch: WriteBatch,
        retention: &graph_search_core::retention::FactRetention,
    ) -> Result<ApplyOutcome> {
        self.inner.publish_retaining(batch, retention)
    }
    fn generation(&self) -> Result<Option<String>> {
        self.inner.generation()
    }
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>> {
        self.inner.snapshot()
    }
    fn manifest(&self) -> Result<Option<Manifest>> {
        self.inner.manifest()
    }
    fn manifest_header(&self) -> Result<Option<Manifest>> {
        self.inner.manifest_header()
    }
    fn extraction_facts(&self, paths: &BTreeSet<String>) -> Result<ExtractionFacts> {
        self.reads.borrow_mut().extend(paths.iter().cloned());
        self.inner.extraction_facts(paths)
    }
    fn dependency_index(
        &self,
    ) -> Result<Option<&graph_search_core::dependencies::DependencyIndex>> {
        self.inner.dependency_index()
    }
    fn commit_manifest(&mut self, manifest: Manifest) -> Result<()> {
        self.inner.commit_manifest(manifest)
    }
}

fn store(persistent: bool, dir: &Path) -> Observed {
    Observed {
        inner: if persistent {
            Box::new(GrafeoStore::open(dir, &StoreOptions::default()).unwrap())
        } else {
            Box::new(MemoryStore::new())
        },
        reads: RefCell::new(BTreeSet::new()),
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Asserts the synced store equals a clean rebuild of the same tree.
fn assert_clean(root: &Path, actual: &Observed, persistent: bool, policy: &WalkPolicy, step: &str) {
    let registry = ListRegistry::new(graph_search_langs::all_extractors());
    let dir = tempfile::tempdir().unwrap();
    let mut clean = store(persistent, dir.path());
    Projector::new(&registry, policy)
        .reindex(root, &mut clean)
        .unwrap();
    let a = actual.snapshot().unwrap();
    let b = clean.snapshot().unwrap();
    assert_eq!(a.all_nodes().unwrap(), b.all_nodes().unwrap(), "{step}");
    assert_eq!(a.all_edges().unwrap(), b.all_edges().unwrap(), "{step}");
    assert_eq!(
        a.occurrence_files().unwrap(),
        b.occurrence_files().unwrap(),
        "{step}"
    );
}

const GROSS: &str = "---\ntitle: Gross\nsources:\n  - id: p\n    resource: policies/p.md\n---\n# Definition\n\nUses [Revenue](./revenue.md) and [tables](/tables/).[^p]\n\n[^p]: Policy.\n";

#[test]
fn okf_sync_matches_a_clean_rebuild_and_rebinds_only_dependents() {
    for persistent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let db = tempfile::tempdir().unwrap();
        write(root, "kb/index.md", "# Bundle\n\n* [metrics](metrics/)\n");
        write(root, "kb/metrics/gross.md", GROSS);
        write(
            root,
            "kb/metrics/revenue.md",
            "---\ntitle: Revenue\n---\n# Definition\n",
        );
        write(
            root,
            "kb/metrics/cold.md",
            "---\ntitle: Cold\n---\nNo links.\n",
        );
        write(root, "kb/policies/p.md", "---\ntitle: Policy\n---\n");
        write(root, "src/lib.rs", "pub fn f() {}\n");
        let registry = ListRegistry::new(graph_search_langs::all_extractors());
        let policy = WalkPolicy::default();
        let projector = Projector::new(&registry, &policy);
        let mut actual = store(persistent, db.path());
        projector.reindex(root, &mut actual).unwrap();

        let steps: [(&str, &dyn Fn()); 6] = [
            ("retitle a link target", &|| {
                write(
                    root,
                    "kb/metrics/revenue.md",
                    "---\ntitle: Net Revenue\n---\n",
                );
            }),
            ("remove a link target", &|| {
                std::fs::remove_file(root.join("kb/metrics/revenue.md")).unwrap();
            }),
            ("restore a link target", &|| {
                write(root, "kb/metrics/revenue.md", "---\ntitle: Revenue\n---\n");
            }),
            ("add a directory listing target", &|| {
                write(root, "kb/tables/index.md", "# Tables\n");
            }),
            ("rename a cited source", &|| {
                std::fs::rename(root.join("kb/policies/p.md"), root.join("kb/policies/q.md"))
                    .unwrap();
            }),
            ("edit unrelated code", &|| {
                write(root, "src/lib.rs", "pub fn g() {}\n");
            }),
        ];
        for (step, edit) in steps {
            edit();
            actual.reads.borrow_mut().clear();
            projector.sync(root, &mut actual).unwrap();
            assert_clean(root, &actual, persistent, &policy, step);
            // A document whose targets did not change is never rebound.
            if persistent {
                let reads = actual.reads.borrow();
                assert!(!reads.contains("kb/metrics/cold.md"), "{step}: {reads:?}");
                if step == "edit unrelated code" {
                    assert!(
                        reads
                            .iter()
                            .all(|path| Path::new(path).extension().is_none_or(|e| e != "md")),
                        "{step}: {reads:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_disabled_okf_language_keeps_sync_and_rebuild_in_agreement() {
    for persistent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let db = tempfile::tempdir().unwrap();
        write(root, "kb/a.md", "---\ntitle: A\n---\n");
        write(root, "kb/b.md", "# B\n");
        let registry = ListRegistry::new(graph_search_langs::all_extractors());
        let policy = WalkPolicy {
            languages: WalkPolicy::default()
                .languages
                .into_iter()
                .filter(|language| *language != Language::Okf)
                .collect(),
            ..WalkPolicy::default()
        };
        let projector = Projector::new(&registry, &policy);
        let mut actual = store(persistent, db.path());
        projector.reindex(root, &mut actual).unwrap();
        write(root, "kb/index.md", "# Bundle\n");
        projector.sync(root, &mut actual).unwrap();
        assert_clean(root, &actual, persistent, &policy, "add index.md");
        std::fs::remove_file(root.join("kb/index.md")).unwrap();
        projector.sync(root, &mut actual).unwrap();
        assert_clean(root, &actual, persistent, &policy, "remove index.md");
        let snapshot = actual.snapshot().unwrap();
        assert!(
            snapshot
                .all_nodes()
                .unwrap()
                .iter()
                .all(|node| node.language != Some(Language::Okf))
        );
    }
}
