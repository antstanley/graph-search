//! Package scope is manifest-owned, hash-bound and refreshed with source facts.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_core::GraphStore;
use graph_search_types::{query::ExploreQuery, source::SourceFileUnits};
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
        ..OpenOptions::default()
    })
    .unwrap()
}
fn facts(index: &Index) -> BTreeMap<String, SourceFileUnits> {
    let store = graph_search_engine::GrafeoStore::open(
        index.store_dir(),
        &graph_search_engine::StoreOptions::default(),
    )
    .unwrap();
    store.snapshot().unwrap().source_files().unwrap().clone()
}
fn assert_fresh(index: &Index) {
    index.sync().unwrap();
    let incremental = facts(index);
    index.reindex().unwrap();
    assert_eq!(facts(index), incremental);
}

#[test]
fn nearest_manifest_scopes_distinguish_packages_and_expose_ambiguity() {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "Cargo.toml",
            "[workspace]\nmembers=['crates/a','crates/b']\n",
        ),
        ("package.json", "{\"name\":\"@native/root\"}"),
        ("crates/a/Cargo.toml", "[package]\nname='duplicate'\n"),
        ("crates/b/Cargo.toml", "[package]\nname='duplicate'\n"),
        (
            "crates/a/src/lib.rs",
            "/// amethyst scope\npub fn first() {}\n",
        ),
        ("crates/b/src/lib.rs", "pub fn second() { /* jade */ }\n"),
        ("crates/a/README.md", "# Emerald package\n"),
        (
            "src/app.ts",
            "export function entry() { return 'tourmaline'; }\n",
        ),
        ("README.md", "# Ambiguous root\n"),
        ("outside.rs", "fn standalone() {}\n"),
    ] {
        write(root.path(), path, text);
    }
    let index = open(root.path());
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.package_scope_incomplete_files, 1);
    let source = facts(&index);
    let a = source["crates/a/src/lib.rs"].package.as_ref().unwrap();
    let b = source["crates/b/src/lib.rs"].package.as_ref().unwrap();
    assert_eq!(a.name, b.name);
    assert_ne!(a.manifest_path, b.manifest_path);
    assert_eq!(a.manifest_hash, source["crates/a/Cargo.toml"].source_hash);
    assert_eq!(source["crates/a/README.md"].package.as_ref(), Some(a));
    assert_eq!(
        source["src/app.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("@native/root")
    );
    assert!(source["README.md"].package_scope_incomplete);
    assert!(source["README.md"].package.is_none());
    assert!(source["outside.rs"].package.is_none());
    drop(index);
    let index = open(root.path());
    let result = index
        .search()
        .explore(&ExploreQuery::new("amethyst scope"))
        .unwrap();
    let evidence = result
        .items
        .iter()
        .find(|hit| hit.node.name == "first")
        .unwrap()
        .evidence
        .as_ref()
        .unwrap();
    assert_eq!(evidence.package_identity(&result.context), Some(a));
    assert_eq!(
        index
            .search()
            .status()
            .unwrap()
            .coverage
            .package_scope_incomplete_files,
        1
    );
}

#[test]
fn package_boundary_creation_invalid_edit_removal_and_rename_match_rebuilds() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "package.json", "{\"name\":\"outer\"}");
    write(
        root.path(),
        "nested/code.ts",
        "export const value = 'sapphire';\n",
    );
    write(
        root.path(),
        "other/code.ts",
        "export const other = 'opal';\n",
    );
    let index = open(root.path());
    index.reindex().unwrap();
    assert_eq!(
        facts(&index)["nested/code.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("outer")
    );
    write(root.path(), "nested/package.json", "{\"name\":\"inner\"}");
    assert_fresh(&index);
    assert_eq!(
        facts(&index)["nested/code.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("inner")
    );
    write(root.path(), "nested/package.json", "invalid JSON");
    assert_fresh(&index);
    let source = facts(&index);
    assert!(source["nested/code.ts"].package.is_none());
    assert!(source["nested/code.ts"].package_scope_incomplete);
    assert_eq!(
        index
            .search()
            .status()
            .unwrap()
            .coverage
            .package_scope_incomplete_files,
        2
    );
    write(root.path(), "nested/package.json", "{\"name\":\"moved\"}");
    std::fs::rename(
        root.path().join("nested/package.json"),
        root.path().join("other/package.json"),
    )
    .unwrap();
    assert_fresh(&index);
    let source = facts(&index);
    assert_eq!(
        source["nested/code.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("outer")
    );
    assert_eq!(
        source["other/code.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("moved")
    );
    std::fs::remove_file(root.path().join("other/package.json")).unwrap();
    assert_fresh(&index);
    assert_eq!(
        facts(&index)["other/code.ts"]
            .package
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("outer")
    );
}

#[test]
fn oversized_manifest_boundaries_invalidate_without_reading_or_inheriting_outer_scope() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "package.json", "{\"name\":\"outer\"}");
    write(
        root.path(),
        "nested/code.ts",
        "export const value = 'sapphire';\n",
    );
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    let bytes = usize::try_from(index.policy().max_file_bytes).unwrap() + 1;
    write(root.path(), "nested/package.json", &" ".repeat(bytes));
    let result = index
        .search()
        .explore(&ExploreQuery::new("sapphire"))
        .unwrap();
    let evidence = result
        .items
        .iter()
        .find_map(|hit| hit.evidence.as_ref())
        .unwrap();
    assert!(evidence.package.is_none());
    assert!(evidence.package_scope_incomplete);
    let source = facts(&index);
    assert!(source["nested/code.ts"].package_scope_incomplete);
    assert!(!source.contains_key("nested/package.json"));
    assert_eq!(index.search().status().unwrap().coverage.oversized_files, 1);
    assert!(index.sync().unwrap().modified.is_empty());
    index.reindex().unwrap();
    assert_eq!(facts(&index), source);
    std::fs::remove_file(root.path().join("nested/package.json")).unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("sapphire"))
        .unwrap();
    let package = result
        .items
        .iter()
        .find_map(|hit| hit.evidence.as_ref())
        .unwrap()
        .package
        .as_ref()
        .unwrap();
    assert_eq!(package.name.as_deref(), Some("outer"));
    assert_fresh(&index);
}

#[test]
fn repeated_result_packages_share_identity_through_budget_trimming_and_roundtrip() {
    use graph_search_types::{ExploreMode, ExploreResult};
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = 'same-name'\nversion = '0.1.0'\n",
    );
    for name in ["alpha", "beta", "gamma", "delta"] {
        write(
            root.path(),
            &format!("{name}.rs"),
            &format!("pub fn {name}() {{\n // saffron boundary\n}}\n"),
        );
    }
    let index = open(root.path());
    index.reindex().unwrap();
    for mode in [ExploreMode::Terms, ExploreMode::Phrase] {
        for cap in [2500, 4000, 8000, 16384] {
            let mut query = ExploreQuery::new("saffron boundary");
            query.retrieval.mode = mode;
            query.max_bytes = cap;
            let result = index.search().explore(&query).unwrap();
            let bytes = serde_json::to_vec(&result).unwrap();
            assert!(bytes.len() <= cap as usize);
            let decoded: ExploreResult = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(decoded, result);
            let mut used = std::collections::BTreeSet::new();
            for item in &result.items {
                let Some(evidence) = &item.evidence else {
                    continue;
                };
                assert!(evidence.package.is_none());
                let key = evidence.package_ref.as_ref().unwrap();
                used.insert(key.clone());
                let identity = evidence.package_identity(&result.context).unwrap();
                assert_eq!(identity.manifest_path, "Cargo.toml");
                assert_eq!(identity.name.as_deref(), Some("same-name"));
                assert_eq!(
                    identity.manifest_hash,
                    graph_search_core::hash::content_hash(
                        std::fs::read(root.path().join("Cargo.toml"))
                            .unwrap()
                            .as_slice()
                    )
                );
            }
            assert_eq!(used, result.context.packages.keys().cloned().collect());
            if cap == 16384 {
                assert_eq!(result.items.len(), 4);
                assert_eq!(used.len(), 1);
            }
        }
    }
}

#[test]
fn authored_cargo_targets_survive_edits_reopen_and_clean_rebuild() {
    use graph_search_types::package::{CargoTargetKind, PackageRole};
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "custom/entry.rs", "pub fn entry() {}\n");
    let states = [
        "[package]\nname='native'\nedition.workspace=true\nautobins=false\n[lib]\npath='custom/entry.rs'\n[[bin]]\nname='worker'\nrequired-features=['runtime']\n",
        "[package]\nname='native'\nedition='2024'\nautobins=true\n[lib]\npath='../shared/entry.rs'\n",
        "[package]\nname='native'\n[lib]\npath=42\n",
        "[package]\nname='native'\n",
    ];
    for (step, text) in states.into_iter().enumerate() {
        write(root.path(), "Cargo.toml", text);
        let index = open(root.path());
        assert_fresh(&index);
        let source = facts(&index);
        let manifest = source["Cargo.toml"].package_manifest.as_ref().unwrap();
        assert_eq!(manifest.role, PackageRole::Package);
        assert_eq!(manifest.name.as_deref(), Some("native"));
        let metadata = manifest.cargo_targets.as_ref().unwrap();
        assert_eq!(metadata.unavailable_reason.is_some(), step == 2);
        if step == 0 {
            assert!(metadata.edition_workspace);
            assert!(!metadata.auto_discovery[&CargoTargetKind::Bin]);
            assert_eq!(metadata.targets[0].path.as_deref(), Some("custom/entry.rs"));
            assert_eq!(metadata.targets[1].required_features, ["runtime"]);
        } else if step == 1 {
            assert_eq!(metadata.edition.as_deref(), Some("2024"));
            assert_eq!(
                metadata.targets[0].path.as_deref(),
                Some("../shared/entry.rs")
            );
        } else {
            assert!(metadata.targets.is_empty());
        }
        assert_eq!(
            source["custom/entry.rs"]
                .package
                .as_ref()
                .unwrap()
                .manifest_hash,
            source["Cargo.toml"].source_hash
        );
        drop(index);
        assert_eq!(facts(&open(root.path())), source);
    }
}
